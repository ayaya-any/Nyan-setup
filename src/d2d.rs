use crate::icon;
use crate::win32_ops::to_wide;
use material_icons::{icon_to_char, Icon as MaterialIcon, FONT as MATERIAL_ICONS_FONT};
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
    D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1PathGeometry,
    ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_ROUND,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LINE_JOIN_ROUND, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFactory5, IDWriteFontCollection,
    IDWriteInMemoryFontFileLoader, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_NONE,
    DWRITE_WORD_WRAPPING_EMERGENCY_BREAK, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy)]
pub struct MaterialPalette {
    pub mode: ThemeMode,
    pub bg: D2D1_COLOR_F,
    pub surface: D2D1_COLOR_F,
    pub surface_elevated: D2D1_COLOR_F,
    pub surface_variant: D2D1_COLOR_F,
    pub nav_bg: D2D1_COLOR_F,
    pub primary: D2D1_COLOR_F,
    pub primary_hover: D2D1_COLOR_F,
    pub primary_container: D2D1_COLOR_F,
    pub on_primary_container: D2D1_COLOR_F,
    pub secondary: D2D1_COLOR_F,
    pub text_primary: D2D1_COLOR_F,
    pub text_secondary: D2D1_COLOR_F,
    pub text_muted: D2D1_COLOR_F,
    pub outline: D2D1_COLOR_F,
    pub outline_subtle: D2D1_COLOR_F,
    pub success: D2D1_COLOR_F,
    pub warning: D2D1_COLOR_F,
    pub error: D2D1_COLOR_F,
}

pub const fn rgb(hex: u32) -> D2D1_COLOR_F {
    rgba(hex, 1.0)
}

pub const fn rgba(hex: u32, a: f32) -> D2D1_COLOR_F {
    let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
    let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
    let b = (hex & 0xFF) as f32 / 255.0;
    D2D1_COLOR_F { r, g, b, a }
}

impl MaterialPalette {
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            bg: rgb(0xEEF2F6),
            surface: rgb(0xFFFFFF),
            surface_elevated: rgb(0xFFFFFF),
            surface_variant: rgb(0xF8FAFC),
            nav_bg: rgb(0xE0E7FF),
            primary: rgb(0x4F46E5),
            primary_hover: rgb(0x4338CA),
            primary_container: rgb(0xE0E7FF),
            on_primary_container: rgb(0x312E81),
            secondary: rgb(0x0EA5E9),
            text_primary: rgb(0x0F172A),
            text_secondary: rgb(0x475569),
            text_muted: rgb(0x94A3B8),
            outline: rgb(0xCBD5E1),
            outline_subtle: rgb(0xE2E8F0),
            success: rgb(0x10B981),
            warning: rgb(0xF59E0B),
            error: rgb(0xEF4444),
        }
    }

    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            bg: rgb(0x0B0F19),
            surface: rgb(0x151C2C),
            surface_elevated: rgb(0x1E293B),
            surface_variant: rgb(0x0F172A),
            nav_bg: rgb(0x111827),
            primary: rgb(0x6366F1),
            primary_hover: rgb(0x818CF8),
            primary_container: rgb(0x312E81),
            on_primary_container: rgb(0xE0E7FF),
            secondary: rgb(0x38BDF8),
            text_primary: rgb(0xF8FAFC),
            text_secondary: rgb(0x94A3B8),
            text_muted: rgb(0x64748B),
            outline: rgb(0x334155),
            outline_subtle: rgb(0x1E293B),
            success: rgb(0x34D399),
            warning: rgb(0xFBBF24),
            error: rgb(0xF87171),
        }
    }
}

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
        let added = s.chars().count();
        self.caret_pos += added;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorIcon {
    Info,
    Folder,
    Document,
    Rocket,
    Check,
    Shield,
    HardDrive,
    Trash,
    Sun,
    Moon,
    Minimize,
    Close,
    Sparkles,
    Play,
    ChevronDown,
    ChevronUp,
}

impl VectorIcon {
    /// Convert to strongly-typed `material_icons::Icon` from the `material-icons` crate.
    pub const fn to_material_icon(self) -> MaterialIcon {
        match self {
            Self::Check => MaterialIcon::Check,
            Self::Folder => MaterialIcon::FolderOpen,
            Self::Document => MaterialIcon::Description,
            Self::Info => MaterialIcon::Info,
            Self::Rocket => MaterialIcon::RocketLaunch,
            Self::Shield => MaterialIcon::VerifiedUser,
            Self::HardDrive => MaterialIcon::Storage,
            Self::Trash => MaterialIcon::Delete,
            Self::Sun => MaterialIcon::LightMode,
            Self::Moon => MaterialIcon::DarkMode,
            Self::Minimize => MaterialIcon::Remove,
            Self::Close => MaterialIcon::Close,
            Self::Sparkles => MaterialIcon::AutoAwesome,
            Self::Play => MaterialIcon::PlayArrow,
            Self::ChevronDown => MaterialIcon::ExpandMore,
            Self::ChevronUp => MaterialIcon::ExpandLess,
        }
    }

    /// Return the Unicode glyph `char` in `MaterialIcons-Regular.ttf` via `material_icons::icon_to_char`.
    pub fn material_char(self) -> char {
        icon_to_char(self.to_material_icon())
    }

