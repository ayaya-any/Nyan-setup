use crate::icon;
use crate::win32_ops::to_wide;
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LINE_JOIN_ROUND,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_TEXT_METRICS, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_TRIMMING_GRANULARITY_NONE, DWRITE_WORD_WRAPPING_EMERGENCY_BREAK,
    DWRITE_WORD_WRAPPING_NO_WRAP,
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
    pub font_mono: IDWriteTextFormat,
    pub dpi_scale: f32,
    icon_cache: RefCell<Vec<(u32, ID2D1Bitmap, f32)>>,
}

impl Drop for D2DRenderer {
    fn drop(&mut self) {
        self.free_dib();
        unsafe {
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
            let font_mono = create_fmt("Consolas", 12.0, DWRITE_FONT_WEIGHT_REGULAR)?;

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
                font_mono,
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
            let _ = fmt.SetTextAlignment(match h_align {
                1 => DWRITE_TEXT_ALIGNMENT_CENTER,
                2 => DWRITE_TEXT_ALIGNMENT_TRAILING,
                _ => DWRITE_TEXT_ALIGNMENT_LEADING,
            });
            let _ = fmt.SetParagraphAlignment(match v_align {
                1 => DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                _ => DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            });
            if wrap {
                let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_EMERGENCY_BREAK);
                let no_trim = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_NONE,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                let _ = fmt.SetTrimming(&no_trim, None);
            } else {
                let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                let ellipsis = self.dwrite_factory.CreateEllipsisTrimmingSign(fmt).ok();
                let trim = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                let _ = fmt.SetTrimming(&trim, ellipsis.as_ref());
            }

            let wide: Vec<u16> = text.encode_utf16().collect();
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

    /// Draw crisp vector icons using Direct2D primitives and PathGeometry.
    pub fn draw_icon(&self, icon: VectorIcon, cx: f32, cy: f32, size: f32, color: D2D1_COLOR_F) {
        let half = size * 0.5;
        match icon {
            VectorIcon::Check => {
                self.draw_line(
                    cx - half * 0.55,
                    cy + half * 0.05,
                    cx - half * 0.12,
                    cy + half * 0.48,
                    2.2,
                    color,
                );
                self.draw_line(
                    cx - half * 0.12,
                    cy + half * 0.48,
                    cx + half * 0.62,
                    cy - half * 0.42,
                    2.2,
                    color,
                );
            }
            VectorIcon::Folder => {
                let r = RectF::new(
                    cx - half * 0.8,
                    cy - half * 0.45,
                    size * 0.8,
                    size * 0.58,
                );
                let tab = RectF::new(
                    cx - half * 0.8,
                    cy - half * 0.68,
                    size * 0.36,
                    size * 0.28,
                );
                self.fill_rounded_rect(tab, 2.0, color);
                self.draw_rounded_rect(r, 2.5, 1.8, color);
            }
            VectorIcon::Document => {
                let r = RectF::new(
                    cx - half * 0.62,
                    cy - half * 0.78,
                    size * 0.62,
                    size * 0.78,
                );
                self.draw_rounded_rect(r, 2.5, 1.7, color);
                self.draw_line(
                    cx - half * 0.35,
                    cy - half * 0.25,
                    cx + half * 0.35,
                    cy - half * 0.25,
                    1.5,
                    color,
                );
                self.draw_line(
                    cx - half * 0.35,
                    cy + half * 0.08,
                    cx + half * 0.35,
                    cy + half * 0.08,
                    1.5,
                    color,
                );
                self.draw_line(
                    cx - half * 0.35,
                    cy + half * 0.40,
                    cx + half * 0.15,
                    cy + half * 0.40,
                    1.5,
                    color,
                );
            }
            VectorIcon::Info => {
                self.draw_circle(cx, cy, half * 0.78, 1.8, color);
                self.fill_circle(cx, cy - half * 0.36, 1.5, color);
                self.draw_line(cx, cy - half * 0.08, cx, cy + half * 0.44, 2.0, color);
            }
            VectorIcon::Shield => {
                unsafe {
                    if let Ok(path) = self.d2d_factory.CreatePathGeometry() {
                        if let Ok(sink) = path.Open() {
                            sink.BeginFigure(
                                D2D_POINT_2F {
                                    x: cx,
                                    y: cy - half * 0.82,
                                },
                                D2D1_FIGURE_BEGIN_HOLLOW,
                            );
                            let pts = [
                                D2D_POINT_2F {
                                    x: cx + half * 0.72,
                                    y: cy - half * 0.52,
                                },
                                D2D_POINT_2F {
                                    x: cx + half * 0.64,
                                    y: cy + half * 0.18,
                                },
                                D2D_POINT_2F {
                                    x: cx,
                                    y: cy + half * 0.82,
                                },
                                D2D_POINT_2F {
                                    x: cx - half * 0.64,
                                    y: cy + half * 0.18,
                                },
                                D2D_POINT_2F {
                                    x: cx - half * 0.72,
                                    y: cy - half * 0.52,
                                },
                            ];
                            sink.AddLines(&pts);
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            let _ = sink.Close();
                            self.rt().DrawGeometry(&path, self.brush(color), 1.8, None);
                        }
                    }
                }
            }
            VectorIcon::Rocket => {
                self.draw_circle(cx, cy - half * 0.1, half * 0.45, 1.8, color);
                self.draw_line(
                    cx,
                    cy - half * 0.85,
                    cx - half * 0.48,
                    cy + half * 0.45,
                    1.8,
                    color,
                );
                self.draw_line(
                    cx,
                    cy - half * 0.85,
                    cx + half * 0.48,
                    cy + half * 0.45,
                    1.8,
                    color,
                );
                self.draw_line(
                    cx - half * 0.48,
                    cy + half * 0.45,
                    cx + half * 0.48,
                    cy + half * 0.45,
                    1.8,
                    color,
                );
                self.draw_line(cx, cy + half * 0.45, cx, cy + half * 0.85, 2.2, color);
            }
            VectorIcon::HardDrive => {
                let r = RectF::new(
                    cx - half * 0.82,
                    cy - half * 0.48,
                    size * 0.82,
                    size * 0.48,
                );
                self.draw_rounded_rect(r, 3.0, 1.8, color);
                self.fill_circle(cx + half * 0.45, cy, 2.0, color);
                self.draw_line(cx - half * 0.52, cy, cx + half * 0.12, cy, 1.8, color);
            }
            VectorIcon::Trash => {
                let body = RectF::new(
                    cx - half * 0.52,
                    cy - half * 0.38,
                    size * 0.52,
                    size * 0.62,
                );
                self.draw_rounded_rect(body, 2.0, 1.7, color);
                self.draw_line(
                    cx - half * 0.72,
                    cy - half * 0.42,
                    cx + half * 0.72,
                    cy - half * 0.42,
                    1.8,
                    color,
                );
                self.draw_line(
                    cx - half * 0.25,
                    cy - half * 0.65,
                    cx + half * 0.25,
                    cy - half * 0.65,
                    1.8,
                    color,
                );
            }
            VectorIcon::Sun => {
                self.draw_circle(cx, cy, half * 0.42, 1.8, color);
                for i in 0..8 {
                    let angle = (i as f32) * std::f32::consts::PI / 4.0;
                    let x1 = cx + angle.cos() * half * 0.62;
                    let y1 = cy + angle.sin() * half * 0.62;
                    let x2 = cx + angle.cos() * half * 0.86;
                    let y2 = cy + angle.sin() * half * 0.86;
                    self.draw_line(x1, y1, x2, y2, 1.6, color);
                }
            }
            VectorIcon::Moon => {
                self.draw_circle(cx, cy, half * 0.62, 1.8, color);
                self.fill_circle(cx + half * 0.25, cy - half * 0.2, half * 0.42, color);
            }
            VectorIcon::Minimize => {
                self.draw_line(cx - 5.5, cy, cx + 5.5, cy, 1.8, color);
            }
            VectorIcon::Close => {
                self.draw_line(cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0, 1.8, color);
                self.draw_line(cx + 5.0, cy - 5.0, cx - 5.0, cy + 5.0, 1.8, color);
            }
            VectorIcon::Sparkles => {
                self.draw_line(cx, cy - half * 0.75, cx, cy + half * 0.75, 1.8, color);
                self.draw_line(cx - half * 0.75, cy, cx + half * 0.75, cy, 1.8, color);
                self.draw_line(
                    cx - half * 0.42,
                    cy - half * 0.42,
                    cx + half * 0.42,
                    cy + half * 0.42,
                    1.4,
                    color,
                );
                self.draw_line(
                    cx + half * 0.42,
                    cy - half * 0.42,
                    cx - half * 0.42,
                    cy + half * 0.42,
                    1.4,
                    color,
                );
            }
            VectorIcon::Play => {
                unsafe {
                    if let Ok(path) = self.d2d_factory.CreatePathGeometry() {
                        if let Ok(sink) = path.Open() {
                            sink.BeginFigure(
                                D2D_POINT_2F {
                                    x: cx - half * 0.4,
                                    y: cy - half * 0.58,
                                },
                                D2D1_FIGURE_BEGIN_FILLED,
                            );
                            let pts = [
                                D2D_POINT_2F {
                                    x: cx + half * 0.58,
                                    y: cy,
                                },
                                D2D_POINT_2F {
                                    x: cx - half * 0.4,
                                    y: cy + half * 0.58,
                                },
                            ];
                            sink.AddLines(&pts);
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            let _ = sink.Close();
                            self.rt().FillGeometry(&path, self.brush(color), None);
                        }
                    }
                }
            }
        }
        let _ = D2D1_BEZIER_SEGMENT::default();
        let _ = D2D1_FIGURE_END_OPEN;
        let _ = D2D_SIZE_F::default();
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
            let text_w = self.measure_text_width(label, &self.font_body_bold);
            let icon_size = 15.0;
            let gap = 7.0;
            let total_w = (icon_size + gap + text_w).min(rect.w - 12.0);
            let start_x = rect.x + (rect.w - total_w) * 0.5;
            let icon_cx = start_x + icon_size * 0.5;
            let icon_cy = rect.y + rect.h * 0.5;

            self.draw_icon(ic, icon_cx, icon_cy, icon_size, fg);
            let text_rect = RectF::new(
                start_x + icon_size + gap,
                rect.y,
                (rect.x + rect.w - (start_x + icon_size + gap) - 4.0).max(10.0),
                rect.h,
            );
            self.draw_text(label, &self.font_body_bold, text_rect, fg, 0, 1, false);
        } else {
            self.draw_text(label, &self.font_body_bold, rect, fg, 1, 1, false);
        }
    }

    /// Draw a Material Outlined TextField with label above and focus highlight.
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
        let label_rect = RectF::new(rect.x + 2.0, rect.y - 20.0, rect.w, 18.0);
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

        let inner = rect.inset(12.0, 4.0);
        unsafe {
            self.rt()
                .PushAxisAlignedClip(&inner.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }

        let display_str = if focused && caret_visible {
            format!("{}|", value)
        } else {
            value.to_string()
        };

        let text_w = self.measure_text_width(&display_str, &self.font_body);
        let draw_rect = if focused && text_w > inner.w {
            RectF::new(inner.x + inner.w - text_w - 4.0, inner.y, text_w + 12.0, inner.h)
        } else {
            inner
        };

        self.draw_text(
            &display_str,
            &self.font_body,
            draw_rect,
            palette.text_primary,
            0,
            1,
            false,
        );

        unsafe {
            self.rt().PopAxisAlignedClip();
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
                "CRT-Static",
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
}
