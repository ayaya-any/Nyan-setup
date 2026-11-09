#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod d2d;
pub mod gui_installer;
pub mod gui_studio;
pub mod icon;
pub mod package;
pub mod win32_ops;

use crate::gui_installer::{run_installer_or_uninstaller_gui, WizardMode};
use crate::gui_studio::run_studio_gui;
use crate::package::{
    build_installer_package, extract_package_files, format_bytes, read_package_from_exe,
    PackageConfig, UninstallReceipt, UNINSTALL_DATA_FILE,
};
use crate::win32_ops::{
    create_demo_app_directory, run_demo_app_greeting, to_wide,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

fn enable_high_dpi() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn attach_parent_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
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

    // Check CLI commands (`build-demo`, `pack`, or `--dump-ui`)
    if args.len() > 1 {
        match args[1].as_str() {
            "--dump-ui" => {
                let out_dir = args
                    .get(2)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("dist"));
                let _ = fs::create_dir_all(&out_dir);

                if let Ok(renderer) = crate::d2d::D2DRenderer::new() {
                    let mut studio = crate::gui_studio::StudioWindowState::new(renderer);
                    for (tab, name) in [
                        (crate::gui_studio::StudioTab::Metadata, "studio_tab1_metadata.bmp"),
                        (crate::gui_studio::StudioTab::FilesAndPaths, "studio_tab2_files.bmp"),
                        (crate::gui_studio::StudioTab::OptionsAndLicense, "studio_tab3_options.bmp"),
                        (crate::gui_studio::StudioTab::BuildAndTest, "studio_tab4_build.bmp"),
                    ] {
                        studio.active_tab = tab;
                        studio.draw();
                        let _ = studio.renderer.save_bmp(&out_dir.join(name));
                    }
                    studio.palette = crate::d2d::MaterialPalette::dark();
                    studio.active_tab = crate::gui_studio::StudioTab::Metadata;
                    studio.draw();
                    let _ = studio.renderer.save_bmp(&out_dir.join("studio_dark.bmp"));
                }

                if let Ok(renderer) = crate::d2d::D2DRenderer::new() {
                    let manifest = crate::package::PackageManifest {
                        config: PackageConfig::default(),
                        files: vec![],
                        total_uncompressed_bytes: 790_000,
                        total_compressed_bytes: 210_000,
                        dict_size: 8 << 20,
                    };
                    let mut inst = crate::gui_installer::InstallerWindowState::new_installer(
                        renderer,
                        current_exe.clone(),
                        manifest,
                    );
                    for (step, name) in [
                        (crate::gui_installer::WizardStep::WelcomeAndLicense, "installer_step1_welcome.bmp"),
                        (crate::gui_installer::WizardStep::CustomOptions, "installer_step2_options.bmp"),
                        (crate::gui_installer::WizardStep::Installing, "installer_step3_installing.bmp"),
                        (crate::gui_installer::WizardStep::Completed, "installer_step4_completed.bmp"),
                    ] {
                        inst.step = step;
                        if step == crate::gui_installer::WizardStep::Installing {
                            inst.progress = 0.68;
                            inst.status_text = "正在释放文件 (3/4): assets\\resources.pak".to_string();
                            inst.logs = vec![
                                "[目标路径] C:\\Users\\admin\\AppData\\Local\\Programs\\Nyan Studio App".to_string(),
                                "  [CRC32 OK] 释放: NyanApp.exe".to_string(),
                                "  [CRC32 OK] 释放: README.txt".to_string(),
                                "  [CRC32 OK] 释放: assets\\resources.pak".to_string(),
                            ];
                        }
                        inst.draw();
                        let _ = inst.renderer.save_bmp(&out_dir.join(name));
                    }

                    // Dump error state with long path and locked process
                    inst.step = crate::gui_installer::WizardStep::Installing;
                    inst.is_working = false;
                    inst.progress = 0.02;
                    let err_msg = "无法写入目标文件 C:\\Users\\admin\\AppData\\Local\\Programs\\Nyan Studio App\\NyanApp.exe: 另一个程序正在使用此文件，进程无法访问。 (os error 32)";
                    inst.install_error = Some(err_msg.to_string());
                    inst.status_text = format!("安装出错: {}", err_msg);
                    inst.logs = vec![
                        "[目标路径] C:\\Users\\admin\\AppData\\Local\\Programs\\Nyan Studio App".to_string(),
                        "  [CRC32 OK] 释放: NyanApp.exe".to_string(),
                        format!("[错误] {}", err_msg),
                    ];
                    inst.draw();
                    let _ = inst.renderer.save_bmp(&out_dir.join("installer_step3_error_longtext.bmp"));

                    // Dump update mode welcome
                    inst.step = crate::gui_installer::WizardStep::WelcomeAndLicense;
                    inst.existing_install = Some(crate::win32_ops::ExistingInstallInfo {
                        installed_version: "0.9.5".to_string(),
                        install_location: PathBuf::from("C:\\Users\\admin\\AppData\\Local\\Programs\\Nyan Studio App"),
                    });
                    inst.running_processes = vec![(1234, "NyanApp.exe".to_string())];
                    inst.draw();
                    let _ = inst.renderer.save_bmp(&out_dir.join("installer_step1_update_mode.bmp"));
                }
                return;
            }
            "build-demo" => {
                attach_parent_console();
                let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let out_exe = args
                    .get(2)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cwd.join("dist").join("NyanStudioApp_Setup.exe"));
                match create_demo_app_directory(&cwd) {
                    Ok((demo_src, main_exe)) => {
                        let mut cfg = PackageConfig::default();
                        cfg.main_exe = main_exe;
                        match build_installer_package(&current_exe, &demo_src, &out_exe, &cfg, None)
                        {
                            Ok((count, raw_sz, final_sz)) => {
                                println!(
                                    "[SUCCESS] Built installer: {}\n  Files: {}\n  Uncompressed payload: {}\n  Final standalone EXE: {}",
                                    out_exe.display(),
                                    count,
                                    format_bytes(raw_sz),
                                    format_bytes(final_sz)
                                );
                                return;
                            }
                            Err(e) => {
                                eprintln!("[ERROR] Build failed: {}", e);
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[ERROR] Failed to create demo app directory: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            "init-config" => {
                attach_parent_console();
                let out_json = args
                    .get(2)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("package.json"));
                let mut sample_cfg = PackageConfig::default();
                sample_cfg.prerequisites = vec![crate::package::PrerequisiteRule::preset_vc_redist_x64()];
                let template = serde_json::json!({
                    "source_dir": "./build_output",
                    "output_exe": "./dist/NyanStudioApp_Setup.exe",
                    "app_id": sample_cfg.app_id,
                    "app_name": sample_cfg.app_name,
                    "app_version": sample_cfg.app_version,
                    "publisher": sample_cfg.publisher,
                    "main_exe": sample_cfg.main_exe,
                    "default_install_dir": sample_cfg.default_install_dir,
                    "create_desktop_shortcut": sample_cfg.create_desktop_shortcut,
                    "create_start_menu_shortcut": sample_cfg.create_start_menu_shortcut,
                    "register_uninstaller": sample_cfg.register_uninstaller,
                    "auto_launch_after_install": sample_cfg.auto_launch_after_install,
                    "process_conflict_policy": sample_cfg.process_conflict_policy,
                    "compression_level": sample_cfg.compression_level,
                    "installer_icon": null,
                    "license_text": sample_cfg.license_text,
                    "prerequisites": sample_cfg.prerequisites,
                });
                let pretty = serde_json::to_string_pretty(&template).unwrap_or_default();
                if let Some(parent) = out_json.parent() {
                    if !parent.as_os_str().is_empty() {
                        let _ = fs::create_dir_all(parent);
                    }
                }
                match fs::write(&out_json, pretty) {
                    Ok(()) => {
                        println!(
                            "[SUCCESS] Generated template configuration file: {}",
                            out_json.display()
                        );
                        return;
                    }
                    Err(e) => {
                        eprintln!(
                            "[ERROR] Failed to write configuration file {}: {}",
                            out_json.display(),
                            e
                        );
                        std::process::exit(1);
                    }
                }
            }
            "pack" => {
                attach_parent_console();
                // Mode A: Config-based packing:
                //   nyan-setup.exe pack --config <package.json> [--source <dir>] [--output <exe>] [--icon <ico>]
                //   or nyan-setup.exe pack <package.json>
                let config_arg_pos = args
                    .iter()
                    .position(|a| a == "--config" || a == "-c")
                    .map(|p| p + 1)
                    .or_else(|| {
                        if args.len() >= 3 && args[2].to_ascii_lowercase().ends_with(".json") {
                            Some(2)
                        } else {
                            None
                        }
                    });

                let (src_dir, out_exe, cfg) = if let Some(cfg_idx) = config_arg_pos {
                    let Some(cfg_path_str) = args.get(cfg_idx) else {
                        eprintln!("Usage: nyan-setup.exe pack --config <package.json> [--source <dir>] [--output <exe>] [--icon <ico>]");
                        std::process::exit(1);
                    };
                    let cfg_path = PathBuf::from(cfg_path_str);
                    let cfg_base_dir = cfg_path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

                    let raw_json = match fs::read_to_string(&cfg_path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!(
                                "[ERROR] 无法读取配置文件 {}: {}",
                                cfg_path.display(),
                                e
                            );
                            std::process::exit(1);
                        }
                    };

                    let val: serde_json::Value = match serde_json::from_str(&raw_json) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!(
                                "[ERROR] 解析 JSON 配置文件 {} 失败: {}",
                                cfg_path.display(),
                                e
                            );
                            std::process::exit(1);
                        }
                    };

                    let has_explicit_install_dir = val.get("default_install_dir").is_some();
                    let json_source_dir = val
                        .get("source_dir")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let json_output_exe = val
                        .get("output_exe")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let mut parsed_cfg: PackageConfig = match serde_json::from_value(val) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!(
                                "[ERROR] 配置文件字段格式错误 {}: {}",
                                cfg_path.display(),
                                e
                            );
                            std::process::exit(1);
                        }
                    };

                    if !has_explicit_install_dir && !parsed_cfg.app_name.trim().is_empty() {
                        parsed_cfg.default_install_dir =
                            format!("%LOCALAPPDATA%\\Programs\\{}", parsed_cfg.app_name);
                    }

                    let cli_source = args
                        .iter()
                        .position(|a| a == "--source" || a == "-s")
                        .and_then(|p| args.get(p + 1))
                        .cloned();
                    let cli_output = args
                        .iter()
                        .position(|a| a == "--output" || a == "-o")
                        .and_then(|p| args.get(p + 1))
                        .cloned();
                    if let Some(pos) = args.iter().position(|a| a == "--icon") {
                        if let Some(ico) = args.get(pos + 1) {
                            parsed_cfg.installer_icon = Some(ico.clone());
                        }
                    }

                    let resolve_rel = |raw: &str| -> PathBuf {
                        let p = PathBuf::from(raw);
                        if p.is_absolute() || p.exists() {
                            p
                        } else {
                            let rel_to_cfg = cfg_base_dir.join(&p);
                            if rel_to_cfg.exists() || !p.starts_with(".") {
                                rel_to_cfg
                            } else {
                                p
                            }
                        }
                    };

                    let Some(src_raw) = cli_source.or(json_source_dir) else {
                        eprintln!("[ERROR] 配置文件缺少 `source_dir` 字段，且未通过 `--source <dir>` 指定源目录");
                        std::process::exit(1);
                    };
                    let Some(out_raw) = cli_output.or(json_output_exe) else {
                        eprintln!("[ERROR] 配置文件缺少 `output_exe` 字段，且未通过 `--output <exe>` 指定输出安装包路径");
                        std::process::exit(1);
                    };

                    if let Some(ref ico_str) = parsed_cfg.installer_icon.clone() {
                        if !ico_str.trim().is_empty() {
                            let resolved_ico = resolve_rel(ico_str.trim());
                            parsed_cfg.installer_icon =
                                Some(resolved_ico.to_string_lossy().to_string());
                        }
                    }

                    for rule in &mut parsed_cfg.prerequisites {
                        if let crate::package::PrereqAction::RunBundledExe {
                            ref mut source_file,
                            ..
                        } = rule.action
                        {
                            if !source_file.trim().is_empty() {
                                let resolved_redist = resolve_rel(source_file.trim());
                                if resolved_redist.is_file() {
                                    *source_file =
                                        resolved_redist.to_string_lossy().to_string();
                                }
                            }
                        }
                    }

                    (resolve_rel(&src_raw), resolve_rel(&out_raw), parsed_cfg)
                } else {
                    // Mode B: Positional CLI quick pack:
                    //   nyan-setup.exe pack <source_dir> <output_exe> [app_name] [version] [main_exe] [--icon <ico>]
                    if args.len() < 4 {
                        eprintln!(
                            "Usage:\n  nyan-setup.exe pack --config <package.json> [--source <dir>] [--output <exe>] [--icon <ico>]\n  nyan-setup.exe pack <source_dir> <output_exe> [app_name] [version] [main_exe] [--icon <ico>]"
                        );
                        std::process::exit(1);
                    }
                    let src_dir = PathBuf::from(&args[2]);
                    let out_exe = PathBuf::from(&args[3]);
                    let mut cfg = PackageConfig::default();
                    if let Some(name) = args.get(4) {
                        if !name.starts_with("--") {
                            cfg.app_name = name.clone();
                            cfg.default_install_dir =
                                format!("%LOCALAPPDATA%\\Programs\\{}", name);
                        }
                    }
                    if let Some(ver) = args.get(5) {
                        if !ver.starts_with("--") {
                            cfg.app_version = ver.clone();
                        }
                    }
                    if let Some(m_exe) = args.get(6) {
                        if !m_exe.starts_with("--") {
                            cfg.main_exe = m_exe.clone();
                        }
                    }
                    if let Some(pos) = args.iter().position(|a| a == "--icon") {
                        if let Some(ico) = args.get(pos + 1) {
                            cfg.installer_icon = Some(ico.clone());
                        }
                    }
                    (src_dir, out_exe, cfg)
                };

                if let Some(parent) = out_exe.parent() {
                    if !parent.as_os_str().is_empty() {
                        let _ = fs::create_dir_all(parent);
                    }
                }

                match build_installer_package(&current_exe, &src_dir, &out_exe, &cfg, None) {
                    Ok((count, raw_sz, final_sz)) => {
                        println!(
                            "[SUCCESS] Packed {} files ({}) -> {} ({})",
                            count,
                            format_bytes(raw_sz),
                            out_exe.display(),
                            format_bytes(final_sz)
                        );
                        return;
                    }
                    Err(e) => {
                        eprintln!("[ERROR] {}", e);
                        std::process::exit(1);
                    }
                }
            }
            _ => {}
        }
    }

    // Default: Launch Material Design Nyan Setup Studio GUI
    if let Err(e) = run_studio_gui() {
        show_error_box("Nyan Setup Studio 启动错误", &e);
    }
}
