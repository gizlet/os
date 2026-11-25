//! Vector icons drawn from primitives, each fitted into a square of side `s`
//! centered on (cx, cy), so they stay sharp at every scale.

use crate::fonts::Weight;
use crate::painter::Painter;
use crate::theme::alpha;
use raylib_sys::Color;

/// Signal arcs; `bars` of 0–4 are lit.
pub fn wifi(p: &Painter, cx: f32, cy: f32, s: f32, bars: u8, on: Color, off: Color) {
    let base = cy + s * 0.34;
    let t = s * 0.11;
    for i in 0..3u8 {
        let r = s * (0.30 + 0.22 * i as f32);
        p.ring(cx, base, r - t, r, 225.0, 315.0, if bars > i + 1 { on } else { off });
    }
    p.circle(cx, base - s * 0.04, s * 0.085, if bars > 0 { on } else { off });
}

/// A battery `2h` wide with its left edge at `x`.
pub fn battery(p: &Painter, x: f32, cy: f32, h: f32, percent: u8, charging: bool, c: Color, fill: Color) {
    let w = h * 2.0;
    let y = cy - h / 2.0;
    let t = (h * 0.12).max(1.5);
    p.outline(x, y, w, h, h * 0.28, t, c);
    p.rect(x + w + t * 0.4, cy - h * 0.2, t * 1.4, h * 0.4, t * 0.6, c);
    let inner = w - t * 4.0;
    let level = inner * (percent as f32 / 100.0).clamp(0.06, 1.0);
    p.rect(x + t * 2.0, y + t * 2.0, level, h - t * 4.0, h * 0.12, fill);
    if charging {
        bolt(p, x + w / 2.0, cy, h * 0.9, c);
    }
}

pub fn bolt(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let k = s * 0.4;
    p.triangle((cx + k * 0.15, cy - k), (cx - k * 0.55, cy + k * 0.12), (cx + k * 0.05, cy + k * 0.12), c);
    p.triangle((cx - k * 0.05, cy - k * 0.12), (cx - k * 0.15, cy + k), (cx + k * 0.55, cy - k * 0.12), c);
}

pub fn grid(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let cell = s * 0.36;
    let gap = s * 0.1;
    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let x = cx + dx * (gap / 2.0 + cell / 2.0) - cell / 2.0;
        let y = cy + dy * (gap / 2.0 + cell / 2.0) - cell / 2.0;
        p.rect(x, y, cell, cell, cell * 0.28, c);
    }
}

pub fn bag(p: &Painter, cx: f32, cy: f32, s: f32, c: Color, hole: Color) {
    let w = s * 0.78;
    let h = s * 0.6;
    let top = cy - h / 2.0 + s * 0.12;
    p.ring(cx, top, s * 0.15, s * 0.22, 180.0, 360.0, c);
    p.rect(cx - w / 2.0, top, w, h, s * 0.1, c);
    p.circle(cx - s * 0.18, top + h * 0.3, s * 0.05, hole);
    p.circle(cx + s * 0.18, top + h * 0.3, s * 0.05, hole);
}

pub fn gamepad(p: &Painter, cx: f32, cy: f32, s: f32, c: Color, hole: Color) {
    let w = s * 0.92;
    let h = s * 0.5;
    p.circle(cx - w * 0.3, cy + h * 0.28, s * 0.2, c);
    p.circle(cx + w * 0.3, cy + h * 0.28, s * 0.2, c);
    p.rect(cx - w / 2.0, cy - h / 2.0, w, h, h * 0.45, c);
    let (dx, dy) = (cx - w * 0.25, cy - h * 0.02);
    let (a, b) = (s * 0.16, s * 0.055);
    p.rect(dx - a / 2.0, dy - b / 2.0, a, b, b * 0.3, hole);
    p.rect(dx - b / 2.0, dy - a / 2.0, b, a, b * 0.3, hole);
    p.circle(cx + w * 0.2, cy - h * 0.12, s * 0.045, hole);
    p.circle(cx + w * 0.31, cy + h * 0.06, s * 0.045, hole);
}

pub fn gear(p: &Painter, cx: f32, cy: f32, s: f32, c: Color, hole: Color) {
    let r = s * 0.3;
    for i in 0..8 {
        let a = i as f32 * 45.0;
        let (sin, cos) = a.to_radians().sin_cos();
        p.rect_rot(cx + cos * r, cy + sin * r, s * 0.17, s * 0.2, a + 90.0, c);
    }
    p.circle(cx, cy, r, c);
    p.circle(cx, cy, s * 0.12, hole);
}

pub fn power(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let t = s * 0.1;
    let r = s * 0.34;
    p.ring(cx, cy + s * 0.03, r - t, r, -60.0, 240.0, c);
    p.line(cx, cy - s * 0.4, cx, cy - s * 0.04, t, c);
}

