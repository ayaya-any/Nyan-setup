use crate::package::{
    bcj_x86_decode, read_package_from_exe, InstallProgressEvent, PackageManifest, PrereqAction,
    PREREQ_VIRTUAL_PREFIX,
};
use crc32fast::Hasher;
use lzma_rust::LZMA2Reader;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Extract all packaged files from the Solid LZMA2 stream inside `installer_exe_path` into `target_install_dir`.
pub fn extract_package_files(
    installer_exe_path: &Path,
    manifest: &PackageManifest,
    target_install_dir: &Path,
    kill_locked_processes: bool,
    progress_tx: Option<&Sender<InstallProgressEvent>>,
) -> Result<Vec<String>, String> {
    let mut archive_file =
        File::open(installer_exe_path).map_err(|e| format!("无法打开安装包文件: {}", e))?;

    let (footer, _) = read_package_from_exe(installer_exe_path)
        .map_err(|e| format!("读取安装包尾标失败: {}", e))?
        .ok_or_else(|| "安装包尾标损坏或格式不受支持".to_string())?;

    // 1. Pre-check Prerequisite rules:
    //    - PromptAndAbort rules fail fast before touching disk
    //    - RunBundledExe rules determine which bundled installers need extraction to %TEMP%
    let mut needed_bundled_prereqs: Vec<(usize, String, PathBuf)> = Vec::new();
    for (rule_idx, rule) in manifest.config.prerequisites.iter().enumerate() {
        if !rule.enabled {
            continue;
        }
        let passed =
            crate::win32_ops::evaluate_prereq_condition(&rule.condition, Some(target_install_dir));
        if !passed {
            match rule.action {
                PrereqAction::PromptAndAbort {
                    ref message,
                    ref download_url,
                } => {
                    crate::win32_ops::prompt_prereq_abort(&rule.name, message, download_url);
                    return Err(format!("环境检测未通过 [{}]: {}", rule.name, message));
                }
                PrereqAction::RunBundledExe {
                    ref bundled_rel_path,
                    ref source_file,
                    ..
                } => {
                    let rel_key = if !bundled_rel_path.is_empty() {
                        bundled_rel_path.clone()
                    } else {
                        let fname = Path::new(source_file)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| format!("prereq_{}.exe", rule_idx));
                        format!("{}prereq_{}_{}", PREREQ_VIRTUAL_PREFIX, rule_idx, fname)
                    };
                    let safe_fname = Path::new(&rel_key)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| format!("prereq_{}.exe", rule_idx));
                    let temp_dest = std::env::temp_dir().join(format!(
                        "nyan_setup_{}_{}_{}",
                        std::process::id(),
                        rule_idx,
                        safe_fname
                    ));
                    needed_bundled_prereqs.push((rule_idx, rel_key, temp_dest));
                }
                PrereqAction::RunCommand { .. } => {}
            }
        }
    }

    let mut candidate_files: Vec<String> = manifest
        .files
        .iter()
        .filter(|f| !f.relative_path.starts_with(PREREQ_VIRTUAL_PREFIX))
        .map(|f| f.relative_path.clone())
        .collect();
    if !manifest.config.main_exe.is_empty() && !candidate_files.contains(&manifest.config.main_exe)
    {
        candidate_files.push(manifest.config.main_exe.clone());
    }

    if kill_locked_processes {
        let killed_msgs = crate::win32_ops::terminate_running_target_processes(
            target_install_dir,
            &candidate_files,
        );
        if let Some(tx) = progress_tx {
            for msg in killed_msgs {
                let _ = tx.send(InstallProgressEvent::KillingProcesses(msg));
            }
        }
    }

    fs::create_dir_all(target_install_dir)
        .map_err(|e| format!("无法创建目标安装目录 {}: {}", target_install_dir.display(), e))?;

    archive_file
        .seek(SeekFrom::Start(footer.data_offset))
        .map_err(|e| format!("定位固实压缩流起点失败: {}", e))?;

    let solid_stream = archive_file.take(manifest.total_compressed_bytes);
    let mut lzma_reader = LZMA2Reader::new(solid_stream, manifest.dict_size.max(4096), None);

    let total_files = manifest.files.len();
    let bytes_total = manifest.total_uncompressed_bytes.max(1);
    let mut bytes_done = 0u64;
    let mut installed_files = Vec::with_capacity(total_files);

    for (idx, entry) in manifest.files.iter().enumerate() {
        if let Some(tx) = progress_tx {
            let _ = tx.send(InstallProgressEvent::Extracting {
                current_index: idx + 1,
                total_files,
                relative_path: entry.relative_path.clone(),
                bytes_done,
                bytes_total,
            });
        }

        // Handle bundled prerequisite files
        if entry.relative_path.starts_with(PREREQ_VIRTUAL_PREFIX) {
            if let Some((_, _, ref temp_dest)) = needed_bundled_prereqs
                .iter()
                .find(|(_, rel_key, _)| rel_key == &entry.relative_path)
            {
                let mut raw_buf = vec![0u8; entry.uncompressed_size as usize];
                lzma_reader
                    .read_exact(&mut raw_buf)
                    .map_err(|e| format!("解压前置组件 {} 失败: {}", entry.relative_path, e))?;
                if entry.bcj_filtered {
                    bcj_x86_decode(&mut raw_buf);
                }
                let mut hasher = Hasher::new();
                hasher.update(&raw_buf);
                let actual_crc = hasher.finalize();
                if actual_crc != entry.crc32 {
                    return Err(format!(
                        "前置组件 {} CRC32 校验失败 (期望 {:08X}, 实际 {:08X})",
                        entry.relative_path, entry.crc32, actual_crc
                    ));
                }
                fs::write(temp_dest, &raw_buf).map_err(|e| {
                    format!("释放临时前置安装包 {} 失败: {}", temp_dest.display(), e)
                })?;
            } else {
                // Prerequisite already satisfied on system -> discard stream bytes without disk write
                io::copy(
                    &mut (&mut lzma_reader).take(entry.uncompressed_size),
                    &mut io::sink(),
                )
                .map_err(|e| {
                    format!("跳过已满足的前置组件流 {} 失败: {}", entry.relative_path, e)
                })?;
            }
            bytes_done += entry.uncompressed_size;
            continue;
        }

        let clean_rel: PathBuf = entry
            .relative_path
            .split(['\\', '/'])
            .filter(|seg| !seg.is_empty() && *seg != "." && *seg != "..")
            .collect();
        let dest_path = target_install_dir.join(&clean_rel);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建子目录 {} 失败: {}", parent.display(), e))?;
        }

        let mut raw_buf = vec![0u8; entry.uncompressed_size as usize];
        lzma_reader.read_exact(&mut raw_buf).map_err(|e| {
            format!("从 LZMA2 固实流解压文件 {} 失败: {}", entry.relative_path, e)
        })?;

        if entry.bcj_filtered {
            bcj_x86_decode(&mut raw_buf);
        }

        let mut hasher = Hasher::new();
        hasher.update(&raw_buf);
        let actual_crc = hasher.finalize();
        if actual_crc != entry.crc32 {
            return Err(format!(
                "文件 {} 完整性校验失败 (CRC32 不匹配: 期望 {:08X}, 实际 {:08X})",
                entry.relative_path, entry.crc32, actual_crc
            ));
        }

        let mut create_res = File::create(&dest_path);
        if create_res.is_err() && kill_locked_processes {
            for _ in 0..6 {
                let killed = crate::win32_ops::terminate_running_target_processes(
                    target_install_dir,
                    &candidate_files,
                );
                if let Some(tx) = progress_tx {
                    for msg in killed {
                        let _ = tx.send(InstallProgressEvent::KillingProcesses(msg));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
                create_res = File::create(&dest_path);
                if create_res.is_ok() {
                    break;
                }
            }
        }

        let mut out_f = create_res.map_err(|e| {
            if e.raw_os_error() == Some(32) || e.raw_os_error() == Some(5) {
                format!(
                    "目标文件被正在运行的进程占用 ({})，请关闭占用进程或点击下方“结束占用进程并重试”。({})",
                    entry.relative_path, e
                )
            } else {
                format!("无法写入目标文件 {}: {}", entry.relative_path, e)
            }
        })?;
        out_f
            .write_all(&raw_buf)
            .map_err(|e| format!("写入文件 {} 失败: {}", entry.relative_path, e))?;

        bytes_done += entry.uncompressed_size;
        installed_files.push(clean_rel.to_string_lossy().to_string());
    }

    // 2. Execute unsatisfied Prerequisite actions (RunBundledExe & RunCommand)
    for (rule_idx, rule) in manifest.config.prerequisites.iter().enumerate() {
        if !rule.enabled {
            continue;
        }
        match rule.action {
            PrereqAction::RunBundledExe {
                ref args,
                elevate,
                wait,
                ref success_exit_codes,
                ..
            } => {
                if let Some((_, _, ref temp_exe)) = needed_bundled_prereqs
                    .iter()
                    .find(|(r_idx, _, _)| *r_idx == rule_idx)
                {
                    if let Some(tx) = progress_tx {
                        let _ = tx.send(InstallProgressEvent::RunningPrerequisite(
                            rule.name.clone(),
                        ));
                    }
                    let exec_res = crate::win32_ops::execute_prereq_command(
                        &rule.name,
                        &temp_exe.to_string_lossy(),
                        args,
                        elevate,
                        wait,
                        success_exit_codes,
                        Some(target_install_dir),
                    );
                    let _ = fs::remove_file(temp_exe);
                    exec_res?;
                }
            }
            PrereqAction::RunCommand {
                ref program,
                ref args,
                elevate,
                wait,
                ref success_exit_codes,
            } => {
                let passed = crate::win32_ops::evaluate_prereq_condition(
                    &rule.condition,
                    Some(target_install_dir),
                );
                if !passed {
                    if let Some(tx) = progress_tx {
                        let _ = tx.send(InstallProgressEvent::RunningPrerequisite(
                            rule.name.clone(),
                        ));
                    }
                    crate::win32_ops::execute_prereq_command(
                        &rule.name,
                        program,
                        args,
                        elevate,
                        wait,
                        success_exit_codes,
                        Some(target_install_dir),
                    )?;
                }
            }
            PrereqAction::PromptAndAbort { .. } => {}
        }
    }

    Ok(installed_files)
}
