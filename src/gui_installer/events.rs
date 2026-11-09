use super::{InstallerWindowState, WizardMode, WizardStep};
use crate::d2d::{rgba, D2DRenderer, MaterialPalette, RectF, ThemeMode};
use crate::win32_ops::{
    execute_uninstall_steps, get_clipboard_text, pick_folder_dialog, schedule_self_delete,
    set_clipboard_text, shell_open,
};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, VK_BACK, VK_CONTROL, VK_DELETE, VK_END, VK_ESCAPE, VK_HOME,
    VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    PostQuitMessage, SendMessageW, ShowWindow, HTCAPTION, SW_MINIMIZE, WM_NCLBUTTONDOWN,
};

impl InstallerWindowState {
    pub fn on_lbutton_down(&mut self, x: f32, y: f32) {
        let w = self.width_dip;
        let h = self.height_dip;
        let prev_focused = self.path_focused;
        self.path_focused = false;
        self.text_input.is_dragging = false;

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
            if self.try_begin_log_scrollbar_drag(x, y) {
                return;
            }
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
                        self.log_scroll_x = 0.0;
                        self.log_scroll_y = 0.0;
                        self.log_auto_scroll = true;
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

        // Modal process conflict popup intercepts all content clicks when active
        if self.show_process_conflict_modal {
            let modal = self.process_conflict_modal_rect();
            let btn_close_x = RectF::new(modal.x + modal.w - 42.0, modal.y + 14.0, 28.0, 28.0);
            let btn_y = modal.y + modal.h - 54.0;
            let btn_cancel = RectF::new(modal.x + 22.0, btn_y, 96.0, 38.0);
            let btn_retry = RectF::new(modal.x + modal.w - 302.0, btn_y, 112.0, 38.0);
            let btn_kill_retry = RectF::new(modal.x + modal.w - 180.0, btn_y, 158.0, 38.0);

            if btn_close_x.contains(x, y) || btn_cancel.contains(x, y) {
                self.show_process_conflict_modal = false;
                self.invalidate();
            } else if btn_retry.contains(x, y) {
                self.add_ripple(x, y, btn_retry, rgba(0x6366F1, 0.22));
                self.refresh_disk_space();
                if self.running_processes.is_empty() {
                    self.show_process_conflict_modal = false;
                    self.opt_kill_running_process = false;
                    self.start_installation();
                } else {
                    self.invalidate();
                }
            } else if btn_kill_retry.contains(x, y) {
                self.add_ripple(x, y, btn_kill_retry, rgba(0xFFFFFF, 0.25));
                self.terminate_occupying_processes();
                self.show_process_conflict_modal = false;
                self.opt_kill_running_process = true;
                self.start_installation();
            }
            return;
        }

        if self.try_begin_log_scrollbar_drag(x, y) {
            return;
        }

        // Installer mode clicks
        let card = self.installer_card_rect();
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;
        let btn_y = card.y + card.h - 50.0;

        match self.step {
            WizardStep::WelcomeAndLicense => {
                let chk_rect = RectF::new(x0, btn_y, 280.0, 38.0);
                let btn_custom = RectF::new(card.x + card.w - 332.0, btn_y, 136.0, 38.0);
                let btn_quick = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);

                if chk_rect.contains(x, y) {
                    self.license_accepted = !self.license_accepted;
                } else if btn_custom.contains(x, y) && self.license_accepted {
                    self.refresh_disk_space();
                    self.step = WizardStep::CustomOptions;
                } else if btn_quick.contains(x, y)
                    && self.license_accepted
                    && self.has_enough_disk_space()
                {
                    self.try_begin_installation();
                }
            }
            WizardStep::CustomOptions => {
                let r_path = RectF::new(x0, card.y + 66.0, w_full - 134.0, 38.0);
                let btn_browse = RectF::new(x0 + w_full - 122.0, card.y + 66.0, 122.0, 38.0);
                let c1 = RectF::new(x0, card.y + 190.0, w_full, 40.0);
                let c2 = RectF::new(x0, card.y + 234.0, w_full, 40.0);
                let c3 = RectF::new(x0, card.y + 278.0, w_full, 40.0);
                let btn_back = RectF::new(x0, btn_y, 120.0, 38.0);
                let btn_install = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);

                if r_path.contains(x, y) {
                    let shift_down =
                        unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
                    let cur_caret = if prev_focused {
                        self.text_input.caret_pos
                    } else {
                        0
                    };
                    let char_idx = self.renderer.hit_test_text_field_char(
                        r_path,
                        &self.install_dir_input,
                        x,
                        cur_caret,
                    );
                    self.path_focused = true;
                    if prev_focused && shift_down {
                        self.text_input
                            .set_caret(&self.install_dir_input, char_idx, true);
                    } else {
                        self.text_input
                            .set_caret(&self.install_dir_input, char_idx, false);
                    }
                    self.text_input.is_dragging = true;
                    self.caret_visible = true;
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
                } else if btn_back.contains(x, y) {
                    self.step = WizardStep::WelcomeAndLicense;
                } else if btn_install.contains(x, y)
                    && self.has_enough_disk_space()
                    && !self.install_dir_input.trim().is_empty()
                {
                    self.try_begin_installation();
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

    pub fn on_mouse_move(&mut self, mx: f32, my: f32, lbutton_down: bool) {
        self.mouse_x = mx;
        self.mouse_y = my;
        if !lbutton_down {
            self.text_input.is_dragging = false;
            self.dragging_log_hscroll = None;
            self.dragging_log_vscroll = None;
        } else if let Some(grab_dx) = self.dragging_log_hscroll {
            if let Some(log_box) = self.current_log_box_rect() {
                let m = self.compute_log_scroll_metrics(log_box);
                if let (Some(track), Some(thumb)) = (m.h_track, m.h_thumb) {
                    let max_travel = (track.w - thumb.w).max(1.0);
                    let new_thumb_x = (mx - grab_dx).clamp(track.x, track.x + max_travel);
                    self.log_scroll_x = ((new_thumb_x - track.x) / max_travel) * m.max_scroll_x;
                }
            }
        } else if let Some(grab_dy) = self.dragging_log_vscroll {
            if let Some(log_box) = self.current_log_box_rect() {
                let m = self.compute_log_scroll_metrics(log_box);
                if let (Some(track), Some(thumb)) = (m.v_track, m.v_thumb) {
                    let max_travel = (track.h - thumb.h).max(1.0);
                    let new_thumb_y = (my - grab_dy).clamp(track.y, track.y + max_travel);
                    self.log_scroll_y = ((new_thumb_y - track.y) / max_travel) * m.max_scroll_y;
                    self.log_auto_scroll = (self.log_scroll_y - m.max_scroll_y).abs() < 2.0;
                }
            }
        } else if self.text_input.is_dragging
            && self.path_focused
            && self.step == WizardStep::CustomOptions
        {
            let card = self.installer_card_rect();
            let x0 = card.x + 26.0;
            let w_full = card.w - 52.0;
            let r_path = RectF::new(x0, card.y + 66.0, w_full - 134.0, 38.0);
            let char_idx = self.renderer.hit_test_text_field_char(
                r_path,
                &self.install_dir_input,
                mx,
                self.text_input.caret_pos,
            );
            self.text_input
                .set_caret(&self.install_dir_input, char_idx, true);
            self.caret_visible = true;
        }
        self.invalidate();
    }

    pub fn on_char(&mut self, ch: char) {
        if ch.is_control() || !self.path_focused {
            return;
        }
        self.text_input.insert_char(&mut self.install_dir_input, ch);
        self.caret_visible = true;
        self.refresh_disk_space();
        self.invalidate();
    }

    pub fn on_keydown(&mut self, vk: u16) {
        if !self.path_focused {
            return;
        }
        let ctrl_down = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
        let shift_down = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };

        if ctrl_down {
            match vk {
                x if x == b'A' as u16 => {
                    self.text_input.select_all(&self.install_dir_input);
                    self.caret_visible = true;
                    self.invalidate();
                    return;
                }
                x if x == b'C' as u16 => {
                    if let Some(sel) = self.text_input.selected_text(&self.install_dir_input) {
                        set_clipboard_text(self.hwnd, sel);
                    }
                    return;
                }
                x if x == b'X' as u16 => {
                    if self.text_input.has_selection() {
                        if let Some(sel) = self.text_input.selected_text(&self.install_dir_input) {
                            set_clipboard_text(self.hwnd, sel);
                        }
                        self.text_input
                            .delete_selection(&mut self.install_dir_input);
                        self.caret_visible = true;
                        self.refresh_disk_space();
                        self.invalidate();
                    }
                    return;
                }
                x if x == b'V' as u16 => {
                    if let Some(clip) = get_clipboard_text(self.hwnd) {
                        let line = clip.lines().next().unwrap_or("").trim();
                        self.text_input
                            .insert_str(&mut self.install_dir_input, line);
                        self.caret_visible = true;
                        self.refresh_disk_space();
                        self.invalidate();
                    }
                    return;
                }
                _ => {}
            }
        }

        match vk {
            x if x == VK_LEFT.0 => {
                self.text_input
                    .move_left(&self.install_dir_input, shift_down);
                self.caret_visible = true;
                self.invalidate();
            }
            x if x == VK_RIGHT.0 => {
                self.text_input
                    .move_right(&self.install_dir_input, shift_down);
                self.caret_visible = true;
                self.invalidate();
            }
            x if x == VK_HOME.0 => {
                self.text_input
                    .move_home(&self.install_dir_input, shift_down);
                self.caret_visible = true;
                self.invalidate();
            }
            x if x == VK_END.0 => {
                self.text_input
                    .move_end(&self.install_dir_input, shift_down);
                self.caret_visible = true;
                self.invalidate();
            }
            x if x == VK_BACK.0 => {
                self.text_input.backspace(&mut self.install_dir_input);
                self.caret_visible = true;
                self.refresh_disk_space();
                self.invalidate();
            }
            x if x == VK_DELETE.0 => {
                self.text_input.delete_forward(&mut self.install_dir_input);
                self.caret_visible = true;
                self.refresh_disk_space();
                self.invalidate();
            }
            x if x == VK_ESCAPE.0 || x == VK_RETURN.0 => {
                self.path_focused = false;
                self.text_input.is_dragging = false;
                self.invalidate();
            }
            _ => {}
        }
    }

    pub fn on_mouse_wheel(&mut self, delta: i16) {
        if !self.show_process_conflict_modal {
            if let Some(log_box) = self.current_log_box_rect() {
                let m = self.compute_log_scroll_metrics(log_box);
                let shift_down = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
                let step = 44.0;
                if (shift_down || m.max_scroll_y <= 0.0) && m.max_scroll_x > 0.0 {
                    if delta < 0 {
                        self.log_scroll_x = (m.scroll_x + step).min(m.max_scroll_x);
                    } else {
                        self.log_scroll_x = (m.scroll_x - step).max(0.0);
                    }
                    self.invalidate();
                } else if m.max_scroll_y > 0.0 {
                    if delta < 0 {
                        self.log_scroll_y = (m.scroll_y + step).min(m.max_scroll_y);
                    } else {
                        self.log_scroll_y = (m.scroll_y - step).max(0.0);
                    }
                    self.log_auto_scroll = (self.log_scroll_y - m.max_scroll_y).abs() < 2.0;
                    self.invalidate();
                }
                return;
            }
        }

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
