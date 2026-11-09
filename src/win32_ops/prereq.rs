use super::registry_clean::bytes_to_u16_vec;
use super::shell::{shell_open, to_wide};
use crate::package::{CheckOperator, PrereqCondition, RegArchView, RegHiveRoot};
use std::env;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_DWORD,
    REG_EXPAND_SZ, REG_QWORD, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONWARNING, MB_OK, MB_YESNO, SW_SHOWNORMAL,
};

/// Expand environment variables (`%SystemRoot%`, `%LOCALAPPDATA%`, etc.) and `%INSTALL_DIR%`.
pub fn expand_env_and_install_vars(raw: &str, install_dir: Option<&Path>) -> String {
    let mut result = raw.to_string();
    if let Some(dir) = install_dir {
        let dir_str = dir.to_string_lossy();
        loop {
            let upper = result.to_uppercase();
            if let Some(pos) = upper.find("%INSTALL_DIR%") {
                result.replace_range(pos..pos + "%INSTALL_DIR%".len(), &dir_str);
            } else {
                break;
            }
        }
    }

    let known_vars = [
        "SYSTEMROOT",
        "WINDIR",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "PROGRAMDATA",
        "USERPROFILE",
        "TEMP",
        "TMP",
        "COMSPEC",
    ];
    for var in known_vars {
        let token = format!("%{}%", var);
        while let Some(pos) = result.to_uppercase().find(&token) {
            if let Ok(val) = env::var(var) {
                result.replace_range(pos..pos + token.len(), &val);
            } else {
                break;
            }
        }
    }
    result
}

/// Compare two version strings (`a.b.c.d`) or numbers numerically; fallback to case-insensitive text.
pub fn compare_version_or_number(actual: &str, expected: &str) -> std::cmp::Ordering {
    let clean_a = actual.trim().trim_start_matches(['v', 'V']);
    let clean_e = expected.trim().trim_start_matches(['v', 'V']);

    let parts_a: Vec<&str> = clean_a
        .split(['.', '-', '_'])
        .filter(|s| !s.is_empty())
        .collect();
    let parts_e: Vec<&str> = clean_e
        .split(['.', '-', '_'])
        .filter(|s| !s.is_empty())
        .collect();

    if !parts_a.is_empty()
        && !parts_e.is_empty()
        && parts_a.iter().all(|p| p.parse::<u64>().is_ok())
        && parts_e.iter().all(|p| p.parse::<u64>().is_ok())
    {
        let max_len = parts_a.len().max(parts_e.len());
        for i in 0..max_len {
            let va = parts_a
                .get(i)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let ve = parts_e
                .get(i)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            match va.cmp(&ve) {
                std::cmp::Ordering::Equal => continue,
                non_eq => return non_eq,
            }
        }
        return std::cmp::Ordering::Equal;
    }

    clean_a.to_lowercase().cmp(&clean_e.to_lowercase())
}

/// Query PE fixed file version (`major.minor.build.revision`) via Win32 `GetFileVersionInfoW`.
pub fn get_pe_file_version(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let path_w = to_wide(&path.to_string_lossy());
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path_w.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buffer = vec![0u8; size as usize];
        if GetFileVersionInfoW(
            PCWSTR(path_w.as_ptr()),
            0,
            size,
            buffer.as_mut_ptr() as *mut _,
        )
        .is_err()
        {
            return None;
        }

        let root_w = to_wide("\\");
        let mut lp_info: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(
            buffer.as_ptr() as *const _,
            PCWSTR(root_w.as_ptr()),
            &mut lp_info,
            &mut len,
        )
        .as_bool()
        {
            return None;
        }
        if lp_info.is_null() || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }

        let info = &*(lp_info as *const VS_FIXEDFILEINFO);
        let major = (info.dwFileVersionMS >> 16) & 0xFFFF;
        let minor = info.dwFileVersionMS & 0xFFFF;
        let build = (info.dwFileVersionLS >> 16) & 0xFFFF;
        let rev = info.dwFileVersionLS & 0xFFFF;
        Some(format!("{}.{}.{}.{}", major, minor, build, rev))
    }
}

