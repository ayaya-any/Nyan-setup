pub mod events;
pub mod log_box;
pub mod steps;
pub mod window;

pub use window::run_installer_or_uninstaller_gui;

use crate::d2d::{D2DRenderer, MaterialPalette, RectF, RippleEffect, TextInputState};
use crate::package::{
    extract_package_files, InstallProgressEvent, PackageManifest, ProcessConflictPolicy,
    UninstallReceipt,
};
use crate::win32_ops::{
    create_shell_shortcut, detect_existing_installation, expand_install_path,
    find_running_target_processes, get_known_folder_path, query_disk_space,
    register_uninstall_entry, terminate_running_target_processes, write_uninstaller_files,
    ExistingInstallInfo,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SetTimer, WM_USER};

pub(crate) const WM_APP_INSTALL_EVENT: u32 = WM_USER + 202;
pub(crate) const TIMER_ANIM: usize = 1;
pub(crate) const TIMER_CARET: usize = 2;

pub const INST_WIDTH_DIP: f32 = 760.0;
pub const INST_HEIGHT_DIP: f32 = 540.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    WelcomeAndLicense = 0,
    CustomOptions = 1,
    Installing = 2,
    Completed = 3,
}

pub enum WizardMode {
    Installer {
        installer_exe: PathBuf,
        manifest: PackageManifest,
    },
    Uninstaller {
        uninstaller_exe: PathBuf,
        receipt: UninstallReceipt,
    },
}

pub struct InstallerWindowState {
    pub hwnd: HWND,
    pub renderer: D2DRenderer,
    pub palette: MaterialPalette,
    pub width_dip: f32,
    pub height_dip: f32,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub ripples: Vec<RippleEffect>,

    pub mode: WizardMode,
    pub step: WizardStep,

    // Installer user selections
    pub license_accepted: bool,
    pub license_scroll_line: usize,
    pub install_dir_input: String,
    pub path_focused: bool,
    pub text_input: TextInputState,
    pub caret_visible: bool,
    pub opt_desktop_shortcut: bool,
    pub opt_start_menu_shortcut: bool,
    pub opt_register_uninstaller: bool,
    pub opt_launch_now: bool,
    pub opt_kill_running_process: bool,

    // Existing installation & running processes detection (Update Mode)
    pub existing_install: Option<ExistingInstallInfo>,
    pub running_processes: Vec<(u32, String)>,
    pub show_process_conflict_modal: bool,

    // Disk space info (free_bytes, total_bytes)
    pub disk_space: Option<(u64, u64)>,

    // Progress & logs
    pub is_working: bool,
    pub progress: f32,
    pub target_progress: f32,
    pub status_text: String,
    pub logs: Vec<String>,
    pub log_scroll_x: f32,
    pub log_scroll_y: f32,
    pub log_auto_scroll: bool,
    pub dragging_log_hscroll: Option<f32>,
    pub dragging_log_vscroll: Option<f32>,
    pub install_error: Option<String>,
    pub rx: Option<Receiver<InstallProgressEvent>>,
    pub uninstall_done: bool,
}