pub fn restart(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let t = s * 0.1;
    let r = s * 0.34;
    p.ring(cx, cy, r - t, r, -40.0, 250.0, c);
    let (sn, cs) = (-40f32).to_radians().sin_cos();
    let (ax, ay) = (cx + cs * (r - t / 2.0), cy + sn * (r - t / 2.0));
    p.triangle((ax - s * 0.16, ay - s * 0.1), (ax + s * 0.02, ay + s * 0.16), (ax + s * 0.12, ay - s * 0.16), c);
}

/// A crescent moon, for sleep; `hole` is the background it sits on.
pub fn moon(p: &Painter, cx: f32, cy: f32, s: f32, c: Color, hole: Color) {
    let r = s * 0.36;
    p.circle(cx, cy, r, c);
    p.circle(cx + r * 0.55, cy - r * 0.45, r * 0.82, hole);
}

pub fn lock(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let w = s * 0.62;
    let h = s * 0.46;
    let top = cy - s * 0.06;
    p.ring(cx, top, s * 0.15, s * 0.23, 180.0, 360.0, c);
    p.rect(cx - s * 0.23, top - 0.5, s * 0.08, s * 0.08, 0.0, c);
    p.rect(cx + s * 0.15, top - 0.5, s * 0.08, s * 0.08, 0.0, c);
    p.rect(cx - w / 2.0, top, w, h, s * 0.08, c);
}

pub fn check(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let t = s * 0.13;
    p.line(cx - s * 0.32, cy + s * 0.02, cx - s * 0.1, cy + s * 0.24, t, c);
    p.line(cx - s * 0.1, cy + s * 0.24, cx + s * 0.34, cy - s * 0.24, t, c);
}

pub fn chevron(p: &Painter, cx: f32, cy: f32, s: f32, right: bool, c: Color) {
    let d = if right { 1.0 } else { -1.0 };
    let t = s * 0.12;
    p.line(cx - d * s * 0.12, cy - s * 0.26, cx + d * s * 0.14, cy, t, c);
    p.line(cx + d * s * 0.14, cy, cx - d * s * 0.12, cy + s * 0.26, t, c);
}

pub fn backspace(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let t = s * 0.09;
    let (l, r, h) = (cx - s * 0.4, cx + s * 0.38, s * 0.26);
    let m = cx - s * 0.16;
    p.line(l, cy, m, cy - h, t, c);
    p.line(m, cy - h, r, cy - h, t, c);
    p.line(r, cy - h, r, cy + h, t, c);
    p.line(r, cy + h, m, cy + h, t, c);
    p.line(m, cy + h, l, cy, t, c);
    let (x, k) = (cx + s * 0.08, s * 0.1);
    p.line(x - k, cy - k, x + k, cy + k, t, c);
    p.line(x - k, cy + k, x + k, cy - k, t, c);
}

pub fn shift(p: &Painter, cx: f32, cy: f32, s: f32, filled: bool, c: Color) {
    let top = cy - s * 0.36;
    let mid = cy - s * 0.02;
    if filled {
        p.triangle((cx, top), (cx - s * 0.36, mid), (cx + s * 0.36, mid), c);
        p.rect(cx - s * 0.16, mid - 1.0, s * 0.32, s * 0.34, 0.0, c);
        return;
    }
    let t = s * 0.09;
    let (l, r, b) = (cx - s * 0.16, cx + s * 0.16, cy + s * 0.32);
    p.line(cx, top, cx - s * 0.36, mid, t, c);
    p.line(cx, top, cx + s * 0.36, mid, t, c);
    p.line(cx - s * 0.36, mid, l, mid, t, c);
    p.line(cx + s * 0.36, mid, r, mid, t, c);
    p.line(l, mid, l, b, t, c);
    p.line(r, mid, r, b, t, c);
    p.line(l, b, r, b, t, c);
}

pub fn spinner(p: &Painter, cx: f32, cy: f32, r: f32, t: f32, c: Color) {
    let start = (p.time * 300.0 % 360.0) as f32;
    let sweep = 110.0 + 70.0 * (p.time * 2.2).sin() as f32;
    p.ring(cx, cy, r - t, r, start, start + sweep, c);
}

pub fn sun(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    p.circle(cx, cy, s * 0.2, c);
    for i in 0..8 {
        let (sn, cs) = (i as f32 * 45.0).to_radians().sin_cos();
        p.line(cx + cs * s * 0.32, cy + sn * s * 0.32, cx + cs * s * 0.44, cy + sn * s * 0.44, s * 0.08, c);
    }
}

