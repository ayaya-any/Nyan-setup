use super::*;
use crate::package::{CheckOperator, PrereqCondition, RegArchView, RegHiveRoot, UninstallReceipt};
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::Threading::GetCurrentProcessId;

#[test]
fn test_register_and_remove_uninstall() {
    let test_app_id = "test.nyan.cleanup.check";
    let temp_dir = std::env::temp_dir().join("fake_nyan_install_test");
    let _ = fs::create_dir_all(&temp_dir);

    register_uninstall_entry(
        test_app_id,
        "Test Nyan App",
        "1.0.0",
        "Test Publisher",
        &temp_dir,
        &temp_dir.join("app.exe"),
        &temp_dir.join("unins000.exe"),
        1024,
    )
    .expect("register");

    let detected = detect_existing_installation(test_app_id, &temp_dir);
    println!("Detected before removal: {:?}", detected);
    assert!(detected.is_some());

    remove_uninstall_entry(test_app_id);

    let parent_w = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall");
    let child_w = to_wide(test_app_id);
    let mut h_parent = HKEY::default();
    let open_parent = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(parent_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_parent,
        )
    };
    assert!(open_parent.is_ok());

    let mut h_child = HKEY::default();
    let open_child = unsafe {
        RegOpenKeyExW(
            h_parent,
            PCWSTR(child_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_child,
        )
    };
    unsafe {
        if open_child.is_ok() {
            let _ = RegCloseKey(h_child);
        }
        let _ = RegCloseKey(h_parent);
    }

    println!("Open child key after remove_uninstall_entry: {:?}", open_child);
    assert!(open_child.is_err(), "Key still exists in registry!");
}

