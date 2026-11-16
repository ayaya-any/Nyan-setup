use super::process::query_restart_manager_locking_services;
use super::shell::{expand_install_path, to_wide};
use super::uninstall::is_safe_application_folder;
use std::path::Path;
use std::time::{Duration, Instant};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HWND};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteKeyW, RegDeleteTreeW, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_DWORD, REG_EXPAND_SZ, REG_SZ,
    REG_VALUE_TYPE,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken, TerminateProcess,
    WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

const SC_MANAGER_CONNECT: u32 = 0x0001;
const SERVICE_QUERY_STATUS: u32 = 0x0004;
const SERVICE_START: u32 = 0x0010;
const SERVICE_STOP: u32 = 0x0020;
const DELETE_ACCESS: u32 = 0x0001_0000;

const SC_STATUS_PROCESS_INFO: u32 = 0;
const SERVICE_CONTROL_STOP: u32 = 0x0000_0001;

pub const SERVICE_STOPPED: u32 = 0x0000_0001;
pub const SERVICE_START_PENDING: u32 = 0x0000_0002;
pub const SERVICE_STOP_PENDING: u32 = 0x0000_0003;
pub const SERVICE_RUNNING: u32 = 0x0000_0004;
pub const SERVICE_CONTINUE_PENDING: u32 = 0x0000_0005;
pub const SERVICE_PAUSE_PENDING: u32 = 0x0000_0006;
pub const SERVICE_PAUSED: u32 = 0x0000_0007;

const ERROR_ACCESS_DENIED_CODE: u32 = 5;
const ERROR_SERVICE_DOES_NOT_EXIST_CODE: u32 = 1060;
const ERROR_SERVICE_NOT_ACTIVE_CODE: u32 = 1062;
const ERROR_SERVICE_MARKED_FOR_DELETE_CODE: u32 = 1072;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ServiceStatusProcess {
    dw_service_type: u32,
    dw_current_state: u32,
    dw_controls_accepted: u32,
    dw_win32_exit_code: u32,
    dw_service_specific_exit_code: u32,
    dw_check_point: u32,
    dw_wait_hint: u32,
    dw_process_id: u32,
    dw_service_flags: u32,
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenSCManagerW(
        lp_machine_name: *const u16,
        lp_database_name: *const u16,
        dw_desired_access: u32,
    ) -> isize;
    fn OpenServiceW(
        h_sc_manager: isize,
        lp_service_name: *const u16,
        dw_desired_access: u32,
    ) -> isize;
    fn QueryServiceStatusEx(
        h_service: isize,
        info_level: u32,
        lp_buffer: *mut u8,
        cb_buf_size: u32,
        pcb_bytes_needed: *mut u32,
    ) -> i32;
    fn ControlService(
        h_service: isize,
        dw_control: u32,
        lp_service_status: *mut ServiceStatusProcess,
    ) -> i32;
    fn StartServiceW(
        h_service: isize,
        dw_num_service_args: u32,
        lp_service_arg_vectors: *const *const u16,
    ) -> i32;
    fn DeleteService(h_service: isize) -> i32;
    fn CloseServiceHandle(h_sc_object: isize) -> i32;
}

/// Check whether the current process is running with elevated Administrator privileges.
pub fn is_current_process_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

