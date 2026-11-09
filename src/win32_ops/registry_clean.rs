use super::shell::to_wide;
use crate::package::UninstallReceipt;
use std::path::Path;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW,
    RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE,
    KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};

pub(crate) fn bytes_to_u16_vec(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

fn clean_matching_values_in_key(
    root: HKEY,
    key_path: &str,
    matcher: impl Fn(&str, &[u8]) -> bool,
) -> usize {
    let path_w = to_wide(key_path);
    let mut hkey = HKEY::default();
    let open_res = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(path_w.as_ptr()),
            0,
            KEY_READ | KEY_WRITE,
            &mut hkey,
        )
    };
    if open_res.is_err() {
        return 0;
    }

    let mut to_delete = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name_buf = vec![0u16; 1024];
        let mut name_len = name_buf.len() as u32;
        let mut val_type = 0u32;
        let mut data_buf = vec![0u8; 4096];
        let mut data_len = data_buf.len() as u32;

        let status = unsafe {
            RegEnumValueW(
                hkey,
                index,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                Some(&mut val_type),
                Some(data_buf.as_mut_ptr()),
                Some(&mut data_len),
            )
        };

        if status.is_err() {
            break;
        }

        let name_str = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        let data_slice = &data_buf[..data_len as usize];

        if matcher(&name_str, data_slice) {
            to_delete.push(name_str);
        }

        index += 1;
    }

    let count = to_delete.len();
    for del_name in to_delete {
        let del_w = to_wide(&del_name);
        unsafe {
            let _ = RegDeleteValueW(hkey, PCWSTR(del_w.as_ptr()));
        }
    }

    unsafe {
        let _ = RegCloseKey(hkey);
    }
    count
}

fn clean_matching_subkeys_in_key(
    root: HKEY,
    key_path: &str,
    matcher: impl Fn(&str) -> bool,
) -> usize {
    let path_w = to_wide(key_path);
    let mut hkey = HKEY::default();
    let open_res = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(path_w.as_ptr()),
            0,
            KEY_READ | KEY_WRITE,
            &mut hkey,
        )
    };
    if open_res.is_err() {
        return 0;
    }

    let mut to_delete = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name_buf = vec![0u16; 512];
        let mut name_len = name_buf.len() as u32;

        let status = unsafe {
            RegEnumKeyExW(
                hkey,
                index,
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

        let name_str = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        if matcher(&name_str) {
            to_delete.push(name_str);
        }
        index += 1;
    }

    let count = to_delete.len();
    for sub in to_delete {
        let sub_w = to_wide(&sub);
        unsafe {
            let _ = RegDeleteTreeW(hkey, PCWSTR(sub_w.as_ptr()));
            let _ = RegDeleteKeyW(hkey, PCWSTR(sub_w.as_ptr()));
        }
    }

    unsafe {
        let _ = RegCloseKey(hkey);
    }
    count
}

/// Recursively scan and clean subtrees under Start Menu `TileProperties` (e.g. KnownFolder GUIDs or drive trees).
fn clean_matching_recursive_subkeys(
    current_key_path: &str,
    app_id: &str,
    app_name: &str,
    install_dir_norm: &str,
    excluded_dirs: &[&str],
    depth: usize,
) -> usize {
    if depth > 10 {
        return 0;
    }

    let path_w = to_wide(current_key_path);
    let mut hkey = HKEY::default();
    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path_w.as_ptr()),
            0,
            KEY_READ | KEY_WRITE,
            &mut hkey,
        )
        .is_err()
        {
            return 0;
        }
    }

    let mut subkeys = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name_buf = [0u16; 512];
        let mut name_len = name_buf.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                hkey,
                index,
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
        let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        subkeys.push(name);
        index += 1;
    }

    unsafe {
        let _ = RegCloseKey(hkey);
    }

    let mut deleted = 0;
    for sub in subkeys {
        let lower = sub.to_lowercase();
        let sub_full_path = format!("{}\\{}", current_key_path, sub);

        let is_excluded = excluded_dirs.iter().any(|&e| e == lower);
        let matches_app = !is_excluded
            && ((!app_name.is_empty() && app_name.len() >= 3 && lower == app_name.to_lowercase())
                || (!app_id.is_empty() && app_id.len() >= 3 && lower == app_id.to_lowercase())
                || (!install_dir_norm.is_empty()
                    && sub_full_path.to_lowercase().ends_with(install_dir_norm)));

        if matches_app {
            let full_w = to_wide(&sub_full_path);
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(full_w.as_ptr()));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(full_w.as_ptr()));
            }
            deleted += 1;
        } else {
            deleted += clean_matching_recursive_subkeys(
                &sub_full_path,
                app_id,
                app_name,
                install_dir_norm,
                excluded_dirs,
                depth + 1,
            );
        }
    }

    deleted
}

