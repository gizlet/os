//! Button test: a handheld drawn from primitives whose parts light up with the raw controls.

use super::chrome::{self, MARGIN, TOP_H};
use super::home::focus_color;
use crate::app::{App, TEST_EXIT_SECONDS};
use crate::fonts::Weight;
use crate::i18n::T;
use crate::icons;
use crate::input::{Button, RawPad};
use crate::painter::Painter;
use crate::theme::{alpha, mix, rgb};

pub fn draw(p: &mut Painter, app: &App, pad: &RawPad) {
    let pal = p.pal;
    chrome::page_title(p, app.t(T::TestTitle));
    chrome::status(p, app);

    let on = focus_color(p);
    let idle = if pal.dark { mix(pal.surface_hi, pal.bg_bottom, 0.1) } else { mix(pal.surface_hi, pal.line, 0.5) };
    let label_c = pal.dim;
    let lit = |b: bool| if b { on } else { idle };
    let ink = |b: bool| if b { pal.on_accent } else { label_c };

    let bw = 820.0f32.min(p.w - 120.0);
    let bh = 380.0;
    let bx = p.w / 2.0 - bw / 2.0;
    let by = TOP_H + 62.0;
    let r = 150.0;

    // Shoulders and triggers behind the body.
    for (left, bumper, trigger, axis, name1, name2) in
        [(true, Button::L1, Button::L2, 4, "L", "L2"), (false, Button::R1, Button::R2, 5, "R", "R2")]
    {
        let sx = if left { bx + 70.0 } else { bx + bw - 70.0 - 190.0 };
        let tv = pad.axes[axis].max(if pad.down(trigger) { 1.0 } else { 0.0 });
        p.rect(sx + 20.0, by - 52.0, 150.0, 40.0, 14.0, idle);
        p.rect(sx + 20.0, by - 52.0, 150.0 * tv, 40.0, 14.0, on);
        p.text_center(name2, sx + 95.0, by - 34.0, 17.0, Weight::Bold, ink(tv > 0.5));
        p.rect(sx, by - 18.0, 190.0, 40.0, 16.0, lit(pad.down(bumper)));
        p.text_center(name1, sx + 95.0, by - 4.0, 18.0, Weight::Bold, ink(pad.down(bumper)));
    }

    p.shadow(bx, by, bw, bh, r, 22.0, 1.0);
    p.rect(bx, by, bw, bh, r, pal.surface);

    // The screen, with the controller's name.
    let sw = bw * 0.4;
    let sh = 190.0;
    let sx = p.w / 2.0 - sw / 2.0;
    let sy = by + 42.0;
    let bezel = if pal.dark { alpha(rgb(0x000000), 0.45) } else { mix(pal.surface_hi, pal.line, 0.6) };
    p.rect(sx - 10.0, sy - 10.0, sw + 20.0, sh + 20.0, 20.0, bezel);
    p.rect(sx, sy, sw, sh, 12.0, mix(pal.bg_bottom, pal.accent, 0.06));
    icons::logo(p, p.w / 2.0, sy + 54.0, 44.0, alpha(pal.accent, 0.9), pal.on_accent);
    let name = pad.name.clone().unwrap_or_else(|| app.t(T::TestNoPad).to_string());
    let lines = p.wrap(&name, sw - 30.0, 17.0, Weight::Regular);
    for (i, line) in lines.iter().take(3).enumerate() {
        p.text_center(line, p.w / 2.0, sy + 106.0 + i as f32 * 25.0, 17.0, Weight::Regular, pal.text);
    }

    // Select and Start under the screen, the function button below them.
    for (b, label, dx) in [(Button::Select, "SELECT", -58.0), (Button::Start, "START", 58.0)] {
        let cx = p.w / 2.0 + dx;
        let cy = sy + sh + 52.0;
        p.rect(cx - 46.0, cy - 13.0, 92.0, 26.0, 13.0, lit(pad.down(b)));
        p.text_center(label, cx, cy, 14.0, Weight::Bold, ink(pad.down(b)));
    }
    p.circle(p.w / 2.0, sy + sh + 96.0, 14.0, lit(pad.down(Button::Home)));

    // D-pad.
    let (dx, dy) = (bx + bw * 0.155, by + 120.0);
    let (arm, thick) = (34.0, 38.0);
    p.rect(dx - thick / 2.0, dy - arm - thick / 2.0, thick, arm * 2.0 + thick, 8.0, idle);
    p.rect(dx - arm - thick / 2.0, dy - thick / 2.0, arm * 2.0 + thick, thick, 8.0, idle);
    for (b, ox, oy) in [(Button::Up, 0.0, -1.0), (Button::Down, 0.0, 1.0), (Button::Left, -1.0, 0.0), (Button::Right, 1.0, 0.0)] {
        let cx = dx + ox * arm;
        let cy = dy + oy * arm;
        let down = pad.down(b);
        if down {
            p.rect(cx - thick / 2.0, cy - thick / 2.0, thick, thick, 8.0, on);
        }
        let tip = 9.0;
        let c = if down { pal.on_accent } else { alpha(label_c, 0.8) };
        let (tx, ty) = (cx + ox * 4.0, cy + oy * 4.0);
        p.triangle(
            (tx + ox * tip, ty + oy * tip),
            (tx - oy * tip * 0.9 - ox * tip * 0.4, ty + ox * tip * 0.9 - oy * tip * 0.4),
            (tx + oy * tip * 0.9 - ox * tip * 0.4, ty - ox * tip * 0.9 - oy * tip * 0.4),
            c,
        );
    }

    // Face buttons by position: X top, A right, B bottom, Y left.
    let (fx, fy) = (bx + bw * 0.845, by + 120.0);
    let d = 42.0;
    for (b, label, ox, oy) in [(Button::X, "X", 0.0, -1.0), (Button::A, "A", 1.0, 0.0), (Button::B, "B", 0.0, 1.0), (Button::Y, "Y", -1.0, 0.0)] {
        let down = pad.down(b);
        let (cx, cy) = (fx + ox * d, fy + oy * d);
        if down {
            p.circle(cx, cy, 30.0, alpha(on, 0.25));
        }
        icons::button_glyph(p, label, cx, cy, 24.0, lit(down), ink(down));
    }

    // Sticks.
    for (left, sx2, thumb, ax, ay) in [(true, bx + bw * 0.27, Button::L3, 0, 1), (false, bx + bw * 0.73, Button::R3, 2, 3)] {
        let cy = by + 272.0;
        let base = 50.0;
        p.circle(sx2, cy, base, mix(idle, pal.bg_bottom, 0.25));
        p.ring(sx2, cy, base - 2.0, base, 0.0, 360.0, alpha(pal.line, 0.8));
        let (vx, vy) = (pad.axes[ax], pad.axes[ay]);
        let moved = vx.abs() > 0.15 || vy.abs() > 0.15;
        let (kx, ky) = (sx2 + vx * (base - 22.0), cy + vy * (base - 22.0));
        if moved {
            p.line(sx2, cy, kx, ky, 4.0, alpha(on, 0.6));
        }
        let active = pad.down(thumb) || moved;
        p.circle(kx, ky, 26.0, lit(active));
        p.circle(kx, ky, 18.0, alpha(if active { pal.on_accent } else { pal.line }, 0.25));
        let label = app.t(if left { T::LeftStick } else { T::RightStick });
        p.text_center(&format!("{label}  {vx:+.2} {vy:+.2}"), sx2, by + bh + 34.0, 17.0, Weight::Regular, pal.dim);
    }

    // The volume keys sit on the handheld's side.
    let vy = by + bh * 0.5;
    let vx = bx + bw + 26.0;
    if vx + 24.0 < p.w {
        for (b, label, oy) in [(Button::VolumeUp, "+", -36.0), (Button::VolumeDown, "-", 36.0)] {
            p.rect(vx - 14.0, vy + oy - 28.0, 28.0, 56.0, 12.0, lit(pad.down(b)));
            p.text_center(label, vx, vy + oy, 22.0, Weight::Bold, ink(pad.down(b)));
        }
    }

    // The hint, and progress toward leaving.
    p.text_center(app.t(T::TestHint), p.w / 2.0, by + bh + 74.0, 19.0, Weight::Regular, pal.faint);
    let back = chrome::glyph(app, "B");
    let progress = (app.test_hold / TEST_EXIT_SECONDS).clamp(0.0, 1.0);
    let ey = p.h - chrome::BOTTOM_H / 2.0;
    let label = T::TestExit.fmt(app.lang(), &[back]);
    let lw = p.measure(&label, 20.0, Weight::Regular);
    let ex = p.w - MARGIN - lw;
    p.text_mid(&label, ex, ey, 20.0, Weight::Regular, pal.text);
    let gx = ex - 26.0;
    p.ring(gx, ey, 15.0, 18.0, 0.0, 360.0, alpha(pal.line, 0.8));
    if progress > 0.0 {
        p.ring(gx, ey, 14.0, 19.0, -90.0, -90.0 + 360.0 * progress, pal.accent);
    }
    p.text_center(back, gx, ey + 1.0, 16.0, Weight::Bold, pal.text);
    chrome::divider(p, p.h - chrome::BOTTOM_H);
}