fn read_reg_string(hkey: HKEY, val_name: &str) -> Option<String> {
    let name_w = to_wide(val_name);
    let mut val_type = REG_VALUE_TYPE::default();
    let mut buf = vec![0u8; 2048];
    let mut byte_len = buf.len() as u32;
    unsafe {
        if RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut val_type),
            Some(buf.as_mut_ptr()),
            Some(&mut byte_len),
        )
        .is_ok()
            && (val_type == REG_SZ || val_type == REG_EXPAND_SZ)
            && byte_len >= 2
        {
            let u16_len = (byte_len as usize) / 2;
            let u16_slice = std::slice::from_raw_parts(buf.as_ptr() as *const u16, u16_len);
            let end = u16_slice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(u16_slice.len());
            let s = String::from_utf16_lossy(&u16_slice[..end]).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn read_reg_dword(hkey: HKEY, val_name: &str) -> Option<u32> {
    let name_w = to_wide(val_name);
    let mut val_type = REG_VALUE_TYPE::default();
    let mut buf = [0u8; 4];
    let mut byte_len = 4u32;
    unsafe {
        if RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut val_type),
            Some(buf.as_mut_ptr()),
            Some(&mut byte_len),
        )
        .is_ok()
            && val_type == REG_DWORD
            && byte_len == 4
        {
            return Some(u32::from_le_bytes(buf));
        }
    }
    None
}

/// Check if a service's `ImagePath` (or wrapper parameter path) points inside `install_dir`.
pub(crate) fn image_path_matches_install_dir(
    raw_image_path: &str,
    dir_norm: &str,
    dir_prefix: &str,
) -> bool {
    if raw_image_path.trim().is_empty() || dir_norm.is_empty() {
        return false;
    }

    let expanded = expand_install_path(raw_image_path.trim())
        .to_string_lossy()
        .to_string();
    let norm = expanded
        .replace('/', "\\")
        .trim()
        .trim_start_matches("\\??\\")
        .to_ascii_lowercase();

    if norm.is_empty() {
        return false;
    }

    // 1. Quoted executable path at start: `"C:\Path\To\App\svc.exe" --args`
    if let Some(rest) = norm.strip_prefix('"') {
        if let Some(end_quote) = rest.find('"') {
            let exe_part = rest[..end_quote].trim_end_matches('\\');
            if exe_part == dir_norm || exe_part.starts_with(dir_prefix) {
                return true;
            }
        }
    }

    // 2. Unquoted executable path at start: `C:\Path\To\App\svc.exe --args`
    if norm == dir_norm || norm.starts_with(dir_prefix) {
        return true;
    }

    // 3. Wrapper service (e.g. nssm.exe / cmd.exe / srvany) passing target inside `install_dir` as argument
    let quoted_prefix = format!("\"{}", dir_prefix);
    let spaced_prefix = format!(" {}", dir_prefix);
    if norm.contains(&quoted_prefix) || norm.contains(&spaced_prefix) {
        return true;
    }

    false
}

