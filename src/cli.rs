use crate::d2d::{D2DRenderer, MaterialPalette};
use crate::gui_installer::{InstallerWindowState, WizardStep};
use crate::gui_studio::{StudioTab, StudioWindowState};
use crate::package::{
    build_installer_package, format_bytes, PackageConfig, PackageManifest, PrereqAction,
    PrerequisiteRule,
};
use crate::win32_ops::{create_demo_app_directory, ExistingInstallInfo};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};

pub fn attach_parent_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Dumps UI snapshots of Studio tabs and Installer wizard steps into a target directory (BMP format).
pub fn run_dump_ui_snapshots(current_exe: &Path, args: &[String]) {
    let out_dir = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dist"));
    let _ = fs::create_dir_all(&out_dir);

    if let Ok(renderer) = D2DRenderer::new() {
        let mut studio = StudioWindowState::new(renderer);
        for (tab, name) in [
            (StudioTab::Metadata, "studio_tab1_metadata.bmp"),
            (StudioTab::FilesAndPaths, "studio_tab2_files.bmp"),
            (StudioTab::OptionsAndLicense, "studio_tab3_options.bmp"),
            (StudioTab::BuildAndTest, "studio_tab4_build.bmp"),
        ] {
            studio.active_tab = tab;
            studio.draw();
            let _ = studio.renderer.save_bmp(&out_dir.join(name));
        }
        studio.palette = MaterialPalette::dark();
        studio.active_tab = StudioTab::Metadata;
        studio.draw();
        let _ = studio.renderer.save_bmp(&out_dir.join("studio_dark.bmp"));
    }

    if let Ok(renderer) = D2DRenderer::new() {
        let manifest = PackageManifest {
            config: PackageConfig::default(),
            files: vec![],
            total_uncompressed_bytes: 790_000,
            total_compressed_bytes: 210_000,
            dict_size: 8 << 20,
        };
        let mut inst = InstallerWindowState::new_installer(
            renderer,
            current_exe.to_path_buf(),
            manifest,
        );
        for (step, name) in [
            (WizardStep::WelcomeAndLicense, "installer_step1_welcome.bmp"),
            (WizardStep::CustomOptions, "installer_step2_options.bmp"),
            (WizardStep::Installing, "installer_step3_installing.bmp"),
            (WizardStep::Completed, "installer_step4_completed.bmp"),
        ] {
            inst.step = step;
            if step == WizardStep::Installing {
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
        inst.step = WizardStep::Installing;
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
        inst.step = WizardStep::WelcomeAndLicense;
        inst.existing_install = Some(ExistingInstallInfo {
            installed_version: "0.9.5".to_string(),
            install_location: PathBuf::from("C:\\Users\\admin\\AppData\\Local\\Programs\\Nyan Studio App"),
        });
        inst.running_processes = vec![(1234, "NyanApp.exe".to_string())];
        inst.draw();
        let _ = inst.renderer.save_bmp(&out_dir.join("installer_step1_update_mode.bmp"));
    }
}

/// Builds the demo app package directly from the CLI.
pub fn run_cli_build_demo(current_exe: &Path, args: &[String]) {
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
            match build_installer_package(current_exe, &demo_src, &out_exe, &cfg, None) {
                Ok((count, raw_sz, final_sz)) => {
                    println!(
                        "[SUCCESS] Built installer: {}\n  Files: {}\n  Uncompressed payload: {}\n  Final standalone EXE: {}",
                        out_exe.display(),
                        count,
                        format_bytes(raw_sz),
                        format_bytes(final_sz)
                    );
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

/// Generates a starter template package configuration JSON file.
pub fn run_cli_init_config(args: &[String]) {
    attach_parent_console();
    let out_json = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("package.json"));
    let mut sample_cfg = PackageConfig::default();
    sample_cfg.prerequisites = vec![PrerequisiteRule::preset_vc_redist_x64()];
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

/// Packs an installer based on either a JSON config file or CLI positional arguments.
pub fn run_cli_pack(current_exe: &Path, args: &[String]) {
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
                eprintln!("[ERROR] 无法读取配置文件 {}: {}", cfg_path.display(), e);
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
                parsed_cfg.installer_icon = Some(resolved_ico.to_string_lossy().to_string());
            }
        }

        for rule in &mut parsed_cfg.prerequisites {
            if let PrereqAction::RunBundledExe {
                ref mut source_file,
                ..
            } = rule.action
            {
                if !source_file.trim().is_empty() {
                    let resolved_redist = resolve_rel(source_file.trim());
                    if resolved_redist.is_file() {
                        *source_file = resolved_redist.to_string_lossy().to_string();
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
                cfg.default_install_dir = format!("%LOCALAPPDATA%\\Programs\\{}", name);
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

    match build_installer_package(current_exe, &src_dir, &out_exe, &cfg, None) {
        Ok((count, raw_sz, final_sz)) => {
            println!(
                "[SUCCESS] Packed {} files ({}) -> {} ({})",
                count,
                format_bytes(raw_sz),
                out_exe.display(),
                format_bytes(final_sz)
            );
        }
        Err(e) => {
            eprintln!("[ERROR] {}", e);
            std::process::exit(1);
        }
    }
}

/// Dispatches CLI subcommands. Returns `true` if a CLI command was matched and processed.
pub fn handle_cli_subcommands(current_exe: &Path, args: &[String]) -> bool {
    if args.len() <= 1 {
        return false;
    }

    match args[1].as_str() {
        "--dump-ui" => {
            run_dump_ui_snapshots(current_exe, args);
            true
        }
        "build-demo" => {
            run_cli_build_demo(current_exe, args);
            true
        }
        "init-config" => {
            run_cli_init_config(args);
            true
        }
        "pack" => {
            run_cli_pack(current_exe, args);
            true
        }
        _ => false,
    }
}
