use crate::package::{
    BuildProgressEvent, FileEntryMetadata, NyanFooter, PackageConfig, PackageManifest,
    PrereqAction, FOOTER_SIZE, PREREQ_VIRTUAL_PREFIX,
};
use crc32fast::Hasher;
use lzma_rust::{CountingWriter, LZMA2Options, LZMA2Reader, LZMA2Writer};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Detect whether a file is a Windows PE executable/library (`MZ` header) eligible for x86 BCJ pre-filtering.
pub fn should_apply_x86_bcj(rel_path: &str, data: &[u8]) -> bool {
    if data.len() < 64 || &data[0..2] != b"MZ" {
        return false;
    }
    let lower = rel_path.to_ascii_lowercase();
    lower.ends_with(".exe")
        || lower.ends_with(".dll")
        || lower.ends_with(".sys")
        || lower.ends_with(".ocx")
}

/// Reversible x86 BCJ (LZX/CAB style E8/E9 Call/Jump) pre-filter:
/// Converts relative 32-bit jump targets pointing inside `[0, file_size)` to absolute addresses.
/// Strictly bijective over non-overlapping 5-byte instruction windows.
pub fn bcj_x86_encode(buf: &mut [u8]) {
    if buf.len() < 5 || buf.len() > (i32::MAX as usize / 2) {
        return;
    }
    let file_size = buf.len() as i32;
    let limit = buf.len() - 4;
    let mut i = 0usize;
    while i < limit {
        let b = buf[i];
        if b == 0xE8 || b == 0xE9 {
            let pos = (i + 5) as i32;
            let val = i32::from_le_bytes([buf[i + 1], buf[i + 2], buf[i + 3], buf[i + 4]]);
            if val >= -pos && val < (file_size - pos) {
                let abs = val + pos;
                buf[i + 1..i + 5].copy_from_slice(&abs.to_le_bytes());
            } else if val >= (file_size - pos) && val < file_size {
                let shifted = val - file_size;
                buf[i + 1..i + 5].copy_from_slice(&shifted.to_le_bytes());
            }
            i += 5;
        } else {
            i += 1;
        }
    }
}

/// Inverse x86 BCJ filter: restores absolute addresses back to relative `0xE8`/`0xE9` offsets.
pub fn bcj_x86_decode(buf: &mut [u8]) {
    if buf.len() < 5 || buf.len() > (i32::MAX as usize / 2) {
        return;
    }
    let file_size = buf.len() as i32;
    let limit = buf.len() - 4;
    let mut i = 0usize;
    while i < limit {
        let b = buf[i];
        if b == 0xE8 || b == 0xE9 {
            let pos = (i + 5) as i32;
            let val = i32::from_le_bytes([buf[i + 1], buf[i + 2], buf[i + 3], buf[i + 4]]);
            if val >= 0 && val < file_size {
                let rel = val - pos;
                buf[i + 1..i + 5].copy_from_slice(&rel.to_le_bytes());
            } else if val >= -pos && val < 0 {
                let unshifted = val + file_size;
                buf[i + 1..i + 5].copy_from_slice(&unshifted.to_le_bytes());
            }
            i += 5;
        } else {
            i += 1;
        }
    }
}

pub(crate) fn lzma2_compress_bytes(data: &[u8], preset: u32) -> io::Result<(Vec<u8>, u32)> {
    let opts = LZMA2Options::with_preset(preset.clamp(0, 9));
    let dict_size = opts.dict_size;
    let mut out = Vec::new();
    {
        let mut writer = LZMA2Writer::new(CountingWriter::new(&mut out), &opts);
        writer.write_all(data)?;
        writer.finish()?;
    }
    Ok((out, dict_size))
}

pub(crate) fn lzma2_decompress_bytes(compressed: &[u8], dict_size: u32) -> io::Result<Vec<u8>> {
    let mut reader = LZMA2Reader::new(compressed, dict_size, None);
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
}

