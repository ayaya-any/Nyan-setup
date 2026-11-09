pub mod events;
pub mod prereq_tab;
pub mod tabs;
pub mod window;

pub use window::run_studio_gui;

use crate::d2d::{D2DRenderer, MaterialPalette, RectF, RippleEffect, TextInputState};
use crate::package::{
    build_installer_package, collect_source_files, format_bytes, BuildProgressEvent, PackageConfig,
    PrereqAction, PrereqCondition,
};
use crate::win32_ops::create_demo_app_directory;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SetTimer, WM_USER};

pub(crate) const WM_APP_BUILD_EVENT: u32 = WM_USER + 101;
pub(crate) const TIMER_ANIM: usize = 1;
pub(crate) const TIMER_CARET: usize = 2;

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
    pub(crate) fn nav_rail_rect(&self) -> RectF {
        RectF::new(14.0, 50.0, 202.0, self.height_dip - 64.0)
    }

    pub(crate) fn nav_item_rect(idx: usize) -> RectF {
        RectF::new(22.0, 88.0 + (idx as f32) * 44.0, 186.0, 38.0)
    }

    pub(crate) fn btn_generate_demo_rect(&self) -> RectF {
        RectF::new(22.0, self.height_dip - 68.0, 186.0, 40.0)
    }

    pub(crate) fn content_card_rect(&self) -> RectF {
        RectF::new(228.0, 50.0, self.width_dip - 242.0, self.height_dip - 64.0)
    }

    pub(crate) fn card_footer_next_btn(card: RectF) -> RectF {
        RectF::new(card.x + card.w - 196.0, card.y + card.h - 50.0, 168.0, 38.0)
    }
}