/// Clean Windows Audio PolicyConfig (`PropertyStore`) registry keys matching the application.
pub(crate) fn clean_audio_policy_config(install_dir_norm: &str, exe_names: &[String]) -> usize {
    if install_dir_norm.is_empty() && exe_names.is_empty() {
        return 0;
    }
    let base_path = "Software\\Microsoft\\Internet Explorer\\LowRegistry\\Audio\\PolicyConfig\\PropertyStore";
    let base_w = to_wide(base_path);
    let mut h_base = HKEY::default();
    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(base_w.as_ptr()),
            0,
            KEY_READ | KEY_WRITE,
            &mut h_base,
        )
        .is_err()
        {
            return 0;
        }
    }

    let mut to_delete = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name_buf = [0u16; 512];
        let mut name_len = name_buf.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                h_base,
                index,
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
        let sub_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        let sub_w = to_wide(&sub_name);

        unsafe {
            let mut h_sub = HKEY::default();
            if RegOpenKeyExW(
                h_base,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ,
                &mut h_sub,
            )
            .is_ok()
            {
                let mut v_idx = 0u32;
                let mut matches = false;
                loop {
                    let mut val_name_buf = [0u16; 512];
                    let mut val_name_len = val_name_buf.len() as u32;
                    let mut val_type = 0u32;
                    let mut data_buf = [0u8; 2048];
                    let mut data_len = data_buf.len() as u32;

                    let v_stat = RegEnumValueW(
                        h_sub,
                        v_idx,
                        PWSTR(val_name_buf.as_mut_ptr()),
                        &mut val_name_len,
                        None,
                        Some(&mut val_type),
                        Some(data_buf.as_mut_ptr()),
                        Some(&mut data_len),
                    );
                    if v_stat.is_err() {
                        break;
                    }

                    let data_u16 = bytes_to_u16_vec(&data_buf[..data_len as usize]);
                    let data_str = String::from_utf16_lossy(&data_u16).to_lowercase();
                    if (!install_dir_norm.is_empty() && data_str.contains(install_dir_norm))
                        || exe_names.iter().any(|e| data_str.contains(e))
                    {
                        matches = true;
                        break;
                    }
                    v_idx += 1;
                }
                let _ = RegCloseKey(h_sub);
                if matches {
                    to_delete.push(sub_name);
                }
            }
        }
        index += 1;
    }

    let count = to_delete.len();
    for sub in to_delete {
        let sub_w = to_wide(&sub);
        unsafe {
            let _ = RegDeleteTreeW(h_base, PCWSTR(sub_w.as_ptr()));
            let _ = RegDeleteKeyW(h_base, PCWSTR(sub_w.as_ptr()));
        }
    }

    unsafe {
        let _ = RegCloseKey(h_base);
    }
    count
}