pub fn speaker(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    p.rect(cx - s * 0.38, cy - s * 0.12, s * 0.18, s * 0.24, s * 0.03, c);
    p.triangle((cx - s * 0.22, cy - s * 0.12), (cx - s * 0.22, cy + s * 0.12), (cx + s * 0.02, cy + s * 0.32), c);
    p.triangle((cx - s * 0.22, cy - s * 0.12), (cx + s * 0.02, cy + s * 0.32), (cx + s * 0.02, cy - s * 0.32), c);
    p.ring(cx + s * 0.04, cy, s * 0.16, s * 0.22, -50.0, 50.0, c);
    p.ring(cx + s * 0.04, cy, s * 0.3, s * 0.36, -50.0, 50.0, c);
}

pub fn globe(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let r = s * 0.4;
    let t = s * 0.08;
    p.ring(cx, cy, r - t, r, 0.0, 360.0, c);
    p.line(cx - r, cy, cx + r, cy, t, c);
    p.ring(cx, cy, r * 0.42 - t, r * 0.42, 0.0, 360.0, c);
}

pub fn clock(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let r = s * 0.4;
    let t = s * 0.08;
    p.ring(cx, cy, r - t, r, 0.0, 360.0, c);
    p.line(cx, cy, cx, cy - r * 0.6, t, c);
    p.line(cx, cy, cx + r * 0.45, cy + r * 0.2, t, c);
}

pub fn chip(p: &Painter, cx: f32, cy: f32, s: f32, c: Color, hole: Color) {
    let w = s * 0.56;
    for i in 0..3 {
        let o = (i as f32 - 1.0) * s * 0.16;
        p.rect(cx - s * 0.42, cy + o - s * 0.03, s * 0.84, s * 0.06, 0.0, c);
        p.rect(cx + o - s * 0.03, cy - s * 0.42, s * 0.06, s * 0.84, 0.0, c);
    }
    p.rect(cx - w / 2.0, cy - w / 2.0, w, w, s * 0.08, c);
    p.rect(cx - w * 0.25, cy - w * 0.25, w * 0.5, w * 0.5, s * 0.04, hole);
}

pub fn cross_circle(p: &Painter, cx: f32, cy: f32, s: f32, c: Color) {
    let t = s * 0.09;
    p.ring(cx, cy, s * 0.42 - t, s * 0.42, 0.0, 360.0, c);
    let k = s * 0.14;
    p.line(cx - k, cy - k, cx + k, cy + k, t, c);
    p.line(cx - k, cy + k, cx + k, cy - k, t, c);
}

/// A terminal window with a prompt, for ssh.
pub fn terminal(p: &Painter, cx: f32, cy: f32, s: f32, c: Color, hole: Color) {
    let (w, h) = (s * 0.84, s * 0.64);
    p.rect(cx - w / 2.0, cy - h / 2.0, w, h, s * 0.1, c);
    let t = s * 0.07;
    let (x, y) = (cx - w * 0.28, cy - h * 0.08);
    p.line(x, y - s * 0.1, x + s * 0.12, y, t, hole);
    p.line(x + s * 0.12, y, x, y + s * 0.1, t, hole);
    p.rect(cx, y + s * 0.07, s * 0.2, t, 0.0, hole);
}

/// A face button glyph: a filled circle with its letter.
pub fn button_glyph(p: &mut Painter, label: &str, cx: f32, cy: f32, r: f32, bg: Color, fg: Color) {
    p.circle(cx, cy, r, bg);
    p.text_center(label, cx, cy + r * 0.04, r * 1.35, Weight::Bold, fg);
}

/// The width of a pill glyph for wide labels such as L, R or ←→.
pub fn pill_width(p: &mut Painter, label: &str, h: f32) -> f32 {
    (p.measure(label, h * 0.66, Weight::Bold) + h * 0.7).max(h * 1.5)
}

pub fn pill_glyph(p: &mut Painter, label: &str, cx: f32, cy: f32, h: f32, bg: Color, fg: Color) {
    let w = pill_width(p, label, h);
    p.rect(cx - w / 2.0, cy - h / 2.0, w, h, h / 2.0, bg);
    p.text_center(label, cx, cy + h * 0.03, h * 0.66, Weight::Bold, fg);
}

/// Gizlet's mark: a rounded square holding four dots placed like face buttons.
pub fn logo(p: &Painter, cx: f32, cy: f32, s: f32, c: Color, fg: Color) {
    p.rect(cx - s / 2.0, cy - s / 2.0, s, s, s * 0.3, c);
    let d = s * 0.19;
    let r = s * 0.095;
    p.circle(cx, cy - d, r, fg);
    p.circle(cx + d, cy, r, alpha(fg, 0.75));
    p.circle(cx, cy + d, r, alpha(fg, 0.55));
    p.circle(cx - d, cy, r, alpha(fg, 0.85));
}