/// Inspect an executable file to check if it contains an attached NyanSetup payload (`NYANPK02`).
pub fn read_package_from_exe(exe_path: &Path) -> io::Result<Option<(NyanFooter, PackageManifest)>> {
    let mut file = File::open(exe_path)?;
    let file_len = file.metadata()?.len();
    if file_len < FOOTER_SIZE + 1024 {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;
    let mut footer_buf = [0u8; 40];
    file.read_exact(&mut footer_buf)?;

    let Some(footer) = NyanFooter::from_bytes(&footer_buf) else {
        return Ok(None);
    };

    if footer.manifest_offset + footer.manifest_size + FOOTER_SIZE > file_len
        || footer.stub_size == 0
        || footer.stub_size > footer.data_offset
    {
        return Ok(None);
    }

    file.seek(SeekFrom::Start(footer.manifest_offset))?;
    let mut compressed_manifest = vec![0u8; footer.manifest_size as usize];
    file.read_exact(&mut compressed_manifest)?;

    let manifest_bytes =
        lzma2_decompress_bytes(&compressed_manifest, LZMA2Options::DICT_SIZE_DEFAULT)?;
    let manifest: PackageManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(Some((footer, manifest)))
}

/// Extract the pure PE stub bytes from the currently running executable.
pub fn extract_pure_stub_bytes(exe_path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::open(exe_path)?;
    let file_len = file.metadata()?.len();

    let stub_len = if let Ok(Some((footer, _))) = read_package_from_exe(exe_path) {
        footer.stub_size
    } else {
        file_len
    };

    file.seek(SeekFrom::Start(0))?;
    let mut buf = vec![0u8; stub_len as usize];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Recursively collect all files inside `source_dir`, grouped by file extension for optimal Solid Compression ratio.
pub fn collect_source_files(source_dir: &Path) -> io::Result<Vec<(PathBuf, String, u64)>> {
    if !source_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("源目录不存在: {}", source_dir.display()),
        ));
    }

    fn visit_dirs(
        root: &Path,
        current: &Path,
        out: &mut Vec<(PathBuf, String, u64)>,
    ) -> io::Result<()> {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                visit_dirs(root, &path, out)?;
            } else if metadata.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                    .to_string_lossy()
                    .replace('/', "\\");
                out.push((path, rel, metadata.len()));
            }
        }
        Ok(())
    }

    let mut results = Vec::new();
    visit_dirs(source_dir, source_dir, &mut results)?;

    // Sort by extension first (so .exe/.dll group together in the solid LZMA2 dictionary), then by relative path
    results.sort_by(|a, b| {
        let ext_a = Path::new(&a.1)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let ext_b = Path::new(&b.1)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        ext_a.cmp(&ext_b).then_with(|| a.1.cmp(&b.1))
    });

    Ok(results)
}