#[test]
fn test_deep_registry_cleanup() {
    let app_id = "test.nyan.deepclean.app";
    let app_name = "DeepCleanApp";
    let publisher = "TestNyanCorp";
    let temp_dir = std::env::temp_dir().join("fake_nyan_deepclean_dir");
    let _ = fs::create_dir_all(&temp_dir);

    // 1. Create Uninstall Entry
    register_uninstall_entry(
        app_id,
        app_name,
        "1.2.3",
        publisher,
        &temp_dir,
        &temp_dir.join("app.exe"),
        &temp_dir.join("unins000.exe"),
        2048,
    )
    .expect("register");

    // 2. Create HKCU\Software\TestNyanCorp\DeepCleanApp
    let pub_path = format!("Software\\{}\\{}", publisher, app_name);
    let pub_path_w = to_wide(&pub_path);
    unsafe {
        let mut h_created = HKEY::default();
        let _ = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(pub_path_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut h_created,
            None,
        );
        if h_created != HKEY::default() {
            let _ = RegCloseKey(h_created);
        }
    }

    // 3. Create dummy MuiCache value
    let mui_path_w = to_wide("Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache");
    let fake_val_name = format!("{}\\app.exe.FriendlyAppName", temp_dir.to_string_lossy());
    let fake_val_w = to_wide(&fake_val_name);
    let fake_val_data = to_wide("DeepCleanApp");
    unsafe {
        let mut h_mui = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(mui_path_w.as_ptr()), 0, KEY_WRITE, &mut h_mui).is_ok() {
            let _ = RegSetValueExW(
                h_mui,
                PCWSTR(fake_val_w.as_ptr()),
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    fake_val_data.as_ptr() as *const u8,
                    fake_val_data.len() * 2,
                )),
            );
            let _ = RegCloseKey(h_mui);
        }
    }

    // 4. Create dummy Start Menu TileProperties nested key
    let tile_base = "Software\\Microsoft\\Windows\\CurrentVersion\\Start\\TileProperties";
    let temp_str = temp_dir.to_string_lossy().replace('/', "\\");
    let fake_tile_path = if let Some(colon_pos) = temp_str.find(':') {
        let drive = &temp_str[..colon_pos + 1];
        let rest = temp_str[colon_pos + 1..].trim_start_matches('\\');
        format!("{}\\W~{}\\{}\\app.exe", tile_base, drive, rest)
    } else {
        format!("{}\\W~{}\\app.exe", tile_base, temp_str)
    };
    let fake_tile_dir_path = if let Some(colon_pos) = temp_str.find(':') {
        let drive = &temp_str[..colon_pos + 1];
        let rest = temp_str[colon_pos + 1..].trim_start_matches('\\');
        format!("{}\\W~{}\\{}", tile_base, drive, rest)
    } else {
        format!("{}\\W~{}", tile_base, temp_str)
    };
    let fake_tile_path_w = to_wide(&fake_tile_path);
    let fake_tile_dir_w = to_wide(&fake_tile_dir_path);
    unsafe {
        let mut h_tile = HKEY::default();
        let res = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(fake_tile_path_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut h_tile,
            None,
        );
        if res.is_ok() && h_tile != HKEY::default() {
            let cat_name = to_wide("Category");
            let cat_val: u32 = 4;
            let _ = RegSetValueExW(
                h_tile,
                PCWSTR(cat_name.as_ptr()),
                0,
                REG_DWORD,
                Some(std::slice::from_raw_parts(
                    &cat_val as *const u32 as *const u8,
                    std::mem::size_of::<u32>(),
                )),
            );
            let _ = RegCloseKey(h_tile);
        }
    }

    let receipt = UninstallReceipt {
        app_id: app_id.to_string(),
        app_name: app_name.to_string(),
        app_version: "1.2.3".to_string(),
        publisher: publisher.to_string(),
        install_dir: temp_dir.to_string_lossy().to_string(),
        installed_files: vec!["app.exe".to_string()],
        desktop_shortcut: None,
        start_menu_shortcut: None,
        registry_key_created: true,
    };

    let logs = clean_all_application_registry_residue(&receipt);
    println!("Cleanup logs: {:?}", logs);

    // Verify uninstall key is gone
    let detected = detect_existing_installation(app_id, &temp_dir);
    assert!(detected.is_none(), "Uninstall entry should be gone!");

    // Verify HKCU\Software\TestNyanCorp\DeepCleanApp is gone
    unsafe {
        let mut h_check = HKEY::default();
        let open_res = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(pub_path_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_check,
        );
        assert!(open_res.is_err(), "Vendor key should be deleted!");
    }

    // Verify MuiCache fake value is gone
    unsafe {
        let mut h_mui = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(mui_path_w.as_ptr()), 0, KEY_READ, &mut h_mui).is_ok() {
            let query_res = RegQueryValueExW(
                h_mui,
                PCWSTR(fake_val_w.as_ptr()),
                None,
                None,
                None,
                None,
            );
            let _ = RegCloseKey(h_mui);
            assert!(query_res.is_err(), "MuiCache entry should be deleted!");
        }
    }

    // Verify TileProperties fake key and directory are completely gone
    unsafe {
        let mut h_tile_check = HKEY::default();
        let query_res = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(fake_tile_path_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_tile_check,
        );
        if query_res.is_ok() {
            let _ = RegCloseKey(h_tile_check);
        }
        assert!(query_res.is_err(), "TileProperties app.exe entry should be deleted!");

        let mut h_dir_check = HKEY::default();
        let query_dir_res = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(fake_tile_dir_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_dir_check,
        );
        if query_dir_res.is_ok() {
            let _ = RegCloseKey(h_dir_check);
        }
        assert!(query_dir_res.is_err(), "TileProperties app directory entry should be deleted!");
    }
}

