use super::process::terminate_running_target_processes;
use super::registry_clean::clean_all_application_registry_residue;
use super::shell::{get_known_folder_path, to_wide};
use crate::package::{
    extract_pure_stub_bytes, UninstallReceipt, UNINSTALL_DATA_FILE, UNINSTALL_EXE_FILE,
};
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::ShellExecuteW;

/// Register the application under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\<app_id>`.
pub fn register_uninstall_entry(
    app_id: &str,
    app_name: &str,
    app_version: &str,
    publisher: &str,
    install_dir: &Path,
    main_exe_path: &Path,
    uninstaller_exe_path: &Path,
    estimated_size_kb: u32,
) -> Result<(), String> {
    let subkey = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        app_id
    );
    let subkey_w = to_wide(&subkey);

    unsafe {
        let mut hkey = HKEY::default();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_w.as_ptr()),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        );
        if status.is_err() {
            return Err(format!("无法创建卸载注册表项: {:?}", status));
        }

        let set_sz = |name: &str, val: &str| {
            let name_w = to_wide(name);
            let val_w = to_wide(val);
            let bytes = std::slice::from_raw_parts(
                val_w.as_ptr() as *const u8,
                val_w.len() * std::mem::size_of::<u16>(),
            );
            let _ = RegSetValueExW(hkey, PCWSTR(name_w.as_ptr()), 0, REG_SZ, Some(bytes));
        };

        let set_dword = |name: &str, val: u32| {
            let name_w = to_wide(name);
            let bytes = val.to_le_bytes();
            let _ = RegSetValueExW(hkey, PCWSTR(name_w.as_ptr()), 0, REG_DWORD, Some(&bytes));
        };

        let uninstall_cmd = format!(
            "\"{}\" --uninstall \"{}\"",
            uninstaller_exe_path.display(),
            install_dir.display()
        );

        set_sz("DisplayName", app_name);
        set_sz("DisplayVersion", app_version);
        set_sz("Publisher", publisher);
        set_sz("InstallLocation", &install_dir.to_string_lossy());
        set_sz("DisplayIcon", &main_exe_path.to_string_lossy());
        set_sz("UninstallString", &uninstall_cmd);
        set_sz("QuietUninstallString", &uninstall_cmd);
        set_dword("EstimatedSize", estimated_size_kb.max(1));
        set_dword("NoModify", 1);
        set_dword("NoRepair", 1);

        let _ = RegCloseKey(hkey);
    }

    Ok(())
}

/// Information about a previously installed version of the application detected on the system.
#[derive(Debug, Clone)]
pub struct ExistingInstallInfo {
    pub installed_version: String,
    pub install_location: PathBuf,
}

fn query_reg_sz(hkey: HKEY, value_name: &str) -> Option<String> {
    let name_w = to_wide(value_name);
    let mut buf = vec![0u16; 512];
    let mut byte_len = (buf.len() * 2) as u32;
    unsafe {
        if RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut byte_len),
        )
        .is_ok()
        {
            let u16_len = (byte_len as usize / 2).min(buf.len());
            let slice = &buf[..u16_len];
            let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
            let s = String::from_utf16_lossy(&slice[..end]).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

/// Detect whether `app_id` is already installed on the machine (via Registry or `unins000.dat` in `candidate_dir`).
pub fn detect_existing_installation(
    app_id: &str,
    candidate_dir: &Path,
) -> Option<ExistingInstallInfo> {
    // 1. Check `unins000.dat` in `candidate_dir` first
    let dat_in_candidate = candidate_dir.join(UNINSTALL_DATA_FILE);
    if dat_in_candidate.exists() {
        if let Ok(bytes) = fs::read(&dat_in_candidate) {
            if let Ok(receipt) = serde_json::from_slice::<UninstallReceipt>(&bytes) {
                if receipt.app_id == app_id || app_id.is_empty() {
                    return Some(ExistingInstallInfo {
                        installed_version: receipt.app_version,
                        install_location: candidate_dir.to_path_buf(),
                    });
                }
            }
        }
    }

    // 2. Check HKCU Uninstall registry entry for `app_id`
    if !app_id.is_empty() {
        let subkey = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
            app_id
        );
        let subkey_w = to_wide(&subkey);
        unsafe {
            let mut hkey = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey_w.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            )
            .is_ok()
            {
                let loc_opt = query_reg_sz(hkey, "InstallLocation");
                let ver_opt = query_reg_sz(hkey, "DisplayVersion");
                let _ = RegCloseKey(hkey);

                if let Some(loc_str) = loc_opt {
                    let loc_path = PathBuf::from(&loc_str);
                    if loc_path.exists() {
                        return Some(ExistingInstallInfo {
                            installed_version: ver_opt.unwrap_or_else(|| "未知版本".to_string()),
                            install_location: loc_path,
                        });
                    }
                }
            }
        }
    }

    None
}

