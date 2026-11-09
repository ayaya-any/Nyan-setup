use crate::d2d::{rgb, D2DRenderer, RectF};
use crate::icon;
use material_icons::{icon_to_char, Icon as MaterialIcon};
use std::path::Path;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING, D2D1_PIXEL_FORMAT,
    D2D_POINT_2F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1Factory, ID2D1PathGeometry, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE,
};
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

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
                    let (Some(x), Some(y)) = (next_num(&mut idx), next_num(&mut idx)) else {
                        break;
                    };
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
                    let (Some(x), Some(y)) = (next_num(&mut idx), next_num(&mut idx)) else {
                        break;
                    };
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
                    cur_x = if cur_cmd == 'h' { cur_x + x } else { x };
                    if in_figure {
                        sink.AddLine(pt(cur_x, cur_y));
                    }
                    last_ctrl_x = cur_x;
                    last_ctrl_y = cur_y;
                    last_cmd = cur_cmd;
                }
                'V' | 'v' => {
                    let Some(y) = next_num(&mut idx) else { break };
                    cur_y = if cur_cmd == 'v' { cur_y + y } else { y };
                    if in_figure {
                        sink.AddLine(pt(cur_x, cur_y));
                    }
                    last_ctrl_x = cur_x;
                    last_ctrl_y = cur_y;
                    last_cmd = cur_cmd;
                }
                'C' | 'c' => {
                    let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x), Some(y)) = (
                        next_num(&mut idx),
                        next_num(&mut idx),
                        next_num(&mut idx),
                        next_num(&mut idx),
                        next_num(&mut idx),
                        next_num(&mut idx),
                    ) else {
                        break;
                    };
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
                    let (Some(x2), Some(y2), Some(x), Some(y)) = (
                        next_num(&mut idx),
                        next_num(&mut idx),
                        next_num(&mut idx),
                        next_num(&mut idx),
                    ) else {
                        break;
                    };
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
                    let (Some(qx1), Some(qy1), Some(x), Some(y)) = (
                        next_num(&mut idx),
                        next_num(&mut idx),
                        next_num(&mut idx),
                        next_num(&mut idx),
                    ) else {
                        break;
                    };
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
                _ => break,
            }
        }

        if in_figure {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close().map_err(|e| format!("Sink.Close 失败: {}", e))?;
        Ok(path)
    }
}

impl D2DRenderer {
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

    /// Lazily create (and cache) a Direct2D bitmap for the given frame size of `resources/app.ico`.
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
}
