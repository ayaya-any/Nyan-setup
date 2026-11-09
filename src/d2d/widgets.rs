use crate::d2d::{
    rgb, rgba, D2DRenderer, MaterialPalette, RectF, RippleEffect, TextInputState, ThemeMode,
    VectorIcon,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_NONE, DWRITE_WORD_WRAPPING_NO_WRAP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Filled,
    Tonal,
    Outlined,
    Danger,
}

impl D2DRenderer {
    /// Draw a Material Elevated Card with soft layered shadow and subtle border.
    pub fn draw_card(&self, rect: RectF, radius: f32, palette: &MaterialPalette) {
        let shadow_alpha = if palette.mode == ThemeMode::Light {
            0.035
        } else {
            0.18
        };
        for i in (1..=3).rev() {
            let spread = i as f32 * 1.5;
            let s_rect = RectF::new(
                rect.x - spread * 0.4,
                rect.y + spread * 0.9,
                rect.w + spread * 0.8,
                rect.h + spread,
            );
            self.fill_rounded_rect(
                s_rect,
                radius + spread * 0.5,
                rgba(0x000000, shadow_alpha / (i as f32)),
            );
        }

        self.fill_rounded_rect(rect, radius, palette.surface);
        self.draw_rounded_rect(rect, radius, 1.0, palette.outline_subtle);
    }

    /// Draw active ripple animations clipped inside `bounds`.
    pub fn draw_ripples(&self, ripples: &[RippleEffect], target_bounds: RectF) {
        unsafe {
            for r in ripples {
                if (r.bounds.x - target_bounds.x).abs() < 1.0
                    && (r.bounds.y - target_bounds.y).abs() < 1.0
                {
                    self.rt().PushAxisAlignedClip(
                        &target_bounds.to_d2d(),
                        D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    );
                    let fade = (1.0 - r.progress).powi(2);
                    let mut c = r.color;
                    c.a *= fade;
                    self.fill_circle(r.origin_x, r.origin_y, r.radius, c);
                    self.rt().PopAxisAlignedClip();
                }
            }
        }
    }

    /// Draw a Material 3 Button with centered icon + label and ripple clipping.
    pub fn draw_button(
        &self,
        rect: RectF,
        label: &str,
        icon: Option<VectorIcon>,
        variant: ButtonVariant,
        hovered: bool,
        enabled: bool,
        palette: &MaterialPalette,
        ripples: &[RippleEffect],
    ) {
        let radius = (rect.h * 0.5).min(10.0);

        let (bg, fg, border) = if !enabled {
            (rgba(0x94A3B8, 0.18), palette.text_muted, None)
        } else {
            match variant {
                ButtonVariant::Filled => {
                    let bg_col = if hovered {
                        palette.primary_hover
                    } else {
                        palette.primary
                    };
                    if hovered {
                        let glow = RectF::new(rect.x, rect.y + 2.0, rect.w, rect.h + 2.0);
                        self.fill_rounded_rect(glow, radius, rgba(0x4F46E5, 0.22));
                    }
                    (bg_col, rgb(0xFFFFFF), None)
                }
                ButtonVariant::Tonal => {
                    let mut bg_col = palette.primary_container;
                    if hovered {
                        bg_col.a = 0.82;
                    }
                    (bg_col, palette.on_primary_container, None)
                }
                ButtonVariant::Outlined => {
                    let bg_col = if hovered {
                        rgba(0x6366F1, 0.08)
                    } else {
                        rgba(0x000000, 0.0)
                    };
                    (bg_col, palette.text_primary, Some(palette.outline))
                }
                ButtonVariant::Danger => {
                    let bg_col = if hovered {
                        rgb(0xDC2626)
                    } else {
                        palette.error
                    };
                    (bg_col, rgb(0xFFFFFF), None)
                }
            }
        };

        if bg.a > 0.0 {
            self.fill_rounded_rect(rect, radius, bg);
        }
        if let Some(b_col) = border {
            self.draw_rounded_rect(rect, radius, 1.2, b_col);
        }

        self.draw_ripples(ripples, rect);

        if let Some(ic) = icon {
            let body_w = self.measure_text_width(label, &self.font_body_bold);
            let icon_size = 14.0;
            let gap = if icon_size + 6.0 + body_w > rect.w - 8.0 {
                4.5
            } else {
                6.5
            };
            let fmt = if icon_size + gap + body_w > rect.w - 6.0 {
                &self.font_caption
            } else {
                &self.font_body_bold
            };
            let text_w = self.measure_text_width(label, fmt);
            let total_w = (icon_size + gap + text_w).min(rect.w - 4.0);
            let start_x = rect.x + (rect.w - total_w) * 0.5;
            self.draw_icon(ic, start_x + icon_size * 0.5, rect.y + rect.h * 0.5, icon_size, fg);
            let text_rect = RectF::new(
                start_x + icon_size + gap,
                rect.y,
                (text_w + 8.0).max(rect.x + rect.w - (start_x + icon_size + gap)),
                rect.h,
            );
            self.draw_text(label, fmt, text_rect, fg, 0, 1, false);
        } else {
            self.draw_text(label, &self.font_body_bold, rect.inset(2.0, 0.0), fg, 1, 1, false);
        }
    }

