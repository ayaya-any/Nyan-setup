pub mod icons;
pub mod palette;
pub mod primitives;
pub mod widgets;

#[cfg(test)]
mod tests;

pub use icons::*;
pub use palette::*;
pub use primitives::*;
pub use widgets::*;

use crate::win32_ops::to_wide;
use material_icons::FONT as MATERIAL_ICONS_FONT;
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LINE_JOIN_ROUND,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_STROKE_STYLE_PROPERTIES,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFactory5, IDWriteFontCollection,
    IDWriteInMemoryFontFileLoader, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_TEXT_METRICS, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_NONE,
    DWRITE_WORD_WRAPPING_EMERGENCY_BREAK, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

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
    pub(crate) icon_cache: RefCell<Vec<(u32, ID2D1Bitmap, f32)>>,
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

    /// Save the current rendered DIB frame to a 32-bit BMP file for visual verification.
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
        header[22..26].copy_from_slice(&(-(h as i32)).to_le_bytes());
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

    /// Wrap log lines so that any line exceeding `max_width` is split into multiple visual lines.
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
            for ch in raw_line.chars() {
                current.push(ch);
                if self.measure_text_width(&current, fmt) > safe_w && current.chars().count() > 4 {
                    let popped = current.pop().unwrap();
                    visual_lines.push((raw_line.clone(), current));
                    current = format!("    {}", popped);
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
            let byte_end = char_indices.get(mid).copied().unwrap_or(text.len());
            let candidate = format!("{}{}", &text[..byte_end], ellipsis);
            if self.measure_text_width(&candidate, fmt) <= limit {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        let byte_end = char_indices.get(low).copied().unwrap_or(text.len());
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
}
