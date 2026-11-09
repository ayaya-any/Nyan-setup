use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;

#[derive(Debug, Clone, Copy)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }

    pub fn to_d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }

    pub fn to_rounded(&self, radius: f32) -> D2D1_ROUNDED_RECT {
        D2D1_ROUNDED_RECT {
            rect: self.to_d2d(),
            radiusX: radius,
            radiusY: radius,
        }
    }

    pub fn inset(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            w: (self.w - dx * 2.0).max(0.0),
            h: (self.h - dy * 2.0).max(0.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RippleEffect {
    pub origin_x: f32,
    pub origin_y: f32,
    pub bounds: RectF,
    pub radius: f32,
    pub max_radius: f32,
    pub progress: f32,
    pub color: D2D1_COLOR_F,
}

impl RippleEffect {
    pub fn new(origin_x: f32, origin_y: f32, bounds: RectF, color: D2D1_COLOR_F) -> Self {
        let dx = (origin_x - bounds.x).max(bounds.x + bounds.w - origin_x);
        let dy = (origin_y - bounds.y).max(bounds.y + bounds.h - origin_y);
        let max_radius = (dx * dx + dy * dy).sqrt().max(24.0);
        Self {
            origin_x,
            origin_y,
            bounds,
            radius: 4.0,
            max_radius,
            progress: 0.0,
            color,
        }
    }

    pub fn step(&mut self) -> bool {
        self.progress += 0.075;
        let t = 1.0 - (1.0 - self.progress.min(1.0)).powi(3);
        self.radius = 6.0 + (self.max_radius - 6.0) * t;
        self.progress < 1.0
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TextInputState {
    pub caret_pos: usize,
    pub sel_anchor: usize,
    pub is_dragging: bool,
}

impl TextInputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn char_len(text: &str) -> usize {
        text.chars().count()
    }

    pub fn byte_index_of_char(text: &str, char_idx: usize) -> usize {
        text.char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(text.len())
    }

    pub fn clamp(&mut self, text: &str) {
        let len = Self::char_len(text);
        self.caret_pos = self.caret_pos.min(len);
        self.sel_anchor = self.sel_anchor.min(len);
    }

    pub fn has_selection(&self) -> bool {
        self.caret_pos != self.sel_anchor
    }

    pub fn selection_range(&self, text: &str) -> (usize, usize) {
        let len = Self::char_len(text);
        let s0 = self.caret_pos.min(self.sel_anchor).min(len);
        let s1 = self.caret_pos.max(self.sel_anchor).min(len);
        (s0, s1)
    }

    pub fn selected_text<'a>(&self, text: &'a str) -> Option<&'a str> {
        let (s0, s1) = self.selection_range(text);
        if s0 == s1 {
            return None;
        }
        let b0 = Self::byte_index_of_char(text, s0);
        let b1 = Self::byte_index_of_char(text, s1);
        Some(&text[b0..b1])
    }

    pub fn select_all(&mut self, text: &str) {
        self.sel_anchor = 0;
        self.caret_pos = Self::char_len(text);
    }

    pub fn move_to_end(&mut self, text: &str) {
        let len = Self::char_len(text);
        self.caret_pos = len;
        self.sel_anchor = len;
    }

    pub fn set_caret(&mut self, text: &str, pos: usize, extend_selection: bool) {
        let clamped = pos.min(Self::char_len(text));
        self.caret_pos = clamped;
        if !extend_selection {
            self.sel_anchor = clamped;
        }
    }

    pub fn delete_selection(&mut self, text: &mut String) -> bool {
        let (s0, s1) = self.selection_range(text);
        if s0 == s1 {
            return false;
        }
        let b0 = Self::byte_index_of_char(text, s0);
        let b1 = Self::byte_index_of_char(text, s1);
        text.replace_range(b0..b1, "");
        self.caret_pos = s0;
        self.sel_anchor = s0;
        true
    }

    pub fn insert_str(&mut self, text: &mut String, s: &str) {
        self.clamp(text);
        self.delete_selection(text);
        let b = Self::byte_index_of_char(text, self.caret_pos);
        text.insert_str(b, s);
        self.caret_pos += s.chars().count();
        self.sel_anchor = self.caret_pos;
    }

    pub fn insert_char(&mut self, text: &mut String, ch: char) {
        let mut buf = [0u8; 4];
        self.insert_str(text, ch.encode_utf8(&mut buf));
    }

    pub fn backspace(&mut self, text: &mut String) -> bool {
        self.clamp(text);
        if self.delete_selection(text) {
            return true;
        }
        if self.caret_pos > 0 {
            let b_prev = Self::byte_index_of_char(text, self.caret_pos - 1);
            let b_cur = Self::byte_index_of_char(text, self.caret_pos);
            text.replace_range(b_prev..b_cur, "");
            self.caret_pos -= 1;
            self.sel_anchor = self.caret_pos;
            return true;
        }
        false
    }

    pub fn delete_forward(&mut self, text: &mut String) -> bool {
        self.clamp(text);
        if self.delete_selection(text) {
            return true;
        }
        let len = Self::char_len(text);
        if self.caret_pos < len {
            let b_cur = Self::byte_index_of_char(text, self.caret_pos);
            let b_next = Self::byte_index_of_char(text, self.caret_pos + 1);
            text.replace_range(b_cur..b_next, "");
            self.sel_anchor = self.caret_pos;
            return true;
        }
        false
    }

    pub fn move_left(&mut self, text: &str, shift: bool) {
        self.clamp(text);
        if shift {
            self.caret_pos = self.caret_pos.saturating_sub(1);
        } else if self.has_selection() {
            let (s0, _) = self.selection_range(text);
            self.caret_pos = s0;
            self.sel_anchor = s0;
        } else {
            self.caret_pos = self.caret_pos.saturating_sub(1);
            self.sel_anchor = self.caret_pos;
        }
    }

    pub fn move_right(&mut self, text: &str, shift: bool) {
        self.clamp(text);
        let len = Self::char_len(text);
        if shift {
            self.caret_pos = (self.caret_pos + 1).min(len);
        } else if self.has_selection() {
            let (_, s1) = self.selection_range(text);
            self.caret_pos = s1;
            self.sel_anchor = s1;
        } else {
            self.caret_pos = (self.caret_pos + 1).min(len);
            self.sel_anchor = self.caret_pos;
        }
    }

    pub fn move_home(&mut self, text: &str, shift: bool) {
        self.clamp(text);
        self.caret_pos = 0;
        if !shift {
            self.sel_anchor = 0;
        }
    }

    pub fn move_end(&mut self, text: &str, shift: bool) {
        self.clamp(text);
        let len = Self::char_len(text);
        self.caret_pos = len;
        if !shift {
            self.sel_anchor = len;
        }
    }
}
