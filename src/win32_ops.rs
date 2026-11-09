use crate::package::{
    extract_pure_stub_bytes, CheckOperator, PrereqCondition, RegArchView, RegHiveRoot,
    UninstallReceipt, UNINSTALL_DATA_FILE, UNINSTALL_EXE_FILE,
};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GlobalFree, HANDLE, HGLOBAL, HWND, MAX_PATH};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    VS_FIXEDFILEINFO,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegDeleteValueW,
    RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, KEY_WRITE,
    KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE,
    REG_QWORD, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW,
    TerminateProcess, WaitForSingleObject, INFINITE, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, FOLDERID_Desktop, FOLDERID_Programs, IFileOpenDialog,
    IFileSaveDialog, IShellItem, IShellLinkW, SHGetKnownFolderPath, ShellExecuteExW,
    ShellExecuteW, ShellLink, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, KF_FLAG_DEFAULT, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_YESNO, SW_SHOWNORMAL,
};

const CF_UNICODETEXT: u32 = 13;

pub fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

pub fn expand_install_path(raw: &str) -> PathBuf {
    let mut result = raw.to_string();
    let vars = [
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "USERPROFILE",
        "TEMP",
    ];
    for var in vars {
        let token = format!("%{}%", var);
        if result.to_uppercase().contains(&token) {
            if let Ok(val) = env::var(var) {
                // Case-insensitive replacement
                let upper = result.to_uppercase();
                if let Some(pos) = upper.find(&token) {
                    result.replace_range(pos..pos + token.len(), &val);
                }
            }
        }
    }
    PathBuf::from(result)
}

/// Query free bytes and total bytes on the disk volume hosting `target_path`.
pub fn query_disk_space(target_path: &Path) -> Option<(u64, u64)> {
    let mut probe = target_path.to_path_buf();
    while !probe.exists() {
        if let Some(parent) = probe.parent() {
            if parent.as_os_str().is_empty() {
                break;
            }
            probe = parent.to_path_buf();
        } else {
            break;
        }
    }

    if !probe.exists() {
        // Fallback to root prefix like C:\
        let s = target_path.to_string_lossy();
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            probe = PathBuf::from(format!("{}\\", &s[0..2]));
        } else {
            probe = env::current_dir().ok()?;
        }
    }

    let wide = to_wide(&probe.to_string_lossy());
    let mut free_available = 0u64;
    let mut total_bytes = 0u64;
    let mut total_free = 0u64;

    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free_available),
            Some(&mut total_bytes),
            Some(&mut total_free),
        )
        .ok()?;
    }

    Some((free_available, total_bytes))
}

/// Open modern Win32 IFileOpenDialog to pick a folder.
pub fn pick_folder_dialog(owner: HWND, title: &str, initial_dir: Option<&Path>) -> Option<PathBuf> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;

        let mut options = dialog.GetOptions().ok()?;
        options |= FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        dialog.SetOptions(options).ok()?;

        let title_w = to_wide(title);
        let _ = dialog.SetTitle(PCWSTR(title_w.as_ptr()));

        let _ = initial_dir;

        if dialog.Show(owner).is_err() {
            return None;
        }

        let item: IShellItem = dialog.GetResult().ok()?;
        let path_pwstr: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path_str = path_pwstr.to_string().ok()?;
        windows::Win32::System::Com::CoTaskMemFree(Some(path_pwstr.0 as *const _));
        Some(PathBuf::from(path_str))
    }
}

