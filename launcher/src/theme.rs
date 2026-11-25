//! Colors for the dark and light themes.

use crate::settings::Theme;
use raylib_sys::Color;

pub const fn rgb(hex: u32) -> Color {
    Color {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
        a: 255,
    }
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color {
        a: (c.a as f32 * a.clamp(0.0, 1.0)) as u8,
        ..c
    }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg_top: Color,
    pub bg_bottom: Color,
    pub surface: Color,
    pub surface_hi: Color,
    pub line: Color,
    pub text: Color,
    pub dim: Color,
    pub faint: Color,
    pub accent: Color,
    pub accent_hi: Color,
    pub on_accent: Color,
    pub danger: Color,
    pub success: Color,
    pub warm: Color,
    pub shadow: Color,
    pub scrim: Color,
}

pub fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Dark => Palette {
            dark: true,
            bg_top: rgb(0x1d2027),
            bg_bottom: rgb(0x121418),
            surface: rgb(0x262a32),
            surface_hi: rgb(0x323743),
            line: rgb(0x3a3f4b),
            text: rgb(0xf3f5f8),
            dim: rgb(0xa4abb8),
            faint: rgb(0x6c7380),
            accent: rgb(0x34b6ff),
            accent_hi: rgb(0x9ee3ff),
            on_accent: rgb(0x06121c),
            danger: rgb(0xff5d63),
            success: rgb(0x3ddc97),
            warm: rgb(0xff9a3d),
            shadow: Color { r: 0, g: 0, b: 0, a: 150 },
            scrim: Color { r: 6, g: 8, b: 12, a: 170 },
        },
        Theme::Light => Palette {
            dark: false,
            bg_top: rgb(0xf4f5f7),
            bg_bottom: rgb(0xe4e7ec),
            surface: rgb(0xffffff),
            surface_hi: rgb(0xeef0f4),
            line: rgb(0xd6dae1),
            text: rgb(0x1a1d23),
            dim: rgb(0x5a6170),
            faint: rgb(0x99a0ab),
            accent: rgb(0x0f86ea),
            accent_hi: rgb(0x5cc1ff),
            on_accent: rgb(0xffffff),
            danger: rgb(0xe5484d),
            success: rgb(0x18a874),
            warm: rgb(0xf07a1a),
            shadow: Color { r: 30, g: 40, b: 60, a: 70 },
            scrim: Color { r: 20, g: 24, b: 32, a: 110 },
        },
    }
}