#[test]
fn test_tileproperties_exact_user_path_cleaned() {
    let install_dir = r"C:\Users\admin\AppData\Local\Programs\Nyan Studio App";
    let tile_path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Start\\TileProperties\\W~C:\\Users\\admin\\AppData\\Local\\Programs\\Nyan Studio App\\NyanApp.exe"
    );
    let tile_dir = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Start\\TileProperties\\W~C:\\Users\\admin\\AppData\\Local\\Programs\\Nyan Studio App"
    );
    let tile_path_w = to_wide(&tile_path);
    let tile_dir_w = to_wide(&tile_dir);

    // 1. Create key with a value
    unsafe {
        let mut h_tile = HKEY::default();
        let res = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(tile_path_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut h_tile,
            None,
        );
        assert!(res.is_ok());
        let cat_name = to_wide("Category");
        let cat_val: u32 = 4;
        let _ = RegSetValueExW(
            h_tile,
            PCWSTR(cat_name.as_ptr()),
            0,
            REG_DWORD,
            Some(std::slice::from_raw_parts(
                &cat_val as *const u32 as *const u8,
                std::mem::size_of::<u32>(),
            )),
        );
        let _ = RegCloseKey(h_tile);
    }

    // 2. Execute deep registry cleanup
    let receipt = UninstallReceipt {
        app_id: "NyanApp".to_string(),
        app_name: "Nyan Studio App".to_string(),
        app_version: "1.0.0".to_string(),
        publisher: "Nyan Studio Team".to_string(),
        install_dir: install_dir.to_string(),
        installed_files: vec!["NyanApp.exe".to_string()],
        desktop_shortcut: None,
        start_menu_shortcut: None,
        registry_key_created: false,
    };

    let logs = clean_all_application_registry_residue(&receipt);
    assert!(logs.iter().any(|l| l.contains("TileProperties")));

    // 3. Verify that both the app directory key and the exe key are completely gone
    unsafe {
        let mut h_check = HKEY::default();
        let res_exe = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(tile_path_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_check,
        );
        if res_exe.is_ok() {
            let _ = RegCloseKey(h_check);
        }
        assert!(res_exe.is_err(), "TileProperties NyanApp.exe key must be deleted!");

        let mut h_dir_check = HKEY::default();
        let res_dir = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(tile_dir_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_dir_check,
        );
        if res_dir.is_ok() {
            let _ = RegCloseKey(h_dir_check);
        }
        assert!(res_dir.is_err(), "TileProperties Nyan Studio App folder key must be deleted!");
    }
}

#[test]
fn test_safe_application_folder_guards() {
    assert!(!is_safe_application_folder(Path::new("C:\\")));
    assert!(!is_safe_application_folder(Path::new("C:\\Windows")));
    assert!(!is_safe_application_folder(Path::new("C:\\Windows\\System32")));
    assert!(!is_safe_application_folder(Path::new("C:\\Program Files")));
    assert!(!is_safe_application_folder(Path::new("C:\\Program Files (x86)")));
    assert!(!is_safe_application_folder(Path::new("C:\\Users")));

    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let up = PathBuf::from(userprofile);
        assert!(!is_safe_application_folder(&up));
        assert!(!is_safe_application_folder(&up.join("Desktop")));
        assert!(!is_safe_application_folder(&up.join("Documents")));
        assert!(!is_safe_application_folder(&up.join("Downloads")));
        assert!(!is_safe_application_folder(&up.join("AppData\\Local")));
        assert!(!is_safe_application_folder(&up.join("AppData\\Local\\Programs")));
        assert!(is_safe_application_folder(&up.join("AppData\\Local\\Programs\\Nyan Studio App")));
    }

    assert!(is_safe_application_folder(Path::new(r"C:\Program Files\Nyan Studio App")));
    assert!(is_safe_application_folder(Path::new(r"D:\Tools\MyApp")));
}