/// Open modern Win32 IFileOpenDialog to pick an existing file.
pub fn pick_open_file_dialog(
    owner: HWND,
    title: &str,
    filter_name: &str,
    filter_pattern: &str,
) -> Option<PathBuf> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;

        let title_w = to_wide(title);
        let _ = dialog.SetTitle(PCWSTR(title_w.as_ptr()));

        let name_w = to_wide(filter_name);
        let spec_w = to_wide(filter_pattern);
        let specs = [COMDLG_FILTERSPEC {
            pszName: PCWSTR(name_w.as_ptr()),
            pszSpec: PCWSTR(spec_w.as_ptr()),
        }];
        let _ = dialog.SetFileTypes(&specs);

        if dialog.Show(owner).is_err() {
            return None;
        }

        let item: IShellItem = dialog.GetResult().ok()?;
        let path_pwstr: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path_str = path_pwstr.to_string().ok()?;
        windows::Win32::System::Com::CoTaskMemFree(Some(path_pwstr.0 as *const _));
        Some(PathBuf::from(path_str))
    }
}

/// Open modern Win32 IFileSaveDialog to pick an output `.exe` path.
pub fn pick_save_exe_dialog(owner: HWND, title: &str, default_filename: &str) -> Option<PathBuf> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;

        let mut options = dialog.GetOptions().ok()?;
        options |= FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT;
        let _ = dialog.SetOptions(options);

        let title_w = to_wide(title);
        let _ = dialog.SetTitle(PCWSTR(title_w.as_ptr()));

        let def_name_w = to_wide(default_filename);
        let _ = dialog.SetFileName(PCWSTR(def_name_w.as_ptr()));

        let ext_w = to_wide("exe");
        let _ = dialog.SetDefaultExtension(PCWSTR(ext_w.as_ptr()));

        let name_w = to_wide("Windows 安装程序 (*.exe)");
        let spec_w = to_wide("*.exe");
        let specs = [COMDLG_FILTERSPEC {
            pszName: PCWSTR(name_w.as_ptr()),
            pszSpec: PCWSTR(spec_w.as_ptr()),
        }];
        let _ = dialog.SetFileTypes(&specs);

        if dialog.Show(owner).is_err() {
            return None;
        }

        let item: IShellItem = dialog.GetResult().ok()?;
        let path_pwstr: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path_str = path_pwstr.to_string().ok()?;
        windows::Win32::System::Com::CoTaskMemFree(Some(path_pwstr.0 as *const _));
        Some(PathBuf::from(path_str))
    }
}

pub fn get_known_folder_path(is_desktop: bool) -> Option<PathBuf> {
    unsafe {
        let rfid = if is_desktop {
            &FOLDERID_Desktop
        } else {
            &FOLDERID_Programs
        };
        let pwstr = SHGetKnownFolderPath(rfid, KF_FLAG_DEFAULT, HANDLE::default()).ok()?;
        let s = pwstr.to_string().ok()?;
        windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0 as *const _));
        Some(PathBuf::from(s))
    }
}