/// Remove the uninstall registry key for `app_id`.
pub fn remove_uninstall_entry(app_id: &str) {
    let dummy_receipt = UninstallReceipt {
        app_id: app_id.to_string(),
        app_name: app_id.to_string(),
        app_version: String::new(),
        publisher: String::new(),
        install_dir: String::new(),
        installed_files: Vec::new(),
        desktop_shortcut: None,
        start_menu_shortcut: None,
        registry_key_created: true,
    };
    let _ = clean_all_application_registry_residue(&dummy_receipt);
}

/// Write `unins000.exe` (pure stub) and `unins000.dat` (receipt JSON) into `install_dir`.
pub fn write_uninstaller_files(
    current_installer_exe: &Path,
    install_dir: &Path,
    receipt: &UninstallReceipt,
) -> Result<PathBuf, String> {
    let stub_bytes = extract_pure_stub_bytes(current_installer_exe)
        .map_err(|e| format!("提取卸载程序二进制失败: {}", e))?;

    let unins_exe = install_dir.join(UNINSTALL_EXE_FILE);
    fs::write(&unins_exe, &stub_bytes)
        .map_err(|e| format!("写入卸载程序 {} 失败: {}", unins_exe.display(), e))?;

    let unins_dat = install_dir.join(UNINSTALL_DATA_FILE);
    let json = serde_json::to_vec_pretty(receipt)
        .map_err(|e| format!("序列化卸载清单失败: {}", e))?;
    fs::write(&unins_dat, json)
        .map_err(|e| format!("写入卸载清单 {} 失败: {}", unins_dat.display(), e))?;

    Ok(unins_exe)
}

/// Recursively clean all empty subdirectories under `dir` (bottom-up traversal).
pub fn clean_empty_subdirectories(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                clean_empty_subdirectories(&path);
                let _ = fs::remove_dir(&path);
            }
        }
    }
}

/// Verify if `path` is a safe dedicated application folder before attempting recursive tree removal.
pub fn is_safe_application_folder(path: &Path) -> bool {
    let components: Vec<_> = path.components().collect();
    if components.len() < 3 {
        return false;
    }

    let norm = path.to_string_lossy().to_lowercase().replace('/', "\\");
    let norm = norm.trim_end_matches('\\');

    let prohibited = [
        "c:", "d:", "e:", "f:", "g:",
        "c:\\windows",
        "c:\\windows\\system32",
        "c:\\program files",
        "c:\\program files (x86)",
        "c:\\programdata",
        "c:\\users",
    ];
    if prohibited.iter().any(|&p| norm == p) {
        return false;
    }

    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let up_norm = userprofile.to_lowercase().replace('/', "\\");
        let up_norm = up_norm.trim_end_matches('\\');
        if norm == up_norm {
            return false;
        }
        let user_roots = [
            format!("{}\\desktop", up_norm),
            format!("{}\\documents", up_norm),
            format!("{}\\downloads", up_norm),
            format!("{}\\pictures", up_norm),
            format!("{}\\music", up_norm),
            format!("{}\\videos", up_norm),
            format!("{}\\appdata", up_norm),
            format!("{}\\appdata\\local", up_norm),
            format!("{}\\appdata\\locallow", up_norm),
            format!("{}\\appdata\\roaming", up_norm),
            format!("{}\\appdata\\local\\programs", up_norm),
        ];
        if user_roots.iter().any(|r| norm == r) {
            return false;
        }
    }

    true
}

