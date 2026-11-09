use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

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