/// Create a Windows `.lnk` shortcut file via COM `IShellLinkW` and `IPersistFile`.
pub fn create_shell_shortcut(
    target_exe: &Path,
    working_dir: &Path,
    shortcut_lnk_path: &Path,
    description: &str,
) -> Result<(), String> {
    if let Some(parent) = shortcut_lnk_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("初始化 IShellLinkW 失败: {}", e))?;

        let target_w = to_wide(&target_exe.to_string_lossy());
        shell_link
            .SetPath(PCWSTR(target_w.as_ptr()))
            .map_err(|e| format!("设置快捷方式目标路径失败: {}", e))?;

        let work_w = to_wide(&working_dir.to_string_lossy());
        shell_link
            .SetWorkingDirectory(PCWSTR(work_w.as_ptr()))
            .map_err(|e| format!("设置快捷方式工作目录失败: {}", e))?;

        let desc_w = to_wide(description);
        let _ = shell_link.SetDescription(PCWSTR(desc_w.as_ptr()));
        let _ = shell_link.SetIconLocation(PCWSTR(target_w.as_ptr()), 0);

        let persist_file: IPersistFile = shell_link
            .cast()
            .map_err(|e| format!("获取 IPersistFile 接口失败: {}", e))?;

        let lnk_w = to_wide(&shortcut_lnk_path.to_string_lossy());
        persist_file
            .Save(PCWSTR(lnk_w.as_ptr()), true)
            .map_err(|e| format!("保存快捷方式 {} 失败: {}", shortcut_lnk_path.display(), e))?;
    }

    Ok(())
}

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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RmUniqueProcess {
    dw_process_id: u32,
    process_start_time: windows::Win32::Foundation::FILETIME,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RmProcessInfo {
    process: RmUniqueProcess,
    str_app_name: [u16; 256],
    str_service_short_name: [u16; 64],
    application_type: i32,
    app_status: u32,
    ts_session_id: u32,
    b_restartable: i32,
}

impl Default for RmProcessInfo {
    fn default() -> Self {
        Self {
            process: RmUniqueProcess::default(),
            str_app_name: [0u16; 256],
            str_service_short_name: [0u16; 64],
            application_type: 0,
            app_status: 0,
            ts_session_id: 0,
            b_restartable: 0,
        }
    }
}

#[link(name = "rstrtmgr")]
extern "system" {
    fn RmStartSession(
        p_session_handle: *mut u32,
        dw_session_flags: u32,
        str_session_key: *mut u16,
    ) -> u32;
    fn RmRegisterResources(
        dw_session_handle: u32,
        n_files: u32,
        rgs_file_names: *const *const u16,
        n_applications: u32,
        rg_applications: *const RmUniqueProcess,
        n_services: u32,
        rgs_service_names: *const *const u16,
    ) -> u32;
    fn RmGetList(
        dw_session_handle: u32,
        pn_proc_info_needed: *mut u32,
        pn_proc_info: *mut u32,
        rg_affected_apps: *mut RmProcessInfo,
        lpdw_reboot_reasons: *mut u32,
    ) -> u32;
    fn RmEndSession(dw_session_handle: u32) -> u32;
}

/// Probe whether an existing file on disk is currently locked against overwriting
/// (e.g., mapped as an active `.exe`/`.dll` image section or held open without write sharing).
pub fn is_file_locked(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;

    let meta = match fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        _ => return false,
    };

    // Open with GENERIC_WRITE and FILE_SHARE_READ (1) only:
    // - Fails if any process mapped the .exe/.dll as an image section (ERROR_SHARING_VIOLATION 32 or ERROR_ACCESS_DENIED 5)
    // - Fails if any process holds a read handle without FILE_SHARE_WRITE, or any write handle
    match fs::OpenOptions::new()
        .write(true)
        .share_mode(1)
        .open(path)
    {
        Ok(_file) => false,
        Err(e) => match e.raw_os_error() {
            // 32 = ERROR_SHARING_VIOLATION, 33 = ERROR_LOCK_VIOLATION, 1224 = ERROR_USER_MAPPED_FILE
            Some(32) | Some(33) | Some(1224) => true,
            // 5 = ERROR_ACCESS_DENIED: when file is not marked read-only, kernel MmFlushImageSection denies write on mapped DLL/EXE
            Some(5) => !meta.permissions().readonly(),
            _ => false,
        },
    }
}

/// Collect all existing files inside `target_install_dir` (from `candidate_files` and directory scan)
/// that are currently locked by a process.
fn collect_locked_files_in_dir(
    target_install_dir: &Path,
    candidate_files: &[String],
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if !target_install_dir.exists() {
        return candidates;
    }

    for rel in candidate_files {
        let clean: PathBuf = rel
            .split(['\\', '/'])
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .collect();
        if clean.as_os_str().is_empty() {
            continue;
        }
        let full = target_install_dir.join(clean);
        if full.is_file() && !candidates.contains(&full) {
            candidates.push(full);
        }
    }

    // Also scan existing files inside `target_install_dir` (e.g. .dll, .exe, .sys, .ocx, .pyd)
    let mut stack = vec![target_install_dir.to_path_buf()];
    let mut scanned = 0usize;
    while let Some(dir) = stack.pop() {
        if scanned >= 512 {
            break;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.is_file() {
                    scanned += 1;
                    let fname_lower = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    if fname_lower == UNINSTALL_EXE_FILE || fname_lower == UNINSTALL_DATA_FILE {
                        continue;
                    }
                    if !candidates.contains(&p) {
                        candidates.push(p);
                    }
                    if scanned >= 512 {
                        break;
                    }
                }
            }
        }
    }

    candidates
        .into_iter()
        .filter(|p| is_file_locked(p))
        .collect()
}