/// Build a single-file self-extracting installer `.exe` using **Solid LZMA2 + x86 BCJ Pre-Filter**.
pub fn build_installer_package(
    stub_exe_path: &Path,
    source_dir: &Path,
    output_exe_path: &Path,
    config: &PackageConfig,
    progress_tx: Option<&Sender<BuildProgressEvent>>,
) -> Result<(usize, u64, u64), String> {
    let notify = |ev: BuildProgressEvent| {
        if let Some(tx) = progress_tx {
            let _ = tx.send(ev);
        }
    };

    let mut files =
        collect_source_files(source_dir).map_err(|e| format!("扫描源目录失败: {}", e))?;
    if files.is_empty() {
        return Err("源目录中没有可打包的文件，请检查源目录路径。".to_string());
    }

    // Resolve and append any bundled prerequisite executables
    let mut manifest_config = config.clone();
    for (rule_idx, rule) in manifest_config.prerequisites.iter_mut().enumerate() {
        if !rule.enabled {
            continue;
        }
        if let PrereqAction::RunBundledExe {
            ref source_file,
            ref mut bundled_rel_path,
            ..
        } = rule.action
        {
            let trimmed = source_file.trim();
            if trimmed.is_empty() {
                return Err(format!("前置规则 [{}] 未指定捆绑可执行文件路径", rule.name));
            }
            let cand = PathBuf::from(trimmed);
            let resolved = if cand.is_file() {
                cand
            } else if source_dir.join(&cand).is_file() {
                source_dir.join(&cand)
            } else {
                return Err(format!(
                    "前置规则 [{}] 指定的捆绑程序不存在: {}",
                    rule.name, trimmed
                ));
            };
            let meta = fs::metadata(&resolved).map_err(|e| {
                format!(
                    "读取前置规则 [{}] 捆绑程序 {} 失败: {}",
                    rule.name,
                    resolved.display(),
                    e
                )
            })?;
            let fname = resolved
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("prereq_{}.exe", rule_idx));
            let v_rel = format!("{}prereq_{}_{}", PREREQ_VIRTUAL_PREFIX, rule_idx, fname);
            *bundled_rel_path = v_rel.clone();
            files.push((resolved, v_rel, meta.len()));
        }
    }

    notify(BuildProgressEvent::Scanning(files.len()));

    if let Some(parent) = output_exe_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("无法创建输出目录: {}", e))?;
        }
    }

    let output_canonical = output_exe_path.canonicalize().ok();
    let mut stub_bytes = extract_pure_stub_bytes(stub_exe_path)
        .map_err(|e| format!("读取安装器 Stub 模板失败: {}", e))?;

    // If custom installer icon is specified, apply it to the PE stub resources
    if let Some(ref icon_str) = config.installer_icon {
        let trimmed = icon_str.trim();
        if !trimmed.is_empty() {
            let icon_path = PathBuf::from(trimmed);
            let resolved_path = if icon_path.is_file() {
                icon_path
            } else if source_dir.join(&icon_path).is_file() {
                source_dir.join(&icon_path)
            } else {
                return Err(format!("指定的安装包图标不存在: {}", trimmed));
            };
            let ico_data = fs::read(&resolved_path).map_err(|e| {
                format!("读取自定义图标文件 {} 失败: {}", resolved_path.display(), e)
            })?;
            stub_bytes = crate::icon::apply_icon_to_stub_bytes(&stub_bytes, &ico_data)
                .map_err(|e| format!("应用自定义安装包图标失败: {}", e))?;
        }
    }

    let stub_size = stub_bytes.len() as u64;
    let mut out_file = File::create(output_exe_path)
        .map_err(|e| format!("无法创建输出文件 {}: {}", output_exe_path.display(), e))?;
    out_file
        .write_all(&stub_bytes)
        .map_err(|e| format!("写入 Stub 头部失败: {}", e))?;

    let data_offset = stub_size;
    let opts = LZMA2Options::with_preset(config.compression_level.clamp(1, 9));
    let dict_size = opts.dict_size;

    let mut solid_compressed_data = Vec::new();
    let mut file_entries = Vec::with_capacity(files.len());
    let mut solid_offset = 0u64;
    let mut total_uncompressed_bytes = 0u64;

    {
        let mut solid_encoder =
            LZMA2Writer::new(CountingWriter::new(&mut solid_compressed_data), &opts);
        let total_files = files.len();

        for (idx, (full_path, rel_path, _size)) in files.into_iter().enumerate() {
            if let (Some(ref out_can), Ok(ref cur_can)) =
                (&output_canonical, full_path.canonicalize())
            {
                if out_can == cur_can {
                    continue;
                }
            }

            notify(BuildProgressEvent::Compressing {
                current_index: idx + 1,
                total_files,
                relative_path: rel_path.clone(),
            });

            let mut raw_data =
                fs::read(&full_path).map_err(|e| format!("读取文件 {} 失败: {}", rel_path, e))?;
            let uncompressed_size = raw_data.len() as u64;

            let mut hasher = Hasher::new();
            hasher.update(&raw_data);
            let crc32 = hasher.finalize();

            let bcj_filtered = should_apply_x86_bcj(&rel_path, &raw_data);
            if bcj_filtered {
                bcj_x86_encode(&mut raw_data);
            }

            solid_encoder
                .write_all(&raw_data)
                .map_err(|e| format!("LZMA2 固实压缩写入 {} 失败: {}", rel_path, e))?;

            file_entries.push(FileEntryMetadata {
                relative_path: rel_path,
                uncompressed_size,
                solid_offset,
                crc32,
                bcj_filtered,
            });

            solid_offset += uncompressed_size;
            total_uncompressed_bytes += uncompressed_size;
        }

        solid_encoder
            .finish()
            .map_err(|e| format!("完成 LZMA2 固实压缩流失败: {}", e))?;
    }

    let total_compressed_bytes = solid_compressed_data.len() as u64;
    out_file
        .write_all(&solid_compressed_data)
        .map_err(|e| format!("写入固实压缩数据块失败: {}", e))?;

    notify(BuildProgressEvent::Finalizing);

    let manifest = PackageManifest {
        config: manifest_config,
        files: file_entries,
        total_uncompressed_bytes,
        total_compressed_bytes,
        dict_size,
    };

    let manifest_json =
        serde_json::to_vec(&manifest).map_err(|e| format!("序列化安装包清单失败: {}", e))?;
    let (compressed_manifest, _) =
        lzma2_compress_bytes(&manifest_json, 6).map_err(|e| format!("LZMA2 压缩清单失败: {}", e))?;

    let manifest_offset = data_offset + total_compressed_bytes;
    let manifest_size = compressed_manifest.len() as u64;

    out_file
        .write_all(&compressed_manifest)
        .map_err(|e| format!("写入清单数据失败: {}", e))?;

    let footer = NyanFooter {
        stub_size,
        data_offset,
        manifest_offset,
        manifest_size,
    };
    out_file
        .write_all(&footer.to_bytes())
        .map_err(|e| format!("写入安装包尾标失败: {}", e))?;
    out_file
        .flush()
        .map_err(|e| format!("刷新文件缓冲失败: {}", e))?;

    let final_size = stub_size + total_compressed_bytes + manifest_size + FOOTER_SIZE;
    notify(BuildProgressEvent::Completed {
        output_path: output_exe_path.to_path_buf(),
        total_files: manifest.files.len(),
        uncompressed_bytes: total_uncompressed_bytes,
        final_installer_bytes: final_size,
    });

    Ok((manifest.files.len(), total_uncompressed_bytes, final_size))
}