/// Helper to clean Start Menu TileProperties registry keys for Win32 apps and UWP packages.
fn clean_start_menu_tile_properties(
    receipt: &UninstallReceipt,
    app_id: &str,
    app_name: &str,
    install_dir_norm: &str,
) -> usize {
    let mut total_cleaned = 0;
    let base_key_str = "Software\\Microsoft\\Windows\\CurrentVersion\\Start\\TileProperties";

    // 1. Target drive-based nested paths (e.g. W~C:\Users\admin\AppData\Local\Programs\Nyan Studio App)
    let mut targets_to_delete = Vec::new();

    let clean_install_dir = receipt
        .install_dir
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_string();

    if !clean_install_dir.is_empty() {
        if let Some(colon_pos) = clean_install_dir.find(':') {
            let drive = &clean_install_dir[..colon_pos + 1];
            let rest = clean_install_dir[colon_pos + 1..].trim_start_matches('\\');
            targets_to_delete.push(format!("{}\\W~{}\\{}", base_key_str, drive, rest));
        } else {
            targets_to_delete.push(format!("{}\\W~{}", base_key_str, clean_install_dir));
        }

        for file in &receipt.installed_files {
            if file.to_ascii_lowercase().ends_with(".exe") {
                let full_exe = Path::new(&clean_install_dir).join(file);
                let exe_str = full_exe.to_string_lossy().replace('/', "\\");
                if let Some(colon_pos) = exe_str.find(':') {
                    let drive = &exe_str[..colon_pos + 1];
                    let rest = exe_str[colon_pos + 1..].trim_start_matches('\\');
                    targets_to_delete.push(format!("{}\\W~{}\\{}", base_key_str, drive, rest));
                }
            }
        }
    }

    // Check start menu shortcut if recorded in receipt
    if let Some(ref menu_sc) = receipt.start_menu_shortcut {
        let sc_clean = menu_sc.replace('/', "\\");
        if let Some(colon_pos) = sc_clean.find(':') {
            let drive = &sc_clean[..colon_pos + 1];
            let rest = sc_clean[colon_pos + 1..].trim_start_matches('\\');
            targets_to_delete.push(format!("{}\\W~{}\\{}", base_key_str, drive, rest));
        }
        if let Some(parent) = Path::new(&sc_clean).parent() {
            let p_str = parent.to_string_lossy().replace('/', "\\");
            if let Some(colon_pos) = p_str.find(':') {
                let drive = &p_str[..colon_pos + 1];
                let rest = p_str[colon_pos + 1..].trim_start_matches('\\');
                targets_to_delete.push(format!("{}\\W~{}\\{}", base_key_str, drive, rest));
            }
        }
    }

    // Direct AUMID / AppId / AppName entries under TileProperties
    if !app_id.is_empty() {
        targets_to_delete.push(format!("{}\\W~{}", base_key_str, app_id));
        targets_to_delete.push(format!("{}\\P~{}", base_key_str, app_id));
    }
    if !app_name.is_empty() && app_name != app_id {
        targets_to_delete.push(format!("{}\\W~{}", base_key_str, app_name));
        targets_to_delete.push(format!("{}\\P~{}", base_key_str, app_name));
    }

    // Delete all targeted keys
    for target in targets_to_delete {
        let target_w = to_wide(&target);
        unsafe {
            let mut h_check = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(target_w.as_ptr()),
                0,
                KEY_READ,
                &mut h_check,
            )
            .is_ok()
            {
                let _ = RegCloseKey(h_check);
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(target_w.as_ptr()));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(target_w.as_ptr()));
                total_cleaned += 1;
            }
        }
    }

    // 2. Scan KnownFolder GUID keys and subtrees under TileProperties (e.g. W~{GUID}\AppName)
    let base_w = to_wide(base_key_str);
    let mut h_base = HKEY::default();
    let opened_base = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(base_w.as_ptr()),
            0,
            KEY_READ,
            &mut h_base,
        )
        .is_ok()
    };

    if opened_base {
        let mut top_subkeys = Vec::new();
        let mut index = 0u32;
        loop {
            let mut name_buf = [0u16; 512];
            let mut name_len = name_buf.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    h_base,
                    index,
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
            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            top_subkeys.push(name);
            index += 1;
        }
        unsafe {
            let _ = RegCloseKey(h_base);
        }

        let excluded_dirs = [
            "users",
            "appdata",
            "local",
            "locallow",
            "roaming",
            "programs",
            "program files",
            "program files (x86)",
            "windows",
            "system32",
            "syswow64",
            "microsoft",
            "start menu",
        ];

        for top in top_subkeys {
            if top.starts_with("W~{")
                || (top.starts_with("W~") && top.len() >= 4 && top.as_bytes()[3] == b':')
            {
                let full_top_path = format!("{}\\{}", base_key_str, top);
                total_cleaned += clean_matching_recursive_subkeys(
                    &full_top_path,
                    app_id,
                    app_name,
                    install_dir_norm,
                    &excluded_dirs,
                    0,
                );
            }
        }
    }

    total_cleaned
}