/// Scan a registry `Services` key (`root\services_base_key`) for Win32 services whose `ImagePath`
/// or `Parameters\Application` (`NSSM` / `srvany`) resides inside `install_dir`.
pub(crate) fn scan_services_in_registry_key(
    root: HKEY,
    services_base_key: &str,
    install_dir: &Path,
) -> Vec<String> {
    let mut matched_services = Vec::new();
    if !is_safe_application_folder(install_dir) {
        return matched_services;
    }

    let dir_norm = install_dir
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    if dir_norm.is_empty() {
        return matched_services;
    }
    let dir_prefix = format!("{}\\", dir_norm);

    let base_w = to_wide(services_base_key);
    let mut h_base = HKEY::default();
    if unsafe { RegOpenKeyExW(root, PCWSTR(base_w.as_ptr()), 0, KEY_READ, &mut h_base) }.is_err() {
        return matched_services;
    }

    let mut svc_names = Vec::new();
    let mut idx = 0u32;
    loop {
        let mut name_buf = vec![0u16; 256];
        let mut name_len = name_buf.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                h_base,
                idx,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                PWSTR::null(),
                None,
                None,
            )
        };
        if status.is_err() {
            break;
        }
        svc_names.push(String::from_utf16_lossy(&name_buf[..name_len as usize]));
        idx += 1;
    }

    for svc_name in svc_names {
        let sub_w = to_wide(&svc_name);
        let mut h_svc = HKEY::default();
        if unsafe { RegOpenKeyExW(h_base, PCWSTR(sub_w.as_ptr()), 0, KEY_READ, &mut h_svc) }.is_ok()
        {
            // Check service `Type`: skip pure kernel/filesystem drivers (0x01, 0x02, 0x04, 0x08)
            // while allowing Win32 services (0x10 own, 0x20 share, 0x50/0x60 user services, etc.)
            let is_win32_service = match read_reg_dword(h_svc, "Type") {
                Some(t) => (t & 0xF0) != 0,
                None => true,
            };

            let mut matched = false;
            if is_win32_service {
                if let Some(img_path) = read_reg_string(h_svc, "ImagePath") {
                    if image_path_matches_install_dir(&img_path, &dir_norm, &dir_prefix) {
                        matched = true;
                    }
                }

                // Also check `Parameters` subkey for NSSM / srvany (`Application`, `AppDirectory`, `AppParameters`)
                if !matched {
                    let param_w = to_wide("Parameters");
                    let mut h_param = HKEY::default();
                    if unsafe {
                        RegOpenKeyExW(h_svc, PCWSTR(param_w.as_ptr()), 0, KEY_READ, &mut h_param)
                    }
                    .is_ok()
                    {
                        for val_key in ["Application", "AppDirectory", "AppParameters"] {
                            if let Some(val_str) = read_reg_string(h_param, val_key) {
                                if image_path_matches_install_dir(&val_str, &dir_norm, &dir_prefix)
                                {
                                    matched = true;
                                    break;
                                }
                            }
                        }
                        unsafe {
                            let _ = RegCloseKey(h_param);
                        }
                    }
                }
            }

            unsafe {
                let _ = RegCloseKey(h_svc);
            }

            if matched {
                matched_services.push(svc_name);
            }
        }
    }

    unsafe {
        let _ = RegCloseKey(h_base);
    }

    matched_services
}

/// Automatically scan `HKLM\SYSTEM\CurrentControlSet\Services` for any Windows services
/// whose binary path or service wrapper target resides inside `install_dir`.
pub fn find_services_in_install_dir(install_dir: &Path) -> Vec<String> {
    scan_services_in_registry_key(
        HKEY_LOCAL_MACHINE,
        "SYSTEM\\CurrentControlSet\\Services",
        install_dir,
    )
}

/// Discover all Windows services associated with the application in `install_dir`:
/// 1. Explicitly configured service names (`explicit_services`)
/// 2. Services in `HKLM\SYSTEM\CurrentControlSet\Services` whose `ImagePath` / `Parameters` points inside `install_dir`
/// 3. Running services detected by Windows Restart Manager (`RmService`) holding locks on files in `install_dir`
pub fn discover_application_services(
    install_dir: &Path,
    candidate_files: &[String],
    explicit_services: &[String],
) -> Vec<String> {
    let mut results: Vec<String> = Vec::new();
    let mut push_unique = |name: String| {
        let trimmed = name.trim();
        if !trimmed.is_empty()
            && !results
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            results.push(trimmed.to_string());
        }
    };

    for s in explicit_services {
        push_unique(s.clone());
    }

    for s in find_services_in_install_dir(install_dir) {
        push_unique(s);
    }

    for s in query_restart_manager_locking_services(install_dir, candidate_files) {
        push_unique(s);
    }

    results
}

/// Query `(dw_current_state, dw_process_id)` of a Windows service via SCM.
/// Returns `None` if the service does not exist or cannot be queried.
pub fn query_service_state(service_name: &str) -> Option<(u32, u32)> {
    let svc_w = to_wide(service_name);
    unsafe {
        let h_scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if h_scm == 0 {
            return None;
        }
        let h_svc = OpenServiceW(h_scm, PCWSTR(svc_w.as_ptr()).0, SERVICE_QUERY_STATUS);
        if h_svc == 0 {
            let _ = CloseServiceHandle(h_scm);
            return None;
        }

        let mut status = ServiceStatusProcess::default();
        let mut bytes_needed = 0u32;
        let ok = QueryServiceStatusEx(
            h_svc,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as *mut u8,
            std::mem::size_of::<ServiceStatusProcess>() as u32,
            &mut bytes_needed,
        ) != 0;

        let _ = CloseServiceHandle(h_svc);
        let _ = CloseServiceHandle(h_scm);

        if ok {
            Some((status.dw_current_state, status.dw_process_id))
        } else {
            None
        }
    }
}