fn evaluate_check_operator(actual: Option<&str>, operator: CheckOperator, expected: &str) -> bool {
    match operator {
        CheckOperator::Exists => actual.is_some(),
        CheckOperator::NotExists => actual.is_none(),
        CheckOperator::Equals => actual
            .map(|a| compare_version_or_number(a, expected) == std::cmp::Ordering::Equal)
            .unwrap_or(false),
        CheckOperator::NotEquals => actual
            .map(|a| compare_version_or_number(a, expected) != std::cmp::Ordering::Equal)
            .unwrap_or(true),
        CheckOperator::GreaterOrEqual => actual
            .map(|a| {
                matches!(
                    compare_version_or_number(a, expected),
                    std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
                )
            })
            .unwrap_or(false),
        CheckOperator::Contains => actual
            .map(|a| a.to_lowercase().contains(&expected.trim().to_lowercase()))
            .unwrap_or(false),
    }
}

/// Evaluate a `PrereqCondition` on the current machine. Returns `true` if satisfied.
pub fn evaluate_prereq_condition(cond: &PrereqCondition, install_dir: Option<&Path>) -> bool {
    match cond {
        PrereqCondition::Registry {
            root,
            subkey,
            value_name,
            arch_view,
            operator,
            expected,
        } => {
            let root_hkey = match root {
                RegHiveRoot::Hklm => HKEY_LOCAL_MACHINE,
                RegHiveRoot::Hkcu => HKEY_CURRENT_USER,
                RegHiveRoot::Hkcr => HKEY_CLASSES_ROOT,
                RegHiveRoot::Hku => HKEY_USERS,
            };
            let sam = match arch_view {
                RegArchView::Default => KEY_READ,
                RegArchView::X64 => KEY_READ | KEY_WOW64_64KEY,
                RegArchView::X86 => KEY_READ | KEY_WOW64_32KEY,
            };

            let clean_key = subkey.trim().trim_matches('\\');
            let key_w = to_wide(clean_key);
            let mut hkey = HKEY::default();
            let open_ok = unsafe {
                RegOpenKeyExW(root_hkey, PCWSTR(key_w.as_ptr()), 0, sam, &mut hkey).is_ok()
            };

            if !open_ok {
                return matches!(operator, CheckOperator::NotExists);
            }

            if value_name.trim().is_empty()
                && matches!(operator, CheckOperator::Exists | CheckOperator::NotExists)
            {
                unsafe {
                    let _ = RegCloseKey(hkey);
                }
                return matches!(operator, CheckOperator::Exists);
            }

            let val_trimmed = value_name.trim();
            let val_w = to_wide(val_trimmed);
            let val_ptr = if val_trimmed.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(val_w.as_ptr())
            };

            let mut val_type = REG_VALUE_TYPE::default();
            let mut data_buf = vec![0u8; 2048];
            let mut data_len = data_buf.len() as u32;

            let query_ok = unsafe {
                RegQueryValueExW(
                    hkey,
                    val_ptr,
                    None,
                    Some(&mut val_type),
                    Some(data_buf.as_mut_ptr()),
                    Some(&mut data_len),
                )
                .is_ok()
            };
            unsafe {
                let _ = RegCloseKey(hkey);
            }

            if !query_ok {
                return evaluate_check_operator(None, *operator, expected);
            }

            let actual_str = if val_type == REG_DWORD && data_len >= 4 {
                let v = u32::from_le_bytes([data_buf[0], data_buf[1], data_buf[2], data_buf[3]]);
                v.to_string()
            } else if val_type == REG_QWORD && data_len >= 8 {
                let v = u64::from_le_bytes([
                    data_buf[0],
                    data_buf[1],
                    data_buf[2],
                    data_buf[3],
                    data_buf[4],
                    data_buf[5],
                    data_buf[6],
                    data_buf[7],
                ]);
                v.to_string()
            } else if val_type == REG_SZ || val_type == REG_EXPAND_SZ {
                let u16_vec = bytes_to_u16_vec(&data_buf[..data_len as usize]);
                String::from_utf16_lossy(&u16_vec)
            } else {
                let u16_vec = bytes_to_u16_vec(&data_buf[..data_len as usize]);
                String::from_utf16_lossy(&u16_vec)
            };

            evaluate_check_operator(Some(&actual_str), *operator, expected)
        }
        PrereqCondition::File {
            path,
            operator,
            expected_version,
        } => {
            let expanded = expand_env_and_install_vars(path.trim(), install_dir);
            let p = Path::new(&expanded);
            let exists = p.exists();

            if !exists {
                return evaluate_check_operator(None, *operator, expected_version);
            }

            if matches!(
                operator,
                CheckOperator::GreaterOrEqual
                    | CheckOperator::Equals
                    | CheckOperator::NotEquals
                    | CheckOperator::Contains
            ) {
                if expected_version.trim().is_empty() {
                    return evaluate_check_operator(Some("1"), *operator, expected_version);
                }
                let ver = get_pe_file_version(p);
                return evaluate_check_operator(ver.as_deref(), *operator, expected_version);
            }

            evaluate_check_operator(Some("exists"), *operator, expected_version)
        }
    }
}

