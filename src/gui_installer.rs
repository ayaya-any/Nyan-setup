use crate::d2d::{
    rgb, rgba, ButtonVariant, D2DRenderer, MaterialPalette, RectF, RippleEffect, ThemeMode,
    VectorIcon,
};
use crate::package::{
    extract_package_files, format_bytes, InstallProgressEvent, PackageManifest, UninstallReceipt,
};
use crate::win32_ops::{
    create_shell_shortcut, detect_existing_installation, execute_uninstall_steps,
    expand_install_path, find_running_target_processes, get_clipboard_text, get_known_folder_path,
    pick_folder_dialog, query_disk_space, register_uninstall_entry, schedule_self_delete,
    shell_open, to_wide, write_uninstaller_files, ExistingInstallInfo,
};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetFocus, VK_BACK, VK_CONTROL, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, LoadCursorW, LoadIconW, PostMessageW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage,
    CREATESTRUCTW, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HTCAPTION, IDC_ARROW,
    ICON_BIG, ICON_SMALL, MSG, SM_CXSCREEN, SM_CYSCREEN, SW_MINIMIZE, SW_SHOW, WM_CHAR,
    WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETICON, WM_SIZE, WM_TIMER, WM_USER, WNDCLASSW,
    WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
};

const WM_APP_INSTALL_EVENT: u32 = WM_USER + 202;
const TIMER_ANIM: usize = 1;
const TIMER_CARET: usize = 2;

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
    pub caret_visible: bool,
    pub opt_desktop_shortcut: bool,
    pub opt_start_menu_shortcut: bool,
    pub opt_register_uninstaller: bool,
    pub opt_launch_now: bool,
    pub opt_kill_running_process: bool,

    // Existing installation & running processes detection (Update Mode)
    pub existing_install: Option<ExistingInstallInfo>,
    pub running_processes: Vec<(u32, String)>,

    // Disk space info (free_bytes, total_bytes)
    pub disk_space: Option<(u64, u64)>,

    // Progress & logs
    pub is_working: bool,
    pub progress: f32,
    pub target_progress: f32,
    pub status_text: String,
    pub logs: Vec<String>,
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

        let mut exe_names: Vec<String> = manifest
            .files
            .iter()
            .filter(|f| f.relative_path.to_ascii_lowercase().ends_with(".exe"))
            .map(|f| f.relative_path.clone())
            .collect();
        if !manifest.config.main_exe.is_empty() && !exe_names.contains(&manifest.config.main_exe) {
            exe_names.push(manifest.config.main_exe.clone());
        }
        let running_processes = find_running_target_processes(&initial_dir, &exe_names);

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
            caret_visible: true,
            opt_desktop_shortcut: opt_desk,
            opt_start_menu_shortcut: opt_menu,
            opt_register_uninstaller: opt_unins,
            opt_launch_now: opt_launch,
            opt_kill_running_process: opt_kill,
            existing_install,
            running_processes,
            disk_space,
            is_working: false,
            progress: 0.0,
            target_progress: 0.0,
            status_text: "准备就绪".to_string(),
            logs: Vec::new(),
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
            caret_visible: false,
            opt_desktop_shortcut: false,
            opt_start_menu_shortcut: false,
            opt_register_uninstaller: false,
            opt_launch_now: false,
            opt_kill_running_process: true,
            existing_install: None,
            running_processes: Vec::new(),
            disk_space: None,
            is_working: false,
            progress: 0.0,
            target_progress: 0.0,
            status_text: "准备卸载应用程序".to_string(),
            logs: Vec::new(),
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
            let mut exe_names: Vec<String> = manifest
                .files
                .iter()
                .filter(|f| f.relative_path.to_ascii_lowercase().ends_with(".exe"))
                .map(|f| f.relative_path.clone())
                .collect();
            if !manifest.config.main_exe.is_empty()
                && !exe_names.contains(&manifest.config.main_exe)
            {
                exe_names.push(manifest.config.main_exe.clone());
            }
            self.running_processes = find_running_target_processes(&p, &exe_names);
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
                    self.target_progress = 0.08 + ratio * 0.76;
                    self.status_text = format!(
                        "正在释放文件 ({}/{}): {}",
                        current_index, total_files, relative_path
                    );
                    self.logs
                        .push(format!("  [CRC32 OK] 释放: {}", relative_path));
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

    fn installer_card_rect(&self) -> RectF {
        RectF::new(18.0, 98.0, self.width_dip - 36.0, self.height_dip - 114.0)
    }

    pub fn draw(&mut self) {
        if !self.hwnd.0.is_null() {
            let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
            if self.renderer.ensure_target(self.hwnd, dpi).is_err() {
                return;
            }
        } else if self
            .renderer
            .ensure_surface(self.width_dip as u32, self.height_dip as u32, 96)
            .is_err()
        {
            return;
        }

        let w = self.width_dip;
        let h = self.height_dip;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let pal = self.palette;

        unsafe {
            self.renderer.rt().BeginDraw();
        }

        self.renderer.fill_rect(RectF::new(0.0, 0.0, w, h), pal.bg);

        let hover_theme = D2DRenderer::header_theme_rect(w).contains(mx, my);
        let hover_min = D2DRenderer::header_min_rect(w).contains(mx, my);
        let hover_close = D2DRenderer::header_close_rect(w).contains(mx, my);

        match &self.mode {
            WizardMode::Installer { manifest, .. } => {
                let is_update = self.existing_install.is_some();
                let title = if is_update {
                    format!(
                        "{} v{} 更新向导",
                        manifest.config.app_name, manifest.config.app_version
                    )
                } else {
                    format!(
                        "{} v{} 安装向导",
                        manifest.config.app_name, manifest.config.app_version
                    )
                };
                let badge = if is_update {
                    "覆盖更新模式"
                } else {
                    "CRT-Static"
                };
                self.renderer.draw_window_header(
                    w,
                    &title,
                    badge,
                    &pal,
                    hover_theme,
                    hover_min,
                    hover_close,
                );
                self.draw_installer_stepper();
                let card = self.installer_card_rect();
                self.renderer.draw_card(card, 14.0, &pal);

                let manifest_clone = manifest.clone();
                match self.step {
                    WizardStep::WelcomeAndLicense => self.draw_step_welcome(card, &manifest_clone),
                    WizardStep::CustomOptions => self.draw_step_options(card, &manifest_clone),
                    WizardStep::Installing => self.draw_step_installing(card),
                    WizardStep::Completed => self.draw_step_completed(card, &manifest_clone),
                }
            }
            WizardMode::Uninstaller { receipt, .. } => {
                let title = format!("{} 卸载向导", receipt.app_name);
                self.renderer.draw_window_header(
                    w,
                    &title,
                    "Uninstaller",
                    &pal,
                    hover_theme,
                    hover_min,
                    hover_close,
                );
                let card = RectF::new(18.0, 52.0, w - 36.0, h - 68.0);
                self.renderer.draw_card(card, 14.0, &pal);
                let receipt_clone = receipt.clone();
                self.draw_uninstaller_view(card, &receipt_clone);
            }
        }

        self.renderer.draw_rounded_rect(
            RectF::new(0.5, 0.5, w - 1.0, h - 1.0),
            8.0,
            1.0,
            pal.outline,
        );

        let hr = unsafe { self.renderer.rt().EndDraw(None, None) };
        if hr.is_err() {
            self.renderer.discard_target();
        }
    }

    /// Draw a clean Pill Stepper where connector lines only span gaps between pills.
    fn draw_installer_stepper(&self) {
        let pal = self.palette;
        let steps = if self.existing_install.is_some() {
            ["许可协议", "更新选项", "正在更新", "更新完成"]
        } else {
            ["许可协议", "安装选项", "正在安装", "安装完成"]
        };
        let cur = self.step as usize;

        let pill_w = 148.0;
        let pill_h = 32.0;
        let gap = 44.0;
        let total_w = pill_w * 4.0 + gap * 3.0;
        let start_x = (self.width_dip - total_w) * 0.5;
        let pill_y = 54.0;
        let cy = pill_y + pill_h * 0.5;

        for (i, label) in steps.iter().enumerate() {
            let px = start_x + (i as f32) * (pill_w + gap);
            let pill_rect = RectF::new(px, pill_y, pill_w, pill_h);

            // Connector line ONLY in the gap between pill i and pill i+1
            if i + 1 < steps.len() {
                let line_x1 = px + pill_w + 6.0;
                let line_x2 = px + pill_w + gap - 6.0;
                let line_col = if i < cur {
                    pal.success
                } else {
                    pal.outline
                };
                self.renderer
                    .draw_line(line_x1, cy, line_x2, cy, 2.0, line_col);
            }

            // Pill background & border
            if i == cur {
                self.renderer
                    .fill_rounded_rect(pill_rect, 16.0, pal.primary_container);
                self.renderer
                    .draw_rounded_rect(pill_rect, 16.0, 1.2, pal.primary);
            } else if i < cur {
                self.renderer.fill_rounded_rect(pill_rect, 16.0, pal.surface);
                self.renderer
                    .draw_rounded_rect(pill_rect, 16.0, 1.0, pal.success);
            } else {
                self.renderer.fill_rounded_rect(pill_rect, 16.0, pal.surface);
                self.renderer
                    .draw_rounded_rect(pill_rect, 16.0, 1.0, pal.outline_subtle);
            }

            // Circle indicator inside left side of pill
            let circle_cx = px + 18.0;
            if i < cur {
                self.renderer.fill_circle(circle_cx, cy, 10.0, pal.success);
                self.renderer
                    .draw_icon(VectorIcon::Check, circle_cx, cy, 11.0, rgb(0xFFFFFF));
            } else if i == cur {
                self.renderer.fill_circle(circle_cx, cy, 10.0, pal.primary);
                self.renderer.draw_text(
                    &format!("{}", i + 1),
                    &self.renderer.font_caption,
                    RectF::new(circle_cx - 10.0, cy - 10.0, 20.0, 20.0),
                    rgb(0xFFFFFF),
                    1,
                    1,
                    false,
                );
            } else {
                self.renderer
                    .fill_circle(circle_cx, cy, 10.0, pal.surface_variant);
                self.renderer
                    .draw_circle(circle_cx, cy, 10.0, 1.2, pal.outline);
                self.renderer.draw_text(
                    &format!("{}", i + 1),
                    &self.renderer.font_caption,
                    RectF::new(circle_cx - 10.0, cy - 10.0, 20.0, 20.0),
                    pal.text_muted,
                    1,
                    1,
                    false,
                );
            }

            let text_col = if i == cur {
                pal.on_primary_container
            } else if i < cur {
                pal.text_primary
            } else {
                pal.text_muted
            };
            self.renderer.draw_text(
                label,
                &self.renderer.font_body_bold,
                RectF::new(px + 34.0, pill_y, pill_w - 40.0, pill_h),
                text_col,
                0,
                1,
                false,
            );
        }
    }

    fn draw_step_welcome(&self, card: RectF, manifest: &PackageManifest) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;

        // App Hero Banner (y = card.y + 18.0, h = 72.0)
        let hero = RectF::new(x0, card.y + 18.0, w_full, 72.0);
        self.renderer
            .fill_rounded_rect(hero, 12.0, pal.primary_container);

        let logo_box = RectF::new(hero.x + 16.0, hero.y + 10.0, 52.0, 52.0);
        self.renderer.draw_nyan_app_icon(logo_box);

        let hero_title = if let Some(ref ex) = self.existing_install {
            format!(
                "检测到已安装 v{} — 升级/更新 {}",
                ex.installed_version, manifest.config.app_name
            )
        } else {
            format!("欢迎安装 {}", manifest.config.app_name)
        };
        let hero_sub = if self.existing_install.is_some() {
            format!(
                "目标版本: v{}   |   原路径: {}   |   共 {} 个文件 ({})",
                manifest.config.app_version,
                self.install_dir_input,
                manifest.files.len(),
                format_bytes(manifest.total_uncompressed_bytes)
            )
        } else {
            format!(
                "版本: v{}   |   发布者: {}   |   包含 {} 个文件 ({})",
                manifest.config.app_version,
                manifest.config.publisher,
                manifest.files.len(),
                format_bytes(manifest.total_uncompressed_bytes)
            )
        };

        self.renderer.draw_text(
            &hero_title,
            &self.renderer.font_title_lg,
            RectF::new(hero.x + 82.0, hero.y + 10.0, hero.w - 96.0, 26.0),
            pal.on_primary_container,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &hero_sub,
            &self.renderer.font_body,
            RectF::new(hero.x + 82.0, hero.y + 40.0, hero.w - 96.0, 22.0),
            pal.on_primary_container,
            0,
            1,
            false,
        );

        // License Agreement Scrollable Box (y = card.y + 118.0, h = 192.0)
        self.renderer.draw_text(
            "软件许可协议 (请使用鼠标滚轮阅读全文):",
            &self.renderer.font_caption,
            RectF::new(x0, card.y + 96.0, w_full, 18.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        let eula_box = RectF::new(x0, card.y + 118.0, w_full, 192.0);
        self.renderer
            .fill_rounded_rect(eula_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(eula_box, 10.0, 1.0, pal.outline_subtle);

        let lines: Vec<&str> = manifest.config.license_text.lines().collect();
        let start = self.license_scroll_line.min(lines.len().saturating_sub(1));
        let visible = lines
            .into_iter()
            .skip(start)
            .take(10)
            .collect::<Vec<_>>()
            .join("\n");

        let inner = eula_box.inset(14.0, 10.0);
        unsafe {
            self.renderer
                .rt()
                .PushAxisAlignedClip(&inner.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        self.renderer.draw_text(
            &visible,
            &self.renderer.font_caption,
            inner,
            pal.text_secondary,
            0,
            0,
            true,
        );
        unsafe {
            self.renderer.rt().PopAxisAlignedClip();
        }

        // Process termination option bar right below EULA box (y = card.y + 318.0, h = 36.0)
        let chk_kill_welcome = RectF::new(x0, card.y + 318.0, w_full, 36.0);
        let kill_label = if !self.running_processes.is_empty() {
            format!(
                "更新/安装前自动结束正在运行的进程 (检测到 {} 正在运行，PID: {})",
                self.running_processes[0].1, self.running_processes[0].0
            )
        } else {
            format!(
                "更新/安装前自动结束正在运行的目标程序进程 ({})",
                manifest.config.main_exe
            )
        };
        self.renderer.draw_checkbox(
            chk_kill_welcome,
            self.opt_kill_running_process,
            chk_kill_welcome.contains(mx, my),
            &kill_label,
            None,
            &pal,
        );

        // Footer separator & controls
        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_y = card.y + card.h - 50.0;
        let chk_rect = RectF::new(x0, btn_y, 280.0, 38.0);
        self.renderer.draw_checkbox(
            chk_rect,
            self.license_accepted,
            chk_rect.contains(mx, my),
            "我已阅读并同意《软件许可协议》",
            None,
            &pal,
        );

        let btn_custom = RectF::new(card.x + card.w - 332.0, btn_y, 136.0, 38.0);
        self.renderer.draw_button(
            btn_custom,
            "自定义选项",
            Some(VectorIcon::Folder),
            ButtonVariant::Tonal,
            btn_custom.contains(mx, my),
            self.license_accepted,
            &pal,
            &self.ripples,
        );

        let quick_label = if self.existing_install.is_some() {
            "一键覆盖更新"
        } else {
            "一键快速安装"
        };
        let btn_quick = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
        self.renderer.draw_button(
            btn_quick,
            quick_label,
            Some(VectorIcon::Rocket),
            ButtonVariant::Filled,
            btn_quick.contains(mx, my),
            self.license_accepted && self.has_enough_disk_space(),
            &pal,
            &self.ripples,
        );
    }

    fn draw_step_options(&self, card: RectF, manifest: &PackageManifest) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;

        let header_str = if let Some(ref ex) = self.existing_install {
            format!(
                "覆盖更新选项 (检测到已安装 v{} → 升级/更新至 v{})",
                ex.installed_version, manifest.config.app_version
            )
        } else {
            "自定义安装目录与系统快捷方式选项".to_string()
        };
        self.renderer.draw_text(
            &header_str,
            &self.renderer.font_title_md,
            RectF::new(x0, card.y + 16.0, w_full, 24.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        // Target path field + Browse button (y = card.y + 66.0)
        let r_path = RectF::new(x0, card.y + 66.0, w_full - 134.0, 38.0);
        self.renderer.draw_text_field(
            r_path,
            "目标安装文件夹路径 (Installation Directory)",
            &self.install_dir_input,
            self.path_focused,
            r_path.contains(mx, my),
            self.caret_visible,
            &pal,
        );

        let btn_browse = RectF::new(x0 + w_full - 122.0, card.y + 66.0, 122.0, 38.0);
        self.renderer.draw_button(
            btn_browse,
            "浏览目录",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_browse.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // Disk Space Card (y = card.y + 114.0, h = 60.0)
        let disk_card = RectF::new(x0, card.y + 114.0, w_full, 60.0);
        self.renderer
            .fill_rounded_rect(disk_card, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(disk_card, 10.0, 1.0, pal.outline_subtle);
        self.renderer.draw_icon(
            VectorIcon::HardDrive,
            disk_card.x + 26.0,
            disk_card.y + 30.0,
            20.0,
            pal.primary,
        );

        let req_bytes = manifest.total_uncompressed_bytes;
        if let Some((free_b, total_b)) = self.disk_space {
            let enough = self.has_enough_disk_space();
            let status_str = if enough {
                "磁盘空间充足"
            } else {
                "磁盘空间不足！"
            };
            let status_col = if enough { pal.success } else { pal.error };

            self.renderer.draw_text(
                &format!(
                    "所需空间: {}   |   目标分区可用空间: {} (总容量 {})",
                    format_bytes(req_bytes),
                    format_bytes(free_b),
                    format_bytes(total_b)
                ),
                &self.renderer.font_body_bold,
                RectF::new(disk_card.x + 50.0, disk_card.y + 8.0, disk_card.w - 170.0, 20.0),
                pal.text_primary,
                0,
                1,
                false,
            );
            self.renderer.draw_text(
                status_str,
                &self.renderer.font_body_bold,
                RectF::new(disk_card.x + disk_card.w - 124.0, disk_card.y + 8.0, 108.0, 20.0),
                status_col,
                2,
                1,
                false,
            );

            let used_ratio = if total_b > 0 {
                1.0 - (free_b as f32 / total_b as f32)
            } else {
                0.2
            };
            let bar_rect =
                RectF::new(disk_card.x + 50.0, disk_card.y + 36.0, disk_card.w - 66.0, 8.0);
            self.renderer.draw_progress_bar(bar_rect, used_ratio, &pal);
        } else {
            self.renderer.draw_text(
                &format!("所需安装空间: {} (无法检测目标盘符可用容量)", format_bytes(req_bytes)),
                &self.renderer.font_body,
                RectF::new(disk_card.x + 50.0, disk_card.y + 18.0, disk_card.w - 66.0, 24.0),
                pal.warning,
                0,
                1,
                false,
            );
        }

        // 4 Checkboxes for Shortcuts, Uninstaller, and Process Termination (y = card.y + 184..336)
        let c1 = RectF::new(x0, card.y + 184.0, w_full, 36.0);
        self.renderer.draw_checkbox(
            c1,
            self.opt_desktop_shortcut,
            c1.contains(mx, my),
            "在桌面创建快捷方式图标 (Desktop Shortcut)",
            None,
            &pal,
        );

        let c2 = RectF::new(x0, card.y + 222.0, w_full, 36.0);
        self.renderer.draw_checkbox(
            c2,
            self.opt_start_menu_shortcut,
            c2.contains(mx, my),
            "在开始菜单程序文件夹创建快捷方式 (Start Menu Shortcut)",
            None,
            &pal,
        );

        let c3 = RectF::new(x0, card.y + 260.0, w_full, 36.0);
        self.renderer.draw_checkbox(
            c3,
            self.opt_register_uninstaller,
            c3.contains(mx, my),
            "生成卸载程序并注册到 Windows“应用和功能”列表",
            None,
            &pal,
        );

        let c4 = RectF::new(x0, card.y + 298.0, w_full, 36.0);
        let kill_title = if !self.running_processes.is_empty() {
            format!(
                "安装/更新前自动结束正在运行的进程 (当前检测到 {} 正在运行，共 {} 个实例)",
                self.running_processes[0].1,
                self.running_processes.len()
            )
        } else {
            format!(
                "安装/更新前自动结束正在运行的目标程序进程 ({})",
                manifest.config.main_exe
            )
        };
        self.renderer.draw_checkbox(
            c4,
            self.opt_kill_running_process,
            c4.contains(mx, my),
            &kill_title,
            None,
            &pal,
        );

        // Footer separator & buttons
        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_y = card.y + card.h - 50.0;
        let btn_back = RectF::new(x0, btn_y, 120.0, 38.0);
        self.renderer.draw_button(
            btn_back,
            "返回上一步",
            None,
            ButtonVariant::Outlined,
            btn_back.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let start_label = if self.existing_install.is_some() {
            "开始更新"
        } else {
            "开始安装"
        };
        let btn_install = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
        self.renderer.draw_button(
            btn_install,
            start_label,
            Some(VectorIcon::Rocket),
            ButtonVariant::Filled,
            btn_install.contains(mx, my),
            self.has_enough_disk_space() && !self.install_dir_input.trim().is_empty(),
            &pal,
            &self.ripples,
        );
    }

    fn draw_step_installing(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;

        self.renderer.draw_text(
            &format!("{:.0}%", self.progress * 100.0),
            &self.renderer.font_title_lg,
            RectF::new(x0, card.y + 22.0, 120.0, 30.0),
            if self.install_error.is_some() {
                pal.error
            } else {
                pal.primary
            },
            0,
            1,
            false,
        );

        let status_color = if self.install_error.is_some() {
            pal.error
        } else {
            pal.text_primary
        };
        self.renderer.draw_text(
            &self.status_text,
            &self.renderer.font_body_bold,
            RectF::new(x0, card.y + 56.0, w_full, 22.0),
            status_color,
            0,
            1,
            false,
        );

        let bar = RectF::new(x0, card.y + 86.0, w_full, 10.0);
        self.renderer.draw_progress_bar(bar, self.progress, &pal);

        let log_box = RectF::new(x0, card.y + 112.0, w_full, 232.0);
        self.renderer
            .fill_rounded_rect(log_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(log_box, 10.0, 1.0, pal.outline_subtle);

        let wrapped_logs = self.renderer.wrap_log_lines(
            &self.logs,
            &self.renderer.font_mono,
            log_box.w - 28.0,
        );
        let max_lines = 9usize;
        let skip = wrapped_logs.len().saturating_sub(max_lines);
        let log_clip = log_box.inset(8.0, 6.0);
        unsafe {
            self.renderer
                .rt()
                .PushAxisAlignedClip(&log_clip.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        for (i, (orig, line)) in wrapped_logs.iter().skip(skip).enumerate() {
            let ly = log_box.y + 12.0 + (i as f32) * 23.0;
            let col = if orig.starts_with("[错误]") {
                pal.error
            } else if orig.starts_with("[进程管理]") || orig.starts_with("[覆盖更新]") {
                pal.warning
            } else if orig.starts_with("[完成]") {
                pal.success
            } else {
                pal.text_secondary
            };
            self.renderer.draw_text(
                line,
                &self.renderer.font_mono,
                RectF::new(log_box.x + 14.0, ly, log_box.w - 28.0, 20.0),
                col,
                0,
                1,
                false,
            );
        }
        unsafe {
            self.renderer.rt().PopAxisAlignedClip();
        }

        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_y = card.y + card.h - 50.0;
        if self.install_error.is_some() && !self.is_working {
            let btn_back_opt = RectF::new(x0, btn_y, 124.0, 38.0);
            self.renderer.draw_button(
                btn_back_opt,
                "返回选项",
                None,
                ButtonVariant::Outlined,
                btn_back_opt.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );

            let btn_kill_retry = RectF::new(card.x + card.w - 368.0, btn_y, 192.0, 38.0);
            self.renderer.draw_button(
                btn_kill_retry,
                "结束占用进程并重试",
                Some(VectorIcon::Rocket),
                ButtonVariant::Tonal,
                btn_kill_retry.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );

            let btn_retry = RectF::new(card.x + card.w - 164.0, btn_y, 138.0, 38.0);
            self.renderer.draw_button(
                btn_retry,
                "直接重试",
                Some(VectorIcon::Rocket),
                ButtonVariant::Filled,
                btn_retry.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        } else {
            self.renderer.draw_text(
                "正在流式解压并执行 CRC32 完整性校验，请稍候...",
                &self.renderer.font_caption,
                RectF::new(x0, btn_y, w_full, 38.0),
                pal.text_muted,
                0,
                1,
                false,
            );
        }
    }

    fn draw_step_completed(&self, card: RectF, manifest: &PackageManifest) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;
        let cx = card.x + card.w * 0.5;

        // Big Success Circle Badge
        self.renderer
            .fill_circle(cx, card.y + 72.0, 34.0, rgba(0x10B981, 0.16));
        self.renderer
            .fill_circle(cx, card.y + 72.0, 24.0, pal.success);
        self.renderer
            .draw_icon(VectorIcon::Check, cx, card.y + 72.0, 22.0, rgb(0xFFFFFF));

        let done_title = if self.existing_install.is_some() {
            format!(
                "{} 已成功更新至 v{}！",
                manifest.config.app_name, manifest.config.app_version
            )
        } else {
            format!("{} 已成功安装！", manifest.config.app_name)
        };
        self.renderer.draw_text(
            &done_title,
            &self.renderer.font_title_lg,
            RectF::new(x0, card.y + 122.0, w_full, 28.0),
            pal.text_primary,
            1,
            1,
            false,
        );

        self.renderer.draw_text(
            &format!("安装位置: {}", self.install_dir_input),
            &self.renderer.font_mono,
            RectF::new(x0, card.y + 156.0, w_full, 22.0),
            pal.text_secondary,
            1,
            1,
            false,
        );

        // Checkbox: Launch application now
        let chk_launch = RectF::new(cx - 170.0, card.y + 206.0, 340.0, 42.0);
        self.renderer.draw_checkbox(
            chk_launch,
            self.opt_launch_now,
            chk_launch.contains(mx, my),
            &format!(
                "立即运行 {} ({})",
                manifest.config.app_name, manifest.config.main_exe
            ),
            None,
            &pal,
        );

        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_y = card.y + card.h - 50.0;
        let btn_open_folder = RectF::new(card.x + card.w - 332.0, btn_y, 136.0, 38.0);
        self.renderer.draw_button(
            btn_open_folder,
            "打开安装目录",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_open_folder.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let btn_finish = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
        self.renderer.draw_button(
            btn_finish,
            "完成 (Finish)",
            Some(VectorIcon::Check),
            ButtonVariant::Filled,
            btn_finish.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
    }

    fn draw_uninstaller_view(&self, card: RectF, receipt: &UninstallReceipt) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;

        let banner = RectF::new(x0, card.y + 22.0, w_full, 74.0);
        self.renderer
            .fill_rounded_rect(banner, 12.0, rgba(0xEF4444, 0.12));
        self.renderer.draw_icon(
            VectorIcon::Trash,
            banner.x + 36.0,
            banner.y + 37.0,
            24.0,
            pal.error,
        );

        self.renderer.draw_text(
            &format!("卸载 {} v{}", receipt.app_name, receipt.app_version),
            &self.renderer.font_title_lg,
            RectF::new(banner.x + 72.0, banner.y + 12.0, banner.w - 88.0, 26.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!(
                "目标目录: {} (共 {} 个文件)",
                receipt.install_dir,
                receipt.installed_files.len()
            ),
            &self.renderer.font_body,
            RectF::new(banner.x + 72.0, banner.y + 42.0, banner.w - 88.0, 22.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        let log_box = RectF::new(x0, card.y + 114.0, w_full, 270.0);
        self.renderer
            .fill_rounded_rect(log_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(log_box, 10.0, 1.0, pal.outline_subtle);

        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );
        let btn_y = card.y + card.h - 50.0;

        if !self.uninstall_done {
            self.renderer.draw_text(
                "点击下方“确认卸载并清理”将从您的电脑中移除以下内容：\n\n\
                • 自动结束正在运行的目标程序进程\n\
                • 安装目录中的所有程序文件与组件\n\
                • 桌面快捷方式与开始菜单程序组快捷方式\n\
                • Windows 注册表中的“应用和功能”卸载项",
                &self.renderer.font_body,
                log_box.inset(20.0, 20.0),
                pal.text_primary,
                0,
                0,
                true,
            );

            let btn_cancel = RectF::new(card.x + card.w - 316.0, btn_y, 120.0, 38.0);
            self.renderer.draw_button(
                btn_cancel,
                "取消",
                None,
                ButtonVariant::Outlined,
                btn_cancel.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );

            let btn_unins = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
            self.renderer.draw_button(
                btn_unins,
                "确认卸载并清理",
                Some(VectorIcon::Trash),
                ButtonVariant::Danger,
                btn_unins.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        } else {
            let wrapped_logs = self.renderer.wrap_log_lines(
                &self.logs,
                &self.renderer.font_mono,
                log_box.w - 28.0,
            );
            let skip = wrapped_logs.len().saturating_sub(10);
            let log_clip = log_box.inset(8.0, 6.0);
            unsafe {
                self.renderer
                    .rt()
                    .PushAxisAlignedClip(&log_clip.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            }
            for (i, (_orig, line)) in wrapped_logs.iter().skip(skip).enumerate() {
                let ly = log_box.y + 12.0 + (i as f32) * 23.0;
                self.renderer.draw_text(
                    line,
                    &self.renderer.font_mono,
                    RectF::new(log_box.x + 14.0, ly, log_box.w - 28.0, 20.0),
                    pal.success,
                    0,
                    1,
                    false,
                );
            }
            unsafe {
                self.renderer.rt().PopAxisAlignedClip();
            }

            let btn_close = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
            self.renderer.draw_button(
                btn_close,
                "完成并退出",
                Some(VectorIcon::Check),
                ButtonVariant::Filled,
                btn_close.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        }
    }

    pub fn on_lbutton_down(&mut self, x: f32, y: f32) {
        let w = self.width_dip;
        let h = self.height_dip;
        self.path_focused = false;

        if D2DRenderer::header_theme_rect(w).contains(x, y) {
            self.palette = match self.palette.mode {
                ThemeMode::Light => MaterialPalette::dark(),
                ThemeMode::Dark => MaterialPalette::light(),
            };
            self.invalidate();
            return;
        }
        if D2DRenderer::header_min_rect(w).contains(x, y) {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
            }
            return;
        }
        if D2DRenderer::header_close_rect(w).contains(x, y) {
            if !self.is_working {
                if self.uninstall_done {
                    if let WizardMode::Uninstaller {
                        ref uninstaller_exe,
                        ref receipt,
                    } = self.mode
                    {
                        schedule_self_delete(uninstaller_exe, Path::new(&receipt.install_dir));
                    }
                }
                unsafe {
                    PostQuitMessage(0);
                }
            }
            return;
        }

        if y < 46.0 {
            unsafe {
                let _ = ReleaseCapture();
                SendMessageW(
                    self.hwnd,
                    WM_NCLBUTTONDOWN,
                    WPARAM(HTCAPTION as usize),
                    LPARAM(0),
                );
            }
            return;
        }

        let is_uninstaller = matches!(self.mode, WizardMode::Uninstaller { .. });
        if is_uninstaller {
            let card = RectF::new(18.0, 52.0, w - 36.0, h - 68.0);
            let btn_y = card.y + card.h - 50.0;
            if !self.uninstall_done {
                let btn_cancel = RectF::new(card.x + card.w - 316.0, btn_y, 120.0, 38.0);
                let btn_unins = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
                if btn_cancel.contains(x, y) {
                    unsafe {
                        PostQuitMessage(0);
                    }
                } else if btn_unins.contains(x, y) {
                    if let WizardMode::Uninstaller { ref receipt, .. } = self.mode {
                        self.logs = execute_uninstall_steps(receipt);
                        self.uninstall_done = true;
                        self.invalidate();
                    }
                }
            } else {
                let btn_close = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
                if btn_close.contains(x, y) {
                    if let WizardMode::Uninstaller {
                        ref uninstaller_exe,
                        ref receipt,
                    } = self.mode
                    {
                        schedule_self_delete(uninstaller_exe, Path::new(&receipt.install_dir));
                    }
                    unsafe {
                        PostQuitMessage(0);
                    }
                }
            }
            return;
        }

        // Installer mode clicks
        let card = self.installer_card_rect();
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;
        let btn_y = card.y + card.h - 50.0;

        match self.step {
            WizardStep::WelcomeAndLicense => {
                let chk_kill_welcome = RectF::new(x0, card.y + 318.0, w_full, 36.0);
                let chk_rect = RectF::new(x0, btn_y, 280.0, 38.0);
                let btn_custom = RectF::new(card.x + card.w - 332.0, btn_y, 136.0, 38.0);
                let btn_quick = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);

                if chk_kill_welcome.contains(x, y) {
                    self.opt_kill_running_process = !self.opt_kill_running_process;
                } else if chk_rect.contains(x, y) {
                    self.license_accepted = !self.license_accepted;
                } else if btn_custom.contains(x, y) && self.license_accepted {
                    self.refresh_disk_space();
                    self.step = WizardStep::CustomOptions;
                } else if btn_quick.contains(x, y)
                    && self.license_accepted
                    && self.has_enough_disk_space()
                {
                    self.start_installation();
                }
            }
            WizardStep::CustomOptions => {
                let r_path = RectF::new(x0, card.y + 66.0, w_full - 134.0, 38.0);
                let btn_browse = RectF::new(x0 + w_full - 122.0, card.y + 66.0, 122.0, 38.0);
                let c1 = RectF::new(x0, card.y + 184.0, w_full, 36.0);
                let c2 = RectF::new(x0, card.y + 222.0, w_full, 36.0);
                let c3 = RectF::new(x0, card.y + 260.0, w_full, 36.0);
                let c4 = RectF::new(x0, card.y + 298.0, w_full, 36.0);
                let btn_back = RectF::new(x0, btn_y, 120.0, 38.0);
                let btn_install = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);

                if r_path.contains(x, y) {
                    self.path_focused = true;
                } else if btn_browse.contains(x, y) {
                    if let Some(picked) = pick_folder_dialog(
                        self.hwnd,
                        "选择安装目标文件夹",
                        Some(Path::new(&self.install_dir_input)),
                    ) {
                        if let WizardMode::Installer { ref manifest, .. } = self.mode {
                            let final_dir = if picked.ends_with(&manifest.config.app_name) {
                                picked
                            } else {
                                picked.join(&manifest.config.app_name)
                            };
                            self.install_dir_input = final_dir.to_string_lossy().to_string();
                            self.refresh_disk_space();
                        }
                    }
                } else if c1.contains(x, y) {
                    self.opt_desktop_shortcut = !self.opt_desktop_shortcut;
                } else if c2.contains(x, y) {
                    self.opt_start_menu_shortcut = !self.opt_start_menu_shortcut;
                } else if c3.contains(x, y) {
                    self.opt_register_uninstaller = !self.opt_register_uninstaller;
                } else if c4.contains(x, y) {
                    self.opt_kill_running_process = !self.opt_kill_running_process;
                } else if btn_back.contains(x, y) {
                    self.step = WizardStep::WelcomeAndLicense;
                } else if btn_install.contains(x, y)
                    && self.has_enough_disk_space()
                    && !self.install_dir_input.trim().is_empty()
                {
                    self.start_installation();
                }
            }
            WizardStep::Installing => {
                if self.install_error.is_some() && !self.is_working {
                    let btn_back_opt = RectF::new(x0, btn_y, 124.0, 38.0);
                    let btn_kill_retry = RectF::new(card.x + card.w - 368.0, btn_y, 192.0, 38.0);
                    let btn_retry = RectF::new(card.x + card.w - 164.0, btn_y, 138.0, 38.0);

                    if btn_back_opt.contains(x, y) {
                        self.refresh_disk_space();
                        self.step = WizardStep::CustomOptions;
                    } else if btn_kill_retry.contains(x, y) {
                        self.opt_kill_running_process = true;
                        self.start_installation();
                    } else if btn_retry.contains(x, y) {
                        self.start_installation();
                    }
                }
            }
            WizardStep::Completed => {
                let cx = card.x + card.w * 0.5;
                let chk_launch = RectF::new(cx - 170.0, card.y + 206.0, 340.0, 42.0);
                let btn_open_folder = RectF::new(card.x + card.w - 332.0, btn_y, 136.0, 38.0);
                let btn_finish = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);

                if chk_launch.contains(x, y) {
                    self.opt_launch_now = !self.opt_launch_now;
                } else if btn_open_folder.contains(x, y) {
                    let dir = PathBuf::from(&self.install_dir_input);
                    shell_open(&dir, None, None);
                } else if btn_finish.contains(x, y) {
                    if self.opt_launch_now {
                        if let WizardMode::Installer { ref manifest, .. } = self.mode {
                            let target_dir = PathBuf::from(&self.install_dir_input);
                            let main_exe = target_dir.join(&manifest.config.main_exe);
                            if main_exe.exists() {
                                shell_open(&main_exe, Some("--demo-app"), Some(&target_dir));
                            }
                        }
                    }
                    unsafe {
                        PostQuitMessage(0);
                    }
                }
            }
        }

        self.invalidate();
    }

    pub fn on_char(&mut self, ch: char) {
        if ch.is_control() || !self.path_focused {
            return;
        }
        self.install_dir_input.push(ch);
        self.refresh_disk_space();
        self.invalidate();
    }

    pub fn on_keydown(&mut self, vk: u16) {
        if !self.path_focused {
            return;
        }
        let ctrl_down = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
        if ctrl_down && vk == b'V' as u16 {
            if let Some(clip) = get_clipboard_text(self.hwnd) {
                let line = clip.lines().next().unwrap_or("").trim();
                self.install_dir_input.push_str(line);
                self.refresh_disk_space();
                self.invalidate();
            }
            return;
        }
        if vk == VK_BACK.0 {
            self.install_dir_input.pop();
            self.refresh_disk_space();
            self.invalidate();
        } else if vk == VK_ESCAPE.0 || vk == VK_RETURN.0 {
            self.path_focused = false;
            self.invalidate();
        }
    }

    pub fn on_mouse_wheel(&mut self, delta: i16) {
        if self.step == WizardStep::WelcomeAndLicense {
            if let WizardMode::Installer { ref manifest, .. } = self.mode {
                let total_lines = manifest.config.license_text.lines().count();
                if delta < 0 {
                    self.license_scroll_line =
                        (self.license_scroll_line + 2).min(total_lines.saturating_sub(4));
                } else {
                    self.license_scroll_line = self.license_scroll_line.saturating_sub(2);
                }
                self.invalidate();
            }
        }
    }
}

pub fn run_installer_or_uninstaller_gui(mode: WizardMode) -> Result<(), String> {
    let renderer = D2DRenderer::new()?;
    let state = Box::new(match mode {
        WizardMode::Installer {
            installer_exe,
            manifest,
        } => InstallerWindowState::new_installer(renderer, installer_exe, manifest),
        WizardMode::Uninstaller {
            uninstaller_exe,
            receipt,
        } => InstallerWindowState::new_uninstaller(renderer, uninstaller_exe, receipt),
    });

    unsafe {
        let h_instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class_name = to_wide("NyanSetupInstallerWndClass");

        let app_icon = LoadIconW(h_instance, PCWSTR(1 as *const u16)).unwrap_or_default();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(installer_wnd_proc),
            hInstance: h_instance.into(),
            hIcon: app_icon,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let sys_dpi = GetDpiForSystem().max(96);
        let scale = (sys_dpi as f32) / 96.0;
        let phys_w = (INST_WIDTH_DIP * scale).round() as i32;
        let phys_h = (INST_HEIGHT_DIP * scale).round() as i32;

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let pos_x = ((screen_w - phys_w) / 2).max(10);
        let pos_y = ((screen_h - phys_h) / 2).max(10);

        let title = to_wide("Nyan Setup 安装程序");
        let state_ptr = Box::into_raw(state);

        // Pure borderless WS_POPUP window: NO WS_CAPTION, NO WS_THICKFRAME
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_SYSMENU | WS_MINIMIZEBOX,
            pos_x,
            pos_y,
            phys_w,
            phys_h,
            None,
            None,
            h_instance,
            Some(state_ptr as *const _),
        )
        .map_err(|e| format!("创建安装向导窗口失败: {}", e))?;

        if !app_icon.is_invalid() {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                WPARAM(ICON_BIG as usize),
                LPARAM(app_icon.0 as isize),
            );
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                WPARAM(ICON_SMALL as usize),
                LPARAM(app_icon.0 as isize),
            );
        }

        let corner_pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const _ as *const _,
            std::mem::size_of_val(&corner_pref) as u32,
        );

        let _ = SetTimer(hwnd, TIMER_CARET, 530, None);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let _ = env::current_dir();
    Ok(())
}

unsafe extern "system" fn installer_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let cs = &*(lparam.0 as *const CREATESTRUCTW);
        let state_ptr = cs.lpCreateParams as *mut InstallerWindowState;
        if !state_ptr.is_null() {
            (*state_ptr).hwnd = hwnd;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        }
        return LRESULT(0);
    }

    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut InstallerWindowState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    let map_mouse = |state: &InstallerWindowState, lparam: LPARAM| -> (f32, f32) {
        let x_px = (lparam.0 & 0xFFFF) as i16 as f32;
        let y_px = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let cw = (rc.right - rc.left).max(1) as f32;
        let ch = (rc.bottom - rc.top).max(1) as f32;
        (x_px * (state.width_dip / cw), y_px * (state.height_dip / ch))
    };

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            let width_px = (lparam.0 & 0xFFFF) as u32;
            let height_px = ((lparam.0 >> 16) & 0xFFFF) as u32;
            if width_px > 0 && height_px > 0 {
                let dpi = GetDpiForWindow(hwnd).max(96);
                state.width_dip = INST_WIDTH_DIP;
                state.height_dip = INST_HEIGHT_DIP;
                state.renderer.resize(width_px, height_px, dpi);
                state.invalidate();
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let suggested = &*(lparam.0 as *const RECT);
            let new_w = (suggested.right - suggested.left).max(1);
            let new_h = (suggested.bottom - suggested.top).max(1);
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                hwnd,
                None,
                suggested.left,
                suggested.top,
                new_w,
                new_h,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );
            state.renderer.resize(new_w as u32, new_h as u32, dpi);
            state.invalidate();
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (mx, my) = map_mouse(state, lparam);
            state.mouse_x = mx;
            state.mouse_y = my;
            state.invalidate();
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (mx, my) = map_mouse(state, lparam);
            state.on_lbutton_down(mx, my);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            state.on_mouse_wheel(delta);
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(ch) = char::from_u32(wparam.0 as u32) {
                state.on_char(ch);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            state.on_keydown(wparam.0 as u16);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_CARET {
                if state.path_focused {
                    state.caret_visible = !state.caret_visible;
                    state.invalidate();
                }
            } else if wparam.0 == TIMER_ANIM {
                state.ripples.retain_mut(|r| r.step());
                if state.is_working {
                    state.poll_install_events();
                    let diff = state.target_progress - state.progress;
                    if diff.abs() > 0.002 {
                        state.progress += diff * 0.22;
                    }
                }
                if state.ripples.is_empty() && !state.is_working {
                    let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(hwnd, TIMER_ANIM);
                }
                state.invalidate();
            }
            LRESULT(0)
        }
        WM_APP_INSTALL_EVENT => {
            state.poll_install_events();
            state.invalidate();
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            state.draw();
            state.renderer.blit_to_hdc(ps.hdc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            if state.uninstall_done {
                if let WizardMode::Uninstaller {
                    ref uninstaller_exe,
                    ref receipt,
                } = state.mode
                {
                    schedule_self_delete(uninstaller_exe, Path::new(&receipt.install_dir));
                }
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(state_ptr));
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