/// Check if a service exists in SCM or under `HKLM\SYSTEM\CurrentControlSet\Services`.
pub fn service_exists(service_name: &str) -> bool {
    if query_service_state(service_name).is_some() {
        return true;
    }
    let key_path = format!("SYSTEM\\CurrentControlSet\\Services\\{}", service_name);
    let key_w = to_wide(&key_path);
    let mut hkey = HKEY::default();
    let exists = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key_w.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
        .is_ok()
    };
    if exists {
        unsafe {
            let _ = RegCloseKey(hkey);
        }
    }
    exists
}

fn force_kill_pid(pid: u32) {
    if pid == 0 || pid == 4 {
        return;
    }
    unsafe {
        let access = PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
        if let Ok(h_proc) =
            OpenProcess(access, false, pid).or_else(|_| OpenProcess(PROCESS_TERMINATE, false, pid))
        {
            if TerminateProcess(h_proc, 0).is_ok() {
                let _ = WaitForSingleObject(h_proc, 2000);
            }
            let _ = CloseHandle(h_proc);
        }
    }
}

/// Stop a Windows service in-process via SCM and wait until `SERVICE_STOPPED`.
/// Returns `Ok(true)` if the service was active and has now stopped, `Ok(false)` if it was already stopped or does not exist,
/// or `Err(win32_error_code)` if SCM access was denied.
pub fn stop_service_in_process(service_name: &str, timeout_ms: u32) -> Result<bool, u32> {
    let svc_w = to_wide(service_name);
    unsafe {
        let h_scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if h_scm == 0 {
            return Err(GetLastError().0);
        }

        let h_svc = OpenServiceW(
            h_scm,
            PCWSTR(svc_w.as_ptr()).0,
            SERVICE_STOP | SERVICE_QUERY_STATUS,
        );
        if h_svc == 0 {
            let err = GetLastError().0;
            let _ = CloseServiceHandle(h_scm);
            if err == ERROR_SERVICE_DOES_NOT_EXIST_CODE {
                return Ok(false);
            }
            return Err(err);
        }

        let mut status = ServiceStatusProcess::default();
        let mut bytes_needed = 0u32;
        if QueryServiceStatusEx(
            h_svc,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as *mut u8,
            std::mem::size_of::<ServiceStatusProcess>() as u32,
            &mut bytes_needed,
        ) != 0
            && status.dw_current_state == SERVICE_STOPPED
        {
            let _ = CloseServiceHandle(h_svc);
            let _ = CloseServiceHandle(h_scm);
            return Ok(false);
        }

        let initial_pid = status.dw_process_id;
        let mut ctrl_status = ServiceStatusProcess::default();
        if ControlService(h_svc, SERVICE_CONTROL_STOP, &mut ctrl_status) == 0 {
            let err = GetLastError().0;
            if err == ERROR_SERVICE_NOT_ACTIVE_CODE {
                let _ = CloseServiceHandle(h_svc);
                let _ = CloseServiceHandle(h_scm);
                return Ok(false);
            }
            if status.dw_current_state != SERVICE_STOP_PENDING {
                let _ = CloseServiceHandle(h_svc);
                let _ = CloseServiceHandle(h_scm);
                return Err(err);
            }
        }

        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms as u64);
        let mut stopped = false;
        let mut last_pid = initial_pid;

        while start.elapsed() < timeout {
            let mut cur = ServiceStatusProcess::default();
            let mut needed = 0u32;
            if QueryServiceStatusEx(
                h_svc,
                SC_STATUS_PROCESS_INFO,
                &mut cur as *mut _ as *mut u8,
                std::mem::size_of::<ServiceStatusProcess>() as u32,
                &mut needed,
            ) == 0
            {
                break;
            }
            if cur.dw_process_id != 0 {
                last_pid = cur.dw_process_id;
            }
            if cur.dw_current_state == SERVICE_STOPPED {
                stopped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(120));
        }

        if !stopped && last_pid != 0 {
            // Fallback: if the service process hung during shutdown, terminate its process so file locks release
            force_kill_pid(last_pid);
        }

        let _ = CloseServiceHandle(h_svc);
        let _ = CloseServiceHandle(h_scm);
        std::thread::sleep(Duration::from_millis(100));
        Ok(true)
    }
}

