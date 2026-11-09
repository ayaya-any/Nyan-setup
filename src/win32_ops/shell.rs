use crate::package::extract_pure_stub_bytes;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND, MAX_PATH};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, FOLDERID_Desktop, FOLDERID_Programs, IFileOpenDialog,
    IFileSaveDialog, IShellItem, IShellLinkW, SHGetKnownFolderPath, ShellExecuteW, ShellLink,
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, KF_FLAG_DEFAULT,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONINFORMATION, MB_OK, SW_SHOWNORMAL,
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