/// Perform uninstallation based on `UninstallReceipt`.
pub fn execute_uninstall_steps(receipt: &UninstallReceipt) -> Vec<String> {
    let mut logs = Vec::new();
    let install_dir = PathBuf::from(&receipt.install_dir);

    // 0. Terminate any running application processes in `install_dir` so `.exe`/`.dll` files are not locked
    let exe_files: Vec<String> = receipt
        .installed_files
        .iter()
        .filter(|f| f.to_ascii_lowercase().ends_with(".exe"))
        .cloned()
        .collect();
    let killed_logs = terminate_running_target_processes(&install_dir, &exe_files);
    for l in killed_logs {
        logs.push(l);
    }

    // 1. Delete installed files & directories
    for rel in receipt.installed_files.iter().rev() {
        let target = install_dir.join(rel);
        if target.exists() {
            if target.is_dir() {
                let _ = fs::remove_dir(&target);
            } else if fs::remove_file(&target).is_ok() {
                logs.push(format!("已移除文件: {}", rel));
            }
        }
        // Try removing empty parent subdirectories
        if let Some(parent) = target.parent() {
            if parent != install_dir {
                let _ = fs::remove_dir(parent);
            }
        }
    }
    // Clean any empty nested subdirectories left behind
    clean_empty_subdirectories(&install_dir);

    // 2. Delete shortcuts & Start Menu application folder
    if let Some(ref desk) = receipt.desktop_shortcut {
        let p = PathBuf::from(desk);
        if p.exists() && fs::remove_file(&p).is_ok() {
            logs.push(format!("已清理桌面快捷方式: {}", p.display()));
        }
    }
    if let Some(ref menu) = receipt.start_menu_shortcut {
        let p = PathBuf::from(menu);
        if p.exists() && fs::remove_file(&p).is_ok() {
            logs.push(format!("已清理开始菜单快捷方式: {}", p.display()));
        }
        if let Some(parent) = p.parent() {
            if let Some(prog_dir) = get_known_folder_path(false) {
                if parent != prog_dir && parent.starts_with(&prog_dir) {
                    // Try removing any lingering auxiliary shortcuts/files in this dedicated app folder
                    if let Ok(entries) = fs::read_dir(parent) {
                        for entry in entries.flatten() {
                            let ep = entry.path();
                            if ep.is_file() {
                                let _ = fs::remove_file(&ep);
                            }
                        }
                    }
                    if fs::remove_dir(parent).is_ok() {
                        logs.push(format!("已清理开始菜单程序组目录: {}", parent.display()));
                    }
                }
            } else {
                let _ = fs::remove_dir(parent);
            }
        }
    }

    // 3. Deep Registry Cleanup (Uninstall keys, App Paths, HKCU Software, MuiCache, UFH\SHC, AppCompatFlags, Explorer, Start Tiles, Notifications, etc.)
    let reg_logs = clean_all_application_registry_residue(receipt);
    for r_log in reg_logs {
        logs.push(r_log);
    }

    // 4. Delete unins000.dat
    let dat_path = install_dir.join(UNINSTALL_DATA_FILE);
    if dat_path.exists() {
        let _ = fs::remove_file(&dat_path);
    }

    logs.push("卸载清理操作已完成。".to_string());
    logs
}

/// Schedule self-deletion of `unins000.exe` and empty `install_dir` after the uninstaller process exits.
pub fn schedule_self_delete(unins_exe: &Path, install_dir: &Path) {
    let is_safe = is_safe_application_folder(install_dir);
    let rmdir_cmd = if is_safe {
        format!("rmdir /S /Q \"{}\"", install_dir.display())
    } else {
        format!("rmdir \"{}\"", install_dir.display())
    };

    let cmd_args = format!(
        "/C ping 127.0.0.1 -n 2 > nul & del /F /Q \"{}\" & {}",
        unins_exe.display(),
        rmdir_cmd
    );
    let op_w = to_wide("open");
    let file_w = to_wide("cmd.exe");
    let args_w = to_wide(&cmd_args);
    unsafe {
        let _ = ShellExecuteW(
            HWND::default(),
            PCWSTR(op_w.as_ptr()),
            PCWSTR(file_w.as_ptr()),
            PCWSTR(args_w.as_ptr()),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
        );
    }
}