/// Comprehensive deep cleaning of all registry entries associated with the application.
pub fn clean_all_application_registry_residue(receipt: &UninstallReceipt) -> Vec<String> {
    let mut logs = Vec::new();
    let app_id = receipt.app_id.trim();
    let app_name = receipt.app_name.trim();
    let publisher = receipt.publisher.trim();
    let install_dir_norm = receipt
        .install_dir
        .to_lowercase()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_string();

    let hives = [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")];
    let access_views = [
        KEY_WRITE | KEY_READ,
        KEY_WRITE | KEY_READ | KEY_WOW64_64KEY,
        KEY_WRITE | KEY_READ | KEY_WOW64_32KEY,
    ];

    // 1. Uninstall registry keys (HKCU & HKLM, 64-bit & 32-bit views)
    let mut targets = Vec::new();
    if !app_id.is_empty() {
        targets.push(app_id.to_string());
    }
    if !app_name.is_empty() && app_name != app_id {
        targets.push(app_name.to_string());
    }

    let uninstall_base = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    let uninstall_base_w = to_wide(uninstall_base);

    for (root, hive_name) in hives {
        for access in access_views {
            unsafe {
                let mut h_base = HKEY::default();
                if RegOpenKeyExW(root, PCWSTR(uninstall_base_w.as_ptr()), 0, access, &mut h_base).is_ok() {
                    for target in &targets {
                        let target_w = to_wide(target);
                        let _ = RegDeleteTreeW(h_base, PCWSTR(target_w.as_ptr()));
                        let _ = RegDeleteKeyW(h_base, PCWSTR(target_w.as_ptr()));
                        let _ = RegDeleteKeyExW(h_base, PCWSTR(target_w.as_ptr()), access.0, 0);
                    }
                    let _ = RegCloseKey(h_base);
                }
            }
        }
        let _ = hive_name;
    }
    logs.push(format!("已彻底移除 Windows 应用卸载项 (Uninstall\\{})", app_id));

    // 2. App Paths (HKCU & HKLM)
    for exe_rel in receipt.installed_files.iter().filter(|f| f.to_lowercase().ends_with(".exe")) {
        if let Some(file_name) = Path::new(exe_rel).file_name().and_then(|f| f.to_str()) {
            let app_paths_base = "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths";
            let app_paths_base_w = to_wide(app_paths_base);
            let file_name_w = to_wide(file_name);
            for (root, _) in hives {
                for access in access_views {
                    unsafe {
                        let mut h_paths = HKEY::default();
                        if RegOpenKeyExW(root, PCWSTR(app_paths_base_w.as_ptr()), 0, access, &mut h_paths).is_ok() {
                            let _ = RegDeleteTreeW(h_paths, PCWSTR(file_name_w.as_ptr()));
                            let _ = RegDeleteKeyW(h_paths, PCWSTR(file_name_w.as_ptr()));
                            let _ = RegCloseKey(h_paths);
                        }
                    }
                }
            }
        }
    }

    // 3. User configuration keys under HKCU\Software & HKLM\Software
    if !app_name.is_empty() {
        let soft_w = to_wide("Software");
        for (root, _) in hives {
            unsafe {
                let mut h_soft = HKEY::default();
                if RegOpenKeyExW(
                    root,
                    PCWSTR(soft_w.as_ptr()),
                    0,
                    KEY_READ | KEY_WRITE,
                    &mut h_soft,
                )
                .is_ok()
                {
                    if !publisher.is_empty() {
                        let pub_w = to_wide(publisher);
                        let mut h_pub = HKEY::default();
                        if RegOpenKeyExW(
                            h_soft,
                            PCWSTR(pub_w.as_ptr()),
                            0,
                            KEY_READ | KEY_WRITE,
                            &mut h_pub,
                        )
                        .is_ok()
                        {
                            let app_w = to_wide(app_name);
                            let _ = RegDeleteTreeW(h_pub, PCWSTR(app_w.as_ptr()));
                            let _ = RegDeleteKeyW(h_pub, PCWSTR(app_w.as_ptr()));
                            let _ = RegCloseKey(h_pub);
                        }
                        let _ = RegDeleteKeyW(h_soft, PCWSTR(pub_w.as_ptr()));
                    }
                    let app_w = to_wide(app_name);
                    let _ = RegDeleteTreeW(h_soft, PCWSTR(app_w.as_ptr()));
                    let _ = RegDeleteKeyW(h_soft, PCWSTR(app_w.as_ptr()));
                    if !app_id.is_empty() && app_id != app_name {
                        let id_w = to_wide(app_id);
                        let _ = RegDeleteTreeW(h_soft, PCWSTR(id_w.as_ptr()));
                        let _ = RegDeleteKeyW(h_soft, PCWSTR(id_w.as_ptr()));
                    }
                    let _ = RegCloseKey(h_soft);
                }
            }
        }
        logs.push("已清理用户与厂商配置项 (Software\\Publisher\\App)".to_string());
    }

    // 4. Windows Shell MuiCache
    if !install_dir_norm.is_empty() {
        let mui_count = clean_matching_values_in_key(
            HKEY_CURRENT_USER,
            "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache",
            |name, _| {
                let lower = name.to_lowercase();
                lower.contains(&install_dir_norm)
                    || (!app_name.is_empty() && lower.contains(&app_name.to_lowercase()))
            },
        );
        if mui_count > 0 {
            logs.push(format!("已清理 Windows MuiCache 资源缓存 ({} 条)", mui_count));
        }
    }

    // 5. Windows Shell Shortcut History (`UFH\SHC`)
    if !install_dir_norm.is_empty() {
        let shc_count = clean_matching_values_in_key(
            HKEY_CURRENT_USER,
            "Software\\Microsoft\\Windows\\CurrentVersion\\UFH\\SHC",
            |name, data| {
                let lower_name = name.to_lowercase();
                let data_str = String::from_utf16_lossy(&bytes_to_u16_vec(data)).to_lowercase();
                lower_name.contains(&install_dir_norm)
                    || data_str.contains(&install_dir_norm)
                    || (!app_name.is_empty() && data_str.contains(&app_name.to_lowercase()))
            },
        );
        if shc_count > 0 {
            logs.push(format!(
                "已清理开始菜单快捷方式运行历史 (UFH\\SHC, {} 条)",
                shc_count
            ));
        }
    }

    // 6. AppCompatFlags Compatibility Assistant Store & Persisted
    if !install_dir_norm.is_empty() {
        let mut compat_count = 0;
        for sub in ["Store", "Persisted"] {
            compat_count += clean_matching_values_in_key(
                HKEY_CURRENT_USER,
                &format!(
                    "Software\\Microsoft\\Windows NT\\CurrentVersion\\AppCompatFlags\\Compatibility Assistant\\{}",
                    sub
                ),
                |name, _| {
                    let lower = name.to_lowercase();
                    lower.contains(&install_dir_norm)
                },
            );
        }
        if compat_count > 0 {
            logs.push(format!(
                "已清理应用程序兼容性助手缓存 (AppCompatFlags, {} 条)",
                compat_count
            ));
        }
    }

    // 7. Explorer FeatureUsage
    if !install_dir_norm.is_empty() {
        let feat_base = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FeatureUsage";
        let feat_w = to_wide(feat_base);
        let mut feat_count = 0;
        unsafe {
            let mut h_feat = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(feat_w.as_ptr()),
                0,
                KEY_READ,
                &mut h_feat,
            )
            .is_ok()
            {
                let mut feat_subs = Vec::new();
                let mut idx = 0u32;
                loop {
                    let mut name_buf = [0u16; 256];
                    let mut name_len = name_buf.len() as u32;
                    let stat = RegEnumKeyExW(
                        h_feat,
                        idx,
                        PWSTR(name_buf.as_mut_ptr()),
                        &mut name_len,
                        None,
                        PWSTR::null(),
                        None,
                        None,
                    );
                    if stat.is_err() {
                        break;
                    }
                    feat_subs.push(String::from_utf16_lossy(&name_buf[..name_len as usize]));
                    idx += 1;
                }
                let _ = RegCloseKey(h_feat);

                for sub in feat_subs {
                    feat_count += clean_matching_values_in_key(
                        HKEY_CURRENT_USER,
                        &format!("{}\\{}", feat_base, sub),
                        |name, _| {
                            let lower = name.to_lowercase();
                            lower.contains(&install_dir_norm)
                                || (!app_name.is_empty() && lower.contains(&app_name.to_lowercase()))
                        },
                    );
                }
            }
        }
        if feat_count > 0 {
            logs.push(format!("已清理系统运行与切换记录 (FeatureUsage, {} 条)", feat_count));
        }
    }

    // 8. Start Menu TileProperties
    let tile_count = clean_start_menu_tile_properties(receipt, app_id, app_name, &install_dir_norm);
    if tile_count > 0 {
        logs.push(format!("已清理开始菜单磁贴属性项 (TileProperties, {} 处)", tile_count));
    }

    // 9. Startup Run and RunOnce entries (HKCU & HKLM)
    let mut run_count = 0;
    for (root, _) in hives {
        for run_key in ["Run", "RunOnce"] {
            run_count += clean_matching_values_in_key(
                root,
                &format!("Software\\Microsoft\\Windows\\CurrentVersion\\{}", run_key),
                |name, data| {
                    let lower_name = name.to_lowercase();
                    let data_str = String::from_utf16_lossy(&bytes_to_u16_vec(data)).to_lowercase();
                    lower_name == app_name.to_lowercase()
                        || lower_name == app_id.to_lowercase()
                        || (!install_dir_norm.is_empty() && data_str.contains(&install_dir_norm))
                },
            );
        }
    }
    if run_count > 0 {
        logs.push(format!("已清理自启动注册项 (Run/RunOnce, {} 条)", run_count));
    }

    // 10. StartupApproved entries (HKCU & HKLM)
    let mut startup_appr_count = 0;
    for (root, _) in hives {
        for sub in ["Run", "Run32", "StartupFolder"] {
            startup_appr_count += clean_matching_values_in_key(
                root,
                &format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\{}", sub),
                |name, data| {
                    let lower_name = name.to_lowercase();
                    let data_str = String::from_utf16_lossy(&bytes_to_u16_vec(data)).to_lowercase();
                    lower_name == app_name.to_lowercase()
                        || lower_name == app_id.to_lowercase()
                        || (!install_dir_norm.is_empty() && (lower_name.contains(&install_dir_norm) || data_str.contains(&install_dir_norm)))
                },
            );
        }
    }
    if startup_appr_count > 0 {
        logs.push(format!("已清理启动任务状态项 (StartupApproved, {} 条)", startup_appr_count));
    }

    // 11. AppCompatFlags Layers
    if !install_dir_norm.is_empty() {
        for (root, _) in hives {
            clean_matching_values_in_key(
                root,
                "Software\\Microsoft\\Windows NT\\CurrentVersion\\AppCompatFlags\\Layers",
                |name, _| {
                    let lower = name.to_lowercase();
                    lower.contains(&install_dir_norm)
                },
            );
        }
    }

    // 12. Notification Toast Settings
    if !app_id.is_empty() || !app_name.is_empty() {
        let mut notif_count = 0;
        for (root, _) in hives {
            notif_count += clean_matching_subkeys_in_key(
                root,
                "Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings",
                |sub| {
                    let lower = sub.to_lowercase();
                    lower == app_id.to_lowercase()
                        || (!app_name.is_empty() && lower.contains(&app_name.to_lowercase()))
                },
            );
        }
        if notif_count > 0 {
            logs.push(format!("已清理系统通知设置项 ({} 条)", notif_count));
        }
    }

    // 13. Classes Applications entries
    for exe_rel in receipt.installed_files.iter().filter(|f| f.to_lowercase().ends_with(".exe")) {
        if let Some(file_name) = Path::new(exe_rel).file_name().and_then(|f| f.to_str()) {
            let class_app = format!("Software\\Classes\\Applications\\{}", file_name);
            let class_app_w = to_wide(&class_app);
            for (root, _) in hives {
                unsafe {
                    let _ = RegDeleteTreeW(root, PCWSTR(class_app_w.as_ptr()));
                    let _ = RegDeleteKeyW(root, PCWSTR(class_app_w.as_ptr()));
                }
            }
        }
    }

    // 14. Windows Audio PolicyConfig (PropertyStore)
    let exe_lowers: Vec<String> = receipt
        .installed_files
        .iter()
        .filter(|f| f.to_lowercase().ends_with(".exe"))
        .filter_map(|f| Path::new(f).file_name().and_then(|n| n.to_str()))
        .map(|s| s.to_lowercase())
        .collect();
    let audio_count = clean_audio_policy_config(&install_dir_norm, &exe_lowers);
    if audio_count > 0 {
        logs.push(format!("已清理系统音频配置缓存 (Audio Policy, {} 处)", audio_count));
    }

    // 15. DirectX UserGpuPreferences
    if !install_dir_norm.is_empty() {
        let gpu_count = clean_matching_values_in_key(
            HKEY_CURRENT_USER,
            "Software\\Microsoft\\DirectX\\UserGpuPreferences",
            |name, _| {
                let lower = name.to_lowercase();
                lower.contains(&install_dir_norm)
            },
        );
        if gpu_count > 0 {
            logs.push(format!("已清理显卡首选项缓存 (DirectX GPU, {} 条)", gpu_count));
        }
    }

    // 16. Windows Defender Firewall Rules (HKLM)
    if !install_dir_norm.is_empty() {
        let fw_count = clean_matching_values_in_key(
            HKEY_LOCAL_MACHINE,
            "SYSTEM\\CurrentControlSet\\Services\\SharedAccess\\Parameters\\FirewallPolicy\\FirewallRules",
            |_, data| {
                let data_str = String::from_utf16_lossy(&bytes_to_u16_vec(data)).to_lowercase();
                data_str.contains(&install_dir_norm)
            },
        );
        if fw_count > 0 {
            logs.push(format!("已清理系统防火墙规则 (FirewallRules, {} 条)", fw_count));
        }
    }

    // 17. Explorer FileExts OpenWithList
    if !exe_lowers.is_empty() {
        let file_exts_base = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts";
        let file_exts_w = to_wide(file_exts_base);
        unsafe {
            let mut h_exts = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(file_exts_w.as_ptr()),
                0,
                KEY_READ,
                &mut h_exts,
            )
            .is_ok()
            {
                let mut ext_subs = Vec::new();
                let mut e_idx = 0u32;
                loop {
                    let mut name_buf = [0u16; 128];
                    let mut name_len = name_buf.len() as u32;
                    if RegEnumKeyExW(
                        h_exts,
                        e_idx,
                        PWSTR(name_buf.as_mut_ptr()),
                        &mut name_len,
                        None,
                        PWSTR::null(),
                        None,
                        None,
                    )
                    .is_err()
                    {
                        break;
                    }
                    ext_subs.push(String::from_utf16_lossy(&name_buf[..name_len as usize]));
                    e_idx += 1;
                }
                let _ = RegCloseKey(h_exts);

                for ext in ext_subs {
                    let open_with_key = format!("{}\\{}\\OpenWithList", file_exts_base, ext);
                    clean_matching_values_in_key(
                        HKEY_CURRENT_USER,
                        &open_with_key,
                        |_, data| {
                            let data_str = String::from_utf16_lossy(&bytes_to_u16_vec(data)).to_lowercase();
                            exe_lowers.iter().any(|e| data_str == *e)
                        },
                    );
                }
            }
        }
    }

    logs
}