fn query_process_full_path(pid: u32) -> Option<String> {
    unsafe {
        let h_proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path_buf = vec![0u16; 1024];
        let mut sz = path_buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h_proc,
            PROCESS_NAME_WIN32,
            PWSTR(path_buf.as_mut_ptr()),
            &mut sz,
        )
        .is_ok();
        let _ = CloseHandle(h_proc);
        if ok && sz > 0 {
            Some(String::from_utf16_lossy(&path_buf[..sz as usize]))
        } else {
            None
        }
    }
}

/// Use Windows Restart Manager (`Rstrtmgr.dll`) to identify all processes holding locks or image mappings
/// on `locked_files` (including external processes outside `target_install_dir` that loaded a `.dll`).
fn query_restart_manager_locking_processes(
    locked_files: &[PathBuf],
    self_pid: u32,
) -> Vec<(u32, String)> {
    let mut results = Vec::new();
    if locked_files.is_empty() {
        return results;
    }

    let wide_paths: Vec<Vec<u16>> = locked_files
        .iter()
        .map(|p| to_wide(&p.to_string_lossy()))
        .collect();
    let ptrs: Vec<*const u16> = wide_paths.iter().map(|w| w.as_ptr()).collect();

    unsafe {
        let mut session: u32 = 0;
        let mut session_key = [0u16; 64];
        if RmStartSession(&mut session, 0, session_key.as_mut_ptr()) != 0 {
            return results;
        }

        if RmRegisterResources(
            session,
            ptrs.len() as u32,
            ptrs.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        ) == 0
        {
            let mut needed: u32 = 0;
            let mut count: u32 = 16;
            let mut reboot_reasons: u32 = 0;
            let mut infos = vec![RmProcessInfo::default(); count as usize];

            let mut status = RmGetList(
                session,
                &mut needed,
                &mut count,
                infos.as_mut_ptr(),
                &mut reboot_reasons,
            );
            // 234 == ERROR_MORE_DATA
            if status == 234 && needed > count {
                count = needed;
                infos.resize(count as usize, RmProcessInfo::default());
                status = RmGetList(
                    session,
                    &mut needed,
                    &mut count,
                    infos.as_mut_ptr(),
                    &mut reboot_reasons,
                );
            }

            if status == 0 {
                for info in infos.iter().take(count as usize) {
                    let pid = info.process.dw_process_id;
                    if pid == 0 || pid == 4 || pid == self_pid {
                        continue;
                    }
                    let exe_name = if let Some(full_path) = query_process_full_path(pid) {
                        Path::new(&full_path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or(full_path)
                    } else {
                        let end = info
                            .str_app_name
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(info.str_app_name.len());
                        let app_str = String::from_utf16_lossy(&info.str_app_name[..end]);
                        if app_str.is_empty() {
                            format!("PID_{}", pid)
                        } else {
                            app_str
                        }
                    };
                    if !results.iter().any(|(p, _)| *p == pid) {
                        results.push((pid, exe_name));
                    }
                }
            }
        }

        let _ = RmEndSession(session);
    }

    results
}

/// Find running processes that actually occupy `target_install_dir`:
/// 1. If `target_install_dir` does not exist yet (fresh install), returns empty immediately (zero false positives).
/// 2. Probes existing target files in `target_install_dir` for write/mapping locks and queries Windows Restart Manager (`Rstrtmgr.dll`)
///    to catch any process (including external processes loading `.dll`s from `target_install_dir`).
/// 3. Checks running processes via `QueryFullProcessImageNameW` whose own executable path is strictly inside `target_install_dir\`.
/// Never returns the current installer/uninstaller process (`GetCurrentProcessId`).
pub fn find_running_target_processes(
    target_install_dir: &Path,
    candidate_files: &[String],
) -> Vec<(u32, String)> {
    let mut found = Vec::new();
    if !target_install_dir.exists() {
        return found;
    }

    let self_pid = unsafe { GetCurrentProcessId() };

    let target_dir_norm = target_install_dir
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    if target_dir_norm.is_empty() {
        return found;
    }
    let target_dir_prefix = format!("{}\\", target_dir_norm);

    // 1. Probe actual locked files in `target_install_dir` and query Windows Restart Manager (`Rstrtmgr.dll`)
    let locked_files = collect_locked_files_in_dir(target_install_dir, candidate_files);
    for (pid, name) in query_restart_manager_locking_processes(&locked_files, self_pid) {
        if !found.iter().any(|(p, _)| *p == pid) {
            found.push((pid, name));
        }
    }

    let locked_exe_basenames: Vec<String> = locked_files
        .iter()
        .filter_map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
        })
        .filter(|n| n.ends_with(".exe") && n != UNINSTALL_EXE_FILE)
        .collect();

    // 2. Enumerate process snapshot for processes executing strictly from inside `target_install_dir\`
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return found,
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                if pid != 0 && pid != 4 && pid != self_pid && !found.iter().any(|(p, _)| *p == pid)
                {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let exe_name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                    let exe_lower = exe_name.to_ascii_lowercase();

                    if exe_lower != UNINSTALL_EXE_FILE {
                        let matched = if let Some(full_path) = query_process_full_path(pid) {
                            let full_norm = full_path.replace('/', "\\").to_ascii_lowercase();
                            full_norm == target_dir_norm || full_norm.starts_with(&target_dir_prefix)
                        } else {
                            // Only fall back to basename match if we cannot query the process path (e.g. elevated process)
                            // AND the corresponding .exe inside `target_install_dir` is confirmed locked on disk.
                            locked_exe_basenames.iter().any(|w| *w == exe_lower)
                        };

                        if matched {
                            found.push((pid, exe_name));
                        }
                    }
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    found
}