#[test]
fn test_clean_empty_subdirectories() {
    let temp_dir = std::env::temp_dir().join("test_clean_empty_subdirs_dir");
    let nested = temp_dir.join("level1").join("level2").join("level3");
    let _ = fs::create_dir_all(&nested);
    assert!(nested.exists());

    clean_empty_subdirectories(&temp_dir);
    assert!(!temp_dir.join("level1").exists(), "Empty subdirectories should be pruned!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_audio_policy_config_cleanup() {
    let base_path = "Software\\Microsoft\\Internet Explorer\\LowRegistry\\Audio\\PolicyConfig\\PropertyStore";
    let fake_key_name = "test_nyan_audio_sub_mock";
    let fake_key_full = format!("{}\\{}", base_path, fake_key_name);
    let fake_key_w = to_wide(&fake_key_full);

    unsafe {
        let mut h_sub = HKEY::default();
        let res = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(fake_key_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut h_sub,
            None,
        );
        assert!(res.is_ok());

        let val_data = to_wide(r"|\Device\HarddiskVolume1\MockNyanAudioApp\MockAudio.exe%b{00000000}");
        let _ = RegSetValueExW(
            h_sub,
            PCWSTR::null(),
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                val_data.as_ptr() as *const u8,
                val_data.len() * 2,
            )),
        );
        let _ = RegCloseKey(h_sub);
    }

    let cleaned = clean_audio_policy_config(
        r"mocknyanaudioapp",
        &["mockaudio.exe".to_string()],
    );
    assert!(cleaned >= 1, "Audio policy key should be cleaned!");

    unsafe {
        let mut h_check = HKEY::default();
        let res = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(fake_key_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_check,
        );
        if res.is_ok() {
            let _ = RegCloseKey(h_check);
        }
        assert!(res.is_err(), "Mock audio policy key should be deleted!");
    }
}

#[test]
fn test_compare_version_or_number() {
    use std::cmp::Ordering;
    assert_eq!(compare_version_or_number("14.38.0.0", "14.38"), Ordering::Equal);
    assert_eq!(
        compare_version_or_number("14.40.33810.0", "14.38.33135.0"),
        Ordering::Greater
    );
    assert_eq!(
        compare_version_or_number("14.29.30133", "14.38.0"),
        Ordering::Less
    );
    assert_eq!(compare_version_or_number("1", "1"), Ordering::Equal);
    assert_eq!(compare_version_or_number("2", "1"), Ordering::Greater);
    assert_eq!(compare_version_or_number("0", "1"), Ordering::Less);
}

#[test]
fn test_evaluate_prereq_file_and_pe_version() {
    let k32_ver = get_pe_file_version(&PathBuf::from(expand_env_and_install_vars(
        "%SystemRoot%\\System32\\kernel32.dll",
        None,
    )));
    println!("kernel32.dll PE FileVersion: {:?}", k32_ver);
    assert!(k32_ver.is_some());

    let cond_exists = PrereqCondition::File {
        path: "%SystemRoot%\\System32\\kernel32.dll".to_string(),
        operator: CheckOperator::GreaterOrEqual,
        expected_version: "6.0.0.0".to_string(),
    };
    assert!(
        evaluate_prereq_condition(&cond_exists, None),
        "kernel32.dll should exist and have PE version >= 6.0.0.0 (actual: {:?})",
        k32_ver
    );

    let cond_too_high = PrereqCondition::File {
        path: "%SystemRoot%\\System32\\kernel32.dll".to_string(),
        operator: CheckOperator::GreaterOrEqual,
        expected_version: "999.0.0.0".to_string(),
    };
    assert!(
        !evaluate_prereq_condition(&cond_too_high, None),
        "kernel32.dll PE version should not be >= 999.0.0.0"
    );

    let cond_missing = PrereqCondition::File {
        path: "%SystemRoot%\\System32\\non_existent_nyan_test_12345.dll".to_string(),
        operator: CheckOperator::Exists,
        expected_version: String::new(),
    };
    assert!(!evaluate_prereq_condition(&cond_missing, None));
}