impl InstallerWindowState {
    pub fn new_installer(
        renderer: D2DRenderer,
        installer_exe: PathBuf,
        manifest: PackageManifest,
    ) -> Self {
        let default_dir = expand_install_path(&manifest.config.default_install_dir);
        let existing_install =
            detect_existing_installation(&manifest.config.app_id, &default_dir);

        let initial_dir = if let Some(ref ex) = existing_install {
            ex.install_location.clone()
        } else {
            default_dir
        };

        let install_dir_str = initial_dir.to_string_lossy().to_string();
        let disk_space = query_disk_space(&initial_dir);

        let mut candidate_files: Vec<String> = manifest
            .files
            .iter()
            .map(|f| f.relative_path.clone())
            .collect();
        if !manifest.config.main_exe.is_empty()
            && !candidate_files.contains(&manifest.config.main_exe)
        {
            candidate_files.push(manifest.config.main_exe.clone());
        }
        let running_processes = find_running_target_processes(&initial_dir, &candidate_files);

        let opt_desk = manifest.config.create_desktop_shortcut;
        let opt_menu = manifest.config.create_start_menu_shortcut;
        let opt_unins = manifest.config.register_uninstaller;
        let opt_launch = manifest.config.auto_launch_after_install;
        let opt_kill = manifest.config.kill_process_on_update;

        Self {
            hwnd: HWND::default(),
            renderer,
            palette: MaterialPalette::light(),
            width_dip: INST_WIDTH_DIP,
            height_dip: INST_HEIGHT_DIP,
            mouse_x: -1.0,
            mouse_y: -1.0,
            ripples: Vec::new(),
            mode: WizardMode::Installer {
                installer_exe,
                manifest,
            },
            step: WizardStep::WelcomeAndLicense,
            license_accepted: true,
            license_scroll_line: 0,
            install_dir_input: install_dir_str,
            path_focused: false,
            text_input: TextInputState::new(),
            caret_visible: true,
            opt_desktop_shortcut: opt_desk,
            opt_start_menu_shortcut: opt_menu,
            opt_register_uninstaller: opt_unins,
            opt_launch_now: opt_launch,
            opt_kill_running_process: opt_kill,
            existing_install,
            running_processes,
            show_process_conflict_modal: false,
            disk_space,
            is_working: false,
            progress: 0.0,
            target_progress: 0.0,
            status_text: "准备就绪".to_string(),
            logs: Vec::new(),
            log_scroll_x: 0.0,
            log_scroll_y: 0.0,
            log_auto_scroll: true,
            dragging_log_hscroll: None,
            dragging_log_vscroll: None,
            install_error: None,
            rx: None,
            uninstall_done: false,
        }
    }

    pub fn new_uninstaller(
        renderer: D2DRenderer,
        uninstaller_exe: PathBuf,
        receipt: UninstallReceipt,
    ) -> Self {
        Self {
            hwnd: HWND::default(),
            renderer,
            palette: MaterialPalette::light(),
            width_dip: INST_WIDTH_DIP,
            height_dip: INST_HEIGHT_DIP,
            mouse_x: -1.0,
            mouse_y: -1.0,
            ripples: Vec::new(),
            mode: WizardMode::Uninstaller {
                uninstaller_exe,
                receipt,
            },
            step: WizardStep::WelcomeAndLicense,
            license_accepted: true,
            license_scroll_line: 0,
            install_dir_input: String::new(),
            path_focused: false,
            text_input: TextInputState::new(),
            caret_visible: false,
            opt_desktop_shortcut: false,
            opt_start_menu_shortcut: false,
            opt_register_uninstaller: false,
            opt_launch_now: false,
            opt_kill_running_process: true,
            existing_install: None,
            running_processes: Vec::new(),
            show_process_conflict_modal: false,
            disk_space: None,
            is_working: false,
            progress: 0.0,
            target_progress: 0.0,
            status_text: "准备卸载应用程序".to_string(),
            logs: Vec::new(),
            log_scroll_x: 0.0,
            log_scroll_y: 0.0,
            log_auto_scroll: true,
            dragging_log_hscroll: None,
            dragging_log_vscroll: None,
            install_error: None,
            rx: None,
            uninstall_done: false,
        }
    }

    pub fn refresh_disk_space(&mut self) {
        let p = PathBuf::from(self.install_dir_input.trim());
        self.disk_space = query_disk_space(&p);
        if let WizardMode::Installer { ref manifest, .. } = self.mode {
            self.existing_install = detect_existing_installation(&manifest.config.app_id, &p);
            let mut candidate_files: Vec<String> = manifest
                .files
                .iter()
                .map(|f| f.relative_path.clone())
                .collect();
            if !manifest.config.main_exe.is_empty()
                && !candidate_files.contains(&manifest.config.main_exe)
            {
                candidate_files.push(manifest.config.main_exe.clone());
            }
            self.running_processes = find_running_target_processes(&p, &candidate_files);
        }
    }

    pub fn has_enough_disk_space(&self) -> bool {
        let required = match &self.mode {
            WizardMode::Installer { manifest, .. } => {
                manifest.total_uncompressed_bytes + 2 * 1024 * 1024
            }
            WizardMode::Uninstaller { .. } => 0,
        };
        if let Some((free, _)) = self.disk_space {
            free >= required
        } else {
            true
        }
    }

    pub fn add_ripple(
        &mut self,
        x: f32,
        y: f32,
        bounds: RectF,
        color: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
    ) {
        self.ripples.push(RippleEffect::new(x, y, bounds, color));
        unsafe {
            let _ = SetTimer(self.hwnd, TIMER_ANIM, 16, None);
        }
    }