/// Start a Windows service in-process via SCM and wait until it leaves `SERVICE_START_PENDING`.
pub fn start_service_in_process(service_name: &str, timeout_ms: u32) -> Result<(), u32> {
    let svc_w = to_wide(service_name);
    unsafe {
        let h_scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if h_scm == 0 {
            return Err(GetLastError().0);
        }

        let h_svc = OpenServiceW(
            h_scm,
            PCWSTR(svc_w.as_ptr()).0,
            SERVICE_START | SERVICE_QUERY_STATUS,
        );
        if h_svc == 0 {
            let err = GetLastError().0;
            let _ = CloseServiceHandle(h_scm);
            return Err(err);
        }

        let mut status = ServiceStatusProcess::default();
        let mut bytes_needed = 0u32;
        if QueryServiceStatusEx(
            h_svc,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as *mut u8,
            std::mem::size_of::<ServiceStatusProcess>() as u32,
            &mut bytes_needed,
        ) != 0
            && status.dw_current_state == SERVICE_RUNNING
        {
            let _ = CloseServiceHandle(h_svc);
            let _ = CloseServiceHandle(h_scm);
            return Ok(());
        }

        if StartServiceW(h_svc, 0, std::ptr::null()) == 0 {
            let err = GetLastError().0;
            let _ = CloseServiceHandle(h_svc);
            let _ = CloseServiceHandle(h_scm);
            return Err(err);
        }

        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms as u64);
        while start.elapsed() < timeout {
            let mut cur = ServiceStatusProcess::default();
            let mut needed = 0u32;
            if QueryServiceStatusEx(
                h_svc,
                SC_STATUS_PROCESS_INFO,
                &mut cur as *mut _ as *mut u8,
                std::mem::size_of::<ServiceStatusProcess>() as u32,
                &mut needed,
            ) == 0
            {
                break;
            }
            if cur.dw_current_state == SERVICE_RUNNING
                || cur.dw_current_state != SERVICE_START_PENDING
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(120));
        }

        let _ = CloseServiceHandle(h_svc);
        let _ = CloseServiceHandle(h_scm);
        Ok(())
    }
}

/// Stop and delete a Windows service in-process via SCM (`ControlService(STOP)` -> wait -> `DeleteService`),
/// and clean its registry key under `HKLM\SYSTEM\CurrentControlSet\Services\<service_name>` if lingering.
pub fn delete_service_in_process(service_name: &str, timeout_ms: u32) -> Result<(), u32> {
    let _ = stop_service_in_process(service_name, timeout_ms);

    let svc_w = to_wide(service_name);
    unsafe {
        let h_scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if h_scm == 0 {
            return Err(GetLastError().0);
        }

        let h_svc = OpenServiceW(h_scm, PCWSTR(svc_w.as_ptr()).0, DELETE_ACCESS);
        if h_svc == 0 {
            let err = GetLastError().0;
            let _ = CloseServiceHandle(h_scm);
            if err == ERROR_SERVICE_DOES_NOT_EXIST_CODE {
                return Ok(());
            }
            return Err(err);
        }

        let del_ok = DeleteService(h_svc) != 0;
        let err = if del_ok { 0 } else { GetLastError().0 };

        let _ = CloseServiceHandle(h_svc);
        let _ = CloseServiceHandle(h_scm);

        if !del_ok && err != ERROR_SERVICE_MARKED_FOR_DELETE_CODE {
            return Err(err);
        }

        // Also attempt to remove the service registry tree if all handles are closed
        let reg_path = format!("SYSTEM\\CurrentControlSet\\Services\\{}", service_name);
        let reg_w = to_wide(&reg_path);
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(reg_w.as_ptr()),
            0,
            KEY_READ | KEY_WRITE,
            &mut hkey,
        )
        .is_ok()
        {
            let _ = RegDeleteTreeW(hkey, PCWSTR::null());
            let _ = RegCloseKey(hkey);
            let _ = RegDeleteKeyW(HKEY_LOCAL_MACHINE, PCWSTR(reg_w.as_ptr()));
        }

        Ok(())
    }
}