#[test]
fn test_evaluate_prereq_registry_condition() {
    let test_key = "Software\\NyanPrereqUnitTestKey";
    let test_key_w = to_wide(test_key);

    unsafe {
        let mut hkey = HKEY::default();
        let res = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(test_key_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        );
        assert!(res.is_ok());

        let v_installed = to_wide("Installed");
        let val_dword: u32 = 1;
        let _ = RegSetValueExW(
            hkey,
            PCWSTR(v_installed.as_ptr()),
            0,
            REG_DWORD,
            Some(std::slice::from_raw_parts(
                &val_dword as *const u32 as *const u8,
                4,
            )),
        );

        let v_ver = to_wide("Version");
        let ver_data = to_wide("14.40.33810.0");
        let _ = RegSetValueExW(
            hkey,
            PCWSTR(v_ver.as_ptr()),
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                ver_data.as_ptr() as *const u8,
                ver_data.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey);
    }

    let cond_dword = PrereqCondition::Registry {
        root: RegHiveRoot::Hkcu,
        subkey: test_key.to_string(),
        value_name: "Installed".to_string(),
        arch_view: RegArchView::Default,
        operator: CheckOperator::GreaterOrEqual,
        expected: "1".to_string(),
    };
    assert!(evaluate_prereq_condition(&cond_dword, None));

    let cond_ver_ok = PrereqCondition::Registry {
        root: RegHiveRoot::Hkcu,
        subkey: test_key.to_string(),
        value_name: "Version".to_string(),
        arch_view: RegArchView::Default,
        operator: CheckOperator::GreaterOrEqual,
        expected: "14.38.0.0".to_string(),
    };
    assert!(evaluate_prereq_condition(&cond_ver_ok, None));

    let cond_ver_fail = PrereqCondition::Registry {
        root: RegHiveRoot::Hkcu,
        subkey: test_key.to_string(),
        value_name: "Version".to_string(),
        arch_view: RegArchView::Default,
        operator: CheckOperator::GreaterOrEqual,
        expected: "15.0.0.0".to_string(),
    };
    assert!(!evaluate_prereq_condition(&cond_ver_fail, None));

    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(test_key_w.as_ptr()));
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(test_key_w.as_ptr()));
    }
}

#[test]
fn test_process_conflict_detection_no_false_positive_and_restart_manager_lock() {
    let temp_root = std::env::temp_dir().join(format!(
        "nyan_proc_lock_test_{}",
        unsafe { GetCurrentProcessId() }
    ));
    let _ = fs::remove_dir_all(&temp_root);

    // 1. Fresh install directory (does not exist yet): must NEVER false-match running system processes by basename
    let fake_candidates = vec![
        "explorer.exe".to_string(),
        "svchost.exe".to_string(),
        "cmd.exe".to_string(),
    ];
    let fresh_found = find_running_target_processes(&temp_root, &fake_candidates);
    assert!(
        fresh_found.is_empty(),
        "Non-existent install dir must not report false-positive same-named processes: {:?}",
        fresh_found
    );

    // 2. Existing install directory with unlocked files: must also return empty even if same-named processes run elsewhere
    fs::create_dir_all(&temp_root).unwrap();
    let dummy_exe = temp_root.join("explorer.exe");
    let dummy_dll = temp_root.join("plugin.dll");
    fs::write(&dummy_exe, b"unlocked dummy exe").unwrap();
    fs::write(&dummy_dll, b"unlocked dummy dll").unwrap();

    let unlocked_found = find_running_target_processes(&temp_root, &fake_candidates);
    assert!(
        unlocked_found.is_empty(),
        "Unlocked files in target dir must not trigger false positives from other directories: {:?}",
        unlocked_found
    );

    // 3. Lock `plugin.dll` and verify both `is_file_locked` and Restart Manager detect it!
    {
        use std::os::windows::fs::OpenOptionsExt;
        let locked_handle = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&dummy_dll)
            .unwrap();
        assert!(
            is_file_locked(&dummy_dll),
            "Exclusively opened DLL/file must be detected as locked"
        );
        let rm_procs =
            query_restart_manager_locking_processes(std::slice::from_ref(&dummy_dll), 0);
        let our_pid = unsafe { GetCurrentProcessId() };
        assert!(
            rm_procs.iter().any(|(pid, _)| *pid == our_pid),
            "Windows Restart Manager must identify the process locking plugin.dll, got: {:?}",
            rm_procs
        );
        drop(locked_handle);
    }

    assert!(
        !is_file_locked(&dummy_dll),
        "Once handle is closed, file must no longer be reported locked"
    );
    let _ = fs::remove_dir_all(&temp_root);
}
