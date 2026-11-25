#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod cli;
pub mod d2d;
pub mod gui_installer;
pub mod gui_studio;
pub mod icon;
pub mod package;
pub mod win32_ops;

use crate::cli::{attach_parent_console, handle_cli_subcommands};
use crate::gui_installer::{run_installer_or_uninstaller_gui, WizardMode};
use crate::gui_studio::run_studio_gui;
use crate::package::{
    extract_package_files, read_package_from_exe, PackageManifest, UninstallReceipt,
    UNINSTALL_DATA_FILE,
};
use crate::win32_ops::{
    detect_existing_installation, discover_application_services, expand_install_path,
    is_current_process_elevated, query_service_state, run_demo_app_greeting, to_wide,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{
    GetTokenInformation, TokenLinkedToken, TOKEN_LINKED_TOKEN, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_OK, SW_SHOWNORMAL,
};

fn enable_high_dpi() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn show_error_box(title: &str, msg: &str) {
    let title_w = to_wide(title);
    let msg_w = to_wide(msg);
    unsafe {
        MessageBoxW(
            HWND::default(),
            PCWSTR(msg_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// A running service must be stopped before the update and restarted after
/// the files are replaced. Doing both from an unelevated installer launches
/// two separate `--service-op` helpers and therefore asks for UAC twice. The
/// installer instead requests elevation once when such a service is found.
fn installer_needs_service_elevation(manifest: &PackageManifest) -> bool {
    let default_dir = expand_install_path(&manifest.config.default_install_dir);
    let install_dir = detect_existing_installation(&manifest.config.app_id, &default_dir)
        .map(|existing| existing.install_location)
        .unwrap_or(default_dir);
    let files: Vec<String> = manifest
        .files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();
    let services =
        discover_application_services(&install_dir, &files, &manifest.config.services);
    services.iter().any(|name| {
        matches!(
            query_service_state(name),
            Some((state, _)) if state != crate::win32_ops::service::SERVICE_STOPPED
        )
    })
}

/// Relaunching the whole installer elevated keeps the same user only when the
/// current filtered token has a linked administrator token (normal UAC consent
/// instead of asking for different credentials).
fn current_user_can_elevate() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut linked = TOKEN_LINKED_TOKEN::default();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenLinkedToken,
            Some(&mut linked as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_LINKED_TOKEN>() as u32,
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(token);
        if ok && !linked.LinkedToken.is_invalid() {
            let _ = CloseHandle(linked.LinkedToken);
            true
        } else {
            false
        }
    }
}

/// Relaunch the current installer through the UAC prompt and let the parent
/// exit. Returns false when the user cancels or the launch fails.
fn relaunch_self_elevated(args: &[String]) -> bool {
    let Ok(exe) = env::current_exe() else {
        return false;
    };
    let mut params: Vec<String> = args.iter().skip(1).cloned().collect();
    params.push("--elevated-installer".to_string());
    let params = params
        .iter()
        .map(|arg| format!("\"{}\"", arg.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ");
    let verb = to_wide("runas");
    let file = to_wide(&exe.to_string_lossy());
    let params_w = to_wide(&params);
    unsafe {
        let result = ShellExecuteW(
            HWND::default(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR(params_w.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        result.0 as isize > 32
    }
}

fn try_load_uninstaller_receipt(exe_path: &Path, args: &[String]) -> Option<UninstallReceipt> {
    // 1. Explicit `--uninstall [dir]` CLI flag
    if let Some(pos) = args.iter().position(|a| a == "--uninstall") {
        let dir = if let Some(dir_arg) = args.get(pos + 1) {
            PathBuf::from(dir_arg)
        } else {
            exe_path.parent()?.to_path_buf()
        };
        let dat_path = dir.join(UNINSTALL_DATA_FILE);
        if let Ok(bytes) = fs::read(&dat_path) {
            if let Ok(receipt) = serde_json::from_slice::<UninstallReceipt>(&bytes) {
                return Some(receipt);
            }
        }
    }

    // 2. If running as `unins000.exe` next to `unins000.dat`
    let file_name = exe_path
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if file_name.starts_with("unins") {
        let dir = exe_path.parent()?;
        let dat_path = dir.join(UNINSTALL_DATA_FILE);
        if let Ok(bytes) = fs::read(&dat_path) {
            if let Ok(receipt) = serde_json::from_slice::<UninstallReceipt>(&bytes) {
                return Some(receipt);
            }
        }
    }

    None
}

fn main() {
    enable_high_dpi();

    let args: Vec<String> = env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--service-op") {
        let code = crate::win32_ops::run_cli_service_op(&args[2..]);
        std::process::exit(code);
    }

    let current_exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("nyan-setup.exe"));

    // Check if running as the installed demo application (`NyanApp.exe` or `--demo-app`)
    let exe_name = current_exe
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if args.iter().any(|a| a == "--demo-app")
        || (exe_name == "nyanapp.exe"
            && current_exe
                .parent()
                .map(|p| p.join("nyan_demo_app.json").exists())
                .unwrap_or(false))
    {
        run_demo_app_greeting();
        return;
    }

    // Check if running in Uninstaller mode
    if let Some(receipt) = try_load_uninstaller_receipt(&current_exe, &args) {
        if let Err(e) = run_installer_or_uninstaller_gui(WizardMode::Uninstaller {
            uninstaller_exe: current_exe,
            receipt,
        }) {
            show_error_box("卸载程序错误", &e);
        }
        return;
    }

    // Check if this executable has an attached NyanSetup Installer payload
    if let Ok(Some((_footer, manifest))) = read_package_from_exe(&current_exe) {
        // Optional headless extraction for automated verification: `Setup.exe --extract-to <dir>`
        if let Some(pos) = args.iter().position(|a| a == "--extract-to") {
            attach_parent_console();
            if let Some(target_dir) = args.get(pos + 1) {
                match extract_package_files(&current_exe, &manifest, Path::new(target_dir), true, None) {
                    Ok(files) => {
                        println!(
                            "[OK] Extracted {} files to {}",
                            files.len(),
                            target_dir
                        );
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("[ERROR] {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }

        // A running service would be stopped and restarted from two separate
        // elevated helpers. Ask for UAC once and run both operations in this
        // elevated installer process instead.
        if !is_current_process_elevated()
            && !args.iter().any(|arg| arg == "--elevated-installer")
            && current_user_can_elevate()
            && installer_needs_service_elevation(&manifest)
        {
            if !relaunch_self_elevated(&args) {
                show_error_box(
                    "需要管理员权限",
                    "覆盖更新需要停止并恢复已安装的 Windows 服务；请在 UAC 提示中允许管理员权限后重试。",
                );
            }
            return;
        }

        // Launch Material Design Installer Wizard GUI
        if let Err(e) = run_installer_or_uninstaller_gui(WizardMode::Installer {
            installer_exe: current_exe,
            manifest,
        }) {
            show_error_box("安装向导启动失败", &e);
        }
        return;
    }

    // Check CLI commands (`build-demo`, `init-config`, `pack`, or `--dump-ui`)
    if handle_cli_subcommands(&current_exe, &args) {
        return;
    }

    // Default: Launch Material Design Nyan Setup Studio GUI
    if let Err(e) = run_studio_gui() {
        show_error_box("Nyan Setup Studio 启动错误", &e);
    }
}
