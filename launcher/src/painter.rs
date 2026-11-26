//! Drawing in logical units, which is what makes one layout fit every screen.
//!
//! Pages are laid out for a screen 720 units tall and at least 960 wide.
//! `scale` maps units to window points: a 640×480 handheld, a 1280×720
//! window and a 4K monitor all see 720 units of height, and wider screens get
//! more units of width. `dpi` maps points to pixels on high-density displays.

use crate::fonts::{Fonts, Weight};
use crate::theme::{alpha, Palette};
use raylib_sys::{self as ffi, Color, Rectangle, Vector2};
use std::ffi::CString;

pub const DESIGN_H: f32 = 720.0;
pub const MIN_W: f32 = 960.0;

/// Fraction of the font size from the top of a line to the middle of its glyphs.
const MIDLINE: f32 = 0.56;

pub struct Painter {
    pub scale: f32,
    pub dpi: f32,
    /// Logical width and height.
    pub w: f32,
    pub h: f32,
    pub pal: Palette,
    pub fonts: Fonts,
    /// Seconds since start, for animations.
    pub time: f64,
    /// How strongly the focus pulses, 0–1: it settles once the user stops, so
    /// an idle screen stays still and need not be redrawn.
    pub pulse: f32,
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { x, y }
}

fn segments(radius_px: f32) -> i32 {
    ((radius_px / 2.0).ceil() as i32).clamp(4, 24)
}

impl Painter {
    pub fn new(fonts: Fonts, pal: Palette) -> Painter {
        let mut p = Painter {
            scale: 1.0,
            dpi: 1.0,
            w: MIN_W,
            h: DESIGN_H,
            pal,
            fonts,
            time: 0.0,
            pulse: 1.0,
        };
        p.resize();
        p
    }

    /// Recomputes the scale from the window size.
    pub fn resize(&mut self) {
        let (sw, sh) = unsafe { (ffi::GetScreenWidth() as f32, ffi::GetScreenHeight() as f32) };
        let dpi = unsafe { ffi::GetWindowScaleDPI().x }.max(1.0);
        let scale = (sh / DESIGN_H).min(sw / MIN_W).max(0.1);
        if (scale - self.scale).abs() > 1e-4 || (dpi - self.dpi).abs() > 1e-4 {
            // Glyphs are rasterized for one pixel size; a new scale needs new atlases.
            self.fonts.clear();
        }
        self.scale = scale;
        self.dpi = dpi;
        self.w = sw / scale;
        self.h = sh / scale;
    }

    fn rec(&self, x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        let s = self.scale;
        Rectangle { x: x * s, y: y * s, width: w * s, height: h * s }
    }

    fn p(&self, x: f32, y: f32) -> Vector2 {
        v2(x * self.scale, y * self.scale)
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, c: Color) {
        if w <= 0.0 || h <= 0.0 || c.a == 0 {
            return;
        }
        let r = self.rec(x, y, w, h);
        unsafe {
            if radius <= 0.0 {
                ffi::DrawRectangleRec(r, c);
            } else {
                let roundness = (2.0 * radius / w.min(h)).min(1.0);
                ffi::DrawRectangleRounded(r, roundness, segments(radius * self.scale), c);
            }
        }
    }

    /// An outline drawn inside the rectangle's bounds.
    /// A texture stretched over the rectangle.
    pub fn texture(&self, tex: ffi::Texture2D, x: f32, y: f32, w: f32, h: f32, tint: Color) {
        if tex.id == 0 || w <= 0.0 || h <= 0.0 {
            return;
        }
        let src = Rectangle { x: 0.0, y: 0.0, width: tex.width as f32, height: tex.height as f32 };
        unsafe { ffi::DrawTexturePro(tex, src, self.rec(x, y, w, h), v2(0.0, 0.0), 0.0, tint) };
    }

