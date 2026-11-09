use super::{FocusedField, StudioTab, StudioWindowState};
use crate::d2d::{rgba, D2DRenderer, MaterialPalette, RectF, ThemeMode};
use crate::package::ProcessConflictPolicy;
use crate::win32_ops::{
    create_demo_app_directory, get_clipboard_text, pick_folder_dialog, pick_open_file_dialog,
    pick_save_exe_dialog, set_clipboard_text, shell_open,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, VK_BACK, VK_CONTROL, VK_DELETE, VK_END, VK_ESCAPE, VK_HOME,
    VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    PostQuitMessage, SendMessageW, ShowWindow, HTCAPTION, SW_MINIMIZE, WM_NCLBUTTONDOWN,
};

impl StudioWindowState {
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
                self.handle_prereq_tab_click(x, y, card, x0, w_full, btn_next, prev_focus);
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