    /// Official Google Material Icons (`MaterialIcons`, 24x24 viewBox) icon identifier.
    pub const fn material_name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Folder => "folder_open",
            Self::Document => "description",
            Self::Info => "info",
            Self::Rocket => "rocket_launch",
            Self::Shield => "verified_user",
            Self::HardDrive => "storage",
            Self::Trash => "delete",
            Self::Sun => "light_mode",
            Self::Moon => "dark_mode",
            Self::Minimize => "remove",
            Self::Close => "close",
            Self::Sparkles => "auto_awesome",
            Self::Play => "play_arrow",
            Self::ChevronDown => "expand_more",
            Self::ChevronUp => "expand_less",
        }
    }

    /// Official Google Material Icons (`MaterialIcons`, 24x24 viewBox) SVG `<path d="..."/>` definition.
    pub const fn material_svg_path(self) -> &'static str {
        match self {
            Self::Check => "M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z",
            Self::Folder => "M20 6h-8l-2-2H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2zm0 12H4V8h16v10z",
            Self::Document => "M14 2H6c-1.1 0-1.99.9-1.99 2L4 20c0 1.1.89 2 1.99 2H18c1.1 0 2-.9 2-2V8l-6-6zm2 16H8v-2h8v2zm0-4H8v-2h8v2zm-3-5V3.5L18.5 9H13z",
            Self::Info => "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-6h2v6zm0-8h-2V7h2v2z",
            Self::Rocket => "M9.19 6.35c-2.04 2.29-3.44 5.58-3.57 5.89L2 10.69l4.05-4.05c.47-.47 1.15-.68 1.81-.55l1.33.26zM11.17 17s3.74-1.55 5.89-3.7c5.4-5.4 4.5-9.62 4.21-10.57-.95-.3-5.17-1.19-10.57 4.21C8.55 9.09 7 12.83 7 12.83L11.17 17zm6.48-2.19c-2.29 2.04-5.58 3.44-5.89 3.57L13.31 22l4.05-4.05c.47-.47.68-1.15.55-1.81l-.26-1.33zM9 18c0 .83-.34 1.58-.88 2.12C6.94 21.3 2 22 2 22s.7-4.94 1.88-6.12C4.42 15.34 5.17 15 6 15c1.66 0 3 1.34 3 3zm4-9c0-1.1.9-2 2-2s2 .9 2 2-.9 2-2 2-2-.9-2-2z",
            Self::Shield => "M12 1L3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5l-9-4zm-2 16l-4-4 1.41-1.41L10 14.17l6.59-6.59L18 9l-8 8z",
            Self::HardDrive => "M2 20h20v-4H2v4zm2-3h2v2H4v-2zM2 4v4h20V4H2zm4 3H4V5h2v2zm-4 7h20v-4H2v4zm2-3h2v2H4v-2z",
            Self::Trash => "M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z",
            Self::Sun => "M12 7c-2.76 0-5 2.24-5 5s2.24 5 5 5 5-2.24 5-5-2.24-5-5-5zM2 13h2c.55 0 1-.45 1-1s-.45-1-1-1H2c-.55 0-1 .45-1 1s.45 1 1 1zm18 0h2c.55 0 1-.45 1-1s-.45-1-1-1h-2c-.55 0-1 .45-1 1s.45 1 1 1zM11 2v2c0 .55.45 1 1 1s1-.45 1-1V2c0-.55-.45-1-1-1s-1 .45-1 1zm0 18v2c0 .55.45 1 1 1s1-.45 1-1v-2c0-.55-.45-1-1-1s-1 .45-1 1zM5.99 4.58c-.39-.39-1.03-.39-1.41 0-.39.39-.39 1.03 0 1.41l1.06 1.06c.39.39 1.03.39 1.41 0s.39-1.03 0-1.41L5.99 4.58zm12.37 12.37c-.39-.39-1.03-.39-1.41 0-.39.39-.39 1.03 0 1.41l1.06 1.06c.39.39 1.03.39 1.41 0 .39-.39.39-1.03 0-1.41l-1.06-1.06zm1.06-10.96c.39-.39.39-1.03 0-1.41-.39-.39-1.03-.39-1.41 0l-1.06 1.06c-.39.39-.39 1.03 0 1.41s1.03.39 1.41 0l1.06-1.06zM7.05 18.36c.39-.39.39-1.03 0-1.41-.39-.39-1.03-.39-1.41 0l-1.06 1.06c-.39.39-.39 1.03 0 1.41s1.03.39 1.41 0l1.06-1.06z",
            Self::Moon => "M12 3c-4.97 0-9 4.03-9 9s4.03 9 9 9 9-4.03 9-9c0-.46-.04-.92-.1-1.36-.98 1.37-2.58 2.26-4.4 2.26-2.98 0-5.4-2.42-5.4-5.4 0-1.81.89-3.42 2.26-4.4-.44-.06-.9-.1-1.36-.1z",
            Self::Minimize => "M19 13H5v-2h14v2z",
            Self::Close => "M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z",
            Self::Sparkles => "M19 9l1.25-2.75L23 5l-2.75-1.25L19 1l-1.25 2.75L15 5l2.75 1.25L19 9zm-7.5.5L9 4 6.5 9.5 1 12l5.5 2.5L9 20l2.5-5.5L17 12l-5.5-2.5zM19 15l-1.25 2.75L15 19l2.75 1.25L19 23l1.25-2.75L23 19l-2.75-1.25L19 15z",
            Self::Play => "M8 5v14l11-7z",
            Self::ChevronDown => "M16.59 8.59L12 13.17 7.41 8.59 6 10l6 6 6-6z",
            Self::ChevronUp => "M12 8l-6 6 1.41 1.41L12 10.83l4.59 4.58L18 14z",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SvgToken {
    Cmd(char),
    Num(f32),
}

fn tokenize_svg_path(d: &str) -> Vec<SvgToken> {
    let mut tokens = Vec::with_capacity(64);
    let bytes = d.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() || b == b',' {
            i += 1;
            continue;
        }
        if b.is_ascii_alphabetic() {
            tokens.push(SvgToken::Cmd(b as char));
            i += 1;
            continue;
        }
        if b == b'+' || b == b'-' || b == b'.' || b.is_ascii_digit() {
            let start = i;
            let mut has_dot = b == b'.';
            i += 1;
            while i < len {
                let c = bytes[i];
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == b'.' && !has_dot {
                    has_dot = true;
                    i += 1;
                } else if (c == b'e' || c == b'E') && i + 1 < len {
                    i += 1;
                    if bytes[i] == b'+' || bytes[i] == b'-' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(s) = std::str::from_utf8(&bytes[start..i]) {
                if let Ok(val) = s.parse::<f32>() {
                    tokens.push(SvgToken::Num(val));
                }
            }
            continue;
        }
        i += 1;
    }
    tokens
}

pub fn build_material_icon_geometry(
    factory: &ID2D1Factory,
    d: &str,
    cx: f32,
    cy: f32,
    size: f32,
) -> Result<ID2D1PathGeometry, String> {
    let tokens = tokenize_svg_path(d);
    let scale = (size * 1.18) / 24.0;
    let ox = cx - 12.0 * scale;
    let oy = cy - 12.0 * scale;
    let pt = |x: f32, y: f32| D2D_POINT_2F {
        x: ox + x * scale,
        y: oy + y * scale,
    };

    unsafe {
        let path = factory
            .CreatePathGeometry()
            .map_err(|e| format!("CreatePathGeometry 失败: {}", e))?;
        let sink = path.Open().map_err(|e| format!("Path.Open 失败: {}", e))?;
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);

        let mut idx = 0usize;
        let mut cur_x = 0.0f32;
        let mut cur_y = 0.0f32;
        let mut start_x = 0.0f32;
        let mut start_y = 0.0f32;
        let mut last_ctrl_x = 0.0f32;
        let mut last_ctrl_y = 0.0f32;
        let mut last_cmd = ' ';
        let mut cur_cmd = ' ';
        let mut in_figure = false;

        let next_num = |idx: &mut usize| -> Option<f32> {
            if let Some(SvgToken::Num(v)) = tokens.get(*idx) {
                *idx += 1;
                Some(*v)
            } else {
                None
            }
        };

        while idx < tokens.len() {
            if let SvgToken::Cmd(c) = tokens[idx] {
                cur_cmd = c;
                idx += 1;
                if cur_cmd == 'Z' || cur_cmd == 'z' {
                    if in_figure {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        in_figure = false;
                    }
                    cur_x = start_x;
                    cur_y = start_y;
                    last_ctrl_x = cur_x;
                    last_ctrl_y = cur_y;
                    last_cmd = cur_cmd;
                    continue;
                }
            }

            match cur_cmd {
                'M' | 'm' => {
                    let Some(x) = next_num(&mut idx) else { break };
                    let Some(y) = next_num(&mut idx) else { break };
                    if in_figure {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    if cur_cmd == 'm' {
                        cur_x += x;
                        cur_y += y;
                    } else {
                        cur_x = x;
                        cur_y = y;
                    }
                    start_x = cur_x;
                    start_y = cur_y;
                    sink.BeginFigure(pt(cur_x, cur_y), D2D1_FIGURE_BEGIN_FILLED);
                    in_figure = true;
                    last_ctrl_x = cur_x;
                    last_ctrl_y = cur_y;
                    last_cmd = cur_cmd;
                    cur_cmd = if cur_cmd == 'm' { 'l' } else { 'L' };
                }
                'L' | 'l' => {
                    let Some(x) = next_num(&mut idx) else { break };
                    let Some(y) = next_num(&mut idx) else { break };
                    if cur_cmd == 'l' {
                        cur_x += x;
                        cur_y += y;
                    } else {
                        cur_x = x;
                        cur_y = y;
                    }
                    if in_figure {
                        sink.AddLine(pt(cur_x, cur_y));
                    }
                    last_ctrl_x = cur_x;
                    last_ctrl_y = cur_y;
                    last_cmd = cur_cmd;
                }
                'H' | 'h' => {
                    let Some(x) = next_num(&mut idx) else { break };
                    if cur_cmd == 'h' {
                        cur_x += x;
                    } else {
                        cur_x = x;
                    }
                    if in_figure {
                        sink.AddLine(pt(cur_x, cur_y));
                    }
                    last_ctrl_x = cur_x;
                    last_ctrl_y = cur_y;
                    last_cmd = cur_cmd;
                }
                'V' | 'v' => {
                    let Some(y) = next_num(&mut idx) else { break };
                    if cur_cmd == 'v' {
                        cur_y += y;
                    } else {
                        cur_y = y;
                    }
                    if in_figure {
                        sink.AddLine(pt(cur_x, cur_y));
                    }
                    last_ctrl_x = cur_x;
                    last_ctrl_y = cur_y;
                    last_cmd = cur_cmd;
                }
                'C' | 'c' => {
                    let Some(x1) = next_num(&mut idx) else { break };
                    let Some(y1) = next_num(&mut idx) else { break };
                    let Some(x2) = next_num(&mut idx) else { break };
                    let Some(y2) = next_num(&mut idx) else { break };
                    let Some(x) = next_num(&mut idx) else { break };
                    let Some(y) = next_num(&mut idx) else { break };
                    let (cp1x, cp1y, cp2x, cp2y, ex, ey) = if cur_cmd == 'c' {
                        (
                            cur_x + x1,
                            cur_y + y1,
                            cur_x + x2,
                            cur_y + y2,
                            cur_x + x,
                            cur_y + y,
                        )
                    } else {
                        (x1, y1, x2, y2, x, y)
                    };
                    if in_figure {
                        sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: pt(cp1x, cp1y),
                            point2: pt(cp2x, cp2y),
                            point3: pt(ex, ey),
                        });
                    }
                    cur_x = ex;
                    cur_y = ey;
                    last_ctrl_x = cp2x;
                    last_ctrl_y = cp2y;
                    last_cmd = cur_cmd;
                }
                'S' | 's' => {
                    let Some(x2) = next_num(&mut idx) else { break };
                    let Some(y2) = next_num(&mut idx) else { break };
                    let Some(x) = next_num(&mut idx) else { break };
                    let Some(y) = next_num(&mut idx) else { break };
                    let (cp1x, cp1y) = if matches!(last_cmd, 'C' | 'c' | 'S' | 's') {
                        (2.0 * cur_x - last_ctrl_x, 2.0 * cur_y - last_ctrl_y)
                    } else {
                        (cur_x, cur_y)
                    };
                    let (cp2x, cp2y, ex, ey) = if cur_cmd == 's' {
                        (cur_x + x2, cur_y + y2, cur_x + x, cur_y + y)
                    } else {
                        (x2, y2, x, y)
                    };
                    if in_figure {
                        sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: pt(cp1x, cp1y),
                            point2: pt(cp2x, cp2y),
                            point3: pt(ex, ey),
                        });
                    }
                    cur_x = ex;
                    cur_y = ey;
                    last_ctrl_x = cp2x;
                    last_ctrl_y = cp2y;
                    last_cmd = cur_cmd;
                }
                'Q' | 'q' => {
                    let Some(qx1) = next_num(&mut idx) else { break };
                    let Some(qy1) = next_num(&mut idx) else { break };
                    let Some(x) = next_num(&mut idx) else { break };
                    let Some(y) = next_num(&mut idx) else { break };
                    let (qx, qy, ex, ey) = if cur_cmd == 'q' {
                        (cur_x + qx1, cur_y + qy1, cur_x + x, cur_y + y)
                    } else {
                        (qx1, qy1, x, y)
                    };
                    let cp1x = cur_x + (2.0 / 3.0) * (qx - cur_x);
                    let cp1y = cur_y + (2.0 / 3.0) * (qy - cur_y);
                    let cp2x = ex + (2.0 / 3.0) * (qx - ex);
                    let cp2y = ey + (2.0 / 3.0) * (qy - ey);
                    if in_figure {
                        sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: pt(cp1x, cp1y),
                            point2: pt(cp2x, cp2y),
                            point3: pt(ex, ey),
                        });
                    }
                    cur_x = ex;
                    cur_y = ey;
                    last_ctrl_x = qx;
                    last_ctrl_y = qy;
                    last_cmd = cur_cmd;
                }
                _ => {
                    break;
                }
            }
        }

        if in_figure {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close().map_err(|e| format!("Sink.Close 失败: {}", e))?;
        let _ = D2D1_FIGURE_BEGIN_HOLLOW;
        let _ = D2D_SIZE_F::default();
        Ok(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Filled,
    Tonal,
    Outlined,
    Danger,
}

pub struct D2DRenderer {
    pub d2d_factory: ID2D1Factory,
    pub dwrite_factory: IDWriteFactory,
    pub target: Option<ID2D1DCRenderTarget>,
    pub brush: Option<ID2D1SolidColorBrush>,
    pub mem_dc: HDC,
    pub dib_bitmap: HBITMAP,
    pub old_obj: HGDIOBJ,
    pub dib_bits: *mut u8,
    pub width_px: u32,
    pub height_px: u32,
    pub font_title_lg: IDWriteTextFormat,
    pub font_title_md: IDWriteTextFormat,
    pub font_subtitle: IDWriteTextFormat,
    pub font_body: IDWriteTextFormat,
    pub font_body_bold: IDWriteTextFormat,
    pub font_caption: IDWriteTextFormat,
    pub font_small: IDWriteTextFormat,
    pub font_mono: IDWriteTextFormat,
    pub font_material_icon: Option<IDWriteTextFormat>,
    icon_font_loader: Option<IDWriteInMemoryFontFileLoader>,
    pub dpi_scale: f32,
    icon_cache: RefCell<Vec<(u32, ID2D1Bitmap, f32)>>,
}

impl Drop for D2DRenderer {
    fn drop(&mut self) {
        self.free_dib();
        unsafe {
            if let Some(ref loader) = self.icon_font_loader {
                if let Ok(factory5) = self.dwrite_factory.cast::<IDWriteFactory5>() {
                    let _ = factory5.UnregisterFontFileLoader(loader);
                }
            }
            if !self.mem_dc.0.is_null() {
                let _ = DeleteDC(self.mem_dc);
            }
        }
    }
}

impl D2DRenderer {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let d2d_factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                    .map_err(|e| format!("D2D1CreateFactory 失败: {}", e))?;

            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
                .map_err(|e| format!("DWriteCreateFactory 失败: {}", e))?;

            let create_fmt =
                |family: &str, size: f32, weight| -> Result<IDWriteTextFormat, String> {
                    let fam_w = to_wide(family);
                    let locale_w = to_wide("zh-CN");
                    dwrite_factory
                        .CreateTextFormat(
                            PCWSTR(fam_w.as_ptr()),
                            None,
                            weight,
                            DWRITE_FONT_STYLE_NORMAL,
                            DWRITE_FONT_STRETCH_NORMAL,
                            size,
                            PCWSTR(locale_w.as_ptr()),
                        )
                        .map_err(|e| format!("CreateTextFormat 失败: {}", e))
                };

            let font_title_lg = create_fmt("Microsoft YaHei UI", 20.0, DWRITE_FONT_WEIGHT_BOLD)?;
            let font_title_md =
                create_fmt("Microsoft YaHei UI", 15.5, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
            let font_subtitle = create_fmt("Microsoft YaHei UI", 13.0, DWRITE_FONT_WEIGHT_MEDIUM)?;
            let font_body = create_fmt("Microsoft YaHei UI", 12.5, DWRITE_FONT_WEIGHT_REGULAR)?;
            let font_body_bold =
                create_fmt("Microsoft YaHei UI", 12.5, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
            let font_caption = create_fmt("Microsoft YaHei UI", 11.5, DWRITE_FONT_WEIGHT_REGULAR)?;
            let font_small = create_fmt("Microsoft YaHei UI", 10.2, DWRITE_FONT_WEIGHT_REGULAR)?;
            let font_mono = create_fmt("Consolas", 12.0, DWRITE_FONT_WEIGHT_REGULAR)?;

            // Load embedded MaterialIcons-Regular.ttf from the `material-icons` crate into DirectWrite
            let mut icon_font_loader: Option<IDWriteInMemoryFontFileLoader> = None;
            let mut font_material_icon: Option<IDWriteTextFormat> = None;
            if let Ok(factory5) = dwrite_factory.cast::<IDWriteFactory5>() {
                if let Ok(loader) = factory5.CreateInMemoryFontFileLoader() {
                    if factory5.RegisterFontFileLoader(&loader).is_ok() {
                        if let Ok(font_file) = loader.CreateInMemoryFontFileReference(
                            &factory5,
                            MATERIAL_ICONS_FONT.as_ptr() as *const _,
                            MATERIAL_ICONS_FONT.len() as u32,
                            None,
                        ) {
                            if let Ok(builder) = factory5.CreateFontSetBuilder() {
                                if builder.AddFontFile(&font_file).is_ok() {
                                    if let Ok(font_set) = builder.CreateFontSet() {
                                        if let Ok(collection1) =
                                            factory5.CreateFontCollectionFromFontSet(&font_set)
                                        {
                                            if let Ok(collection) =
                                                collection1.cast::<IDWriteFontCollection>()
                                            {
                                                let fam_w = to_wide("Material Icons");
                                                let locale_w = to_wide("en-US");
                                                if let Ok(fmt) = dwrite_factory.CreateTextFormat(
                                                    PCWSTR(fam_w.as_ptr()),
                                                    &collection,
                                                    DWRITE_FONT_WEIGHT_REGULAR,
                                                    DWRITE_FONT_STYLE_NORMAL,
                                                    DWRITE_FONT_STRETCH_NORMAL,
                                                    16.0,
                                                    PCWSTR(locale_w.as_ptr()),
                                                ) {
                                                    let _ = fmt.SetTextAlignment(
                                                        DWRITE_TEXT_ALIGNMENT_CENTER,
                                                    );
                                                    let _ = fmt.SetParagraphAlignment(
                                                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                                                    );
                                                    let _ = fmt.SetWordWrapping(
                                                        DWRITE_WORD_WRAPPING_NO_WRAP,
                                                    );
                                                    font_material_icon = Some(fmt);
                                                    icon_font_loader = Some(loader);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let mem_dc = CreateCompatibleDC(HDC::default());

            Ok(Self {
                d2d_factory,
                dwrite_factory,
                target: None,
                brush: None,
                mem_dc,
                dib_bitmap: HBITMAP::default(),
                old_obj: HGDIOBJ::default(),
                dib_bits: std::ptr::null_mut(),
                width_px: 0,
                height_px: 0,
                font_title_lg,
                font_title_md,
                font_subtitle,
                font_body,
                font_body_bold,
                font_caption,
                font_small,
                font_mono,
                font_material_icon,
                icon_font_loader,
                dpi_scale: 1.0,
                icon_cache: RefCell::new(Vec::new()),
            })
        }
    }

    fn free_dib(&mut self) {
        unsafe {
            if !self.dib_bitmap.0.is_null() {
                if !self.old_obj.0.is_null() {
                    SelectObject(self.mem_dc, self.old_obj);
                    self.old_obj = HGDIOBJ::default();
                }
                let _ = DeleteObject(self.dib_bitmap);
                self.dib_bitmap = HBITMAP::default();
                self.dib_bits = std::ptr::null_mut();
            }
        }
    }

    pub fn ensure_surface(&mut self, width_px: u32, height_px: u32, dpi: u32) -> Result<(), String> {
        let w = width_px.max(1);
        let h = height_px.max(1);
        self.dpi_scale = (dpi as f32) / 96.0;

        unsafe {
            if self.dib_bitmap.0.is_null() || self.width_px != w || self.height_px != h {
                self.free_dib();
                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w as i32,
                        biHeight: -(h as i32), // top-down DIB
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
                let hbm = CreateDIBSection(
                    self.mem_dc,
                    &bmi,
                    DIB_RGB_COLORS,
                    &mut bits_ptr,
                    None,
                    0,
                )
                .map_err(|e| format!("CreateDIBSection 失败: {}", e))?;

                self.old_obj = SelectObject(self.mem_dc, hbm);
                self.dib_bitmap = hbm;
                self.dib_bits = bits_ptr as *mut u8;
                self.width_px = w;
                self.height_px = h;
            }

            if self.target.is_none() {
                let render_props = D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_IGNORE,
                    },
                    dpiX: dpi as f32,
                    dpiY: dpi as f32,
                    usage: D2D1_RENDER_TARGET_USAGE_NONE,
                    minLevel: Default::default(),
                };
                let _ = D2D1_ALPHA_MODE_PREMULTIPLIED;
                let _ = D2D_SIZE_U::default();

                let rt = self
                    .d2d_factory
                    .CreateDCRenderTarget(&render_props)
                    .map_err(|e| format!("CreateDCRenderTarget 失败: {}", e))?;

                rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

                let brush = rt
                    .CreateSolidColorBrush(&rgb(0xFFFFFF), None)
                    .map_err(|e| format!("CreateSolidColorBrush 失败: {}", e))?;

                self.target = Some(rt);
                self.brush = Some(brush);
            }

            if let Some(ref rt) = self.target {
                rt.SetDpi(dpi as f32, dpi as f32);
                let rc = RECT {
                    left: 0,
                    top: 0,
                    right: w as i32,
                    bottom: h as i32,
                };
                rt.BindDC(self.mem_dc, &rc)
                    .map_err(|e| format!("BindDC 失败: {}", e))?;
            }
        }
        Ok(())
    }

    pub fn ensure_target(&mut self, hwnd: HWND, dpi: u32) -> Result<(), String> {
        unsafe {
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let width = (rc.right - rc.left).max(1) as u32;
            let height = (rc.bottom - rc.top).max(1) as u32;
            self.ensure_surface(width, height, dpi)
        }
    }

    pub fn resize(&mut self, width_px: u32, height_px: u32, dpi: u32) {
        let _ = self.ensure_surface(width_px, height_px, dpi);
    }

    pub fn blit_to_hdc(&self, target_hdc: HDC) {
        if !target_hdc.0.is_null() && !self.mem_dc.0.is_null() {
            unsafe {
                let _ = BitBlt(
                    target_hdc,
                    0,
                    0,
                    self.width_px as i32,
                    self.height_px as i32,
                    self.mem_dc,
                    0,
                    0,
                    SRCCOPY,
                );
            }
        }
    }

    /// Save the current rendered DIB frame to a 24-bit or 32-bit BMP file for visual verification.
    pub fn save_bmp(&self, path: &Path) -> std::io::Result<()> {
        if self.dib_bits.is_null() || self.width_px == 0 || self.height_px == 0 {
            return Ok(());
        }
        let w = self.width_px;
        let h = self.height_px;
        let data_size = (w * h * 4) as usize;
        let file_size = 54 + data_size as u32;

        let mut f = File::create(path)?;
        let mut header = [0u8; 54];
        header[0] = b'B';
        header[1] = b'M';
        header[2..6].copy_from_slice(&file_size.to_le_bytes());
        header[10..14].copy_from_slice(&54u32.to_le_bytes());
        header[14..18].copy_from_slice(&40u32.to_le_bytes());
        header[18..22].copy_from_slice(&(w as i32).to_le_bytes());
        header[22..26].copy_from_slice(&(-(h as i32)).to_le_bytes()); // top-down
        header[26..28].copy_from_slice(&1u16.to_le_bytes());
        header[28..30].copy_from_slice(&32u16.to_le_bytes());
        header[34..38].copy_from_slice(&(data_size as u32).to_le_bytes());

        f.write_all(&header)?;
        let pixels = unsafe { std::slice::from_raw_parts(self.dib_bits, data_size) };
        f.write_all(pixels)?;
        Ok(())
    }

    pub fn discard_target(&mut self) {
        self.brush = None;
        self.icon_cache.borrow_mut().clear();
        self.target = None;
    }

    pub fn brush(&self, color: D2D1_COLOR_F) -> &ID2D1SolidColorBrush {
        let b = self.brush.as_ref().expect("Brush initialized");
        unsafe {
            b.SetColor(&color);
        }
        b
    }

    pub fn rt(&self) -> &ID2D1DCRenderTarget {
        self.target.as_ref().expect("RenderTarget initialized")
    }

    /// Measure single-line text width in DIPs using DirectWrite `IDWriteTextLayout`.
    pub fn measure_text_width(&self, text: &str, fmt: &IDWriteTextFormat) -> f32 {
        unsafe {
            let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let no_trim = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_NONE,
                delimiter: 0,
                delimiterCount: 0,
            };
            let _ = fmt.SetTrimming(&no_trim, None);
            let wide: Vec<u16> = text.encode_utf16().collect();
            if let Ok(layout) = self.dwrite_factory.CreateTextLayout(&wide, fmt, 8192.0, 200.0) {
                let mut metrics = DWRITE_TEXT_METRICS::default();
                if layout.GetMetrics(&mut metrics).is_ok() {
                    return metrics.widthIncludingTrailingWhitespace;
                }
            }
            (text.chars().count() as f32) * 10.0
        }
    }

    /// Wrap log lines so that any line exceeding `max_width` is split into multiple visual lines
    /// that fit strictly inside `max_width`.
    pub fn wrap_log_lines(
        &self,
        logs: &[String],
        fmt: &IDWriteTextFormat,
        max_width: f32,
    ) -> Vec<(String, String)> {
        let mut visual_lines = Vec::with_capacity(logs.len());
        let safe_w = (max_width - 6.0).max(80.0);

        for raw_line in logs {
            if self.measure_text_width(raw_line, fmt) <= safe_w {
                visual_lines.push((raw_line.clone(), raw_line.clone()));
                continue;
            }

            let mut current = String::new();
            let mut is_first = true;
            for ch in raw_line.chars() {
                current.push(ch);
                if self.measure_text_width(&current, fmt) > safe_w && current.chars().count() > 4 {
                    let popped = current.pop().unwrap();
                    visual_lines.push((raw_line.clone(), current));
                    current = if is_first {
                        is_first = false;
                        format!("    {}", popped)
                    } else {
                        format!("    {}", popped)
                    };
                }
            }
            if !current.is_empty() {
                visual_lines.push((raw_line.clone(), current));
            }
        }
        visual_lines
    }

    pub fn fill_rect(&self, rect: RectF, color: D2D1_COLOR_F) {
        unsafe {
            self.rt().FillRectangle(&rect.to_d2d(), self.brush(color));
        }
    }

    pub fn fill_rounded_rect(&self, rect: RectF, radius: f32, color: D2D1_COLOR_F) {
        unsafe {
            self.rt()
                .FillRoundedRectangle(&rect.to_rounded(radius), self.brush(color));
        }
    }

    pub fn draw_rounded_rect(&self, rect: RectF, radius: f32, stroke: f32, color: D2D1_COLOR_F) {
        unsafe {
            self.rt().DrawRoundedRectangle(
                &rect.to_rounded(radius),
                self.brush(color),
                stroke,
                None,
            );
        }
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, r: f32, color: D2D1_COLOR_F) {
        unsafe {
            let ellipse = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: cx, y: cy },
                radiusX: r,
                radiusY: r,
            };
            self.rt().FillEllipse(&ellipse, self.brush(color));
        }
    }

    pub fn draw_circle(&self, cx: f32, cy: f32, r: f32, stroke: f32, color: D2D1_COLOR_F) {
        unsafe {
            let ellipse = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: cx, y: cy },
                radiusX: r,
                radiusY: r,
            };
            self.rt()
                .DrawEllipse(&ellipse, self.brush(color), stroke, None);
        }
    }

    pub fn draw_line(&self, x1: f32, y1: f32, x2: f32, y2: f32, stroke: f32, color: D2D1_COLOR_F) {
        unsafe {
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                ..Default::default()
            };
            let style = self.d2d_factory.CreateStrokeStyle(&props, None).ok();
            self.rt().DrawLine(
                D2D_POINT_2F { x: x1, y: y1 },
                D2D_POINT_2F { x: x2, y: y2 },
                self.brush(color),
                stroke,
                style.as_ref(),
            );
        }
    }

    /// Truncate single-line text and append `"..."` if its rendered width exceeds `max_width`.
    pub fn ellipsize_text(&self, text: &str, fmt: &IDWriteTextFormat, max_width: f32) -> String {
        let limit = (max_width - 2.0).max(0.0);
        if self.measure_text_width(text, fmt) <= limit {
            return text.to_string();
        }
        let ellipsis = "...";
        if self.measure_text_width(ellipsis, fmt) >= limit {
            return ellipsis.to_string();
        }
        let char_indices: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let mut low = 0usize;
        let mut high = char_indices.len();
        while low < high {
            let mid = (low + high + 1) / 2;
            let byte_end = if mid < char_indices.len() {
                char_indices[mid]
            } else {
                text.len()
            };
            let candidate = format!("{}{}", &text[..byte_end], ellipsis);
            if self.measure_text_width(&candidate, fmt) <= limit {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        let byte_end = if low < char_indices.len() {
            char_indices[low]
        } else {
            text.len()
        };
        format!("{}{}", text[..byte_end].trim_end(), ellipsis)
    }

    /// Draw single-line text preserving `fmt` size/weight and formatting overflow at the end as `"..."`.
    pub fn draw_text_ellipsis(
        &self,
        text: &str,
        fmt: &IDWriteTextFormat,
        rect: RectF,
        color: D2D1_COLOR_F,
        h_align: i32,
        v_align: i32,
    ) {
        unsafe {
            let display = if rect.w > 8.0 {
                self.ellipsize_text(text, fmt, rect.w)
            } else {
                text.to_string()
            };
            let _ = fmt.SetTextAlignment(match h_align {
                1 => DWRITE_TEXT_ALIGNMENT_CENTER,
                2 => DWRITE_TEXT_ALIGNMENT_TRAILING,
                _ => DWRITE_TEXT_ALIGNMENT_LEADING,
            });
            let _ = fmt.SetParagraphAlignment(match v_align {
                1 => DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                _ => DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            });
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let no_trim = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_NONE,
                delimiter: 0,
                delimiterCount: 0,
            };
            let _ = fmt.SetTrimming(&no_trim, None);

            let wide: Vec<u16> = display.encode_utf16().collect();
            self.rt().DrawText(
                &wide,
                fmt,
                &rect.to_d2d(),
                self.brush(color),
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }
    }

    pub fn draw_text(
        &self,
        text: &str,
        fmt: &IDWriteTextFormat,
        rect: RectF,
        color: D2D1_COLOR_F,
        h_align: i32,
        v_align: i32,
        wrap: bool,
    ) {
        unsafe {
            // Auto-shrink single-line body/caption text so it fits inside rect.w without clipping
            let mut active_fmt = fmt;
            if !wrap && rect.w > 8.0 {
                if std::ptr::eq(fmt, &self.font_body) || std::ptr::eq(fmt, &self.font_body_bold) {
                    if self.measure_text_width(text, fmt) > rect.w - 1.0 {
                        active_fmt = &self.font_caption;
                        if self.measure_text_width(text, active_fmt) > rect.w - 1.0 {
                            active_fmt = &self.font_small;
                        }
                    }
                } else if std::ptr::eq(fmt, &self.font_caption)
                    && self.measure_text_width(text, fmt) > rect.w - 1.0
                {
                    active_fmt = &self.font_small;
                }
            }

            let display_cow = if !wrap
                && rect.w > 8.0
                && self.measure_text_width(text, active_fmt) > rect.w - 1.0
            {
                std::borrow::Cow::Owned(self.ellipsize_text(text, active_fmt, rect.w))
            } else {
                std::borrow::Cow::Borrowed(text)
            };

            let _ = active_fmt.SetTextAlignment(match h_align {
                1 => DWRITE_TEXT_ALIGNMENT_CENTER,
                2 => DWRITE_TEXT_ALIGNMENT_TRAILING,
                _ => DWRITE_TEXT_ALIGNMENT_LEADING,
            });
            let _ = active_fmt.SetParagraphAlignment(match v_align {
                1 => DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                _ => DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            });
            if wrap {
                let _ = active_fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_EMERGENCY_BREAK);
            } else {
                let _ = active_fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            }
            let no_trim = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_NONE,
                delimiter: 0,
                delimiterCount: 0,
            };
            let _ = active_fmt.SetTrimming(&no_trim, None);

            let wide: Vec<u16> = display_cow.encode_utf16().collect();
            self.rt().DrawText(
                &wide,
                active_fmt,
                &rect.to_d2d(),
                self.brush(color),
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }
    }

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

    /// Draw crisp Google MaterialIcons using the embedded `material-icons` crate font (`MaterialIcons-Regular.ttf`)
    /// with Direct2D `ID2D1PathGeometry` as fallback.
    pub fn draw_icon(&self, icon: VectorIcon, cx: f32, cy: f32, size: f32, color: D2D1_COLOR_F) {
        if let Some(ref fmt) = self.font_material_icon {
            let ch = icon.material_char();
            let mut buf = [0u16; 2];
            let wide = ch.encode_utf16(&mut buf);
            let box_size = size * 1.6;
            unsafe {
                if let Ok(layout) =
                    self.dwrite_factory
                        .CreateTextLayout(wide, fmt, box_size, box_size)
                {
                    let _ = layout.SetFontSize(
                        size * 1.18,
                        DWRITE_TEXT_RANGE {
                            startPosition: 0,
                            length: wide.len() as u32,
                        },
                    );
                    self.rt().DrawTextLayout(
                        D2D_POINT_2F {
                            x: cx - box_size * 0.5,
                            y: cy - box_size * 0.5,
                        },
                        &layout,
                        self.brush(color),
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                    return;
                }
            }
        }

        if let Ok(path) = build_material_icon_geometry(
            &self.d2d_factory,
            icon.material_svg_path(),
            cx,
            cy,
            size,
        ) {
            unsafe {
                self.rt().FillGeometry(&path, self.brush(color), None);
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
            let use_caption = icon_size + gap + body_w > rect.w - 6.0;
            let fmt = if use_caption {
                &self.font_caption
            } else {
                &self.font_body_bold
            };
            let text_w = self.measure_text_width(label, fmt);
            let total_w = (icon_size + gap + text_w).min(rect.w - 4.0);
            let start_x = rect.x + (rect.w - total_w) * 0.5;
            let icon_cx = start_x + icon_size * 0.5;
            let icon_cy = rect.y + rect.h * 0.5;

            self.draw_icon(ic, icon_cx, icon_cy, icon_size, fg);
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
        let clamped_caret = caret_pos.min(char_len);
        let prefix_b = TextInputState::byte_index_of_char(value, clamped_caret);
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
        let default_sel = TextInputState {
            caret_pos: TextInputState::char_len(value),
            sel_anchor: TextInputState::char_len(value),
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
        let label_rect = RectF::new(
            rect.x + 2.0,
            rect.y - 20.0,
            (rect.w - 2.0).max(0.0),
            18.0,
        );
        let label_color = if focused {
            palette.primary
        } else {
            palette.text_secondary
        };
        self.draw_text(
            label,
            &self.font_caption,
            label_rect,
            label_color,
            0,
            1,
            false,
        );

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

        // 1. Draw selection highlight if focused and has selection
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

        // 2. Draw text content without ellipsis trimming
        let total_w = self.measure_text_width(value, &self.font_body);
        let draw_rect = RectF::new(
            inner.x - scroll_x,
            inner.y,
            (total_w + 40.0).max(inner.w),
            inner.h,
        );
        unsafe {
            let _ = self
                .font_body
                .SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
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

        // 3. Draw vertical caret bar at caret_pos
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
            let label_rect = RectF::new(
                rect.x + 2.0,
                rect.y - 20.0,
                (rect.w - 2.0).max(0.0),
                18.0,
            );
            let label_color = if is_open {
                palette.primary
            } else {
                palette.text_secondary
            };
            self.draw_text(
                label,
                &self.font_caption,
                label_rect,
                label_color,
                0,
                1,
                false,
            );
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
        let mut max_item_w = 0.0f32;
        for item in items {
            let w = self.measure_text_width(item, &self.font_body);
            if w > max_item_w {
                max_item_w = w;
            }
        }
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

        // Elevated drop shadow
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
                self.draw_icon(
                    VectorIcon::Check,
                    ir.x + 14.0,
                    ir.y + item_h * 0.5,
                    12.0,
                    palette.primary,
                );
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
            let t_rect = RectF::new(text_x, rect.y + 5.0, text_w, 19.0);
            self.draw_text(
                title,
                &self.font_body_bold,
                t_rect,
                palette.text_primary,
                0,
                1,
                false,
            );
            let s_rect = RectF::new(text_x, rect.y + 24.0, text_w, 17.0);
            self.draw_text(
                sub,
                &self.font_caption,
                s_rect,
                palette.text_secondary,
                0,
                1,
                false,
            );
        } else {
            let t_rect = RectF::new(text_x, rect.y, text_w, rect.h);
            self.draw_text(
                title,
                &self.font_body,
                t_rect,
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

    /// Create a Direct2D bitmap from a decoded icon image.
    pub fn create_bitmap_from_icon(&self, img: &icon::IconImage) -> Option<ID2D1Bitmap> {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        unsafe {
            self.rt()
                .CreateBitmap(
                    D2D_SIZE_U {
                        width: img.size,
                        height: img.size,
                    },
                    Some(img.bgra.as_ptr() as *const _),
                    img.size * 4,
                    &props,
                )
                .ok()
        }
    }

    /// Lazily create (and cache) a Direct2D bitmap for the given frame size of the
    /// embedded `resources/app.ico` - the very same icon used as the exe icon.
    fn app_icon_bitmap(&self, size: u32) -> Option<(ID2D1Bitmap, f32)> {
        if let Some((_, bmp, scale)) = self
            .icon_cache
            .borrow()
            .iter()
            .find(|(s, _, _)| *s == size)
        {
            return Some((bmp.clone(), *scale));
        }
        let img = icon::decode(size)?;
        let art_scale = img.art_scale;
        let bmp = self.create_bitmap_from_icon(&img)?;
        self.icon_cache
            .borrow_mut()
            .push((size, bmp.clone(), art_scale));
        Some((bmp, art_scale))
    }

    /// Draw the application icon (`resources/app.ico`) into `rect`.
    /// The artwork inside the icon canvas is scaled by `art_scale` so it nicely fills `rect`.
    pub fn draw_nyan_app_icon(&self, rect: RectF) {
        let size = icon::pick_size(rect.w * self.dpi_scale);
        match self.app_icon_bitmap(size) {
            Some((bmp, art_scale)) => {
                let dw = rect.w * art_scale;
                let dh = rect.h * art_scale;
                let dest = RectF::new(
                    rect.x + (rect.w - dw) * 0.5,
                    rect.y + (rect.h - dh) * 0.5,
                    dw,
                    dh,
                );
                unsafe {
                    self.rt().DrawBitmap(
                        &bmp,
                        Some(&dest.to_d2d()),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                }
            }
            None => self.fill_rounded_rect(rect, rect.h * 0.23, rgb(0x4F46E5)),
        }
    }

    /// Draw the application icon, with optional custom `.ico` file override (for Studio live preview).
    pub fn draw_app_icon_with_override(&self, rect: RectF, custom_ico_path: Option<&str>) {
        if let Some(path_str) = custom_ico_path {
            let path = Path::new(path_str.trim());
            if path.is_file() {
                if let Ok(bytes) = std::fs::read(path) {
                    let size = icon::pick_size(rect.w * self.dpi_scale);
                    if let Some(img) = icon::decode_ico(&bytes, size) {
                        if let Some(bmp) = self.create_bitmap_from_icon(&img) {
                            let dw = rect.w * img.art_scale;
                            let dh = rect.h * img.art_scale;
                            let dest = RectF::new(
                                rect.x + (rect.w - dw) * 0.5,
                                rect.y + (rect.h - dh) * 0.5,
                                dw,
                                dh,
                            );
                            unsafe {
                                self.rt().DrawBitmap(
                                    &bmp,
                                    Some(&dest.to_d2d()),
                                    1.0,
                                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                    None,
                                );
                            }
                            return;
                        }
                    }
                }
            }
        }
        self.draw_nyan_app_icon(rect);
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
        let logo_rect = RectF::new(14.0, 9.0, 28.0, 28.0);
        self.draw_nyan_app_icon(logo_rect);

        let title_w = self
            .measure_text_width(title, &self.font_body_bold)
            .min(width - 280.0);
        let title_rect = RectF::new(50.0, 0.0, title_w + 6.0, header_h);
        self.draw_text(
            title,
            &self.font_body_bold,
            title_rect,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_draw_titlebar_header_renders() {
        let mut d2d = D2DRenderer::new().expect("D2D Init");
        d2d.resize(800, 46, 96);
        let pal = MaterialPalette::dark();
        unsafe {
            d2d.rt().BeginDraw();
            d2d.rt().Clear(Some(&pal.bg));
            d2d.draw_window_header(
                800.0,
                "Nyan Studio App v1.0.0 安装向导",
                "",
                &pal,
                false,
                false,
                false,
            );
            let _ = d2d.rt().EndDraw(None, None);
        }
        let out_dir = std::path::Path::new("target");
        let out_bmp = out_dir.join("test_titlebar.bmp");
        let _ = d2d.save_bmp(&out_bmp);
        assert!(out_bmp.exists());
    }

    #[test]
    fn test_text_input_selection_and_editing() {
        let mut s = "Hello 世界".to_string();
        let mut st = TextInputState::default();
        st.select_all(&s);
        assert!(st.has_selection());
        assert_eq!(st.selected_text(&s), Some("Hello 世界"));

        st.insert_str(&mut s, "Nyan");
        assert_eq!(s, "Nyan");
        assert!(!st.has_selection());
        assert_eq!(st.caret_pos, 4);

        st.move_left(&s, true);
        st.move_left(&s, true);
        assert_eq!(st.selected_text(&s), Some("an"));

        st.backspace(&mut s);
        assert_eq!(s, "Ny");
    }

    #[test]
    fn test_material_icons_svg_geometry_renders_all() {
        let mut d2d = D2DRenderer::new().expect("D2D Init");
        assert!(
            d2d.font_material_icon.is_some(),
            "In-memory MaterialIcons-Regular.ttf from material-icons crate should load into DirectWrite"
        );
        d2d.resize(512, 64, 96);
        let pal = MaterialPalette::light();
        let all_icons = [
            VectorIcon::Info,
            VectorIcon::Folder,
            VectorIcon::Document,
            VectorIcon::Rocket,
            VectorIcon::Check,
            VectorIcon::Shield,
            VectorIcon::HardDrive,
            VectorIcon::Trash,
            VectorIcon::Sun,
            VectorIcon::Moon,
            VectorIcon::Minimize,
            VectorIcon::Close,
            VectorIcon::Sparkles,
            VectorIcon::Play,
            VectorIcon::ChevronDown,
            VectorIcon::ChevronUp,
        ];
        unsafe {
            d2d.rt().BeginDraw();
            d2d.rt().Clear(Some(&pal.surface));
            for (i, icon) in all_icons.iter().enumerate() {
                assert!(!icon.material_name().is_empty());
                assert!(icon.material_char() as u32 >= 0xE000);
                let geom = build_material_icon_geometry(
                    &d2d.d2d_factory,
                    icon.material_svg_path(),
                    20.0 + (i as f32) * 30.0,
                    32.0,
                    20.0,
                );
                assert!(geom.is_ok(), "Failed to build MaterialIcon {:?}", icon);
                d2d.draw_icon(*icon, 20.0 + (i as f32) * 30.0, 32.0, 20.0, pal.primary);
            }
            let _ = d2d.rt().EndDraw(None, None);
        }
    }
}