    pub fn outline(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, thick: f32, c: Color) {
        if c.a == 0 || w <= thick || h <= thick {
            return;
        }
        let t = thick;
        let (x, y, w, h, radius) = (x + t / 2.0, y + t / 2.0, w - t, h - t, (radius - t / 2.0).max(0.5));
        let roundness = (2.0 * radius / w.min(h)).min(1.0);
        unsafe {
            ffi::DrawRectangleRoundedLinesEx(
                self.rec(x, y, w, h),
                roundness,
                segments(radius * self.scale),
                t * self.scale,
                c,
            );
        }
    }

    /// A soft drop shadow from stacked translucent rounded rectangles.
    pub fn shadow(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, spread: f32, strength: f32) {
        let steps = 5;
        for i in 0..steps {
            let f = (i + 1) as f32 / steps as f32;
            let grow = spread * f;
            let a = strength * (1.0 - f) * 0.4;
            self.rect(
                x - grow,
                y - grow + spread * 0.35,
                w + grow * 2.0,
                h + grow * 2.0,
                radius + grow,
                alpha(self.pal.shadow, a),
            );
        }
    }

    pub fn gradient_v(&self, x: f32, y: f32, w: f32, h: f32, top: Color, bottom: Color) {
        let s = self.scale;
        unsafe {
            ffi::DrawRectangleGradientV(
                (x * s) as i32,
                (y * s) as i32,
                (w * s).ceil() as i32,
                (h * s).ceil() as i32,
                top,
                bottom,
            )
        };
    }

    pub fn circle(&self, cx: f32, cy: f32, r: f32, c: Color) {
        if c.a > 0 {
            unsafe { ffi::DrawCircleV(self.p(cx, cy), r * self.scale, c) };
        }
    }

    /// A disc fading from `inner` at the center to `outer` at the rim.
    pub fn circle_gradient(&self, cx: f32, cy: f32, r: f32, inner: Color, outer: Color) {
        let c = self.p(cx, cy);
        unsafe { ffi::DrawCircleGradient(c, r * self.scale, inner, outer) };
    }

