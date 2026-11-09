use super::shell::to_wide;
use crate::package::{UNINSTALL_DATA_FILE, UNINSTALL_EXE_FILE};
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RmUniqueProcess {
    dw_process_id: u32,
    process_start_time: windows::Win32::Foundation::FILETIME,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RmProcessInfo {
    process: RmUniqueProcess,
    str_app_name: [u16; 256],
    str_service_short_name: [u16; 64],
    application_type: i32,
    app_status: u32,
    ts_session_id: u32,
    b_restartable: i32,
}

impl Default for RmProcessInfo {
    fn default() -> Self {
        Self {
            process: RmUniqueProcess::default(),
            str_app_name: [0u16; 256],
            str_service_short_name: [0u16; 64],
            application_type: 0,
            app_status: 0,
            ts_session_id: 0,
            b_restartable: 0,
        }
    }
}

#[link(name = "rstrtmgr")]
extern "system" {
    fn RmStartSession(
        p_session_handle: *mut u32,
        dw_session_flags: u32,
        str_session_key: *mut u16,
    ) -> u32;
    fn RmRegisterResources(
        dw_session_handle: u32,
        n_files: u32,
        rgs_file_names: *const *const u16,
        n_applications: u32,
        rg_applications: *const RmUniqueProcess,
        n_services: u32,
        rgs_service_names: *const *const u16,
    ) -> u32;
    fn RmGetList(
        dw_session_handle: u32,
        pn_proc_info_needed: *mut u32,
        pn_proc_info: *mut u32,
        rg_affected_apps: *mut RmProcessInfo,
        lpdw_reboot_reasons: *mut u32,
    ) -> u32;
    fn RmEndSession(dw_session_handle: u32) -> u32;
}

/// Probe whether an existing file on disk is currently locked against overwriting
/// (e.g., mapped as an active `.exe`/`.dll` image section or held open without write sharing).
pub fn is_file_locked(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;

    let meta = match fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        _ => return false,
    };

    // Open with GENERIC_WRITE and FILE_SHARE_READ (1) only:
    // - Fails if any process mapped the .exe/.dll as an image section (ERROR_SHARING_VIOLATION 32 or ERROR_ACCESS_DENIED 5)
    // - Fails if any process holds a read handle without FILE_SHARE_WRITE, or any write handle
    match fs::OpenOptions::new()
        .write(true)
        .share_mode(1)
        .open(path)
    {
        Ok(_file) => false,
        Err(e) => match e.raw_os_error() {
            // 32 = ERROR_SHARING_VIOLATION, 33 = ERROR_LOCK_VIOLATION, 1224 = ERROR_USER_MAPPED_FILE
            Some(32) | Some(33) | Some(1224) => true,
            // 5 = ERROR_ACCESS_DENIED: when file is not marked read-only, kernel MmFlushImageSection denies write on mapped DLL/EXE
            Some(5) => !meta.permissions().readonly(),
            _ => false,
        },
    }
}

/// Collect all existing files inside `target_install_dir` (from `candidate_files` and directory scan)
/// that are currently locked by a process.
fn collect_locked_files_in_dir(
    target_install_dir: &Path,
    candidate_files: &[String],
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if !target_install_dir.exists() {
        return candidates;
    }

    for rel in candidate_files {
        let clean: PathBuf = rel
            .split(['\\', '/'])
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .collect();
        if clean.as_os_str().is_empty() {
            continue;
        }
        let full = target_install_dir.join(clean);
        if full.is_file() && !candidates.contains(&full) {
            candidates.push(full);
        }
    }

    // Also scan existing files inside `target_install_dir` (e.g. .dll, .exe, .sys, .ocx, .pyd)
    let mut stack = vec![target_install_dir.to_path_buf()];
    let mut scanned = 0usize;
    while let Some(dir) = stack.pop() {
        if scanned >= 512 {
            break;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.is_file() {
                    scanned += 1;
                    let fname_lower = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    if fname_lower == UNINSTALL_EXE_FILE || fname_lower == UNINSTALL_DATA_FILE {
                        continue;
                    }
                    if !candidates.contains(&p) {
                        candidates.push(p);
                    }
                    if scanned >= 512 {
                        break;
                    }
                }
            }
        }
    }

    candidates
        .into_iter()
        .filter(|p| is_file_locked(p))
        .collect()
}

fn query_process_full_path(pid: u32) -> Option<String> {
    unsafe {
        let h_proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path_buf = vec![0u16; 1024];
        let mut sz = path_buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h_proc,
            PROCESS_NAME_WIN32,
            PWSTR(path_buf.as_mut_ptr()),
            &mut sz,
        )
        .is_ok();
        let _ = CloseHandle(h_proc);
        if ok && sz > 0 {
            Some(String::from_utf16_lossy(&path_buf[..sz as usize]))
        } else {
            None
        }
    }
}

