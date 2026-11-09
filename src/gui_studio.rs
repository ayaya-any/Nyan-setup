use crate::d2d::{
    rgb, rgba, ButtonVariant, D2DRenderer, MaterialPalette, RectF, RippleEffect, TextInputState,
    ThemeMode, VectorIcon,
};
use crate::package::{
    build_installer_package, collect_source_files, format_bytes, BuildProgressEvent, CheckOperator,
    PackageConfig, PrereqAction, PrereqCondition, PrerequisiteRule, ProcessConflictPolicy,
    RegArchView, RegHiveRoot,
};
use crate::win32_ops::{
    create_demo_app_directory, evaluate_prereq_condition, get_clipboard_text, pick_folder_dialog,
    pick_open_file_dialog, pick_save_exe_dialog, set_clipboard_text, shell_open, to_wide,
};
use std::env;
use std::fs;
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
    GetKeyState, ReleaseCapture, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE, VK_END, VK_ESCAPE,
    VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, LoadCursorW, LoadIconW, PostMessageW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage,
    CREATESTRUCTW, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HTCAPTION, IDC_ARROW,
    ICON_BIG, ICON_SMALL, MSG, SM_CXSCREEN, SM_CYSCREEN, SW_MINIMIZE, SW_SHOW, WM_CHAR,
    WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETICON, WM_SIZE,
    WM_TIMER, WM_USER, WNDCLASSW, WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
};

const WM_APP_BUILD_EVENT: u32 = WM_USER + 101;
const TIMER_ANIM: usize = 1;
const TIMER_CARET: usize = 2;