/// Launch the current executable with UAC elevation (`runas`) in hidden mode to perform `--service-op <stop|start|delete> <services...>`.
fn run_elevated_service_helper(op: &str, services: &[String]) -> Result<(), String> {
    if services.is_empty() {
        return Ok(());
    }
    let cur_exe = std::env::current_exe().map_err(|e| format!("无法获取当前程序路径: {}", e))?;
    let mut args_parts = vec![format!("--service-op {}", op)];
    for s in services {
        args_parts.push(format!("\"{}\"", s.replace('"', "")));
    }
    let params_str = args_parts.join(" ");

    let verb_w = to_wide("runas");
    let file_w = to_wide(&cur_exe.to_string_lossy());
    let params_w = to_wide(&params_str);

    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            hwnd: HWND::default(),
            lpVerb: PCWSTR(verb_w.as_ptr()),
            lpFile: PCWSTR(file_w.as_ptr()),
            lpParameters: PCWSTR(params_w.as_ptr()),
            lpDirectory: PCWSTR::null(),
            nShow: SW_HIDE.0,
            ..Default::default()
        };

        if ShellExecuteExW(&mut sei).is_err() {
            return Err("执行系统服务管理操作需要管理员权限（UAC 提权被取消）".to_string());
        }

        if !sei.hProcess.is_invalid() && !sei.hProcess.0.is_null() {
            let _ = WaitForSingleObject(sei.hProcess, INFINITE);
            let mut exit_code = 0u32;
            let _ = GetExitCodeProcess(sei.hProcess, &mut exit_code);
            let _ = CloseHandle(sei.hProcess);
            if exit_code != 0 {
                return Err(format!("提权服务助手进程返回错误码: {}", exit_code));
            }
        }
    }
    Ok(())
}

/// Handle `--service-op <stop|start|delete> <svc1> [svc2 ...]` invoked by the elevated helper.
pub fn run_cli_service_op(args: &[String]) -> i32 {
    let Some(op) = args.first().map(|s| s.as_str()) else {
        return 1;
    };
    let services = &args[1..];
    let mut exit_code = 0;

    for svc in services {
        let res = match op {
            "stop" => stop_service_in_process(svc, 15_000).map(|_| ()),
            "start" => start_service_in_process(svc, 10_000),
            "delete" => delete_service_in_process(svc, 15_000),
            _ => Err(1),
        };
        if res.is_err() {
            exit_code = 1;
        }
    }
    exit_code
}