    /// Compute horizontal scroll offset for a text field so that `caret_pos` remains in view.
    pub fn text_field_scroll_x(&self, rect: RectF, value: &str, caret_pos: usize) -> f32 {
        let inner = rect.inset(10.0, 4.0);
        let char_len = TextInputState::char_len(value);
        let prefix_b = TextInputState::byte_index_of_char(value, caret_pos.min(char_len));
        let caret_x = self.measure_text_width(&value[..prefix_b], &self.font_body);
        if caret_x > inner.w - 6.0 {
            caret_x - (inner.w - 6.0)
        } else {
            0.0
        }
    }

    /// Hit-test a mouse `mx` coordinate inside a text field `rect`, returning the closest character index.
    pub fn hit_test_text_field_char(
        &self,
        rect: RectF,
        value: &str,
        mx: f32,
        current_caret_pos: usize,
    ) -> usize {
        let inner = rect.inset(10.0, 4.0);
        let scroll_x = if current_caret_pos <= TextInputState::char_len(value) {
            self.text_field_scroll_x(rect, value, current_caret_pos)
        } else {
            0.0
        };
        let target_x = (mx - inner.x + scroll_x).max(0.0);

        let char_len = TextInputState::char_len(value);
        let mut best_idx = 0usize;
        let mut best_dist = target_x.abs();

        for i in 1..=char_len {
            let b = TextInputState::byte_index_of_char(value, i);
            let w = self.measure_text_width(&value[..b], &self.font_body);
            let dist = (w - target_x).abs();
            if dist <= best_dist {
                best_dist = dist;
                best_idx = i;
            } else {
                break;
            }
        }
        best_idx
    }

    /// Draw a Material Outlined TextField with label above, selection highlight, and cursor caret.
    pub fn draw_text_field(
        &self,
        rect: RectF,
        label: &str,
        value: &str,
        focused: bool,
        hovered: bool,
        caret_visible: bool,
        palette: &MaterialPalette,
    ) {
        let len = TextInputState::char_len(value);
        let default_sel = TextInputState {
            caret_pos: len,
            sel_anchor: len,
            is_dragging: false,
        };
        self.draw_text_field_with_sel(
            rect,
            label,
            value,
            focused,
            hovered,
            caret_visible,
            &default_sel,
            palette,
        );
    }