    pub fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    pub fn try_begin_installation(&mut self) {
        self.refresh_disk_space();
        let policy = match &self.mode {
            WizardMode::Installer { manifest, .. } => manifest.config.process_conflict_policy,
            _ => return,
        };
        if policy == ProcessConflictPolicy::PromptUser && !self.running_processes.is_empty() {
            self.show_process_conflict_modal = true;
            self.invalidate();
            return;
        }
        self.opt_kill_running_process = true;
        self.start_installation();
    }

    pub fn terminate_occupying_processes(&mut self) {
        let target_dir = PathBuf::from(self.install_dir_input.trim());
        if let WizardMode::Installer { ref manifest, .. } = self.mode {
            let mut candidate_files: Vec<String> = manifest
                .files
                .iter()
                .map(|f| f.relative_path.clone())
                .collect();
            if !manifest.config.main_exe.is_empty()
                && !candidate_files.contains(&manifest.config.main_exe)
            {
                candidate_files.push(manifest.config.main_exe.clone());
            }
            let _ = terminate_running_target_processes(&target_dir, &candidate_files);
        }
        self.refresh_disk_space();
    }

    pub fn start_installation(&mut self) {
        if self.is_working {
            return;
        }
        let (installer_exe, manifest) = match &self.mode {
            WizardMode::Installer {
                installer_exe,
                manifest,
            } => (installer_exe.clone(), manifest.clone()),
            _ => return,
        };

        let target_dir = PathBuf::from(self.install_dir_input.trim());
        let create_desk = self.opt_desktop_shortcut;
        let create_menu = self.opt_start_menu_shortcut;
        let reg_unins = self.opt_register_uninstaller;
        let kill_procs = self.opt_kill_running_process;
        let is_update = self.existing_install.is_some();
        let hwnd_raw = self.hwnd.0 as isize;

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.step = WizardStep::Installing;
        self.is_working = true;
        self.progress = 0.02;
        self.target_progress = 0.05;
        self.install_error = None;
        self.logs.clear();
        self.log_scroll_x = 0.0;
        self.log_scroll_y = 0.0;
        self.log_auto_scroll = true;
        self.dragging_log_hscroll = None;
        self.dragging_log_vscroll = None;
        if let Some(ref ex) = self.existing_install {
            self.logs.push(format!(
                "[覆盖更新] 检测到已安装 v{} → 升级/更新至 v{}",
                ex.installed_version, manifest.config.app_version
            ));
        }
        self.logs
            .push(format!("[目标路径] {}", target_dir.display()));
        self.status_text = if is_update {
            "正在准备覆盖更新并校验安装包数据...".to_string()
        } else {
            "正在解压并校验安装包数据...".to_string()
        };

        unsafe {
            let _ = SetTimer(self.hwnd, TIMER_ANIM, 16, None);
        }

        thread::spawn(move || {
            match extract_package_files(
                &installer_exe,
                &manifest,
                &target_dir,
                kill_procs,
                Some(&tx),
            ) {
                Ok(installed_files) => {
                    let main_exe_full = target_dir.join(&manifest.config.main_exe);
                    let mut desk_lnk_saved = None;
                    let mut menu_lnk_saved = None;

                    if create_desk || create_menu {
                        let _ = tx.send(InstallProgressEvent::CreatingShortcuts);
                    }

                    if create_desk {
                        if let Some(desk_dir) = get_known_folder_path(true) {
                            let lnk = desk_dir.join(format!("{}.lnk", manifest.config.app_name));
                            if create_shell_shortcut(
                                &main_exe_full,
                                &target_dir,
                                &lnk,
                                &manifest.config.app_name,
                            )
                            .is_ok()
                            {
                                desk_lnk_saved = Some(lnk.to_string_lossy().to_string());
                            }
                        }
                    }

                    if create_menu {
                        if let Some(prog_dir) = get_known_folder_path(false) {
                            let lnk = prog_dir
                                .join(&manifest.config.app_name)
                                .join(format!("{}.lnk", manifest.config.app_name));
                            if create_shell_shortcut(
                                &main_exe_full,
                                &target_dir,
                                &lnk,
                                &manifest.config.app_name,
                            )
                            .is_ok()
                            {
                                menu_lnk_saved = Some(lnk.to_string_lossy().to_string());
                            }
                        }
                    }

                    if reg_unins {
                        let _ = tx.send(InstallProgressEvent::RegisteringUninstaller);
                        let receipt = UninstallReceipt {
                            app_id: manifest.config.app_id.clone(),
                            app_name: manifest.config.app_name.clone(),
                            app_version: manifest.config.app_version.clone(),
                            publisher: manifest.config.publisher.clone(),
                            install_dir: target_dir.to_string_lossy().to_string(),
                            installed_files,
                            desktop_shortcut: desk_lnk_saved,
                            start_menu_shortcut: menu_lnk_saved,
                            registry_key_created: true,
                        };

                        if let Ok(unins_exe) =
                            write_uninstaller_files(&installer_exe, &target_dir, &receipt)
                        {
                            let est_kb = (manifest.total_uncompressed_bytes / 1024).max(64) as u32;
                            let _ = register_uninstall_entry(
                                &manifest.config.app_id,
                                &manifest.config.app_name,
                                &manifest.config.app_version,
                                &manifest.config.publisher,
                                &target_dir,
                                &main_exe_full,
                                &unins_exe,
                                est_kb,
                            );
                        }
                    }

                    let _ = tx.send(InstallProgressEvent::Completed {
                        install_dir: target_dir,
                    });
                }
                Err(e) => {
                    let _ = tx.send(InstallProgressEvent::Error(e));
                }
            }

            unsafe {
                let hwnd = HWND(hwnd_raw as *mut _);
                let _ = PostMessageW(hwnd, WM_APP_INSTALL_EVENT, WPARAM(0), LPARAM(0));
            }
        });

        self.invalidate();
    }

