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
    extract_package_files, read_package_from_exe, UninstallReceipt, UNINSTALL_DATA_FILE,
};
use crate::win32_ops::{run_demo_app_greeting, to_wide};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

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