    /// An arc between angles in degrees, 0° pointing right and growing clockwise.
    pub fn ring(&self, cx: f32, cy: f32, inner: f32, outer: f32, start: f32, end: f32, c: Color) {
        if c.a == 0 {
            return;
        }
        let segs = (((end - start).abs() / 6.0).ceil() as i32).max(8);
        unsafe {
            ffi::DrawRing(self.p(cx, cy), inner * self.scale, outer * self.scale, start, end, segs, c)
        };
    }

    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, thick: f32, c: Color) {
        unsafe { ffi::DrawLineEx(self.p(x1, y1), self.p(x2, y2), thick * self.scale, c) };
        // Round caps keep thick strokes from looking cut off.
        if thick >= 2.0 {
            self.circle(x1, y1, thick / 2.0, c);
            self.circle(x2, y2, thick / 2.0, c);
        }
    }

    /// A filled triangle; the vertex order does not matter.
    pub fn triangle(&self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: Color) {
        let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        let (b, c) = if cross > 0.0 { (c, b) } else { (b, c) };
        unsafe { ffi::DrawTriangle(self.p(a.0, a.1), self.p(b.0, b.1), self.p(c.0, c.1), color) };
    }

    /// A rectangle of size (w, h) centered on (cx, cy), rotated by `deg`.
    pub fn rect_rot(&self, cx: f32, cy: f32, w: f32, h: f32, deg: f32, c: Color) {
        let s = self.scale;
        let r = Rectangle { x: cx * s, y: cy * s, width: w * s, height: h * s };
        unsafe { ffi::DrawRectanglePro(r, v2(w * s / 2.0, h * s / 2.0), deg, c) };
    }

    pub fn clip(&self, x: f32, y: f32, w: f32, h: f32) {
        let s = self.scale;
        unsafe { ffi::BeginScissorMode((x * s) as i32, (y * s) as i32, (w * s).ceil() as i32, (h * s).ceil() as i32) };
    }

    pub fn unclip(&self) {
        unsafe { ffi::EndScissorMode() };
    }

    fn face(&mut self, size: f32, weight: Weight) -> ffi::Font {
        let px = (size * self.scale * self.dpi).round() as u32;
        self.fonts.face(weight, px)
    }

    fn cstring(text: &str) -> CString {
        CString::new(text.replace('\0', "")).unwrap_or_default()
    }

    pub fn measure(&mut self, text: &str, size: f32, weight: Weight) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.fonts.ensure(text);
        let font = self.face(size, weight);
        let c = Self::cstring(text);
        unsafe { ffi::MeasureTextEx(font, c.as_ptr(), size * self.scale, 0.0).x / self.scale }
    }

    /// Draws `text` with its top-left corner at (x, y) and returns its width.
    pub fn text(&mut self, text: &str, x: f32, y: f32, size: f32, weight: Weight, c: Color) -> f32 {
        if text.is_empty() || c.a == 0 {
            return 0.0;
        }
        self.fonts.ensure(text);
        let font = self.face(size, weight);
        let cs = Self::cstring(text);
        let s = self.scale;
        unsafe {
            // Whole physical pixels keep glyphs sharp.
            let px = (x * s * self.dpi).round() / self.dpi;
            let py = (y * s * self.dpi).round() / self.dpi;
            ffi::DrawTextEx(font, cs.as_ptr(), v2(px, py), size * s, 0.0, c);
            ffi::MeasureTextEx(font, cs.as_ptr(), size * s, 0.0).x / s
        }
    }

    /// Left-aligned text centered vertically on `cy`.
    pub fn text_mid(&mut self, text: &str, x: f32, cy: f32, size: f32, weight: Weight, c: Color) -> f32 {
        self.text(text, x, cy - size * MIDLINE, size, weight, c)
    }

    /// Text centered on (cx, cy).
    pub fn text_center(&mut self, text: &str, cx: f32, cy: f32, size: f32, weight: Weight, c: Color) -> f32 {
        let w = self.measure(text, size, weight);
        self.text(text, cx - w / 2.0, cy - size * MIDLINE, size, weight, c)
    }

    /// Right-aligned text ending at `right`, centered vertically on `cy`.
    pub fn text_right(&mut self, text: &str, right: f32, cy: f32, size: f32, weight: Weight, c: Color) -> f32 {
        let w = self.measure(text, size, weight);
        self.text(text, right - w, cy - size * MIDLINE, size, weight, c)
    }

    /// `text` shortened with an ellipsis to fit `max_w`.
    pub fn fit(&mut self, text: &str, max_w: f32, size: f32, weight: Weight) -> String {
        if self.measure(text, size, weight) <= max_w {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let candidate: String = chars[..mid].iter().collect::<String>() + "…";
            if self.measure(&candidate, size, weight) <= max_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        chars[..lo].iter().collect::<String>().trim_end().to_string() + "…"
    }

    /// Splits `text` into lines no wider than `max_w`, between words or CJK characters.
    pub fn wrap(&mut self, text: &str, max_w: f32, size: f32, weight: Weight) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut tokens: Vec<String> = Vec::new();
            let mut word = String::new();
            for c in para.chars() {
                if c.is_ascii_alphanumeric() || "'-./<>_@:".contains(c) {
                    word.push(c);
                } else {
                    if !word.is_empty() {
                        tokens.push(std::mem::take(&mut word));
                    }
                    tokens.push(c.to_string());
                }
            }
            if !word.is_empty() {
                tokens.push(word);
            }
            let mut line = String::new();
            for tok in tokens {
                let candidate = format!("{line}{tok}");
                if !line.is_empty() && self.measure(candidate.trim_end(), size, weight) > max_w {
                    lines.push(line.trim_end().to_string());
                    line = tok.trim_start().to_string();
                } else {
                    line = candidate;
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }
}