/// Terminate all running target processes found via `find_running_target_processes` and wait for OS file locks to release.
pub fn terminate_running_target_processes(
    target_install_dir: &Path,
    exe_names: &[String],
) -> Vec<String> {
    let targets = find_running_target_processes(target_install_dir, exe_names);
    let mut logs = Vec::new();
    let mut killed_any = false;

    for (pid, exe_name) in targets {
        unsafe {
            let access = PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
            let h_res = OpenProcess(access, false, pid)
                .or_else(|_| OpenProcess(PROCESS_TERMINATE, false, pid));
            if let Ok(h_proc) = h_res {
                if TerminateProcess(h_proc, 0).is_ok() {
                    let _ = WaitForSingleObject(h_proc, 1500);
                    logs.push(format!(
                        "已自动结束运行中的进程: {} (PID: {})",
                        exe_name, pid
                    ));
                    killed_any = true;
                } else {
                    logs.push(format!(
                        "尝试结束进程失败 (权限不足): {} (PID: {})",
                        exe_name, pid
                    ));
                }
                let _ = CloseHandle(h_proc);
            }
        }
    }

    if killed_any {
        // Brief pause so the Windows kernel finishes releasing section handles on the .exe/.dll files
        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    logs
}

fn bytes_to_u16_vec(bytes: &[u8]) -> Vec<u16> {
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
                windows::core::PWSTR(name_buf.as_mut_ptr()),
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
                windows::core::PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                windows::core::PWSTR::null(),
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
fn clean_audio_policy_config(install_dir_norm: &str, exe_names: &[String]) -> usize {
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
/// Windows 10/11 Start Menu organizes desktop app tile properties hierarchically:
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Start\TileProperties\W~<Drive>:\<Path>\<Exe>`
/// or `W~{KnownFolderGUID}\<Path>\<Exe>`
/// or `W~<AUMID>` / `P~<PackageFamilyName>!<AppId>`.
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

/// Comprehensive deep cleaning of all registry entries associated with the application:
/// 1. `HKCU` & `HKLM` Uninstall keys (both 64-bit and 32-bit views) for `app_id` and `app_name`.
/// 2. `HKCU` & `HKLM` `App Paths` keys for all bundled `.exe` executables.
/// 3. `HKCU\Software\<Publisher>\<AppName>` and `HKCU\Software\<AppName>`.
/// 4. Windows Explorer Shell `MuiCache` entries matching install directory or app name.
/// 5. Start Menu Shortcut history (`UFH\SHC`) entries matching install directory or app name.
/// 6. Compatibility Assistant store (`AppCompatFlags`) entries matching install directory.
/// 7. Explorer FeatureUsage (`AppSwitched` & `ShowJumpView`).
/// 8. Start Menu TileProperties entries.
/// 9. Startup Run entries.
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
                        // If publisher key has no other subkeys left, delete publisher key itself
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

    // 7. Explorer FeatureUsage (All subkeys: AppLaunch, AppBadgeUpdated, AppSwitched, ShowJumpView, etc.)
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

    // 11. AppCompatFlags Layers (Compatibility and RunAsAdmin flags for exe paths)
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

    // 12. Notification Toast Settings (HKCU & HKLM)
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

    // 13. Classes Applications entries (HKCU & HKLM)
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

/// Launch an executable or open a folder in Windows Explorer.
pub fn shell_open(path: &Path, args: Option<&str>, work_dir: Option<&Path>) {
    let op_w = to_wide("open");
    let file_w = to_wide(&path.to_string_lossy());
    let args_w = args.map(to_wide);
    let dir_w = work_dir.map(|d| to_wide(&d.to_string_lossy()));

    unsafe {
        let _ = ShellExecuteW(
            HWND::default(),
            PCWSTR(op_w.as_ptr()),
            PCWSTR(file_w.as_ptr()),
            args_w
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            dir_w
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            SW_SHOWNORMAL,
        );
    }
}

pub fn get_clipboard_text(owner: HWND) -> Option<String> {
    unsafe {
        OpenClipboard(owner).ok()?;
        let res = (|| {
            let h_data = GetClipboardData(CF_UNICODETEXT).ok()?;
            let h_global = HGLOBAL(h_data.0);
            let ptr = GlobalLock(h_global) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *ptr.add(len) != 0 && len < 65536 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let text = String::from_utf16_lossy(slice);
            let _ = GlobalUnlock(h_global);
            Some(text)
        })();
        let _ = CloseClipboard();
        res
    }
}

pub fn set_clipboard_text(owner: HWND, text: &str) -> bool {
    unsafe {
        if OpenClipboard(owner).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let wide = to_wide(text);
        let byte_len = wide.len() * 2;
        let Ok(h_global) = GlobalAlloc(GMEM_MOVEABLE, byte_len) else {
            let _ = CloseClipboard();
            return false;
        };
        let ptr = GlobalLock(h_global) as *mut u16;
        if ptr.is_null() {
            let _ = GlobalFree(h_global);
            let _ = CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        let _ = GlobalUnlock(h_global);
        let ok = SetClipboardData(CF_UNICODETEXT, HANDLE(h_global.0)).is_ok();
        if !ok {
            let _ = GlobalFree(h_global);
        }
        let _ = CloseClipboard();
        ok
    }
}

/// Show a demo application greeting when launched with `--demo-app`.
pub fn run_demo_app_greeting() {
    let exe_path = env::current_exe().unwrap_or_default();
    let msg = format!(
        "欢迎运行 Nyan Studio 示例应用程序！\n\n\
        当前运行路径:\n{}\n\n\
        静态 CRT 链接: 已启用 (+crt-static)\n\
        如果您是通过安装程序安装的本应用，您可以在桌面、开始菜单或 Windows“设置 -> 应用 -> 安装的应用”中查看或卸载它。",
        exe_path.display()
    );
    let title_w = to_wide("Nyan Studio App - 运行成功");
    let msg_w = to_wide(&msg);
    unsafe {
        MessageBoxW(
            HWND::default(),
            PCWSTR(msg_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Create a ready-to-pack demo source folder containing a runnable executable and sample assets.
pub fn create_demo_app_directory(base_dir: &Path) -> Result<(PathBuf, String), String> {
    let demo_src = base_dir.join("demo_app_source");
    fs::create_dir_all(demo_src.join("assets"))
        .map_err(|e| format!("无法创建演示目录: {}", e))?;
    fs::create_dir_all(demo_src.join("config"))
        .map_err(|e| format!("无法创建配置子目录: {}", e))?;

    // Copy pure stub as `NyanApp.exe` and also create a marker `nyan_demo_app.json`
    // so when `NyanApp.exe` is double-clicked, it recognizes it's the demo target app!
    if let Ok(cur_exe) = env::current_exe() {
        if let Ok(stub) = extract_pure_stub_bytes(&cur_exe) {
            let _ = fs::write(demo_src.join("NyanApp.exe"), stub);
        }
    }

    fs::write(
        demo_src.join("nyan_demo_app.json"),
        r#"{"app": "Nyan Studio App", "version": "1.0.0", "mode": "demo"}"#,
    )
    .map_err(|e| e.to_string())?;

    fs::write(
        demo_src.join("README.txt"),
        "Nyan Studio App v1.0.0\r\n======================\r\n这是由 Nyan Setup Studio 自动生成的演示应用程序包。\r\n包含主程序、配置文件与静态资源。\r\n",
    )
    .map_err(|e| e.to_string())?;

    fs::write(
        demo_src.join("config").join("settings.json"),
        r#"{
  "theme": "material-indigo",
  "hardware_acceleration": true,
  "crt_linkage": "static"
}"#,
    )
    .map_err(|e| e.to_string())?;

    // Generate a sample resource file (~120 KB compressible data) to demonstrate compression ratio
    let mut sample_asset = String::with_capacity(128 * 1024);
    for i in 0..2000 {
        sample_asset.push_str(&format!(
            "[AssetChunk #{:04}] Material Design Vector Shader & Resource Table Entry - NyanSetup Static CRT Engine\r\n",
            i
        ));
    }
    fs::write(demo_src.join("assets").join("resources.pak"), sample_asset)
        .map_err(|e| e.to_string())?;

    let _ = MAX_PATH;
    Ok((demo_src, "NyanApp.exe".to_string()))
}

// ============================================================================
// Prerequisite Environment Check & Execution Engine
// ============================================================================

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

            // If value_name is empty and user only checks Exists / NotExists, key existence is enough
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

#[cfg(test)]
mod tests {
    use super::*;

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

        // 3. Lock `plugin.dll` from an external child process (`powershell` / `cmd`) outside `temp_root`
        //    and verify both `is_file_locked` and Restart Manager (`query_restart_manager_locking_processes`) detect it!
        {
            use std::os::windows::fs::OpenOptionsExt;
            let locked_handle = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0) // exclusive lock
                .open(&dummy_dll)
                .unwrap();
            assert!(
                is_file_locked(&dummy_dll),
                "Exclusively opened DLL/file must be detected as locked"
            );
            // Query Restart Manager with dummy self_pid = 0 so our own lock is returned
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
}