/// Before installing/updating: discover all services belonging to `install_dir` (or `explicit_services`),
/// stop any that are currently running, and return `(services_to_restart, log_messages)`.
pub fn stop_services_for_update(
    install_dir: &Path,
    candidate_files: &[String],
    explicit_services: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut services_to_restart = Vec::new();
    let mut logs = Vec::new();

    if !install_dir.exists() && explicit_services.is_empty() {
        return (services_to_restart, logs);
    }

    let discovered = discover_application_services(install_dir, candidate_files, explicit_services);
    if discovered.is_empty() {
        return (services_to_restart, logs);
    }

    let mut active_services = Vec::new();
    for svc in discovered {
        if let Some((state, _pid)) = query_service_state(&svc) {
            if state != SERVICE_STOPPED {
                active_services.push(svc);
            }
        }
    }

    if active_services.is_empty() {
        return (services_to_restart, logs);
    }

    let mut need_elevated = Vec::new();
    for svc in &active_services {
        match stop_service_in_process(svc, 15_000) {
            Ok(true) => {
                services_to_restart.push(svc.clone());
                logs.push(format!("已自动停止运行中的系统服务: {}", svc));
            }
            Ok(false) => {}
            Err(err) if err == ERROR_ACCESS_DENIED_CODE && !is_current_process_elevated() => {
                need_elevated.push(svc.clone());
            }
            Err(err) => {
                logs.push(format!("停止系统服务 {} 失败 (错误码: {})", svc, err));
            }
        }
    }

    if !need_elevated.is_empty() {
        match run_elevated_service_helper("stop", &need_elevated) {
            Ok(()) => {
                for svc in need_elevated {
                    services_to_restart.push(svc.clone());
                    logs.push(format!("已通过管理员权限停止系统服务: {}", svc));
                }
            }
            Err(e) => {
                logs.push(format!("提权停止系统服务未完成: {}", e));
            }
        }
    }

    (services_to_restart, logs)
}

/// After updating files: restart all services that were running prior to the update.
pub fn restart_services_after_update(services_to_restart: &[String]) -> Vec<String> {
    let mut logs = Vec::new();
    if services_to_restart.is_empty() {
        return logs;
    }

    let mut need_elevated = Vec::new();
    for svc in services_to_restart {
        match start_service_in_process(svc, 10_000) {
            Ok(()) => {
                logs.push(format!("已恢复启动系统服务: {}", svc));
            }
            Err(err) if err == ERROR_ACCESS_DENIED_CODE && !is_current_process_elevated() => {
                need_elevated.push(svc.clone());
            }
            Err(err) => {
                logs.push(format!("重新启动系统服务 {} 失败 (错误码: {})", svc, err));
            }
        }
    }

    if !need_elevated.is_empty() {
        match run_elevated_service_helper("start", &need_elevated) {
            Ok(()) => {
                for svc in need_elevated {
                    logs.push(format!("已通过管理员权限恢复启动系统服务: {}", svc));
                }
            }
            Err(e) => {
                logs.push(format!("提权启动系统服务未完成: {}", e));
            }
        }
    }

    logs
}

/// During uninstallation: discover all services belonging to `install_dir` (plus `explicit_services`),
/// stop each one, wait for `SERVICE_STOPPED`, and delete it from SCM.
pub fn stop_and_delete_application_services(
    install_dir: &Path,
    candidate_files: &[String],
    explicit_services: &[String],
) -> Vec<String> {
    let mut logs = Vec::new();
    let discovered = discover_application_services(install_dir, candidate_files, explicit_services);
    if discovered.is_empty() {
        return logs;
    }

    let existing: Vec<String> = discovered
        .into_iter()
        .filter(|svc| service_exists(svc))
        .collect();
    if existing.is_empty() {
        return logs;
    }

    let mut need_elevated = Vec::new();
    for svc in &existing {
        match delete_service_in_process(svc, 15_000) {
            Ok(()) => {
                logs.push(format!("已停止并删除系统服务: {}", svc));
            }
            Err(err) if err == ERROR_ACCESS_DENIED_CODE && !is_current_process_elevated() => {
                need_elevated.push(svc.clone());
            }
            Err(err) => {
                logs.push(format!("删除系统服务 {} 失败 (错误码: {})", svc, err));
            }
        }
    }

    if !need_elevated.is_empty() {
        match run_elevated_service_helper("delete", &need_elevated) {
            Ok(()) => {
                for svc in need_elevated {
                    logs.push(format!("已通过管理员权限停止并删除系统服务: {}", svc));
                }
            }
            Err(e) => {
                logs.push(format!("提权删除系统服务未完成: {}", e));
            }
        }
    }

    logs
}