pub const WIN_WIDTH_DIP: f32 = 940.0;
pub const WIN_HEIGHT_DIP: f32 = 640.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioTab {
    Metadata = 0,
    FilesAndPaths = 1,
    OptionsAndLicense = 2,
    Prerequisites = 3,
    BuildAndTest = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedField {
    None,
    AppName,
    AppVersion,
    Publisher,
    AppId,
    MainExe,
    InstallerIcon,
    SourceDir,
    DefaultInstallDir,
    OutputExe,
    PrereqName,
    PrereqTarget,
    PrereqValueName,
    PrereqExpected,
    PrereqActionPrimary,
    PrereqActionSecondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrereqDropdown {
    ConditionKind,
    RegHive,
    RegArch,
    Operator,
    ActionKind,
    ElevateMode,
}

pub struct StudioWindowState {
    pub hwnd: HWND,
    pub renderer: D2DRenderer,
    pub palette: MaterialPalette,
    pub width_dip: f32,
    pub height_dip: f32,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub active_tab: StudioTab,
    pub focused_field: FocusedField,
    pub text_input: TextInputState,
    pub open_dropdown: Option<PrereqDropdown>,
    pub caret_visible: bool,
    pub ripples: Vec<RippleEffect>,

    // Package configuration fields
    pub config: PackageConfig,
    pub source_dir: String,
    pub output_exe: String,
    pub installer_icon_input: String,
    pub selected_prereq_idx: usize,

    // Scanned source directory preview
    pub scanned_files: Vec<(String, u64)>,
    pub scanned_total_bytes: u64,
    pub file_scroll_offset: usize,
    pub license_scroll_line: usize,

    // Build state
    pub is_building: bool,
    pub build_progress: f32,
    pub build_target_progress: f32,
    pub build_status_text: String,
    pub build_logs: Vec<String>,
    pub last_built_exe: Option<PathBuf>,
    pub last_built_size: u64,
    pub build_rx: Option<Receiver<BuildProgressEvent>>,
}

impl StudioWindowState {
    pub fn new(renderer: D2DRenderer) -> Self {
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let default_src = cwd.join("demo_app_source");
        let default_out = cwd.join("dist").join("NyanStudioApp_Setup.exe");

        let mut state = Self {
            hwnd: HWND::default(),
            renderer,
            palette: MaterialPalette::light(),
            width_dip: WIN_WIDTH_DIP,
            height_dip: WIN_HEIGHT_DIP,
            mouse_x: -1.0,
            mouse_y: -1.0,
            active_tab: StudioTab::Metadata,
            focused_field: FocusedField::None,
            text_input: TextInputState::new(),
            open_dropdown: None,
            caret_visible: true,
            ripples: Vec::new(),
            config: PackageConfig::default(),
            source_dir: default_src.to_string_lossy().to_string(),
            output_exe: default_out.to_string_lossy().to_string(),
            installer_icon_input: String::new(),
            selected_prereq_idx: 0,
            scanned_files: Vec::new(),
            scanned_total_bytes: 0,
            file_scroll_offset: 0,
            license_scroll_line: 0,
            is_building: false,
            build_progress: 0.0,
            build_target_progress: 0.0,
            build_status_text: "就绪：点击右下角“开始构建安装包”合成单文件静态 CRT 安装程序".to_string(),
            build_logs: vec![
                "[系统] Nyan Setup Studio 已就绪 (Direct2D 硬件加速 + 静态 CRT)".to_string(),
                "[提示] 已自动加载演示应用源目录，可直接点击右下角按钮构建测试。".to_string(),
            ],
            last_built_exe: None,
            last_built_size: 0,
            build_rx: None,
        };

        if !default_src.exists() {
            if let Ok((demo_dir, main_exe)) = create_demo_app_directory(&cwd) {
                state.source_dir = demo_dir.to_string_lossy().to_string();
                state.config.main_exe = main_exe;
            }
        }
        state.rescan_source_dir();
        state
    }

    pub fn rescan_source_dir(&mut self) {
        let p = PathBuf::from(&self.source_dir);
        match collect_source_files(&p) {
            Ok(list) => {
                self.scanned_total_bytes = list.iter().map(|(_, _, sz)| *sz).sum();
                self.scanned_files = list.into_iter().map(|(_, rel, sz)| (rel, sz)).collect();
                if !self
                    .scanned_files
                    .iter()
                    .any(|(r, _)| r.eq_ignore_ascii_case(&self.config.main_exe))
                {
                    if let Some((exe_rel, _)) = self
                        .scanned_files
                        .iter()
                        .find(|(r, _)| r.to_lowercase().ends_with(".exe"))
                    {
                        self.config.main_exe = exe_rel.clone();
                    }
                }
            }
            Err(_) => {
                self.scanned_files.clear();
                self.scanned_total_bytes = 0;
            }
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

    pub fn field_value_ref(&self, field: FocusedField) -> Option<&str> {
        let idx = self.selected_prereq_idx;
        match field {
            FocusedField::None => None,
            FocusedField::AppName => Some(&self.config.app_name),
            FocusedField::AppVersion => Some(&self.config.app_version),
            FocusedField::Publisher => Some(&self.config.publisher),
            FocusedField::AppId => Some(&self.config.app_id),
            FocusedField::MainExe => Some(&self.config.main_exe),
            FocusedField::InstallerIcon => Some(&self.installer_icon_input),
            FocusedField::SourceDir => Some(&self.source_dir),
            FocusedField::DefaultInstallDir => Some(&self.config.default_install_dir),
            FocusedField::OutputExe => Some(&self.output_exe),
            FocusedField::PrereqName => self
                .config
                .prerequisites
                .get(idx)
                .map(|r| r.name.as_str()),
            FocusedField::PrereqTarget => self
                .config
                .prerequisites
                .get(idx)
                .map(|r| match &r.condition {
                    PrereqCondition::Registry { subkey, .. } => subkey.as_str(),
                    PrereqCondition::File { path, .. } => path.as_str(),
                }),
            FocusedField::PrereqValueName => self.config.prerequisites.get(idx).and_then(|r| {
                if let PrereqCondition::Registry { value_name, .. } = &r.condition {
                    Some(value_name.as_str())
                } else {
                    None
                }
            }),
            FocusedField::PrereqExpected => self
                .config
                .prerequisites
                .get(idx)
                .map(|r| match &r.condition {
                    PrereqCondition::Registry { expected, .. } => expected.as_str(),
                    PrereqCondition::File {
                        expected_version, ..
                    } => expected_version.as_str(),
                }),
            FocusedField::PrereqActionPrimary => self
                .config
                .prerequisites
                .get(idx)
                .map(|r| match &r.action {
                    PrereqAction::RunBundledExe { source_file, .. } => source_file.as_str(),
                    PrereqAction::RunCommand { program, .. } => program.as_str(),
                    PrereqAction::PromptAndAbort { message, .. } => message.as_str(),
                }),
            FocusedField::PrereqActionSecondary => self
                .config
                .prerequisites
                .get(idx)
                .map(|r| match &r.action {
                    PrereqAction::RunBundledExe { args, .. } => args.as_str(),
                    PrereqAction::RunCommand { args, .. } => args.as_str(),
                    PrereqAction::PromptAndAbort { download_url, .. } => download_url.as_str(),
                }),
        }
    }

    pub fn field_rect(&self, field: FocusedField) -> Option<RectF> {
        let card = self.content_card_rect();
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;
        let w_half = (w_full - 20.0) * 0.5;
        let right_x = x0 + 230.0;
        let right_w = w_full - 230.0;
        let ex = right_x + 16.0;
        let ew = right_w - 32.0;
        let idx = self.selected_prereq_idx;

        match field {
            FocusedField::None => None,
            FocusedField::AppName => Some(RectF::new(x0, card.y + 112.0, w_half, 38.0)),
            FocusedField::AppVersion => {
                Some(RectF::new(x0 + w_half + 20.0, card.y + 112.0, w_half, 38.0))
            }
            FocusedField::Publisher => Some(RectF::new(x0, card.y + 182.0, w_half, 38.0)),
            FocusedField::AppId => {
                Some(RectF::new(x0 + w_half + 20.0, card.y + 182.0, w_half, 38.0))
            }
            FocusedField::MainExe => Some(RectF::new(x0, card.y + 252.0, w_half - 100.0, 38.0)),
            FocusedField::InstallerIcon => Some(RectF::new(
                x0 + w_half + 20.0,
                card.y + 252.0,
                w_half - 100.0,
                38.0,
            )),
            FocusedField::SourceDir => Some(RectF::new(x0, card.y + 112.0, w_full - 138.0, 38.0)),
            FocusedField::DefaultInstallDir => {
                Some(RectF::new(x0, card.y + 180.0, w_full - 246.0, 38.0))
            }
            FocusedField::OutputExe => Some(RectF::new(x0, card.y + 248.0, w_full - 138.0, 38.0)),
            FocusedField::PrereqName => Some(RectF::new(ex, card.y + 152.0, ew - 178.0, 34.0)),
            FocusedField::PrereqTarget => {
                let rule = self.config.prerequisites.get(idx)?;
                match &rule.condition {
                    PrereqCondition::Registry { .. } => {
                        Some(RectF::new(ex + 204.0, card.y + 210.0, ew - 204.0, 34.0))
                    }
                    PrereqCondition::File { .. } => {
                        Some(RectF::new(ex, card.y + 210.0, ew, 34.0))
                    }
                }
            }
            FocusedField::PrereqValueName => {
                let rule = self.config.prerequisites.get(idx)?;
                match &rule.condition {
                    PrereqCondition::Registry { .. } => {
                        Some(RectF::new(ex, card.y + 268.0, 130.0, 34.0))
                    }
                    PrereqCondition::File { .. } => None,
                }
            }
            FocusedField::PrereqExpected => {
                let rule = self.config.prerequisites.get(idx)?;
                match &rule.condition {
                    PrereqCondition::Registry { .. } => {
                        Some(RectF::new(ex + 268.0, card.y + 268.0, ew - 268.0, 34.0))
                    }
                    PrereqCondition::File { .. } => {
                        Some(RectF::new(ex + 156.0, card.y + 268.0, ew - 156.0, 34.0))
                    }
                }
            }
            FocusedField::PrereqActionPrimary => {
                let rule = self.config.prerequisites.get(idx)?;
                match &rule.action {
                    PrereqAction::RunBundledExe { .. } => {
                        Some(RectF::new(ex, card.y + 388.0, ew - 96.0, 34.0))
                    }
                    PrereqAction::RunCommand { .. } | PrereqAction::PromptAndAbort { .. } => {
                        Some(RectF::new(ex, card.y + 388.0, ew, 34.0))
                    }
                }
            }
            FocusedField::PrereqActionSecondary => {
                let rule = self.config.prerequisites.get(idx)?;
                match &rule.action {
                    PrereqAction::RunBundledExe { .. } | PrereqAction::RunCommand { .. } => {
                        Some(RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0))
                    }
                    PrereqAction::PromptAndAbort { .. } => {
                        Some(RectF::new(ex, card.y + 448.0, ew, 34.0))
                    }
                }
            }
        }
    }

    pub fn click_text_field(&mut self, field: FocusedField, rect: RectF, click_x: f32) {
        let shift_down = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
        let val = self.field_value_ref(field).unwrap_or("").to_string();
        let cur_caret = if self.focused_field == field {
            self.text_input.caret_pos
        } else {
            0
        };
        let char_idx =
            self.renderer
                .hit_test_text_field_char(rect, &val, click_x, cur_caret);
        if self.focused_field == field && shift_down {
            self.text_input.set_caret(&val, char_idx, true);
        } else {
            self.focused_field = field;
            self.text_input.set_caret(&val, char_idx, false);
        }
        self.text_input.is_dragging = true;
        self.caret_visible = true;
    }

    pub fn active_field_mut(&mut self) -> Option<&mut String> {
        let idx = self.selected_prereq_idx;
        match self.focused_field {
            FocusedField::None => None,
            FocusedField::AppName => Some(&mut self.config.app_name),
            FocusedField::AppVersion => Some(&mut self.config.app_version),
            FocusedField::Publisher => Some(&mut self.config.publisher),
            FocusedField::AppId => Some(&mut self.config.app_id),
            FocusedField::MainExe => Some(&mut self.config.main_exe),
            FocusedField::InstallerIcon => Some(&mut self.installer_icon_input),
            FocusedField::SourceDir => Some(&mut self.source_dir),
            FocusedField::DefaultInstallDir => Some(&mut self.config.default_install_dir),
            FocusedField::OutputExe => Some(&mut self.output_exe),
            FocusedField::PrereqName => self
                .config
                .prerequisites
                .get_mut(idx)
                .map(|r| &mut r.name),
            FocusedField::PrereqTarget => self
                .config
                .prerequisites
                .get_mut(idx)
                .map(|r| match &mut r.condition {
                    PrereqCondition::Registry { subkey, .. } => subkey,
                    PrereqCondition::File { path, .. } => path,
                }),
            FocusedField::PrereqValueName => self.config.prerequisites.get_mut(idx).and_then(|r| {
                if let PrereqCondition::Registry { value_name, .. } = &mut r.condition {
                    Some(value_name)
                } else {
                    None
                }
            }),
            FocusedField::PrereqExpected => self
                .config
                .prerequisites
                .get_mut(idx)
                .map(|r| match &mut r.condition {
                    PrereqCondition::Registry { expected, .. } => expected,
                    PrereqCondition::File {
                        expected_version, ..
                    } => expected_version,
                }),
            FocusedField::PrereqActionPrimary => self
                .config
                .prerequisites
                .get_mut(idx)
                .map(|r| match &mut r.action {
                    PrereqAction::RunBundledExe { source_file, .. } => source_file,
                    PrereqAction::RunCommand { program, .. } => program,
                    PrereqAction::PromptAndAbort { message, .. } => message,
                }),
            FocusedField::PrereqActionSecondary => self
                .config
                .prerequisites
                .get_mut(idx)
                .map(|r| match &mut r.action {
                    PrereqAction::RunBundledExe { args, .. } => args,
                    PrereqAction::RunCommand { args, .. } => args,
                    PrereqAction::PromptAndAbort { download_url, .. } => download_url,
                }),
        }
    }

    pub fn start_build(&mut self) {
        if self.is_building {
            return;
        }
        self.rescan_source_dir();
        if self.scanned_files.is_empty() {
            self.build_status_text = "错误：源目录为空或不存在，请先选择有效的源文件夹！".to_string();
            self.build_logs
                .push(format!("[错误] 源目录无效: {}", self.source_dir));
            self.invalidate();
            return;
        }

        let stub_exe = match env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                self.build_status_text = format!("获取当前程序路径失败: {}", e);
                return;
            }
        };

        let source_path = PathBuf::from(self.source_dir.trim());
        let output_path = PathBuf::from(self.output_exe.trim());
        let mut cfg = self.config.clone();
        cfg.installer_icon = if self.installer_icon_input.trim().is_empty() {
            None
        } else {
            Some(self.installer_icon_input.trim().to_string())
        };
        let hwnd_raw = self.hwnd.0 as isize;

        let (tx, rx) = mpsc::channel();
        self.build_rx = Some(rx);
        self.is_building = true;
        self.build_progress = 0.02;
        self.build_target_progress = 0.05;
        self.build_status_text = "正在扫描并准备打包源文件...".to_string();
        self.build_logs.clear();
        self.build_logs.push(format!(
            "[开始构建] 应用: {} v{} | 源目录: {}",
            cfg.app_name,
            cfg.app_version,
            source_path.display()
        ));
        self.build_logs.push(format!(
            "[配置] 引擎: LZMA2 固实压缩 (Lv.{}) + x86 BCJ 预过滤 | 静态 CRT Stub",
            cfg.compression_level
        ));

        unsafe {
            let _ = SetTimer(self.hwnd, TIMER_ANIM, 16, None);
        }

        thread::spawn(move || {
            if let Err(err) =
                build_installer_package(&stub_exe, &source_path, &output_path, &cfg, Some(&tx))
            {
                let _ = tx.send(BuildProgressEvent::Error(err));
            }
            unsafe {
                let hwnd = HWND(hwnd_raw as *mut _);
                let _ = PostMessageW(hwnd, WM_APP_BUILD_EVENT, WPARAM(0), LPARAM(0));
            }
        });

        self.invalidate();
    }

    pub fn poll_build_events(&mut self) {
        let mut events = Vec::new();
        if let Some(ref rx) = self.build_rx {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }

        for ev in events {
            match ev {
                BuildProgressEvent::Scanning(count) => {
                    self.build_status_text = format!("已发现 {} 个待打包文件，正在压缩...", count);
                    self.build_target_progress = 0.1;
                }
                BuildProgressEvent::Compressing {
                    current_index,
                    total_files,
                    relative_path,
                } => {
                    let ratio = (current_index as f32) / (total_files.max(1) as f32);
                    self.build_target_progress = 0.1 + ratio * 0.8;
                    self.build_status_text = format!(
                        "正在压缩 ({}/{}): {}",
                        current_index, total_files, relative_path
                    );
                    self.build_logs
                        .push(format!("  + 压缩并写入: {}", relative_path));
                }
                BuildProgressEvent::Finalizing => {
                    self.build_target_progress = 0.95;
                    self.build_status_text = "正在写入安装包清单与 CRC32 校验尾标...".to_string();
                }
                BuildProgressEvent::Completed {
                    output_path,
                    total_files,
                    uncompressed_bytes,
                    final_installer_bytes,
                } => {
                    self.is_building = false;
                    self.build_progress = 1.0;
                    self.build_target_progress = 1.0;
                    self.last_built_exe = Some(output_path.clone());
                    self.last_built_size = final_installer_bytes;
                    self.build_status_text = format!(
                        "构建成功！共打包 {} 个文件，生成独立安装包 ({})",
                        total_files,
                        format_bytes(final_installer_bytes)
                    );
                    self.build_logs.push(format!(
                        "[完成] 原始数据: {} -> 安装包总大小 (含 Stub): {}",
                        format_bytes(uncompressed_bytes),
                        format_bytes(final_installer_bytes)
                    ));
                    self.build_logs
                        .push(format!("[输出文件] {}", output_path.display()));
                }
                BuildProgressEvent::Error(err) => {
                    self.is_building = false;
                    self.build_progress = 0.0;
                    self.build_target_progress = 0.0;
                    self.build_status_text = format!("构建失败: {}", err);
                    self.build_logs.push(format!("[错误] {}", err));
                }
            }
        }
    }

    // Unified Grid Layout Helpers (940 x 640 DIPs)
    fn nav_rail_rect(&self) -> RectF {
        RectF::new(14.0, 50.0, 202.0, self.height_dip - 64.0)
    }

    fn nav_item_rect(idx: usize) -> RectF {
        RectF::new(22.0, 88.0 + (idx as f32) * 44.0, 186.0, 38.0)
    }

    fn btn_generate_demo_rect(&self) -> RectF {
        RectF::new(22.0, self.height_dip - 68.0, 186.0, 40.0)
    }

    fn content_card_rect(&self) -> RectF {
        RectF::new(228.0, 50.0, self.width_dip - 242.0, self.height_dip - 64.0)
    }

    fn card_footer_next_btn(card: RectF) -> RectF {
        RectF::new(card.x + card.w - 196.0, card.y + card.h - 50.0, 168.0, 38.0)
    }

    fn draw_card_header(&self, card: RectF, title: &str, subtitle: &str) {
        let pal = self.palette;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;

        self.renderer.draw_text(
            title,
            &self.renderer.font_title_lg,
            RectF::new(x0, card.y + 16.0, w_full, 26.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            subtitle,
            &self.renderer.font_body,
            RectF::new(x0, card.y + 44.0, w_full, 20.0),
            pal.text_secondary,
            0,
            1,
            false,
        );
        self.renderer.draw_line(
            card.x + 20.0,
            card.y + 74.0,
            card.x + card.w - 20.0,
            card.y + 74.0,
            1.0,
            pal.outline_subtle,
        );
    }

    fn draw_card_footer(&self, card: RectF, hint: &str, next_label: &str, next_icon: Option<VectorIcon>) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let footer_line_y = card.y + card.h - 62.0;

        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        self.renderer.draw_text(
            hint,
            &self.renderer.font_caption,
            RectF::new(card.x + 28.0, card.y + card.h - 50.0, card.w - 236.0, 38.0),
            pal.text_muted,
            0,
            1,
            false,
        );

        let btn_next = Self::card_footer_next_btn(card);
        self.renderer.draw_button(
            btn_next,
            next_label,
            next_icon,
            ButtonVariant::Filled,
            btn_next.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
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

        // 1. Window background & Left Navigation Rail
        self.renderer.fill_rect(RectF::new(0.0, 0.0, w, h), pal.bg);
        let nav_rect = self.nav_rail_rect();
        self.renderer.fill_rounded_rect(nav_rect, 14.0, pal.nav_bg);

        // 2. Custom Borderless Titlebar
        let hover_theme = D2DRenderer::header_theme_rect(w).contains(mx, my);
        let hover_min = D2DRenderer::header_min_rect(w).contains(mx, my);
        let hover_close = D2DRenderer::header_close_rect(w).contains(mx, my);
        self.renderer.draw_window_header(
            w,
            "Nyan Setup Studio",
            "",
            &pal,
            hover_theme,
            hover_min,
            hover_close,
        );

        // 3. Left Navigation Rail Items
        self.renderer.draw_text(
            "打包配置步骤",
            &self.renderer.font_caption,
            RectF::new(28.0, 62.0, 170.0, 20.0),
            pal.text_muted,
            0,
            1,
            false,
        );

        let tabs = [
            (StudioTab::Metadata, VectorIcon::Info, "1. 应用元数据"),
            (StudioTab::FilesAndPaths, VectorIcon::Folder, "2. 目录与路径"),
            (StudioTab::OptionsAndLicense, VectorIcon::Shield, "3. 选项与协议"),
            (StudioTab::Prerequisites, VectorIcon::Sparkles, "4. 环境检测"),
            (StudioTab::BuildAndTest, VectorIcon::Rocket, "5. 构建与测试"),
        ];

        for (i, (tab, icon, title)) in tabs.iter().enumerate() {
            let r = Self::nav_item_rect(i);
            let selected = self.active_tab == *tab;
            let hovered = r.contains(mx, my);

            if selected {
                self.renderer.fill_rounded_rect(r, 19.0, pal.primary);
            } else if hovered {
                self.renderer
                    .fill_rounded_rect(r, 19.0, rgba(0x6366F1, 0.12));
            }
            self.renderer.draw_ripples(&self.ripples, r);

            let fg = if selected {
                rgb(0xFFFFFF)
            } else {
                pal.text_primary
            };
            self.renderer
                .draw_icon(*icon, r.x + 22.0, r.y + r.h * 0.5, 15.0, fg);
            let t_rect = RectF::new(r.x + 42.0, r.y, r.w - 48.0, r.h);
            let font = if selected {
                &self.renderer.font_body_bold
            } else {
                &self.renderer.font_body
            };
            self.renderer.draw_text(title, font, t_rect, fg, 0, 1, false);
        }

        // Summary box inside left rail
        let stat_card = RectF::new(22.0, 316.0, 186.0, 238.0);
        self.renderer.fill_rounded_rect(stat_card, 12.0, pal.surface);
        self.renderer
            .draw_rounded_rect(stat_card, 12.0, 1.0, pal.outline_subtle);

        self.renderer.draw_text(
            "当前源目录统计",
            &self.renderer.font_caption,
            RectF::new(36.0, 326.0, 158.0, 18.0),
            pal.text_secondary,
            0,
            1,
            false,
        );
        self.renderer.draw_line(
            32.0,
            348.0,
            198.0,
            348.0,
            1.0,
            pal.outline_subtle,
        );

        self.renderer.draw_text(
            &format!("{} 个文件", self.scanned_files.len()),
            &self.renderer.font_title_lg,
            RectF::new(36.0, 356.0, 158.0, 28.0),
            pal.primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!("未压缩体积: {}", format_bytes(self.scanned_total_bytes)),
            &self.renderer.font_body_bold,
            RectF::new(36.0, 386.0, 158.0, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let info_rows = [
            "渲染引擎: Direct2D 硬件加速",
            "运行库: 静态链接 CRT",
            "压缩流: LZMA2 固实 + BCJ",
            "卸载器: 自动生成 unins000",
        ];
        for (idx, line) in info_rows.iter().enumerate() {
            let ry = 420.0 + (idx as f32) * 25.0;
            self.renderer.fill_circle(38.0, ry + 9.0, 3.0, pal.success);
            self.renderer.draw_text(
                line,
                &self.renderer.font_caption,
                RectF::new(48.0, ry, 154.0, 18.0),
                pal.text_secondary,
                0,
                1,
                false,
            );
        }

        let demo_btn = self.btn_generate_demo_rect();
        self.renderer.draw_button(
            demo_btn,
            "生成演示项目",
            Some(VectorIcon::Sparkles),
            ButtonVariant::Outlined,
            demo_btn.contains(mx, my),
            !self.is_building,
            &pal,
            &self.ripples,
        );

        // 4. Right Content Card
        let card = self.content_card_rect();
        self.renderer.draw_card(card, 14.0, &pal);

        match self.active_tab {
            StudioTab::Metadata => self.draw_tab_metadata(card),
            StudioTab::FilesAndPaths => self.draw_tab_files(card),
            StudioTab::OptionsAndLicense => self.draw_tab_options(card),
            StudioTab::Prerequisites => self.draw_tab_prerequisites(card),
            StudioTab::BuildAndTest => self.draw_tab_build(card),
        }

        // Subtle outer border around the borderless window
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

    fn draw_tab_metadata(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;
        let w_half = (w_full - 20.0) * 0.5;

        self.draw_card_header(
            card,
            "应用基本元数据配置",
            "设置安装向导展示的应用名称、版本号、发布者及注册表卸载标识。",
        );

        // Row 1 (y = card.y + 112.0)
        let r_name = RectF::new(x0, card.y + 112.0, w_half, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_name,
            "应用程序名称 (App Name)",
            &self.config.app_name,
            self.focused_field == FocusedField::AppName,
            r_name.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let r_ver = RectF::new(x0 + w_half + 20.0, card.y + 112.0, w_half, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_ver,
            "版本号 (App Version)",
            &self.config.app_version,
            self.focused_field == FocusedField::AppVersion,
            r_ver.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        // Row 2 (y = card.y + 182.0)
        let r_pub = RectF::new(x0, card.y + 182.0, w_half, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_pub,
            "发布者 / 开发者名称 (Publisher)",
            &self.config.publisher,
            self.focused_field == FocusedField::Publisher,
            r_pub.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let r_id = RectF::new(x0 + w_half + 20.0, card.y + 182.0, w_half, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_id,
            "应用唯一标识符 (App ID / 注册表卸载键名)",
            &self.config.app_id,
            self.focused_field == FocusedField::AppId,
            r_id.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        // Row 3 (y = card.y + 252.0): Main Exe (left) & Installer Icon (right)
        let r_exe = RectF::new(x0, card.y + 252.0, w_half - 100.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_exe,
            "主程序相对路径 (指向 .exe)",
            &self.config.main_exe,
            self.focused_field == FocusedField::MainExe,
            r_exe.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let btn_pick_exe = RectF::new(x0 + w_half - 92.0, card.y + 252.0, 92.0, 38.0);
        self.renderer.draw_button(
            btn_pick_exe,
            "选择exe",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_pick_exe.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let r_ico = RectF::new(x0 + w_half + 20.0, card.y + 252.0, w_half - 100.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_ico,
            "安装包图标 (可选 .ico，留空默认)",
            &self.installer_icon_input,
            self.focused_field == FocusedField::InstallerIcon,
            r_ico.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let btn_pick_ico = RectF::new(x0 + w_full - 92.0, card.y + 252.0, 92.0, 38.0);
        self.renderer.draw_button(
            btn_pick_ico,
            "选择ico",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_pick_ico.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // Live Brand Preview Card (y = card.y + 314.0, h = 176.0)
        let preview_box = RectF::new(x0, card.y + 314.0, w_full, 176.0);
        self.renderer
            .fill_rounded_rect(preview_box, 12.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(preview_box, 12.0, 1.0, pal.outline_subtle);

        self.renderer.draw_text(
            "安装向导品牌卡片与系统注册信息实时预览",
            &self.renderer.font_caption,
            RectF::new(preview_box.x + 18.0, preview_box.y + 12.0, 360.0, 18.0),
            pal.text_muted,
            0,
            1,
            false,
        );

        let icon_box = RectF::new(preview_box.x + 18.0, preview_box.y + 42.0, 52.0, 52.0);
        self.renderer.draw_app_icon_with_override(icon_box, Some(&self.installer_icon_input));

        self.renderer.draw_text(
            &format!("{} v{}", self.config.app_name, self.config.app_version),
            &self.renderer.font_title_md,
            RectF::new(preview_box.x + 84.0, preview_box.y + 40.0, 520.0, 24.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!(
                "发布者: {}   |   启动入口: {}",
                self.config.publisher, self.config.main_exe
            ),
            &self.renderer.font_body,
            RectF::new(preview_box.x + 84.0, preview_box.y + 68.0, 520.0, 22.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        self.renderer.draw_line(
            preview_box.x + 18.0,
            preview_box.y + 108.0,
            preview_box.x + preview_box.w - 18.0,
            preview_box.y + 108.0,
            1.0,
            pal.outline_subtle,
        );

        self.renderer.draw_text(
            &format!(
                "快捷方式: {}.lnk   |   卸载项: Uninstall\\{}",
                self.config.app_name, self.config.app_id
            ),
            &self.renderer.font_mono,
            RectF::new(preview_box.x + 18.0, preview_box.y + 120.0, preview_box.w - 36.0, 20.0),
            pal.text_secondary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!("默认安装路径: {}", self.config.default_install_dir),
            &self.renderer.font_mono,
            RectF::new(preview_box.x + 18.0, preview_box.y + 144.0, preview_box.w - 36.0, 20.0),
            pal.primary,
            0,
            1,
            false,
        );

        self.draw_card_footer(
            card,
            "提示：支持鼠标拖拽选中、Ctrl+A 全选、Ctrl+C/X/V 复制粘贴与方向键移动光标",
            "下一步：目录与路径",
            None,
        );
    }

    fn draw_tab_files(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;

        self.draw_card_header(
            card,
            "打包源目录与输出路径配置",
            "选择需要打包发布的应用程序文件夹，并指定生成的单文件安装包保存位置。",
        );

        // 1. Source Directory (y = card.y + 112.0)
        let r_src = RectF::new(x0, card.y + 112.0, w_full - 138.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_src,
            "待打包的源文件夹路径 (Source Directory)",
            &self.source_dir,
            self.focused_field == FocusedField::SourceDir,
            r_src.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );
        let btn_src = RectF::new(x0 + w_full - 126.0, card.y + 112.0, 126.0, 38.0);
        self.renderer.draw_button(
            btn_src,
            "浏览目录",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_src.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // 2. Default Target Install Directory (y = card.y + 180.0)
        let r_inst = RectF::new(x0, card.y + 180.0, w_full - 246.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_inst,
            "默认目标安装路径 (支持 %LOCALAPPDATA% 或 %PROGRAMFILES%)",
            &self.config.default_install_dir,
            self.focused_field == FocusedField::DefaultInstallDir,
            r_inst.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );
        let btn_localappdata = RectF::new(x0 + w_full - 234.0, card.y + 180.0, 112.0, 38.0);
        self.renderer.draw_button(
            btn_localappdata,
            "LocalAppData",
            None,
            ButtonVariant::Tonal,
            btn_localappdata.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
        let btn_progfiles = RectF::new(x0 + w_full - 114.0, card.y + 180.0, 114.0, 38.0);
        self.renderer.draw_button(
            btn_progfiles,
            "Program Files",
            None,
            ButtonVariant::Tonal,
            btn_progfiles.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // 3. Output Installer .exe path (y = card.y + 248.0)
        let r_out = RectF::new(x0, card.y + 248.0, w_full - 138.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_out,
            "生成的单文件安装包保存路径 (Output Setup .exe)",
            &self.output_exe,
            self.focused_field == FocusedField::OutputExe,
            r_out.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );
        let btn_out = RectF::new(x0 + w_full - 126.0, card.y + 248.0, 126.0, 38.0);
        self.renderer.draw_button(
            btn_out,
            "另存为",
            Some(VectorIcon::Document),
            ButtonVariant::Outlined,
            btn_out.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // 4. Scanned Files Table Card (y = card.y + 302.0, h = 192.0)
        let list_box = RectF::new(x0, card.y + 302.0, w_full, 192.0);
        self.renderer
            .fill_rounded_rect(list_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(list_box, 10.0, 1.0, pal.outline_subtle);

        let header_bar = RectF::new(list_box.x, list_box.y, list_box.w, 30.0);
        self.renderer
            .fill_rounded_rect(header_bar, 10.0, rgba(0x6366F1, 0.08));
        self.renderer.draw_text(
            &format!(
                "源目录待打包文件列表 (共 {} 个文件，总计 {})",
                self.scanned_files.len(),
                format_bytes(self.scanned_total_bytes)
            ),
            &self.renderer.font_caption,
            RectF::new(list_box.x + 14.0, list_box.y + 5.0, list_box.w - 140.0, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            "原始大小",
            &self.renderer.font_caption,
            RectF::new(list_box.x + list_box.w - 114.0, list_box.y + 5.0, 98.0, 20.0),
            pal.text_secondary,
            2,
            1,
            false,
        );

        if self.scanned_files.is_empty() {
            self.renderer.draw_text(
                "当前源目录下没有文件。请检查路径，或点击左下角“生成演示项目”。",
                &self.renderer.font_body,
                list_box,
                pal.warning,
                1,
                1,
                false,
            );
        } else {
            let max_visible = 6usize;
            let start = self
                .file_scroll_offset
                .min(self.scanned_files.len().saturating_sub(1));
            for (row, (rel, sz)) in self
                .scanned_files
                .iter()
                .skip(start)
                .take(max_visible)
                .enumerate()
            {
                let ry = list_box.y + 34.0 + (row as f32) * 25.0;
                if row % 2 == 1 {
                    self.renderer.fill_rounded_rect(
                        RectF::new(list_box.x + 6.0, ry, list_box.w - 12.0, 23.0),
                        4.0,
                        rgba(0x6366F1, 0.04),
                    );
                }
                self.renderer.draw_text(
                    rel,
                    &self.renderer.font_mono,
                    RectF::new(list_box.x + 14.0, ry, list_box.w - 130.0, 23.0),
                    pal.text_primary,
                    0,
                    1,
                    false,
                );
                self.renderer.draw_text(
                    &format_bytes(*sz),
                    &self.renderer.font_mono,
                    RectF::new(list_box.x + list_box.w - 114.0, ry, 98.0, 23.0),
                    pal.text_secondary,
                    2,
                    1,
                    false,
                );
            }
        }

        self.draw_card_footer(
            card,
            "提示：可在文件列表区域使用鼠标滚轮上下滚动查看全部文件",
            "下一步：选项与协议",
            None,
        );
    }

    fn draw_tab_options(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;
        let left_w = 304.0;
        let right_x = x0 + left_w + 20.0;
        let right_w = w_full - left_w - 20.0;

        self.draw_card_header(
            card,
            "安装行为选项与软件许可协议",
            "配置快捷方式生成、控制面板卸载注册、数据压缩比及终端用户许可协议 (EULA)。",
        );

        // Left column: 4 Checkboxes + Process Conflict Policy + Compression Level
        self.renderer.draw_text(
            "默认快捷方式与安装行为",
            &self.renderer.font_body_bold,
            RectF::new(x0 + 4.0, card.y + 84.0, left_w, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let c1 = RectF::new(x0, card.y + 108.0, left_w, 42.0);
        self.renderer.draw_checkbox(
            c1,
            self.config.create_desktop_shortcut,
            c1.contains(mx, my),
            "创建桌面快捷方式",
            Some("在用户桌面生成指向主程序的 .lnk 图标"),
            &pal,
        );

        let c2 = RectF::new(x0, card.y + 152.0, left_w, 42.0);
        self.renderer.draw_checkbox(
            c2,
            self.config.create_start_menu_shortcut,
            c2.contains(mx, my),
            "创建开始菜单快捷方式",
            Some("在 Windows 开始菜单程序组创建快捷方式"),
            &pal,
        );

        let c3 = RectF::new(x0, card.y + 196.0, left_w, 42.0);
        self.renderer.draw_checkbox(
            c3,
            self.config.register_uninstaller,
            c3.contains(mx, my),
            "生成卸载程序并注册系统卸载项",
            Some("释放 unins000.exe 并注册到“应用和功能”"),
            &pal,
        );

        let c4 = RectF::new(x0, card.y + 240.0, left_w, 42.0);
        self.renderer.draw_checkbox(
            c4,
            self.config.auto_launch_after_install,
            c4.contains(mx, my),
            "安装完成页默认勾选“立即运行”",
            Some("安装结束时允许用户一键启动主程序"),
            &pal,
        );

        // Process Conflict Policy Selector (y = card.y + 292..374)
        self.renderer.draw_text(
            "安装/更新时进程占用处理策略",
            &self.renderer.font_body_bold,
            RectF::new(x0 + 4.0, card.y + 292.0, left_w, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let pol_w = (left_w - 8.0) * 0.5;
        let btn_pol_auto = RectF::new(x0, card.y + 316.0, pol_w, 34.0);
        let btn_pol_prompt = RectF::new(x0 + pol_w + 8.0, card.y + 316.0, pol_w, 34.0);
        self.renderer.draw_button(
            btn_pol_auto,
            "自动结束占用进程",
            None,
            if self.config.process_conflict_policy == ProcessConflictPolicy::AutoKill {
                ButtonVariant::Filled
            } else {
                ButtonVariant::Outlined
            },
            btn_pol_auto.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
        self.renderer.draw_button(
            btn_pol_prompt,
            "弹出提示窗口",
            None,
            if self.config.process_conflict_policy == ProcessConflictPolicy::PromptUser {
                ButtonVariant::Filled
            } else {
                ButtonVariant::Outlined
            },
            btn_pol_prompt.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
        let pol_hint = match self.config.process_conflict_policy {
            ProcessConflictPolicy::AutoKill => "检测到目标程序运行时，自动静默结束进程并继续安装",
            ProcessConflictPolicy::PromptUser => {
                "检测到占用时弹窗列出运行进程，提供“重试”与“结束进程并重试”"
            }
        };
        self.renderer.draw_text(
            pol_hint,
            &self.renderer.font_caption,
            RectF::new(x0 + 4.0, card.y + 354.0, left_w, 18.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        // Compression Level Selector (y = card.y + 382..444)
        self.renderer.draw_text(
            "LZMA2 固实压缩 + x86 BCJ 字典等级",
            &self.renderer.font_body_bold,
            RectF::new(x0 + 4.0, card.y + 382.0, left_w, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let levels = [(1u32, "快速 (Lv.1)"), (6u32, "标准 (Lv.6)"), (9u32, "极限 (Lv.9)")];
        let seg_w = (left_w - 16.0) / 3.0;
        for (i, (lv, label)) in levels.iter().enumerate() {
            let sr = RectF::new(x0 + (i as f32) * (seg_w + 8.0), card.y + 408.0, seg_w, 36.0);
            let variant = if self.config.compression_level == *lv {
                ButtonVariant::Filled
            } else {
                ButtonVariant::Outlined
            };
            self.renderer.draw_button(
                sr,
                label,
                None,
                variant,
                sr.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        }

        // Right column: License Agreement Preview & Import
        self.renderer.draw_text(
            "软件许可协议内容 (支持鼠标滚轮滚动)",
            &self.renderer.font_body_bold,
            RectF::new(right_x, card.y + 88.0, right_w, 22.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let eula_box = RectF::new(right_x, card.y + 114.0, right_w, 332.0);
        self.renderer
            .fill_rounded_rect(eula_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(eula_box, 10.0, 1.0, pal.outline_subtle);

        let lines: Vec<&str> = self.config.license_text.lines().collect();
        let start_line = self.license_scroll_line.min(lines.len().saturating_sub(1));
        let visible_text = lines
            .into_iter()
            .skip(start_line)
            .take(13)
            .collect::<Vec<_>>()
            .join("\n");

        let inner_eula = eula_box.inset(12.0, 10.0);
        unsafe {
            self.renderer
                .rt()
                .PushAxisAlignedClip(&inner_eula.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        self.renderer.draw_text(
            &visible_text,
            &self.renderer.font_caption,
            inner_eula,
            pal.text_secondary,
            0,
            0,
            true,
        );
        unsafe {
            self.renderer.rt().PopAxisAlignedClip();
        }

        let btn_load_eula = RectF::new(right_x, card.y + 456.0, (right_w - 10.0) * 0.5, 34.0);
        self.renderer.draw_button(
            btn_load_eula,
            "导入协议文件",
            Some(VectorIcon::Document),
            ButtonVariant::Outlined,
            btn_load_eula.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let btn_paste_eula = RectF::new(
            right_x + (right_w - 10.0) * 0.5 + 10.0,
            card.y + 456.0,
            (right_w - 10.0) * 0.5,
            34.0,
        );
        self.renderer.draw_button(
            btn_paste_eula,
            "从剪贴板粘贴",
            None,
            ButtonVariant::Tonal,
            btn_paste_eula.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        self.draw_card_footer(
            card,
            "提示：用户安装软件时需勾选同意上述许可协议方可继续安装",
            "下一步：环境检测",
            None,
        );
    }

    fn draw_tab_prerequisites(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;

        self.draw_card_header(
            card,
            "前置运行环境检测与自动部署 (Prerequisites)",
            "配置安装时需校验的注册表键值或系统文件/PE版本，未满足时自动运行随包组件或自定义逻辑。",
        );

        // Top Preset Bar (y = card.y + 84.0, h = 34.0)
        let presets = [
            "+ VC++ 2015-2022 (x64)",
            "+ VC++ 2015-2022 (x86)",
            "+ 注册表检测规则",
            "+ 文件/PE版本检测",
        ];
        let preset_w = (w_full - 30.0) / 4.0;
        for (i, label) in presets.iter().enumerate() {
            let pr = RectF::new(
                x0 + (i as f32) * (preset_w + 10.0),
                card.y + 84.0,
                preset_w,
                34.0,
            );
            self.renderer.draw_button(
                pr,
                label,
                None,
                ButtonVariant::Tonal,
                pr.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        }

        // Left Rule List Box (x = x0, y = card.y + 128.0, w = 216.0, h = 368.0)
        let list_w = 216.0;
        let list_box = RectF::new(x0, card.y + 128.0, list_w, 368.0);
        self.renderer
            .fill_rounded_rect(list_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(list_box, 10.0, 1.0, pal.outline_subtle);

        self.renderer.draw_text(
            &format!("检测规则列表 ({})", self.config.prerequisites.len()),
            &self.renderer.font_caption,
            RectF::new(list_box.x + 12.0, list_box.y + 8.0, list_w - 24.0, 18.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        let max_rules_vis = 6usize;
        for (idx, rule) in self
            .config
            .prerequisites
            .iter()
            .take(max_rules_vis)
            .enumerate()
        {
            let item_r = RectF::new(
                list_box.x + 8.0,
                list_box.y + 32.0 + (idx as f32) * 46.0,
                list_w - 16.0,
                42.0,
            );
            let is_sel = idx == self.selected_prereq_idx;
            let is_hov = item_r.contains(mx, my);
            if is_sel {
                self.renderer
                    .fill_rounded_rect(item_r, 8.0, pal.primary_container);
                self.renderer
                    .draw_rounded_rect(item_r, 8.0, 1.2, pal.primary);
            } else if is_hov {
                self.renderer
                    .fill_rounded_rect(item_r, 8.0, rgba(0x6366F1, 0.08));
            }

            // Checkbox indicator on left of item
            let chk_box = RectF::new(item_r.x + 8.0, item_r.y + 12.0, 18.0, 18.0);
            if rule.enabled {
                self.renderer.fill_rounded_rect(chk_box, 4.0, pal.primary);
                self.renderer.draw_icon(
                    VectorIcon::Check,
                    chk_box.x + 9.0,
                    chk_box.y + 9.0,
                    11.0,
                    rgb(0xFFFFFF),
                );
            } else {
                self.renderer
                    .draw_rounded_rect(chk_box, 4.0, 1.2, pal.outline);
            }

            self.renderer.draw_text(
                &rule.name,
                &self.renderer.font_body_bold,
                RectF::new(item_r.x + 30.0, item_r.y + 4.0, item_r.w - 34.0, 18.0),
                if is_sel {
                    pal.on_primary_container
                } else {
                    pal.text_primary
                },
                0,
                1,
                false,
            );

            let local_ok = evaluate_prereq_condition(&rule.condition, None);
            let sub_tag = format!(
                "{} · {}",
                rule.condition.short_tag(),
                if local_ok { "本机已满足" } else { "本机未满足" }
            );
            self.renderer.draw_text(
                &sub_tag,
                &self.renderer.font_caption,
                RectF::new(item_r.x + 30.0, item_r.y + 22.0, item_r.w - 34.0, 16.0),
                if local_ok { pal.success } else { pal.warning },
                0,
                1,
                false,
            );
        }

        let btn_del_rule = RectF::new(
            list_box.x + 10.0,
            list_box.y + list_box.h - 42.0,
            list_w - 20.0,
            32.0,
        );
        self.renderer.draw_button(
            btn_del_rule,
            "删除选中规则",
            None,
            ButtonVariant::Outlined,
            btn_del_rule.contains(mx, my),
            !self.config.prerequisites.is_empty(),
            &pal,
            &self.ripples,
        );

        // Right Rule Editor Panel (x = x0 + 230.0, y = card.y + 128.0, w = w_full - 230.0, h = 368.0)
        let right_x = x0 + 230.0;
        let right_w = w_full - 230.0;
        let edit_box = RectF::new(right_x, card.y + 128.0, right_w, 368.0);
        self.renderer
            .fill_rounded_rect(edit_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(edit_box, 10.0, 1.0, pal.outline_subtle);

        let Some(rule) = self.config.prerequisites.get(self.selected_prereq_idx) else {
            self.renderer.draw_text(
                "当前未配置任何前置环境检测规则。\n点击上方预设按钮可一键添加 VC++ 运行库、注册表键值或文件/PE版本检测。",
                &self.renderer.font_body,
                edit_box.inset(24.0, 120.0),
                pal.text_muted,
                1,
                1,
                true,
            );
            self.draw_card_footer(
                card,
                "提示：随包运行库仅在目标机器检测未通过时解压至 %TEMP% 执行并自动清理",
                "下一步：构建安装包",
                Some(VectorIcon::Rocket),
            );
            return;
        };

        let ex = edit_box.x + 16.0;
        let ew = edit_box.w - 32.0;

        // Row 1 (y = card.y + 152.0): Rule Name + Condition Kind Dropdown
        let r_rname = RectF::new(ex, card.y + 152.0, ew - 178.0, 34.0);
        self.renderer.draw_text_field_with_sel(
            r_rname,
            "规则名称 (安装进度展示)",
            &rule.name,
            self.focused_field == FocusedField::PrereqName,
            r_rname.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );
        let dd_cond_kind = RectF::new(ex + ew - 168.0, card.y + 152.0, 168.0, 34.0);
        let cond_kind_label = match rule.condition {
            PrereqCondition::Registry { .. } => "注册表键值检测",
            PrereqCondition::File { .. } => "文件 / PE版本",
        };
        self.renderer.draw_dropdown(
            dd_cond_kind,
            "检测条件类型",
            cond_kind_label,
            self.open_dropdown == Some(PrereqDropdown::ConditionKind),
            dd_cond_kind.contains(mx, my),
            &pal,
        );

        // Row 2 & Row 3: Condition Details
        match &rule.condition {
            PrereqCondition::Registry {
                root,
                subkey,
                value_name,
                arch_view,
                operator,
                expected,
            } => {
                // Row 2 (y = card.y + 210.0): Hive Dropdown + Arch Dropdown + Key Path
                let dd_hive = RectF::new(ex, card.y + 210.0, 86.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_hive,
                    "注册表根键",
                    root.label(),
                    self.open_dropdown == Some(PrereqDropdown::RegHive),
                    dd_hive.contains(mx, my),
                    &pal,
                );
                let dd_arch = RectF::new(ex + 94.0, card.y + 210.0, 102.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_arch,
                    "注册表视图",
                    arch_view.label(),
                    self.open_dropdown == Some(PrereqDropdown::RegArch),
                    dd_arch.contains(mx, my),
                    &pal,
                );
                let r_kpath = RectF::new(ex + 204.0, card.y + 210.0, ew - 204.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_kpath,
                    "注册表子键路径 (Subkey)",
                    subkey,
                    self.focused_field == FocusedField::PrereqTarget,
                    r_kpath.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );

                // Row 3 (y = card.y + 268.0): Value Name + Operator Dropdown + Expected Value
                let r_vname = RectF::new(ex, card.y + 268.0, 130.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_vname,
                    "值名称 (空=仅测键)",
                    value_name,
                    self.focused_field == FocusedField::PrereqValueName,
                    r_vname.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let dd_op = RectF::new(ex + 138.0, card.y + 268.0, 122.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_op,
                    "比较条件",
                    operator.label(),
                    self.open_dropdown == Some(PrereqDropdown::Operator),
                    dd_op.contains(mx, my),
                    &pal,
                );
                let r_exp = RectF::new(ex + 268.0, card.y + 268.0, ew - 268.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_exp,
                    "期望值 / 版本",
                    expected,
                    self.focused_field == FocusedField::PrereqExpected,
                    r_exp.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
            }
            PrereqCondition::File {
                path,
                operator,
                expected_version,
            } => {
                // Row 2 (y = card.y + 210.0): File Path
                let r_fpath = RectF::new(ex, card.y + 210.0, ew, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_fpath,
                    "目标文件路径 (支持 %SystemRoot% / %PROGRAMFILES% / %INSTALL_DIR%)",
                    path,
                    self.focused_field == FocusedField::PrereqTarget,
                    r_fpath.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );

                // Row 3 (y = card.y + 268.0): Operator Dropdown + Expected Version
                let dd_op = RectF::new(ex, card.y + 268.0, 146.0, 34.0);
                let file_op_label = match operator {
                    CheckOperator::Exists => "文件存在",
                    CheckOperator::GreaterOrEqual => "PE版本 >=",
                    CheckOperator::Equals => "PE版本 ==",
                    CheckOperator::NotExists => "文件不存在",
                    other => other.label(),
                };
                self.renderer.draw_dropdown(
                    dd_op,
                    "检测方式",
                    file_op_label,
                    self.open_dropdown == Some(PrereqDropdown::Operator),
                    dd_op.contains(mx, my),
                    &pal,
                );
                let r_exp = RectF::new(ex + 156.0, card.y + 268.0, ew - 156.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_exp,
                    "期望 PE 版本号 (如 14.38.0.0，仅测存在留空)",
                    expected_version,
                    self.focused_field == FocusedField::PrereqExpected,
                    r_exp.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
            }
        }

        // Row 4 (y = card.y + 328.0): Action Type Dropdown
        let dd_act_kind = RectF::new(ex, card.y + 328.0, ew, 34.0);
        let cur_act_label = match rule.action {
            PrereqAction::RunBundledExe { .. } => {
                "1. 随包打包并运行安装器 (解压至 %TEMP% 执行并自动清理)"
            }
            PrereqAction::RunCommand { .. } => {
                "2. 执行系统命令或脚本 (支持 cmd / powershell / %INSTALL_DIR%)"
            }
            PrereqAction::PromptAndAbort { .. } => {
                "3. 弹窗提示用户并中止安装 (可选引导打开下载链接)"
            }
        };
        self.renderer.draw_dropdown(
            dd_act_kind,
            "检测未通过时的执行动作",
            cur_act_label,
            self.open_dropdown == Some(PrereqDropdown::ActionKind),
            dd_act_kind.contains(mx, my),
            &pal,
        );

        // Row 5 (y = card.y + 388.0) & Row 6 (y = card.y + 448.0): Action Fields
        match &rule.action {
            PrereqAction::RunBundledExe {
                source_file,
                args,
                elevate,
                ..
            } => {
                let r_act1 = RectF::new(ex, card.y + 388.0, ew - 96.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act1,
                    "随包打包的安装器路径 (如 vc_redist.x64.exe)",
                    source_file,
                    self.focused_field == FocusedField::PrereqActionPrimary,
                    r_act1.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let btn_pick_redist = RectF::new(ex + ew - 88.0, card.y + 388.0, 88.0, 34.0);
                self.renderer.draw_button(
                    btn_pick_redist,
                    "选择exe",
                    Some(VectorIcon::Folder),
                    ButtonVariant::Outlined,
                    btn_pick_redist.contains(mx, my),
                    true,
                    &pal,
                    &self.ripples,
                );

                let r_act2 = RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act2,
                    "静默安装参数 (如 /install /quiet /norestart)",
                    args,
                    self.focused_field == FocusedField::PrereqActionSecondary,
                    r_act2.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let dd_elev = RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_elev,
                    "权限模式",
                    if *elevate {
                        "UAC 提权运行"
                    } else {
                        "普通权限运行"
                    },
                    self.open_dropdown == Some(PrereqDropdown::ElevateMode),
                    dd_elev.contains(mx, my),
                    &pal,
                );
            }
            PrereqAction::RunCommand {
                program,
                args,
                elevate,
                ..
            } => {
                let r_act1 = RectF::new(ex, card.y + 388.0, ew, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act1,
                    "执行程序路径 (支持 cmd / powershell / %INSTALL_DIR%)",
                    program,
                    self.focused_field == FocusedField::PrereqActionPrimary,
                    r_act1.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );

                let r_act2 = RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act2,
                    "命令行参数 (Arguments)",
                    args,
                    self.focused_field == FocusedField::PrereqActionSecondary,
                    r_act2.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let dd_elev = RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_elev,
                    "权限模式",
                    if *elevate {
                        "UAC 提权运行"
                    } else {
                        "普通权限运行"
                    },
                    self.open_dropdown == Some(PrereqDropdown::ElevateMode),
                    dd_elev.contains(mx, my),
                    &pal,
                );
            }
            PrereqAction::PromptAndAbort {
                message,
                download_url,
            } => {
                let r_act1 = RectF::new(ex, card.y + 388.0, ew, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act1,
                    "检测未通过时的弹窗警告文本 (Message)",
                    message,
                    self.focused_field == FocusedField::PrereqActionPrimary,
                    r_act1.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let r_act2 = RectF::new(ex, card.y + 448.0, ew, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act2,
                    "引导用户打开的下载链接 URL (可选，留空仅提示)",
                    download_url,
                    self.focused_field == FocusedField::PrereqActionSecondary,
                    r_act2.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
            }
        }

        self.draw_card_footer(
            card,
            "提示：随包运行库仅在目标机器检测未通过时解压至 %TEMP% 执行并自动清理",
            "下一步：构建安装包",
            Some(VectorIcon::Rocket),
        );

        // Render active floating dropdown menu overlay on top of everything
        if let Some(open_dd) = self.open_dropdown {
            self.draw_prereq_dropdown_overlay(open_dd, rule, ex, ew, card.y);
        }
    }

    fn draw_prereq_dropdown_overlay(
        &self,
        open_dd: PrereqDropdown,
        rule: &PrerequisiteRule,
        ex: f32,
        ew: f32,
        card_y: f32,
    ) {
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let w = self.width_dip;
        let h = self.height_dip;
        let pal = self.palette;

        match open_dd {
            PrereqDropdown::ConditionKind => {
                let trig = RectF::new(ex + ew - 168.0, card_y + 152.0, 168.0, 34.0);
                let items = ["注册表键值检测", "文件 / PE版本"];
                let sel = match rule.condition {
                    PrereqCondition::Registry { .. } => 0,
                    PrereqCondition::File { .. } => 1,
                };
                self.renderer
                    .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
            }
            PrereqDropdown::RegHive => {
                if let PrereqCondition::Registry { root, .. } = &rule.condition {
                    let trig = RectF::new(ex, card_y + 210.0, 86.0, 34.0);
                    let items = [
                        "HKLM (本机系统)",
                        "HKCU (当前用户)",
                        "HKCR (类与关联)",
                        "HKU (所有用户)",
                    ];
                    let sel = match root {
                        RegHiveRoot::Hklm => 0,
                        RegHiveRoot::Hkcu => 1,
                        RegHiveRoot::Hkcr => 2,
                        RegHiveRoot::Hku => 3,
                    };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
            }
            PrereqDropdown::RegArch => {
                if let PrereqCondition::Registry { arch_view, .. } = &rule.condition {
                    let trig = RectF::new(ex + 94.0, card_y + 210.0, 102.0, 34.0);
                    let items = ["默认视图", "64位视图 (x64)", "32位视图 (x86)"];
                    let sel = match arch_view {
                        RegArchView::Default => 0,
                        RegArchView::X64 => 1,
                        RegArchView::X86 => 2,
                    };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
            }
            PrereqDropdown::Operator => match &rule.condition {
                PrereqCondition::Registry { operator, .. } => {
                    let trig = RectF::new(ex + 138.0, card_y + 268.0, 122.0, 34.0);
                    let items = [
                        "存在",
                        "版本/数值 >=",
                        "等于 (==)",
                        "包含文本",
                        "不等于 (!=)",
                        "不存在",
                    ];
                    let sel = match operator {
                        CheckOperator::Exists => 0,
                        CheckOperator::GreaterOrEqual => 1,
                        CheckOperator::Equals => 2,
                        CheckOperator::Contains => 3,
                        CheckOperator::NotEquals => 4,
                        CheckOperator::NotExists => 5,
                    };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
                PrereqCondition::File { operator, .. } => {
                    let trig = RectF::new(ex, card_y + 268.0, 146.0, 34.0);
                    let items = ["文件存在", "PE版本 >=", "PE版本 ==", "文件不存在"];
                    let sel = match operator {
                        CheckOperator::Exists => 0,
                        CheckOperator::GreaterOrEqual => 1,
                        CheckOperator::Equals => 2,
                        CheckOperator::NotExists => 3,
                        _ => 1,
                    };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
            },
            PrereqDropdown::ActionKind => {
                let trig = RectF::new(ex, card_y + 328.0, ew, 34.0);
                let items = [
                    "1. 随包打包并运行安装器 (解压至 %TEMP% 执行并自动清理)",
                    "2. 执行系统命令或安装目录脚本 (支持 cmd / powershell / %INSTALL_DIR%)",
                    "3. 弹窗提示用户并中止安装 (可选引导打开下载链接)",
                ];
                let sel = match rule.action {
                    PrereqAction::RunBundledExe { .. } => 0,
                    PrereqAction::RunCommand { .. } => 1,
                    PrereqAction::PromptAndAbort { .. } => 2,
                };
                self.renderer
                    .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
            }
            PrereqDropdown::ElevateMode => {
                let cur_elev = match &rule.action {
                    PrereqAction::RunBundledExe { elevate, .. }
                    | PrereqAction::RunCommand { elevate, .. } => Some(*elevate),
                    PrereqAction::PromptAndAbort { .. } => None,
                };
                if let Some(elevate) = cur_elev {
                    let trig = RectF::new(ex + ew - 126.0, card_y + 448.0, 126.0, 34.0);
                    let items = ["UAC 提权运行 (管理员)", "普通权限运行 (当前用户)"];
                    let sel = if elevate { 0 } else { 1 };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
            }
        }
    }

    fn draw_tab_build(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;

        self.draw_card_header(
            card,
            "构建独立单文件安装程序 (Build & Verify)",
            "将当前配置与源目录全部文件压缩封装至静态 CRT Stub 中，输出独立 .exe 安装包。",
        );

        // Summary Banner Card (y = card.y + 90.0, h = 84.0)
        let sum_box = RectF::new(x0, card.y + 90.0, w_full, 84.0);
        self.renderer
            .fill_rounded_rect(sum_box, 12.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(sum_box, 12.0, 1.0, pal.outline_subtle);

        self.renderer.draw_text(
            &format!(
                "目标应用: {} v{} ({})   |   待打包文件: {} 个 ({})",
                self.config.app_name,
                self.config.app_version,
                self.config.publisher,
                self.scanned_files.len(),
                format_bytes(self.scanned_total_bytes)
            ),
            &self.renderer.font_body_bold,
            RectF::new(sum_box.x + 16.0, sum_box.y + 12.0, sum_box.w - 32.0, 22.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!("输出安装包路径: {}", self.output_exe),
            &self.renderer.font_mono,
            RectF::new(sum_box.x + 16.0, sum_box.y + 36.0, sum_box.w - 32.0, 20.0),
            pal.primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            "静态 CRT 链接: target-feature=+crt-static (无需目标机器安装 VC++ 运行库，单文件直接运行)",
            &self.renderer.font_caption,
            RectF::new(sum_box.x + 16.0, sum_box.y + 58.0, sum_box.w - 32.0, 18.0),
            pal.success,
            0,
            1,
            false,
        );

        // Progress bar & Status text (y = card.y + 190..236)
        self.renderer.draw_text(
            &self.build_status_text,
            &self.renderer.font_body_bold,
            RectF::new(x0, card.y + 190.0, w_full - 70.0, 22.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!("{:.0}%", self.build_progress * 100.0),
            &self.renderer.font_body_bold,
            RectF::new(x0 + w_full - 64.0, card.y + 190.0, 64.0, 22.0),
            pal.primary,
            2,
            1,
            false,
        );

        let prog_rect = RectF::new(x0, card.y + 220.0, w_full, 10.0);
        self.renderer
            .draw_progress_bar(prog_rect, self.build_progress, &pal);

        // Build Log Console Card (y = card.y + 244.0, h = 252.0)
        let log_box = RectF::new(x0, card.y + 244.0, w_full, 252.0);
        self.renderer
            .fill_rounded_rect(log_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(log_box, 10.0, 1.0, pal.outline_subtle);

        let wrapped_logs = self.renderer.wrap_log_lines(
            &self.build_logs,
            &self.renderer.font_mono,
            log_box.w - 28.0,
        );
        let max_logs = 10usize;
        let skip = wrapped_logs.len().saturating_sub(max_logs);
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
            } else if orig.starts_with("[完成]") || orig.starts_with("[输出文件]") {
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

        // Footer bar with Build & Test actions
        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_open_dir = RectF::new(x0, card.y + card.h - 50.0, 146.0, 38.0);
        self.renderer.draw_button(
            btn_open_dir,
            "打开输出目录",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_open_dir.contains(mx, my),
            self.last_built_exe.is_some() && !self.is_building,
            &pal,
            &self.ripples,
        );

        let btn_run = RectF::new(x0 + 158.0, card.y + card.h - 50.0, 186.0, 38.0);
        self.renderer.draw_button(
            btn_run,
            "运行测试安装包",
            Some(VectorIcon::Play),
            ButtonVariant::Tonal,
            btn_run.contains(mx, my),
            self.last_built_exe.is_some() && !self.is_building,
            &pal,
            &self.ripples,
        );

        let btn_build = RectF::new(card.x + card.w - 236.0, card.y + card.h - 50.0, 208.0, 38.0);
        let build_label = if self.is_building {
            "正在打包构建中..."
        } else {
            "开始构建安装包 (.exe)"
        };
        self.renderer.draw_button(
            btn_build,
            build_label,
            Some(VectorIcon::Rocket),
            ButtonVariant::Filled,
            btn_build.contains(mx, my),
            !self.is_building,
            &pal,
            &self.ripples,
        );
    }

    fn try_click_prereq_dropdown_menu(
        &mut self,
        open_dd: PrereqDropdown,
        x: f32,
        y: f32,
    ) -> bool {
        let card = self.content_card_rect();
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;
        let right_x = x0 + 230.0;
        let right_w = w_full - 230.0;
        let ex = right_x + 16.0;
        let ew = right_w - 32.0;
        let w = self.width_dip;
        let h = self.height_dip;

        let Some(rule_ref) = self.config.prerequisites.get(self.selected_prereq_idx) else {
            return false;
        };

        let (trig, items_vec): (RectF, Vec<&str>) = match open_dd {
            PrereqDropdown::ConditionKind => (
                RectF::new(ex + ew - 168.0, card.y + 152.0, 168.0, 34.0),
                vec!["注册表键值检测", "文件 / PE版本"],
            ),
            PrereqDropdown::RegHive => (
                RectF::new(ex, card.y + 210.0, 86.0, 34.0),
                vec![
                    "HKLM (本机系统)",
                    "HKCU (当前用户)",
                    "HKCR (类与关联)",
                    "HKU (所有用户)",
                ],
            ),
            PrereqDropdown::RegArch => (
                RectF::new(ex + 94.0, card.y + 210.0, 102.0, 34.0),
                vec!["默认视图", "64位视图 (x64)", "32位视图 (x86)"],
            ),
            PrereqDropdown::Operator => match &rule_ref.condition {
                PrereqCondition::Registry { .. } => (
                    RectF::new(ex + 138.0, card.y + 268.0, 122.0, 34.0),
                    vec![
                        "存在",
                        "版本/数值 >=",
                        "等于 (==)",
                        "包含文本",
                        "不等于 (!=)",
                        "不存在",
                    ],
                ),
                PrereqCondition::File { .. } => (
                    RectF::new(ex, card.y + 268.0, 146.0, 34.0),
                    vec!["文件存在", "PE版本 >=", "PE版本 ==", "文件不存在"],
                ),
            },
            PrereqDropdown::ActionKind => (
                RectF::new(ex, card.y + 328.0, ew, 34.0),
                vec![
                    "1. 随包打包并运行安装器 (解压至 %TEMP% 执行并自动清理)",
                    "2. 执行系统命令或安装目录脚本 (支持 cmd / powershell / %INSTALL_DIR%)",
                    "3. 弹窗提示用户并中止安装 (可选引导打开下载链接)",
                ],
            ),
            PrereqDropdown::ElevateMode => (
                RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0),
                vec!["UAC 提权运行 (管理员)", "普通权限运行 (当前用户)"],
            ),
        };

        // Clicking trigger again toggles closed
        if trig.contains(x, y) {
            return true;
        }

        let menu_rect = self.renderer.dropdown_menu_rect(trig, &items_vec, w, h);
        if !menu_rect.contains(x, y) {
            return false;
        }

        let item_h = 32.0;
        let rel_y = (y - (menu_rect.y + 4.0)).max(0.0);
        let picked_idx = (rel_y / item_h).floor() as usize;
        if picked_idx >= items_vec.len() {
            return true;
        }

        if let Some(rule) = self.config.prerequisites.get_mut(self.selected_prereq_idx) {
            match open_dd {
                PrereqDropdown::ConditionKind => {
                    if picked_idx == 0 && matches!(rule.condition, PrereqCondition::File { .. }) {
                        rule.condition = PrereqCondition::Registry {
                            root: RegHiveRoot::Hklm,
                            subkey: "SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64"
                                .to_string(),
                            value_name: "Installed".to_string(),
                            arch_view: RegArchView::X64,
                            operator: CheckOperator::GreaterOrEqual,
                            expected: "1".to_string(),
                        };
                    } else if picked_idx == 1
                        && matches!(rule.condition, PrereqCondition::Registry { .. })
                    {
                        rule.condition = PrereqCondition::File {
                            path: "%SystemRoot%\\System32\\vcruntime140.dll".to_string(),
                            operator: CheckOperator::GreaterOrEqual,
                            expected_version: "14.38.0.0".to_string(),
                        };
                    }
                }
                PrereqDropdown::RegHive => {
                    if let PrereqCondition::Registry { root, .. } = &mut rule.condition {
                        *root = match picked_idx {
                            0 => RegHiveRoot::Hklm,
                            1 => RegHiveRoot::Hkcu,
                            2 => RegHiveRoot::Hkcr,
                            _ => RegHiveRoot::Hku,
                        };
                    }
                }
                PrereqDropdown::RegArch => {
                    if let PrereqCondition::Registry { arch_view, .. } = &mut rule.condition {
                        *arch_view = match picked_idx {
                            0 => RegArchView::Default,
                            1 => RegArchView::X64,
                            _ => RegArchView::X86,
                        };
                    }
                }
                PrereqDropdown::Operator => match &mut rule.condition {
                    PrereqCondition::Registry { operator, .. } => {
                        *operator = match picked_idx {
                            0 => CheckOperator::Exists,
                            1 => CheckOperator::GreaterOrEqual,
                            2 => CheckOperator::Equals,
                            3 => CheckOperator::Contains,
                            4 => CheckOperator::NotEquals,
                            _ => CheckOperator::NotExists,
                        };
                    }
                    PrereqCondition::File { operator, .. } => {
                        *operator = match picked_idx {
                            0 => CheckOperator::Exists,
                            1 => CheckOperator::GreaterOrEqual,
                            2 => CheckOperator::Equals,
                            _ => CheckOperator::NotExists,
                        };
                    }
                },
                PrereqDropdown::ActionKind => {
                    let cur_kind = match rule.action {
                        PrereqAction::RunBundledExe { .. } => 0,
                        PrereqAction::RunCommand { .. } => 1,
                        PrereqAction::PromptAndAbort { .. } => 2,
                    };
                    if picked_idx != cur_kind {
                        rule.action = match picked_idx {
                            0 => PrereqAction::RunBundledExe {
                                source_file: "vc_redist.x64.exe".to_string(),
                                bundled_rel_path: String::new(),
                                args: "/install /quiet /norestart".to_string(),
                                elevate: true,
                                wait: true,
                                success_exit_codes: vec![0, 1638, 3010],
                            },
                            1 => PrereqAction::RunCommand {
                                program: "cmd.exe".to_string(),
                                args: "/C echo Configuring prerequisite...".to_string(),
                                elevate: false,
                                wait: true,
                                success_exit_codes: vec![0],
                            },
                            _ => PrereqAction::PromptAndAbort {
                                message: "检测到系统缺少必要的运行库组件，请先安装该运行库后再运行本安装程序。"
                                    .to_string(),
                                download_url:
                                    "https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist"
                                        .to_string(),
                            },
                        };
                    }
                }
                PrereqDropdown::ElevateMode => match &mut rule.action {
                    PrereqAction::RunBundledExe { elevate, .. }
                    | PrereqAction::RunCommand { elevate, .. } => {
                        *elevate = picked_idx == 0;
                    }
                    PrereqAction::PromptAndAbort { .. } => {}
                },
            }
        }
        true
    }

    pub fn on_lbutton_down(&mut self, x: f32, y: f32) {
        let w = self.width_dip;
        let card = self.content_card_rect();
        self.text_input.is_dragging = false;

        // 0. If a dropdown menu is currently open in Prerequisites tab, handle its clicks first
        if self.active_tab == StudioTab::Prerequisites {
            if let Some(open_dd) = self.open_dropdown {
                if self.try_click_prereq_dropdown_menu(open_dd, x, y) {
                    self.open_dropdown = None;
                    self.invalidate();
                    return;
                }
                self.open_dropdown = None;
            }
        } else {
            self.open_dropdown = None;
        }

        let prev_focus = self.focused_field;
        self.focused_field = FocusedField::None;

        // 1. Header buttons
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
            unsafe {
                PostQuitMessage(0);
            }
            return;
        }

        // Drag window by titlebar
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

        // 2. Left Navigation Rail Tabs
        let tabs = [
            StudioTab::Metadata,
            StudioTab::FilesAndPaths,
            StudioTab::OptionsAndLicense,
            StudioTab::Prerequisites,
            StudioTab::BuildAndTest,
        ];
        for (i, tab) in tabs.iter().enumerate() {
            let r = Self::nav_item_rect(i);
            if r.contains(x, y) {
                self.active_tab = *tab;
                if *tab == StudioTab::FilesAndPaths || *tab == StudioTab::BuildAndTest {
                    self.rescan_source_dir();
                }
                self.add_ripple(x, y, r, rgba(0xFFFFFF, 0.25));
                self.invalidate();
                return;
            }
        }

        // Generate Demo Project button
        let demo_btn = self.btn_generate_demo_rect();
        if demo_btn.contains(x, y) && !self.is_building {
            self.add_ripple(x, y, demo_btn, rgba(0x6366F1, 0.25));
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if let Ok((demo_dir, main_exe)) = create_demo_app_directory(&cwd) {
                self.source_dir = demo_dir.to_string_lossy().to_string();
                self.config.main_exe = main_exe;
                self.rescan_source_dir();
                self.build_logs.push(format!(
                    "[演示项目] 已重置演示源目录: {}",
                    demo_dir.display()
                ));
            }
            self.invalidate();
            return;
        }

        // 3. Content area interactions
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;
        let w_half = (w_full - 20.0) * 0.5;
        let btn_next = Self::card_footer_next_btn(card);

        match self.active_tab {
            StudioTab::Metadata => {
                let r_name = RectF::new(x0, card.y + 112.0, w_half, 38.0);
                let r_ver = RectF::new(x0 + w_half + 20.0, card.y + 112.0, w_half, 38.0);
                let r_pub = RectF::new(x0, card.y + 182.0, w_half, 38.0);
                let r_id = RectF::new(x0 + w_half + 20.0, card.y + 182.0, w_half, 38.0);
                let r_exe = RectF::new(x0, card.y + 252.0, w_half - 100.0, 38.0);
                let btn_pick_exe = RectF::new(x0 + w_half - 92.0, card.y + 252.0, 92.0, 38.0);
                let r_ico = RectF::new(x0 + w_half + 20.0, card.y + 252.0, w_half - 100.0, 38.0);
                let btn_pick_ico = RectF::new(x0 + w_full - 92.0, card.y + 252.0, 92.0, 38.0);

                if r_name.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::AppName, r_name, x);
                } else if r_ver.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::AppVersion, r_ver, x);
                } else if r_pub.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::Publisher, r_pub, x);
                } else if r_id.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::AppId, r_id, x);
                } else if r_exe.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::MainExe, r_exe, x);
                } else if r_ico.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::InstallerIcon, r_ico, x);
                } else if btn_pick_exe.contains(x, y) {
                    if let Some(picked) = pick_open_file_dialog(
                        self.hwnd,
                        "选择主程序可执行文件 (.exe)",
                        "可执行文件 (*.exe)",
                        "*.exe",
                    ) {
                        let src_path = PathBuf::from(&self.source_dir);
                        if let Ok(rel) = picked.strip_prefix(&src_path) {
                            self.config.main_exe = rel.to_string_lossy().to_string();
                        } else if let Some(fname) = picked.file_name() {
                            self.config.main_exe = fname.to_string_lossy().to_string();
                        }
                    }
                } else if btn_pick_ico.contains(x, y) {
                    if let Some(picked) = pick_open_file_dialog(
                        self.hwnd,
                        "选择安装包图标文件 (.ico)",
                        "Windows 图标文件 (*.ico)",
                        "*.ico",
                    ) {
                        self.installer_icon_input = picked.to_string_lossy().to_string();
                        self.config.installer_icon = Some(self.installer_icon_input.clone());
                    }
                } else if btn_next.contains(x, y) {
                    self.active_tab = StudioTab::FilesAndPaths;
                    self.rescan_source_dir();
                }
            }
            StudioTab::FilesAndPaths => {
                let r_src = RectF::new(x0, card.y + 112.0, w_full - 138.0, 38.0);
                let btn_src = RectF::new(x0 + w_full - 126.0, card.y + 112.0, 126.0, 38.0);
                let r_inst = RectF::new(x0, card.y + 180.0, w_full - 246.0, 38.0);
                let btn_localappdata =
                    RectF::new(x0 + w_full - 234.0, card.y + 180.0, 112.0, 38.0);
                let btn_progfiles = RectF::new(x0 + w_full - 114.0, card.y + 180.0, 114.0, 38.0);
                let r_out = RectF::new(x0, card.y + 248.0, w_full - 138.0, 38.0);
                let btn_out = RectF::new(x0 + w_full - 126.0, card.y + 248.0, 126.0, 38.0);

                if r_src.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::SourceDir, r_src, x);
                } else if btn_src.contains(x, y) {
                    if let Some(folder) = pick_folder_dialog(
                        self.hwnd,
                        "选择待打包的源目录",
                        Some(Path::new(&self.source_dir)),
                    ) {
                        self.source_dir = folder.to_string_lossy().to_string();
                        self.rescan_source_dir();
                    }
                } else if r_inst.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::DefaultInstallDir, r_inst, x);
                } else if btn_localappdata.contains(x, y) {
                    self.config.default_install_dir =
                        format!("%LOCALAPPDATA%\\Programs\\{}", self.config.app_name);
                } else if btn_progfiles.contains(x, y) {
                    self.config.default_install_dir =
                        format!("%PROGRAMFILES%\\{}", self.config.app_name);
                } else if r_out.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::OutputExe, r_out, x);
                } else if btn_out.contains(x, y) {
                    let def_name = format!("{}_Setup.exe", self.config.app_name.replace(' ', ""));
                    if let Some(save_path) =
                        pick_save_exe_dialog(self.hwnd, "选择输出安装包保存位置", &def_name)
                    {
                        self.output_exe = save_path.to_string_lossy().to_string();
                    }
                } else if btn_next.contains(x, y) {
                    self.active_tab = StudioTab::OptionsAndLicense;
                }
            }
            StudioTab::OptionsAndLicense => {
                let left_w = 304.0;
                let right_x = x0 + left_w + 20.0;
                let right_w = w_full - left_w - 20.0;
                let c1 = RectF::new(x0, card.y + 108.0, left_w, 42.0);
                let c2 = RectF::new(x0, card.y + 152.0, left_w, 42.0);
                let c3 = RectF::new(x0, card.y + 196.0, left_w, 42.0);
                let c4 = RectF::new(x0, card.y + 240.0, left_w, 42.0);
                let pol_w = (left_w - 8.0) * 0.5;
                let btn_pol_auto = RectF::new(x0, card.y + 316.0, pol_w, 34.0);
                let btn_pol_prompt = RectF::new(x0 + pol_w + 8.0, card.y + 316.0, pol_w, 34.0);

                if c1.contains(x, y) {
                    self.config.create_desktop_shortcut = !self.config.create_desktop_shortcut;
                } else if c2.contains(x, y) {
                    self.config.create_start_menu_shortcut =
                        !self.config.create_start_menu_shortcut;
                } else if c3.contains(x, y) {
                    self.config.register_uninstaller = !self.config.register_uninstaller;
                } else if c4.contains(x, y) {
                    self.config.auto_launch_after_install = !self.config.auto_launch_after_install;
                } else if btn_pol_auto.contains(x, y) {
                    self.config.process_conflict_policy = ProcessConflictPolicy::AutoKill;
                    self.config.kill_process_on_update = true;
                    self.add_ripple(x, y, btn_pol_auto, rgba(0xFFFFFF, 0.25));
                } else if btn_pol_prompt.contains(x, y) {
                    self.config.process_conflict_policy = ProcessConflictPolicy::PromptUser;
                    self.config.kill_process_on_update = false;
                    self.add_ripple(x, y, btn_pol_prompt, rgba(0xFFFFFF, 0.25));
                }

                let levels = [1u32, 6u32, 9u32];
                let seg_w = (left_w - 16.0) / 3.0;
                for (i, lv) in levels.iter().enumerate() {
                    let sr =
                        RectF::new(x0 + (i as f32) * (seg_w + 8.0), card.y + 408.0, seg_w, 36.0);
                    if sr.contains(x, y) {
                        self.config.compression_level = *lv;
                    }
                }

                let btn_load_eula =
                    RectF::new(right_x, card.y + 456.0, (right_w - 10.0) * 0.5, 34.0);
                let btn_paste_eula = RectF::new(
                    right_x + (right_w - 10.0) * 0.5 + 10.0,
                    card.y + 456.0,
                    (right_w - 10.0) * 0.5,
                    34.0,
                );
                if btn_load_eula.contains(x, y) {
                    if let Some(txt_path) = pick_open_file_dialog(
                        self.hwnd,
                        "导入许可协议文本文件",
                        "文本文件 (*.txt;*.md)",
                        "*.txt;*.md",
                    ) {
                        if let Ok(content) = fs::read_to_string(txt_path) {
                            self.config.license_text = content;
                            self.license_scroll_line = 0;
                        }
                    }
                } else if btn_paste_eula.contains(x, y) {
                    if let Some(clip) = get_clipboard_text(self.hwnd) {
                        if !clip.trim().is_empty() {
                            self.config.license_text = clip;
                            self.license_scroll_line = 0;
                        }
                    }
                } else if btn_next.contains(x, y) {
                    self.active_tab = StudioTab::Prerequisites;
                }
            }
            StudioTab::Prerequisites => {
                // 1. Top Preset Bar (y = card.y + 84.0, h = 34.0)
                let preset_w = (w_full - 30.0) / 4.0;
                for i in 0..4usize {
                    let pr = RectF::new(
                        x0 + (i as f32) * (preset_w + 10.0),
                        card.y + 84.0,
                        preset_w,
                        34.0,
                    );
                    if pr.contains(x, y) {
                        let new_rule = match i {
                            0 => PrerequisiteRule::preset_vc_redist_x64(),
                            1 => PrerequisiteRule::preset_vc_redist_x86(),
                            2 => PrerequisiteRule::preset_custom_registry(),
                            _ => PrerequisiteRule::preset_custom_file_version(),
                        };
                        self.config.prerequisites.push(new_rule);
                        self.selected_prereq_idx =
                            self.config.prerequisites.len().saturating_sub(1);
                        self.add_ripple(x, y, pr, rgba(0x6366F1, 0.25));
                        self.invalidate();
                        return;
                    }
                }

                // 2. Left Rule List & Delete button
                let list_w = 216.0;
                let list_box = RectF::new(x0, card.y + 128.0, list_w, 368.0);
                for idx in 0..self.config.prerequisites.len().min(6) {
                    let item_r = RectF::new(
                        list_box.x + 8.0,
                        list_box.y + 32.0 + (idx as f32) * 46.0,
                        list_w - 16.0,
                        42.0,
                    );
                    if item_r.contains(x, y) {
                        let chk_area = RectF::new(item_r.x, item_r.y, 32.0, item_r.h);
                        if chk_area.contains(x, y) {
                            if let Some(r) = self.config.prerequisites.get_mut(idx) {
                                r.enabled = !r.enabled;
                            }
                        }
                        self.selected_prereq_idx = idx;
                        self.invalidate();
                        return;
                    }
                }

                let btn_del_rule = RectF::new(
                    list_box.x + 10.0,
                    list_box.y + list_box.h - 42.0,
                    list_w - 20.0,
                    32.0,
                );
                if btn_del_rule.contains(x, y) && !self.config.prerequisites.is_empty() {
                    let rem_idx = self
                        .selected_prereq_idx
                        .min(self.config.prerequisites.len() - 1);
                    self.config.prerequisites.remove(rem_idx);
                    if self.selected_prereq_idx >= self.config.prerequisites.len()
                        && !self.config.prerequisites.is_empty()
                    {
                        self.selected_prereq_idx = self.config.prerequisites.len() - 1;
                    }
                    self.invalidate();
                    return;
                }

                if btn_next.contains(x, y) {
                    self.active_tab = StudioTab::BuildAndTest;
                    self.rescan_source_dir();
                    self.invalidate();
                    return;
                }

                // 3. Right Rule Editor Controls
                let right_x = x0 + 230.0;
                let right_w = w_full - 230.0;
                let ex = right_x + 16.0;
                let ew = right_w - 32.0;

                let has_rule = self.config.prerequisites.get(self.selected_prereq_idx).is_some();
                if has_rule {
                    let r_rname = RectF::new(ex, card.y + 152.0, ew - 178.0, 34.0);
                    let dd_cond_kind = RectF::new(ex + ew - 168.0, card.y + 152.0, 168.0, 34.0);
                    let dd_act_kind = RectF::new(ex, card.y + 328.0, ew, 34.0);

                    if r_rname.contains(x, y) {
                        self.focused_field = prev_focus;
                        self.click_text_field(FocusedField::PrereqName, r_rname, x);
                    } else if dd_cond_kind.contains(x, y) {
                        self.open_dropdown = Some(PrereqDropdown::ConditionKind);
                    } else if dd_act_kind.contains(x, y) {
                        self.open_dropdown = Some(PrereqDropdown::ActionKind);
                    } else {
                        let is_reg = matches!(
                            self.config.prerequisites[self.selected_prereq_idx].condition,
                            PrereqCondition::Registry { .. }
                        );
                        if is_reg {
                            let dd_hive = RectF::new(ex, card.y + 210.0, 86.0, 34.0);
                            let dd_arch = RectF::new(ex + 94.0, card.y + 210.0, 102.0, 34.0);
                            let r_kpath = RectF::new(ex + 204.0, card.y + 210.0, ew - 204.0, 34.0);
                            let r_vname = RectF::new(ex, card.y + 268.0, 130.0, 34.0);
                            let dd_op = RectF::new(ex + 138.0, card.y + 268.0, 122.0, 34.0);
                            let r_exp = RectF::new(ex + 268.0, card.y + 268.0, ew - 268.0, 34.0);

                            if dd_hive.contains(x, y) {
                                self.open_dropdown = Some(PrereqDropdown::RegHive);
                            } else if dd_arch.contains(x, y) {
                                self.open_dropdown = Some(PrereqDropdown::RegArch);
                            } else if r_kpath.contains(x, y) {
                                self.focused_field = prev_focus;
                                self.click_text_field(FocusedField::PrereqTarget, r_kpath, x);
                            } else if r_vname.contains(x, y) {
                                self.focused_field = prev_focus;
                                self.click_text_field(FocusedField::PrereqValueName, r_vname, x);
                            } else if dd_op.contains(x, y) {
                                self.open_dropdown = Some(PrereqDropdown::Operator);
                            } else if r_exp.contains(x, y) {
                                self.focused_field = prev_focus;
                                self.click_text_field(FocusedField::PrereqExpected, r_exp, x);
                            }
                        } else {
                            let r_fpath = RectF::new(ex, card.y + 210.0, ew, 34.0);
                            let dd_op = RectF::new(ex, card.y + 268.0, 146.0, 34.0);
                            let r_exp = RectF::new(ex + 156.0, card.y + 268.0, ew - 156.0, 34.0);

                            if r_fpath.contains(x, y) {
                                self.focused_field = prev_focus;
                                self.click_text_field(FocusedField::PrereqTarget, r_fpath, x);
                            } else if dd_op.contains(x, y) {
                                self.open_dropdown = Some(PrereqDropdown::Operator);
                            } else if r_exp.contains(x, y) {
                                self.focused_field = prev_focus;
                                self.click_text_field(FocusedField::PrereqExpected, r_exp, x);
                            }
                        }

                        // Row 5 & Row 6: Action Fields
                        let act_kind = match self.config.prerequisites[self.selected_prereq_idx].action {
                            PrereqAction::RunBundledExe { .. } => 0,
                            PrereqAction::RunCommand { .. } => 1,
                            PrereqAction::PromptAndAbort { .. } => 2,
                        };
                        match act_kind {
                            0 => {
                                let r_act1 = RectF::new(ex, card.y + 388.0, ew - 96.0, 34.0);
                                let btn_pick_redist =
                                    RectF::new(ex + ew - 88.0, card.y + 388.0, 88.0, 34.0);
                                let r_act2 = RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0);
                                let dd_elev =
                                    RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0);

                                if r_act1.contains(x, y) {
                                    self.focused_field = prev_focus;
                                    self.click_text_field(
                                        FocusedField::PrereqActionPrimary,
                                        r_act1,
                                        x,
                                    );
                                } else if btn_pick_redist.contains(x, y) {
                                    if let Some(picked) = pick_open_file_dialog(
                                        self.hwnd,
                                        "选择随包打包的前置组件安装程序 (.exe)",
                                        "可执行安装程序 (*.exe)",
                                        "*.exe",
                                    ) {
                                        if let Some(rule) = self
                                            .config
                                            .prerequisites
                                            .get_mut(self.selected_prereq_idx)
                                        {
                                            if let PrereqAction::RunBundledExe {
                                                source_file, ..
                                            } = &mut rule.action
                                            {
                                                *source_file =
                                                    picked.to_string_lossy().to_string();
                                            }
                                        }
                                    }
                                } else if r_act2.contains(x, y) {
                                    self.focused_field = prev_focus;
                                    self.click_text_field(
                                        FocusedField::PrereqActionSecondary,
                                        r_act2,
                                        x,
                                    );
                                } else if dd_elev.contains(x, y) {
                                    self.open_dropdown = Some(PrereqDropdown::ElevateMode);
                                }
                            }
                            1 => {
                                let r_act1 = RectF::new(ex, card.y + 388.0, ew, 34.0);
                                let r_act2 = RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0);
                                let dd_elev =
                                    RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0);

                                if r_act1.contains(x, y) {
                                    self.focused_field = prev_focus;
                                    self.click_text_field(
                                        FocusedField::PrereqActionPrimary,
                                        r_act1,
                                        x,
                                    );
                                } else if r_act2.contains(x, y) {
                                    self.focused_field = prev_focus;
                                    self.click_text_field(
                                        FocusedField::PrereqActionSecondary,
                                        r_act2,
                                        x,
                                    );
                                } else if dd_elev.contains(x, y) {
                                    self.open_dropdown = Some(PrereqDropdown::ElevateMode);
                                }
                            }
                            _ => {
                                let r_act1 = RectF::new(ex, card.y + 388.0, ew, 34.0);
                                let r_act2 = RectF::new(ex, card.y + 448.0, ew, 34.0);
                                if r_act1.contains(x, y) {
                                    self.focused_field = prev_focus;
                                    self.click_text_field(
                                        FocusedField::PrereqActionPrimary,
                                        r_act1,
                                        x,
                                    );
                                } else if r_act2.contains(x, y) {
                                    self.focused_field = prev_focus;
                                    self.click_text_field(
                                        FocusedField::PrereqActionSecondary,
                                        r_act2,
                                        x,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            StudioTab::BuildAndTest => {
                let btn_open_dir = RectF::new(x0, card.y + card.h - 50.0, 146.0, 38.0);
                let btn_run = RectF::new(x0 + 158.0, card.y + card.h - 50.0, 186.0, 38.0);
                let btn_build =
                    RectF::new(card.x + card.w - 236.0, card.y + card.h - 50.0, 208.0, 38.0);

                if btn_build.contains(x, y) && !self.is_building {
                    self.add_ripple(x, y, btn_build, rgba(0xFFFFFF, 0.35));
                    self.start_build();
                } else if btn_run.contains(x, y) && !self.is_building {
                    if let Some(exe) = self.last_built_exe.clone() {
                        self.add_ripple(x, y, btn_run, rgba(0x6366F1, 0.25));
                        shell_open(&exe, None, exe.parent());
                    }
                } else if btn_open_dir.contains(x, y) && !self.is_building {
                    if let Some(exe) = self.last_built_exe.clone() {
                        if let Some(parent) = exe.parent() {
                            shell_open(parent, None, None);
                        }
                    }
                }
            }
        }

        self.invalidate();
    }

    pub fn on_mouse_move(&mut self, mx: f32, my: f32, lbutton_down: bool) {
        self.mouse_x = mx;
        self.mouse_y = my;
        if !lbutton_down {
            self.text_input.is_dragging = false;
        } else if self.text_input.is_dragging && self.focused_field != FocusedField::None {
            if let Some(rect) = self.field_rect(self.focused_field) {
                let val = self
                    .field_value_ref(self.focused_field)
                    .unwrap_or("")
                    .to_string();
                let char_idx = self.renderer.hit_test_text_field_char(
                    rect,
                    &val,
                    mx,
                    self.text_input.caret_pos,
                );
                self.text_input.set_caret(&val, char_idx, true);
                self.caret_visible = true;
            }
        }
        self.invalidate();
    }

    pub fn on_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        let is_src = self.focused_field == FocusedField::SourceDir;
        let mut ti = self.text_input;
        if let Some(field) = self.active_field_mut() {
            ti.insert_char(field, ch);
            self.text_input = ti;
            self.caret_visible = true;
            if is_src {
                self.rescan_source_dir();
            }
            self.invalidate();
        }
    }

    pub fn on_keydown(&mut self, vk: u16) {
        let ctrl_down = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
        let shift_down = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };

        if vk == VK_ESCAPE.0 {
            if self.open_dropdown.is_some() {
                self.open_dropdown = None;
            } else {
                self.focused_field = FocusedField::None;
                self.text_input.is_dragging = false;
            }
            self.invalidate();
            return;
        }

        if self.focused_field == FocusedField::None {
            return;
        }

        if ctrl_down {
            match vk {
                x if x == b'A' as u16 => {
                    if let Some(val) = self.field_value_ref(self.focused_field).map(str::to_string)
                    {
                        self.text_input.select_all(&val);
                        self.caret_visible = true;
                        self.invalidate();
                    }
                    return;
                }
                x if x == b'C' as u16 => {
                    if let Some(val) = self.field_value_ref(self.focused_field) {
                        if let Some(sel) = self.text_input.selected_text(val) {
                            set_clipboard_text(self.hwnd, sel);
                        }
                    }
                    return;
                }
                x if x == b'X' as u16 => {
                    if self.text_input.has_selection() {
                        if let Some(val) = self.field_value_ref(self.focused_field) {
                            if let Some(sel) = self.text_input.selected_text(val) {
                                set_clipboard_text(self.hwnd, sel);
                            }
                        }
                        let is_src = self.focused_field == FocusedField::SourceDir;
                        let mut ti = self.text_input;
                        if let Some(field) = self.active_field_mut() {
                            ti.delete_selection(field);
                            self.text_input = ti;
                            self.caret_visible = true;
                            if is_src {
                                self.rescan_source_dir();
                            }
                            self.invalidate();
                        }
                    }
                    return;
                }
                x if x == b'V' as u16 => {
                    if let Some(clip) = get_clipboard_text(self.hwnd) {
                        let clean: String = clip.lines().next().unwrap_or("").trim().to_string();
                        let is_src = self.focused_field == FocusedField::SourceDir;
                        let mut ti = self.text_input;
                        if let Some(field) = self.active_field_mut() {
                            ti.insert_str(field, &clean);
                            self.text_input = ti;
                            self.caret_visible = true;
                            if is_src {
                                self.rescan_source_dir();
                            }
                            self.invalidate();
                        }
                    }
                    return;
                }
                _ => {}
            }
        }

        match vk {
            x if x == VK_LEFT.0 => {
                if let Some(val) = self.field_value_ref(self.focused_field).map(str::to_string) {
                    self.text_input.move_left(&val, shift_down);
                    self.caret_visible = true;
                    self.invalidate();
                }
            }
            x if x == VK_RIGHT.0 => {
                if let Some(val) = self.field_value_ref(self.focused_field).map(str::to_string) {
                    self.text_input.move_right(&val, shift_down);
                    self.caret_visible = true;
                    self.invalidate();
                }
            }
            x if x == VK_HOME.0 => {
                if let Some(val) = self.field_value_ref(self.focused_field).map(str::to_string) {
                    self.text_input.move_home(&val, shift_down);
                    self.caret_visible = true;
                    self.invalidate();
                }
            }
            x if x == VK_END.0 => {
                if let Some(val) = self.field_value_ref(self.focused_field).map(str::to_string) {
                    self.text_input.move_end(&val, shift_down);
                    self.caret_visible = true;
                    self.invalidate();
                }
            }
            x if x == VK_BACK.0 => {
                let is_src = self.focused_field == FocusedField::SourceDir;
                let mut ti = self.text_input;
                if let Some(field) = self.active_field_mut() {
                    ti.backspace(field);
                    self.text_input = ti;
                    self.caret_visible = true;
                    if is_src {
                        self.rescan_source_dir();
                    }
                    self.invalidate();
                }
            }
            x if x == VK_DELETE.0 => {
                let is_src = self.focused_field == FocusedField::SourceDir;
                let mut ti = self.text_input;
                if let Some(field) = self.active_field_mut() {
                    ti.delete_forward(field);
                    self.text_input = ti;
                    self.caret_visible = true;
                    if is_src {
                        self.rescan_source_dir();
                    }
                    self.invalidate();
                }
            }
            x if x == VK_RETURN.0 => {
                self.focused_field = FocusedField::None;
                self.text_input.is_dragging = false;
                self.invalidate();
            }
            x if x == VK_TAB.0 => {
                self.focused_field = match self.focused_field {
                    FocusedField::AppName => FocusedField::AppVersion,
                    FocusedField::AppVersion => FocusedField::Publisher,
                    FocusedField::Publisher => FocusedField::AppId,
                    FocusedField::AppId => FocusedField::MainExe,
                    FocusedField::MainExe => FocusedField::InstallerIcon,
                    FocusedField::InstallerIcon => FocusedField::AppName,
                    FocusedField::SourceDir => FocusedField::DefaultInstallDir,
                    FocusedField::DefaultInstallDir => FocusedField::OutputExe,
                    FocusedField::OutputExe => FocusedField::SourceDir,
                    FocusedField::PrereqName => FocusedField::PrereqTarget,
                    FocusedField::PrereqTarget => {
                        if self.field_rect(FocusedField::PrereqValueName).is_some() {
                            FocusedField::PrereqValueName
                        } else {
                            FocusedField::PrereqExpected
                        }
                    }
                    FocusedField::PrereqValueName => FocusedField::PrereqExpected,
                    FocusedField::PrereqExpected => FocusedField::PrereqActionPrimary,
                    FocusedField::PrereqActionPrimary => FocusedField::PrereqActionSecondary,
                    FocusedField::PrereqActionSecondary => FocusedField::PrereqName,
                    FocusedField::None => FocusedField::None,
                };
                if let Some(val) = self.field_value_ref(self.focused_field).map(str::to_string) {
                    self.text_input.select_all(&val);
                }
                self.caret_visible = true;
                self.invalidate();
            }
            _ => {}
        }
    }

    pub fn on_mouse_wheel(&mut self, delta: i16) {
        if self.active_tab == StudioTab::FilesAndPaths {
            if delta < 0 {
                self.file_scroll_offset = (self.file_scroll_offset + 2)
                    .min(self.scanned_files.len().saturating_sub(1));
            } else {
                self.file_scroll_offset = self.file_scroll_offset.saturating_sub(2);
            }
            self.invalidate();
        } else if self.active_tab == StudioTab::OptionsAndLicense {
            let total_lines = self.config.license_text.lines().count();
            if delta < 0 {
                self.license_scroll_line =
                    (self.license_scroll_line + 3).min(total_lines.saturating_sub(5));
            } else {
                self.license_scroll_line = self.license_scroll_line.saturating_sub(3);
            }
            self.invalidate();
        }
    }
}

pub fn run_studio_gui() -> Result<(), String> {
    let renderer = D2DRenderer::new()?;
    let state = Box::new(StudioWindowState::new(renderer));

    unsafe {
        let h_instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class_name = to_wide("NyanSetupStudioWndClass");
        let app_icon = LoadIconW(h_instance, PCWSTR(1 as *const u16)).unwrap_or_default();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(studio_wnd_proc),
            hInstance: h_instance.into(),
            hIcon: app_icon,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let sys_dpi = GetDpiForSystem().max(96);
        let scale = (sys_dpi as f32) / 96.0;
        let phys_w = (WIN_WIDTH_DIP * scale).round() as i32;
        let phys_h = (WIN_HEIGHT_DIP * scale).round() as i32;

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let pos_x = ((screen_w - phys_w) / 2).max(10);
        let pos_y = ((screen_h - phys_h) / 2).max(10);

        let title = to_wide("Nyan Setup Studio - Material 安装包制作工具");
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
        .map_err(|e| format!("创建窗口失败: {}", e))?;

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

        // Enable Win11 native rounded window corners & DWM shadow on WS_POPUP
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

    Ok(())
}

unsafe extern "system" fn studio_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let cs = &*(lparam.0 as *const CREATESTRUCTW);
        let state_ptr = cs.lpCreateParams as *mut StudioWindowState;
        if !state_ptr.is_null() {
            (*state_ptr).hwnd = hwnd;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        }
        return LRESULT(0);
    }

    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut StudioWindowState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    let map_mouse = |state: &StudioWindowState, lparam: LPARAM| -> (f32, f32) {
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
                state.width_dip = WIN_WIDTH_DIP;
                state.height_dip = WIN_HEIGHT_DIP;
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
            let lbutton_down = (wparam.0 & 0x0001) != 0;
            state.on_mouse_move(mx, my, lbutton_down);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (mx, my) = map_mouse(state, lparam);
            state.on_lbutton_down(mx, my);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            state.text_input.is_dragging = false;
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
                if state.focused_field != FocusedField::None {
                    state.caret_visible = !state.caret_visible;
                    state.invalidate();
                }
            } else if wparam.0 == TIMER_ANIM {
                state.ripples.retain_mut(|r| r.step());
                if state.is_building {
                    state.poll_build_events();
                    let diff = state.build_target_progress - state.build_progress;
                    if diff.abs() > 0.002 {
                        state.build_progress += diff * 0.22;
                    }
                }
                if state.ripples.is_empty() && !state.is_building {
                    let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(hwnd, TIMER_ANIM);
                }
                state.invalidate();
            }
            LRESULT(0)
        }
        WM_APP_BUILD_EVENT => {
            state.poll_build_events();
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
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(state_ptr));
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
