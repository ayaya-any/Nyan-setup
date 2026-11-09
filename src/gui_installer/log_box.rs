use super::{InstallerWindowState, WizardMode, WizardStep};
use crate::d2d::{rgba, RectF};
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
use windows::Win32::UI::Input::KeyboardAndMouse::SetCapture;

#[derive(Debug, Clone, Copy)]
pub(crate) struct LogScrollMetrics {
    pub viewport: RectF,
    pub row_h: f32,
    pub content_w: f32,
    pub max_scroll_x: f32,
    pub max_scroll_y: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub h_track: Option<RectF>,
    pub h_thumb: Option<RectF>,
    pub v_track: Option<RectF>,
    pub v_thumb: Option<RectF>,
}

impl InstallerWindowState {
    pub(crate) fn current_log_box_rect(&self) -> Option<RectF> {
        match &self.mode {
            WizardMode::Installer { .. } => {
                if self.step == WizardStep::Installing {
                    let card = self.installer_card_rect();
                    Some(RectF::new(
                        card.x + 26.0,
                        card.y + 112.0,
                        card.w - 52.0,
                        232.0,
                    ))
                } else {
                    None
                }
            }
            WizardMode::Uninstaller { .. } => {
                if self.uninstall_done {
                    let card =
                        RectF::new(18.0, 52.0, self.width_dip - 36.0, self.height_dip - 68.0);
                    Some(RectF::new(
                        card.x + 26.0,
                        card.y + 114.0,
                        card.w - 52.0,
                        270.0,
                    ))
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn compute_log_scroll_metrics(&self, log_box: RectF) -> LogScrollMetrics {
        let row_h = 22.0;
        let pad_x = 12.0;
        let pad_y = 8.0;
        let scrollbar_reserve = 14.0;

        let content_h = (self.logs.len() as f32) * row_h;
        let content_w = self
            .logs
            .iter()
            .map(|line| {
                self.renderer
                    .measure_text_width(line, &self.renderer.font_mono)
            })
            .fold(0.0_f32, f32::max);

        let base_w = (log_box.w - pad_x * 2.0).max(40.0);
        let base_h = (log_box.h - pad_y * 2.0).max(40.0);

        let mut need_h = content_w > base_w + 1.0;
        let mut avail_h = if need_h {
            (base_h - scrollbar_reserve).max(20.0)
        } else {
            base_h
        };
        let need_v = content_h > avail_h + 1.0;
        let avail_w = if need_v {
            (base_w - scrollbar_reserve).max(20.0)
        } else {
            base_w
        };
        if !need_h && content_w > avail_w + 1.0 {
            need_h = true;
            avail_h = (base_h - scrollbar_reserve).max(20.0);
        }

        let viewport = RectF::new(log_box.x + pad_x, log_box.y + pad_y, avail_w, avail_h);
        let max_scroll_x = (content_w - avail_w).max(0.0);
        let max_scroll_y = (content_h - avail_h).max(0.0);
        let scroll_x = self.log_scroll_x.clamp(0.0, max_scroll_x);
        let scroll_y = if self.log_auto_scroll {
            max_scroll_y
        } else {
            self.log_scroll_y.clamp(0.0, max_scroll_y)
        };

        let (h_track, h_thumb) = if need_h && max_scroll_x > 0.0 {
            let track_w =
                (log_box.w - 20.0 - if need_v { scrollbar_reserve } else { 0.0 }).max(32.0);
            let track = RectF::new(
                log_box.x + 10.0,
                log_box.y + log_box.h - 13.0,
                track_w,
                7.0,
            );
            let thumb_w = ((avail_w / content_w.max(1.0)) * track.w).clamp(28.0, track.w);
            let max_travel = (track.w - thumb_w).max(0.0);
            let thumb_x = track.x + (scroll_x / max_scroll_x) * max_travel;
            let thumb = RectF::new(thumb_x, track.y, thumb_w, track.h);
            (Some(track), Some(thumb))
        } else {
            (None, None)
        };

        let (v_track, v_thumb) = if need_v && max_scroll_y > 0.0 {
            let track_h =
                (log_box.h - 16.0 - if need_h { scrollbar_reserve } else { 0.0 }).max(32.0);
            let track = RectF::new(
                log_box.x + log_box.w - 13.0,
                log_box.y + 8.0,
                7.0,
                track_h,
            );
            let thumb_h = ((avail_h / content_h.max(1.0)) * track.h).clamp(28.0, track.h);
            let max_travel = (track.h - thumb_h).max(0.0);
            let thumb_y = track.y + (scroll_y / max_scroll_y) * max_travel;
            let thumb = RectF::new(track.x, thumb_y, track.w, thumb_h);
            (Some(track), Some(thumb))
        } else {
            (None, None)
        };

        LogScrollMetrics {
            viewport,
            row_h,
            content_w,
            max_scroll_x,
            max_scroll_y,
            scroll_x,
            scroll_y,
            h_track,
            h_thumb,
            v_track,
            v_thumb,
        }
    }

    pub(crate) fn draw_scrollable_log_box(&self, log_box: RectF, is_uninstaller_list: bool) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;

        self.renderer
            .fill_rounded_rect(log_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(log_box, 10.0, 1.0, pal.outline_subtle);

        let m = self.compute_log_scroll_metrics(log_box);
        unsafe {
            self.renderer
                .rt()
                .PushAxisAlignedClip(&m.viewport.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }

        let line_draw_w = m.content_w.max(m.viewport.w) + 32.0;
        for (i, line) in self.logs.iter().enumerate() {
            let ly = m.viewport.y + (i as f32) * m.row_h - m.scroll_y;
            if ly + m.row_h < m.viewport.y || ly > m.viewport.y + m.viewport.h {
                continue;
            }
            let col = if is_uninstaller_list {
                if line.starts_with("[错误]") {
                    pal.error
                } else {
                    pal.success
                }
            } else if line.starts_with("[错误]") {
                pal.error
            } else if line.starts_with("[进程管理]") || line.starts_with("[覆盖更新]") {
                pal.warning
            } else if line.starts_with("[完成]") {
                pal.success
            } else {
                pal.text_secondary
            };
            self.renderer.draw_text(
                line,
                &self.renderer.font_mono,
                RectF::new(m.viewport.x - m.scroll_x, ly, line_draw_w, m.row_h),
                col,
                0,
                1,
                false,
            );
        }

        unsafe {
            self.renderer.rt().PopAxisAlignedClip();
        }

        if let (Some(track), Some(thumb)) = (m.h_track, m.h_thumb) {
            let hit_track = RectF::new(track.x, track.y - 5.0, track.w, track.h + 10.0);
            let is_active = self.dragging_log_hscroll.is_some() || hit_track.contains(mx, my);
            self.renderer
                .fill_rounded_rect(track, 3.5, rgba(0x64748B, 0.16));
            let thumb_col = if is_active {
                pal.primary
            } else {
                rgba(0x64748B, 0.48)
            };
            self.renderer.fill_rounded_rect(thumb, 3.5, thumb_col);
        }

        if let (Some(track), Some(thumb)) = (m.v_track, m.v_thumb) {
            let hit_track = RectF::new(track.x - 5.0, track.y, track.w + 10.0, track.h);
            let is_active = self.dragging_log_vscroll.is_some() || hit_track.contains(mx, my);
            self.renderer
                .fill_rounded_rect(track, 3.5, rgba(0x64748B, 0.16));
            let thumb_col = if is_active {
                pal.primary
            } else {
                rgba(0x64748B, 0.48)
            };
            self.renderer.fill_rounded_rect(thumb, 3.5, thumb_col);
        }
    }

    pub(crate) fn try_begin_log_scrollbar_drag(&mut self, x: f32, y: f32) -> bool {
        if self.show_process_conflict_modal {
            return false;
        }
        let Some(log_box) = self.current_log_box_rect() else {
            return false;
        };
        let m = self.compute_log_scroll_metrics(log_box);
        self.log_scroll_x = m.scroll_x;
        self.log_scroll_y = m.scroll_y;

        if let (Some(track), Some(thumb)) = (m.h_track, m.h_thumb) {
            let hit_track = RectF::new(track.x, track.y - 5.0, track.w, track.h + 10.0);
            if hit_track.contains(x, y) {
                if x >= thumb.x && x <= thumb.x + thumb.w {
                    self.dragging_log_hscroll = Some(x - thumb.x);
                } else {
                    let grab_offset = thumb.w * 0.5;
                    let max_travel = (track.w - thumb.w).max(1.0);
                    let new_thumb_x = (x - grab_offset).clamp(track.x, track.x + max_travel);
                    self.log_scroll_x = ((new_thumb_x - track.x) / max_travel) * m.max_scroll_x;
                    self.dragging_log_hscroll = Some(x - new_thumb_x);
                }
                unsafe {
                    let _ = SetCapture(self.hwnd);
                }
                self.invalidate();
                return true;
            }
        }

        if let (Some(track), Some(thumb)) = (m.v_track, m.v_thumb) {
            let hit_track = RectF::new(track.x - 5.0, track.y, track.w + 10.0, track.h);
            if hit_track.contains(x, y) {
                self.log_auto_scroll = false;
                if y >= thumb.y && y <= thumb.y + thumb.h {
                    self.dragging_log_vscroll = Some(y - thumb.y);
                } else {
                    let grab_offset = thumb.h * 0.5;
                    let max_travel = (track.h - thumb.h).max(1.0);
                    let new_thumb_y = (y - grab_offset).clamp(track.y, track.y + max_travel);
                    self.log_scroll_y = ((new_thumb_y - track.y) / max_travel) * m.max_scroll_y;
                    self.dragging_log_vscroll = Some(y - new_thumb_y);
                }
                unsafe {
                    let _ = SetCapture(self.hwnd);
                }
                self.invalidate();
                return true;
            }
        }

        false
    }
}