    /// Draw a Material Outlined TextField with full `TextInputState` (selection range + cursor position).
    pub fn draw_text_field_with_sel(
        &self,
        rect: RectF,
        label: &str,
        value: &str,
        focused: bool,
        hovered: bool,
        caret_visible: bool,
        input_state: &TextInputState,
        palette: &MaterialPalette,
    ) {
        let label_rect = RectF::new(rect.x + 2.0, rect.y - 20.0, (rect.w - 2.0).max(0.0), 18.0);
        let label_color = if focused {
            palette.primary
        } else {
            palette.text_secondary
        };
        self.draw_text(label, &self.font_caption, label_rect, label_color, 0, 1, false);

        self.fill_rounded_rect(rect, 8.0, palette.surface_variant);
        let (border_col, stroke) = if focused {
            (palette.primary, 2.0)
        } else if hovered {
            (palette.text_secondary, 1.2)
        } else {
            (palette.outline, 1.0)
        };
        self.draw_rounded_rect(rect, 8.0, stroke, border_col);

        let inner = rect.inset(10.0, 4.0);
        unsafe {
            self.rt()
                .PushAxisAlignedClip(&inner.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }

        let char_len = TextInputState::char_len(value);
        let caret_pos = if focused {
            input_state.caret_pos.min(char_len)
        } else {
            0
        };
        let scroll_x = if focused {
            self.text_field_scroll_x(rect, value, caret_pos)
        } else {
            0.0
        };

        if focused && input_state.has_selection() {
            let (s0, s1) = input_state.selection_range(value);
            let b0 = TextInputState::byte_index_of_char(value, s0);
            let b1 = TextInputState::byte_index_of_char(value, s1);
            let x0 = inner.x - scroll_x + self.measure_text_width(&value[..b0], &self.font_body);
            let x1 = inner.x - scroll_x + self.measure_text_width(&value[..b1], &self.font_body);
            let sel_rect = RectF::new(x0, inner.y + 3.0, (x1 - x0).max(2.0), inner.h - 6.0);
            let sel_col = if palette.mode == ThemeMode::Light {
                rgba(0x4F46E5, 0.24)
            } else {
                rgba(0x6366F1, 0.42)
            };
            self.fill_rounded_rect(sel_rect, 3.0, sel_col);
        }

        let total_w = self.measure_text_width(value, &self.font_body);
        let draw_rect = RectF::new(
            inner.x - scroll_x,
            inner.y,
            (total_w + 40.0).max(inner.w),
            inner.h,
        );
        unsafe {
            let _ = self.font_body.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = self
                .font_body
                .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = self.font_body.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let no_trim = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_NONE,
                delimiter: 0,
                delimiterCount: 0,
            };
            let _ = self.font_body.SetTrimming(&no_trim, None);
            let wide: Vec<u16> = value.encode_utf16().collect();
            self.rt().DrawText(
                &wide,
                &self.font_body,
                &draw_rect.to_d2d(),
                self.brush(palette.text_primary),
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }

        if focused && caret_visible {
            let b_caret = TextInputState::byte_index_of_char(value, caret_pos);
            let cx =
                (inner.x - scroll_x + self.measure_text_width(&value[..b_caret], &self.font_body))
                    .clamp(inner.x + 1.0, inner.x + inner.w - 1.5);
            self.draw_line(
                cx,
                inner.y + 4.0,
                cx,
                inner.y + inner.h - 4.0,
                1.8,
                palette.primary,
            );
        }

        unsafe {
            self.rt().PopAxisAlignedClip();
        }
    }

    /// Draw a Material 3 Dropdown Select Trigger Box with label above and chevron icon.
    pub fn draw_dropdown(
        &self,
        rect: RectF,
        label: &str,
        selected_text: &str,
        is_open: bool,
        hovered: bool,
        palette: &MaterialPalette,
    ) {
        if !label.is_empty() {
            let label_rect =
                RectF::new(rect.x + 2.0, rect.y - 20.0, (rect.w - 2.0).max(0.0), 18.0);
            let label_color = if is_open {
                palette.primary
            } else {
                palette.text_secondary
            };
            self.draw_text(label, &self.font_caption, label_rect, label_color, 0, 1, false);
        }

        let bg = if is_open {
            palette.primary_container
        } else if hovered {
            rgba(0x6366F1, 0.06)
        } else {
            palette.surface_variant
        };
        self.fill_rounded_rect(rect, 8.0, bg);

        let (border_col, stroke) = if is_open {
            (palette.primary, 1.8)
        } else if hovered {
            (palette.primary, 1.2)
        } else {
            (palette.outline, 1.0)
        };
        self.draw_rounded_rect(rect, 8.0, stroke, border_col);

        let text_rect = RectF::new(rect.x + 10.0, rect.y, (rect.w - 30.0).max(10.0), rect.h);
        let fg = if is_open {
            palette.on_primary_container
        } else {
            palette.text_primary
        };
        self.draw_text(selected_text, &self.font_body, text_rect, fg, 0, 1, false);

        let chev = if is_open {
            VectorIcon::ChevronUp
        } else {
            VectorIcon::ChevronDown
        };
        self.draw_icon(
            chev,
            rect.x + rect.w - 14.0,
            rect.y + rect.h * 0.5,
            13.0,
            if is_open {
                palette.primary
            } else {
                palette.text_secondary
            },
        );
    }

    /// Compute the bounding rectangle of an open dropdown popup menu.
    pub fn dropdown_menu_rect(
        &self,
        trigger_rect: RectF,
        items: &[&str],
        window_w: f32,
        window_h: f32,
    ) -> RectF {
        let max_item_w = items
            .iter()
            .map(|item| self.measure_text_width(item, &self.font_body))
            .fold(0.0f32, f32::max);
        let menu_w = trigger_rect.w.max(max_item_w + 46.0);
        let menu_x = trigger_rect.x.min((window_w - menu_w - 16.0).max(16.0));
        let item_h = 32.0;
        let menu_h = (items.len() as f32) * item_h + 8.0;
        let below_y = trigger_rect.y + trigger_rect.h + 4.0;
        let menu_y = if below_y + menu_h > window_h - 14.0 {
            (trigger_rect.y - menu_h - 4.0).max(14.0)
        } else {
            below_y
        };
        RectF::new(menu_x, menu_y, menu_w, menu_h)
    }

    /// Draw a floating Material 3 Dropdown Popup Menu on top of other UI controls.
    pub fn draw_dropdown_menu(
        &self,
        trigger_rect: RectF,
        items: &[&str],
        selected_idx: usize,
        mx: f32,
        my: f32,
        window_w: f32,
        window_h: f32,
        palette: &MaterialPalette,
    ) {
        let menu_rect = self.dropdown_menu_rect(trigger_rect, items, window_w, window_h);

        for i in (1..=4).rev() {
            let sp = i as f32 * 2.0;
            let sr = RectF::new(
                menu_rect.x - sp * 0.4,
                menu_rect.y + sp * 0.8,
                menu_rect.w + sp * 0.8,
                menu_rect.h + sp,
            );
            self.fill_rounded_rect(sr, 10.0, rgba(0x000000, 0.08 / (i as f32)));
        }

        self.fill_rounded_rect(menu_rect, 8.0, palette.surface_elevated);
        self.draw_rounded_rect(menu_rect, 8.0, 1.3, palette.primary);

        let item_h = 32.0;
        for (idx, item_label) in items.iter().enumerate() {
            let ir = RectF::new(
                menu_rect.x + 4.0,
                menu_rect.y + 4.0 + (idx as f32) * item_h,
                menu_rect.w - 8.0,
                item_h,
            );
            let is_sel = idx == selected_idx;
            let is_hov = ir.contains(mx, my);

            if is_hov {
                self.fill_rounded_rect(ir, 6.0, palette.primary_container);
            } else if is_sel {
                self.fill_rounded_rect(ir, 6.0, rgba(0x6366F1, 0.08));
            }

            if is_sel {
                self.draw_icon(VectorIcon::Check, ir.x + 14.0, ir.y + item_h * 0.5, 12.0, palette.primary);
            }

            let fg = if is_hov {
                palette.on_primary_container
            } else if is_sel {
                palette.primary
            } else {
                palette.text_primary
            };
            let font = if is_sel {
                &self.font_body_bold
            } else {
                &self.font_body
            };
            self.draw_text(
                item_label,
                font,
                RectF::new(ir.x + 28.0, ir.y, ir.w - 34.0, item_h),
                fg,
                0,
                1,
                false,
            );
        }
    }

    /// Draw a Material Checkbox with title and optional subtitle.
    pub fn draw_checkbox(
        &self,
        rect: RectF,
        checked: bool,
        hovered: bool,
        title: &str,
        subtitle: Option<&str>,
        palette: &MaterialPalette,
    ) {
        if hovered {
            self.fill_rounded_rect(rect, 8.0, rgba(0x6366F1, 0.06));
        }

        let box_size = 18.0;
        let box_y = if subtitle.is_some() {
            rect.y + 9.0
        } else {
            rect.y + (rect.h - box_size) * 0.5
        };
        let box_rect = RectF::new(rect.x + 8.0, box_y, box_size, box_size);

        if checked {
            self.fill_rounded_rect(box_rect, 5.0, palette.primary);
            self.draw_icon(
                VectorIcon::Check,
                box_rect.x + box_size * 0.5,
                box_rect.y + box_size * 0.5,
                13.0,
                rgb(0xFFFFFF),
            );
        } else {
            self.fill_rounded_rect(box_rect, 5.0, palette.surface_variant);
            let b_col = if hovered {
                palette.primary
            } else {
                palette.outline
            };
            self.draw_rounded_rect(box_rect, 5.0, 1.5, b_col);
        }

        let text_x = box_rect.x + box_size + 10.0;
        let text_w = (rect.w - (text_x - rect.x) - 6.0).max(10.0);

        if let Some(sub) = subtitle {
            self.draw_text(
                title,
                &self.font_body_bold,
                RectF::new(text_x, rect.y + 5.0, text_w, 19.0),
                palette.text_primary,
                0,
                1,
                false,
            );
            self.draw_text(
                sub,
                &self.font_caption,
                RectF::new(text_x, rect.y + 24.0, text_w, 17.0),
                palette.text_secondary,
                0,
                1,
                false,
            );
        } else {
            self.draw_text(
                title,
                &self.font_body,
                RectF::new(text_x, rect.y, text_w, rect.h),
                palette.text_primary,
                0,
                1,
                false,
            );
        }
    }

    /// Draw a Material Linear Progress Indicator (`progress` in `0.0..=1.0`).
    pub fn draw_progress_bar(&self, rect: RectF, progress: f32, palette: &MaterialPalette) {
        let radius = rect.h * 0.5;
        self.fill_rounded_rect(rect, radius, palette.primary_container);

        let p = progress.clamp(0.0, 1.0);
        if p > 0.005 {
            let fill_w = (rect.w * p).max(rect.h);
            let fill_rect = RectF::new(rect.x, rect.y, fill_w.min(rect.w), rect.h);
            self.fill_rounded_rect(fill_rect, radius, palette.primary);
        }
    }

    /// Draw custom borderless Material window titlebar with dynamic badge placement.
    pub fn draw_window_header(
        &self,
        width: f32,
        title: &str,
        badge: &str,
        palette: &MaterialPalette,
        hover_theme: bool,
        hover_min: bool,
        hover_close: bool,
    ) {
        let header_h = 46.0;
        self.draw_nyan_app_icon(RectF::new(14.0, 9.0, 28.0, 28.0));

        let title_w = self
            .measure_text_width(title, &self.font_body_bold)
            .min(width - 280.0);
        self.draw_text(
            title,
            &self.font_body_bold,
            RectF::new(50.0, 0.0, title_w + 6.0, header_h),
            palette.text_primary,
            0,
            1,
            false,
        );

        if !badge.is_empty() {
            let badge_w = self.measure_text_width(badge, &self.font_caption) + 20.0;
            let badge_x = 50.0 + title_w + 12.0;
            if badge_x + badge_w < width - 140.0 {
                let badge_rect = RectF::new(badge_x, 12.0, badge_w, 22.0);
                self.fill_rounded_rect(badge_rect, 11.0, palette.primary_container);
                self.draw_text(
                    badge,
                    &self.font_caption,
                    badge_rect,
                    palette.on_primary_container,
                    1,
                    1,
                    false,
                );
            }
        }

        let btn_theme = Self::header_theme_rect(width);
        let btn_min = Self::header_min_rect(width);
        let btn_close = Self::header_close_rect(width);

        if hover_theme {
            self.fill_rounded_rect(btn_theme, 6.0, palette.primary_container);
        }
        let theme_icon = if palette.mode == ThemeMode::Light {
            VectorIcon::Moon
        } else {
            VectorIcon::Sun
        };
        self.draw_icon(
            theme_icon,
            btn_theme.x + btn_theme.w * 0.5,
            btn_theme.y + btn_theme.h * 0.5,
            15.0,
            palette.text_secondary,
        );

        if hover_min {
            self.fill_rounded_rect(btn_min, 6.0, rgba(0x64748B, 0.16));
        }
        self.draw_icon(
            VectorIcon::Minimize,
            btn_min.x + btn_min.w * 0.5,
            btn_min.y + btn_min.h * 0.5,
            14.0,
            palette.text_secondary,
        );

        if hover_close {
            self.fill_rounded_rect(btn_close, 6.0, rgb(0xEF4444));
        }
        self.draw_icon(
            VectorIcon::Close,
            btn_close.x + btn_close.w * 0.5,
            btn_close.y + btn_close.h * 0.5,
            14.0,
            if hover_close {
                rgb(0xFFFFFF)
            } else {
                palette.text_secondary
            },
        );
    }

    pub fn header_theme_rect(width: f32) -> RectF {
        RectF::new(width - 122.0, 8.0, 34.0, 30.0)
    }

    pub fn header_min_rect(width: f32) -> RectF {
        RectF::new(width - 82.0, 8.0, 34.0, 30.0)
    }

    pub fn header_close_rect(width: f32) -> RectF {
        RectF::new(width - 42.0, 8.0, 34.0, 30.0)
    }
}