    pub fn poll_install_events(&mut self) {
        let mut events = Vec::new();
        if let Some(ref rx) = self.rx {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }

        for ev in events {
            match ev {
                InstallProgressEvent::KillingProcesses(msg) => {
                    self.status_text = msg.clone();
                    self.logs.push(format!("[进程管理] {}", msg));
                }
                InstallProgressEvent::Extracting {
                    current_index,
                    total_files,
                    relative_path,
                    bytes_done,
                    bytes_total,
                } => {
                    let ratio = (bytes_done as f32) / (bytes_total.max(1) as f32);
                    self.target_progress = 0.08 + ratio * 0.72;
                    self.status_text = format!(
                        "正在释放文件 ({}/{}): {}",
                        current_index, total_files, relative_path
                    );
                    self.logs
                        .push(format!("  [CRC32 OK] 释放: {}", relative_path));
                }
                InstallProgressEvent::RunningPrerequisite(name) => {
                    self.target_progress = 0.84;
                    self.status_text = format!("正在检测并配置前置运行环境: {} ...", name);
                    self.logs
                        .push(format!("  [环境组件] 正在执行前置环境动作: {}", name));
                }
                InstallProgressEvent::CreatingShortcuts => {
                    self.target_progress = 0.88;
                    self.status_text = "正在创建桌面与开始菜单快捷方式...".to_string();
                    self.logs.push("  [快捷方式] 已生成 .lnk 快捷图标".to_string());
                }
                InstallProgressEvent::RegisteringUninstaller => {
                    self.target_progress = 0.95;
                    self.status_text = "正在生成 unins000.exe 并注册系统卸载项...".to_string();
                    self.logs
                        .push("  [注册表] 已写入 Windows 应用卸载信息".to_string());
                }
                InstallProgressEvent::Completed { install_dir } => {
                    self.is_working = false;
                    self.progress = 1.0;
                    self.target_progress = 1.0;
                    self.status_text = format!("已完成: {}", install_dir.display());
                    self.logs.push("[完成] 所有组件均已成功部署！".to_string());
                    self.step = WizardStep::Completed;
                }
                InstallProgressEvent::Error(err) => {
                    self.is_working = false;
                    self.status_text = format!("安装出错: {}", err);
                    self.install_error = Some(err.clone());
                    self.logs.push(format!("[错误] {}", err));
                }
            }
        }
    }

    pub(crate) fn installer_card_rect(&self) -> RectF {
        RectF::new(18.0, 98.0, self.width_dip - 36.0, self.height_dip - 114.0)
    }

    pub(crate) fn process_conflict_modal_rect(&self) -> RectF {
        let mw = 516.0;
        let mh = 336.0;
        RectF::new(
            (self.width_dip - mw) * 0.5,
            (self.height_dip - mh) * 0.5 + 14.0,
            mw,
            mh,
        )
    }
}