/// Use Windows Restart Manager (`Rstrtmgr.dll`) to identify all processes holding locks or image mappings
/// on `locked_files` (including external processes outside `target_install_dir` that loaded a `.dll`).
pub(crate) fn query_restart_manager_locking_processes(
    locked_files: &[PathBuf],
    self_pid: u32,
) -> Vec<(u32, String)> {
    let mut results = Vec::new();
    if locked_files.is_empty() {
        return results;
    }

    let wide_paths: Vec<Vec<u16>> = locked_files
        .iter()
        .map(|p| to_wide(&p.to_string_lossy()))
        .collect();
    let ptrs: Vec<*const u16> = wide_paths.iter().map(|w| w.as_ptr()).collect();

    unsafe {
        let mut session: u32 = 0;
        let mut session_key = [0u16; 64];
        if RmStartSession(&mut session, 0, session_key.as_mut_ptr()) != 0 {
            return results;
        }

        if RmRegisterResources(
            session,
            ptrs.len() as u32,
            ptrs.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        ) == 0
        {
            let mut needed: u32 = 0;
            let mut count: u32 = 16;
            let mut reboot_reasons: u32 = 0;
            let mut infos = vec![RmProcessInfo::default(); count as usize];

            let mut status = RmGetList(
                session,
                &mut needed,
                &mut count,
                infos.as_mut_ptr(),
                &mut reboot_reasons,
            );
            // 234 == ERROR_MORE_DATA
            if status == 234 && needed > count {
                count = needed;
                infos.resize(count as usize, RmProcessInfo::default());
                status = RmGetList(
                    session,
                    &mut needed,
                    &mut count,
                    infos.as_mut_ptr(),
                    &mut reboot_reasons,
                );
            }

            if status == 0 {
                for info in infos.iter().take(count as usize) {
                    let pid = info.process.dw_process_id;
                    if pid == 0 || pid == 4 || pid == self_pid {
                        continue;
                    }
                    let exe_name = if let Some(full_path) = query_process_full_path(pid) {
                        Path::new(&full_path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or(full_path)
                    } else {
                        let end = info
                            .str_app_name
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(info.str_app_name.len());
                        let app_str = String::from_utf16_lossy(&info.str_app_name[..end]);
                        if app_str.is_empty() {
                            format!("PID_{}", pid)
                        } else {
                            app_str
                        }
                    };
                    if !results.iter().any(|(p, _)| *p == pid) {
                        results.push((pid, exe_name));
                    }
                }
            }
        }

        let _ = RmEndSession(session);
    }

    results
}

/// Find running processes that actually occupy `target_install_dir`:
/// 1. If `target_install_dir` does not exist yet (fresh install), returns empty immediately (zero false positives).
/// 2. Probes existing target files in `target_install_dir` for write/mapping locks and queries Windows Restart Manager (`Rstrtmgr.dll`)
///    to catch any process (including external processes loading `.dll`s from `target_install_dir`).
/// 3. Checks running processes via `QueryFullProcessImageNameW` whose own executable path is strictly inside `target_install_dir\`.
/// Never returns the current installer/uninstaller process (`GetCurrentProcessId`).
pub fn find_running_target_processes(
    target_install_dir: &Path,
    candidate_files: &[String],
) -> Vec<(u32, String)> {
    let mut found = Vec::new();
    if !target_install_dir.exists() {
        return found;
    }

    let self_pid = unsafe { GetCurrentProcessId() };

    let target_dir_norm = target_install_dir
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    if target_dir_norm.is_empty() {
        return found;
    }
    let target_dir_prefix = format!("{}\\", target_dir_norm);

    // 1. Probe actual locked files in `target_install_dir` and query Windows Restart Manager (`Rstrtmgr.dll`)
    let locked_files = collect_locked_files_in_dir(target_install_dir, candidate_files);
    for (pid, name) in query_restart_manager_locking_processes(&locked_files, self_pid) {
        if !found.iter().any(|(p, _)| *p == pid) {
            found.push((pid, name));
        }
    }

    let locked_exe_basenames: Vec<String> = locked_files
        .iter()
        .filter_map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
        })
        .filter(|n| n.ends_with(".exe") && n != UNINSTALL_EXE_FILE)
        .collect();

    // 2. Enumerate process snapshot for processes executing strictly from inside `target_install_dir\`
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return found,
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                if pid != 0 && pid != 4 && pid != self_pid && !found.iter().any(|(p, _)| *p == pid)
                {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let exe_name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                    let exe_lower = exe_name.to_ascii_lowercase();

                    if exe_lower != UNINSTALL_EXE_FILE {
                        let matched = if let Some(full_path) = query_process_full_path(pid) {
                            let full_norm = full_path.replace('/', "\\").to_ascii_lowercase();
                            full_norm == target_dir_norm || full_norm.starts_with(&target_dir_prefix)
                        } else {
                            // Only fall back to basename match if we cannot query the process path (e.g. elevated process)
                            // AND the corresponding .exe inside `target_install_dir` is confirmed locked on disk.
                            locked_exe_basenames.iter().any(|w| *w == exe_lower)
                        };

                        if matched {
                            found.push((pid, exe_name));
                        }
                    }
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    found
}

/// Terminate all running target processes found via `find_running_target_processes` and wait for OS file locks to release.
pub fn terminate_running_target_processes(
    target_install_dir: &Path,
    exe_names: &[String],
) -> Vec<String> {
    let targets = find_running_target_processes(target_install_dir, exe_names);
    let mut logs = Vec::new();
    let mut killed_any = false;

    for (pid, exe_name) in targets {
        unsafe {
            let access = PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
            let h_res = OpenProcess(access, false, pid)
                .or_else(|_| OpenProcess(PROCESS_TERMINATE, false, pid));
            if let Ok(h_proc) = h_res {
                if TerminateProcess(h_proc, 0).is_ok() {
                    let _ = WaitForSingleObject(h_proc, 1500);
                    logs.push(format!(
                        "已自动结束运行中的进程: {} (PID: {})",
                        exe_name, pid
                    ));
                    killed_any = true;
                } else {
                    logs.push(format!(
                        "尝试结束进程失败 (权限不足): {} (PID: {})",
                        exe_name, pid
                    ));
                }
                let _ = CloseHandle(h_proc);
            }
        }
    }

    if killed_any {
        // Brief pause so the Windows kernel finishes releasing section handles on the .exe/.dll files
        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    logs
}