/// Show a modal warning dialog when a `PromptAndAbort` prerequisite fails, optionally opening `download_url`.
pub fn prompt_prereq_abort(rule_name: &str, message: &str, download_url: &str) {
    let title = format!("环境检测未通过 - {}", rule_name);
    let title_w = to_wide(&title);
    let has_url = !download_url.trim().is_empty();

    let full_msg = if has_url {
        format!(
            "{}\n\n是否立即在浏览器中打开组件下载页面？\n{}",
            message,
            download_url.trim()
        )
    } else {
        message.to_string()
    };
    let msg_w = to_wide(&full_msg);

    unsafe {
        let flags = if has_url {
            MB_YESNO | MB_ICONWARNING
        } else {
            MB_OK | MB_ICONWARNING
        };
        let resp = MessageBoxW(
            HWND::default(),
            PCWSTR(msg_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            flags,
        );
        if has_url && resp == IDYES {
            shell_open(Path::new(download_url.trim()), None, None);
        }
    }
}

/// Execute a prerequisite installer or command via `ShellExecuteExW`, supporting UAC elevation and exit code validation.
pub fn execute_prereq_command(
    rule_name: &str,
    program: &str,
    args: &str,
    elevate: bool,
    wait: bool,
    success_exit_codes: &[i32],
    install_dir: Option<&Path>,
) -> Result<(), String> {
    let prog_expanded = expand_env_and_install_vars(program.trim(), install_dir);
    let args_expanded = expand_env_and_install_vars(args.trim(), install_dir);

    let verb_w = to_wide(if elevate { "runas" } else { "open" });
    let file_w = to_wide(&prog_expanded);
    let params_w = to_wide(&args_expanded);
    let dir_w = install_dir
        .filter(|d| d.exists())
        .map(|d| to_wide(&d.to_string_lossy()));

    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            hwnd: HWND::default(),
            lpVerb: PCWSTR(verb_w.as_ptr()),
            lpFile: PCWSTR(file_w.as_ptr()),
            lpParameters: if args_expanded.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(params_w.as_ptr())
            },
            lpDirectory: dir_w
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };

        if ShellExecuteExW(&mut sei).is_err() {
            return Err(format!(
                "前置环境组件 [{}] 启动失败（可能被用户拒绝 UAC 提权或可执行文件不存在）: {}",
                rule_name, prog_expanded
            ));
        }

        if wait && !sei.hProcess.is_invalid() && !sei.hProcess.0.is_null() {
            let _ = WaitForSingleObject(sei.hProcess, INFINITE);
            let mut exit_code = 0u32;
            let _ = GetExitCodeProcess(sei.hProcess, &mut exit_code);
            let _ = CloseHandle(sei.hProcess);

            let code_i32 = exit_code as i32;
            let allowed = if success_exit_codes.is_empty() {
                matches!(code_i32, 0 | 1638 | 3010)
            } else {
                success_exit_codes.contains(&code_i32)
            };

            if !allowed {
                return Err(format!(
                    "前置环境组件 [{}] 安装未完成，返回退出码: {} (允许的退出码: {:?})",
                    rule_name, code_i32, success_exit_codes
                ));
            }
        } else if !sei.hProcess.is_invalid() && !sei.hProcess.0.is_null() {
            let _ = CloseHandle(sei.hProcess);
        }
    }

    Ok(())
}
