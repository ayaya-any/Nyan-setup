use super::*;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn test_bcj_and_solid_lzma2_roundtrip() {
    let mut sample_pe = b"MZ\x90\x00".to_vec();
    for i in 0..5000u32 {
        sample_pe.push(0xE8);
        let rel = (0x1000u32).wrapping_sub(i * 5);
        sample_pe.extend_from_slice(&rel.to_le_bytes());
    }
    let original = sample_pe.clone();
    bcj_x86_encode(&mut sample_pe);
    assert_ne!(sample_pe, original);
    bcj_x86_decode(&mut sample_pe);
    assert_eq!(sample_pe, original);

    let (compressed, dict) = lzma2_compress_bytes(&original, 6).expect("compress");
    let decompressed = lzma2_decompress_bytes(&compressed, dict).expect("decompress");
    assert_eq!(decompressed, original);
}

#[test]
fn test_built_demo_installer_extraction() {
    let setup_exe = Path::new("dist/NyanStudioApp_Setup.exe");
    if setup_exe.exists() {
        let (_footer, manifest) = read_package_from_exe(setup_exe)
            .expect("read_package_from_exe IO")
            .expect("valid NYANPK02 footer");
        let out_dir = Path::new("dist/test_extract_out");
        let extracted = extract_package_files(setup_exe, &manifest, out_dir, false, None)
            .expect("extract_package_files");
        assert_eq!(extracted.len(), manifest.files.len());
        let _ = fs::remove_dir_all(out_dir);
    }
}

#[test]
fn test_custom_installer_and_uninstaller_icon() {
    let current_exe = std::env::current_exe().expect("current exe");
    let temp_dir =
        std::env::temp_dir().join(format!("nyan_test_custom_ico_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    let src_dir = temp_dir.join("src");
    let _ = fs::create_dir_all(&src_dir);
    fs::write(src_dir.join("app.txt"), b"Hello Nyan World!").unwrap();

    // Create a custom 16x16 red icon
    let custom_ico = crate::icon::tests::make_test_ico(0, 0, 255);
    let custom_ico_path = temp_dir.join("custom.ico");
    fs::write(&custom_ico_path, &custom_ico).unwrap();

    let out_installer = temp_dir.join("MyCustomSetup.exe");
    let mut cfg = PackageConfig::default();
    cfg.installer_icon = Some(custom_ico_path.to_string_lossy().to_string());
    cfg.main_exe = "app.txt".to_string();

    let res = build_installer_package(&current_exe, &src_dir, &out_installer, &cfg, None);
    assert!(
        res.is_ok(),
        "build_installer_package with custom icon failed: {:?}",
        res
    );

    // 1. Verify installer PE has the custom icon in its resources
    let (group_inst, first_inst, _) = crate::icon::tests::read_icon_resources(&out_installer);
    let custom_frames = crate::icon::parse_ico(&custom_ico).unwrap();
    assert_eq!(
        u16::from_le_bytes([group_inst[4], group_inst[5]]),
        1,
        "installer has 1 icon frame in group"
    );
    assert_eq!(
        first_inst, custom_frames[0].data,
        "installer RT_ICON equals custom icon frame"
    );

    // 2. Simulate install: generate uninstaller using extract_pure_stub_bytes
    let unins_exe = temp_dir.join("unins000.exe");
    let stub_bytes =
        extract_pure_stub_bytes(&out_installer).expect("extract stub from custom installer");
    fs::write(&unins_exe, &stub_bytes).unwrap();

    // 3. Verify uninstaller PE has the exact same custom icon in its resources
    let (group_unins, first_unins, _) = crate::icon::tests::read_icon_resources(&unins_exe);
    assert_eq!(
        u16::from_le_bytes([group_unins[4], group_unins[5]]),
        1,
        "uninstaller has 1 icon frame in group"
    );
    assert_eq!(
        first_unins, custom_frames[0].data,
        "uninstaller RT_ICON equals custom icon frame"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_bundled_prerequisite_packaging_and_conditional_execution() {
    let current_exe = std::env::current_exe().expect("current exe");
    let temp_dir = std::env::temp_dir().join(format!("nyan_test_prereq_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    let src_dir = temp_dir.join("src");
    let _ = fs::create_dir_all(&src_dir);
    fs::write(src_dir.join("app.txt"), b"Main Application Payload").unwrap();

    let sys_cmd = PathBuf::from(
        std::env::var("COMSPEC").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string()),
    );
    let mock_redist = temp_dir.join("mock_vc_redist.exe");
    fs::copy(&sys_cmd, &mock_redist).expect("copy cmd.exe as mock_vc_redist.exe");

    let out_installer = temp_dir.join("PrereqSetup.exe");
    let mut cfg = PackageConfig::default();
    cfg.main_exe = "app.txt".to_string();

    let satisfied_marker = temp_dir.join("should_not_exist.txt");
    cfg.prerequisites.push(PrerequisiteRule {
        name: "Satisfied Rule".to_string(),
        enabled: true,
        condition: PrereqCondition::File {
            path: "%SystemRoot%\\System32\\kernel32.dll".to_string(),
            operator: CheckOperator::Exists,
            expected_version: String::new(),
        },
        action: PrereqAction::RunBundledExe {
            source_file: mock_redist.to_string_lossy().to_string(),
            bundled_rel_path: String::new(),
            args: format!("/C echo skip > \"{}\"", satisfied_marker.display()),
            elevate: false,
            wait: true,
            success_exit_codes: vec![0],
        },
    });

    let unsatisfied_marker = temp_dir.join("prereq_executed_ok.txt");
    cfg.prerequisites.push(PrerequisiteRule {
        name: "Unsatisfied VC Runtime Mock".to_string(),
        enabled: true,
        condition: PrereqCondition::Registry {
            root: RegHiveRoot::Hkcu,
            subkey: "Software\\NyanNonExistentRuntimeKey_99999".to_string(),
            value_name: "Installed".to_string(),
            arch_view: RegArchView::Default,
            operator: CheckOperator::Equals,
            expected: "1".to_string(),
        },
        action: PrereqAction::RunBundledExe {
            source_file: mock_redist.to_string_lossy().to_string(),
            bundled_rel_path: String::new(),
            args: format!("/C echo installed > \"{}\"", unsatisfied_marker.display()),
            elevate: false,
            wait: true,
            success_exit_codes: vec![0],
        },
    });

    build_installer_package(&current_exe, &src_dir, &out_installer, &cfg, None)
        .expect("build installer with bundled prerequisites");

    let (_footer, manifest) = read_package_from_exe(&out_installer)
        .expect("read io")
        .expect("manifest");
    assert_eq!(manifest.files.len(), 3);

    let install_target = temp_dir.join("installed_app");
    let installed_files =
        extract_package_files(&out_installer, &manifest, &install_target, false, None)
            .expect("extract and run prereqs");

    assert_eq!(installed_files, vec!["app.txt".to_string()]);
    assert!(install_target.join("app.txt").exists());
    assert!(
        !satisfied_marker.exists(),
        "Satisfied prerequisite should be skipped!"
    );
    assert!(
        unsatisfied_marker.exists(),
        "Unsatisfied bundled prerequisite should have executed!"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
