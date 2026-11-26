//! Toasts, menus and the on-screen keyboard, drawn over the page.

use super::chrome::{self, BOTTOM_H};
use super::ease_out;
use super::home::{focus_color, focus_ring};
use crate::app::{App, DialogAction, TOAST_SECONDS};
use crate::fonts::Weight;
use crate::i18n::T;
use crate::icons;
use crate::keyboard::{key_center, Key};
use crate::painter::Painter;
use crate::system::PowerAction;
use crate::theme::{alpha, rgb};

pub fn toasts(p: &mut Painter, app: &App) {
    let pal = p.pal;
    let mut y = p.h - BOTTOM_H - 26.0;
    for toast in app.toasts.iter().rev() {
        let fade_in = ease_out(toast.age / 0.2);
        let fade_out = ((TOAST_SECONDS - toast.age) / 0.3).clamp(0.0, 1.0);
        let a = fade_in.min(fade_out);
        let text = p.fit(&toast.text, p.w * 0.8, 20.0, Weight::Regular);
        let tw = p.measure(&text, 20.0, Weight::Regular);
        let (w, h) = (tw + 48.0, 50.0);
        let x = p.w / 2.0 - w / 2.0;
        let ty = y - h + (1.0 - fade_in) * 14.0;
        let bg = if pal.dark { rgb(0x3a404c) } else { rgb(0x262a32) };
        p.shadow(x, ty, w, h, h / 2.0, 10.0, a * 0.8);
        p.rect(x, ty, w, h, h / 2.0, alpha(bg, a * 0.97));
        p.text_center(&text, p.w / 2.0, ty + h / 2.0, 20.0, Weight::Regular, alpha(rgb(0xf3f5f8), a));
        y -= h + 12.0;
    }
}

/// A menu of choices, such as the power options.
pub fn dialog(p: &mut Painter, app: &App) {
    let Some(d) = &app.dialog else { return };
    let pal = p.pal;
    let t = ease_out(((app.time - d.opened) as f32) / 0.18);
    p.rect(0.0, 0.0, p.w, p.h, 0.0, alpha(pal.scrim, t));

    let w = 600.0f32.min(p.w - 80.0);
    let pad = 36.0;
    let title_lines = p.wrap(&d.title, w - pad * 2.0, 26.0, Weight::Display);
    let head_h = title_lines.len() as f32 * 36.0;
    let buttons_h = d.buttons.len() as f32 * 64.0 + 16.0;
    let h = pad + head_h + pad * 0.7 + buttons_h;
    let x = p.w / 2.0 - w / 2.0;
    let y = p.h / 2.0 - h / 2.0 + (1.0 - t) * 24.0;

    p.shadow(x, y, w, h, 22.0, 24.0, t);
    p.rect(x, y, w, h, 22.0, alpha(pal.surface, t.max(0.01)));
    let mut ty = y + pad;
    for line in &title_lines {
        p.text(line, x + pad, ty, 26.0, Weight::Display, pal.text);
        ty += 36.0;
    }

    let by = y + h - buttons_h;
    p.rect(x, by, w, 1.5, 0.0, alpha(pal.line, 0.9));
    for (i, (label, action)) in d.buttons.iter().enumerate() {
        let ry = by + 8.0 + i as f32 * 64.0;
        let focused = i == d.focus;
        if focused {
            p.rect(x + 12.0, ry + 4.0, w - 24.0, 56.0, 12.0, pal.surface_hi);
            focus_ring(p, x + 16.0, ry + 8.0, w - 32.0, 48.0, 8.0, -3.0);
        }
        let danger = matches!(action, DialogAction::WifiForget(_) | DialogAction::WifiDisconnect);
        let c = if danger { pal.danger } else { pal.text };
        let ic = if danger { pal.danger } else { pal.dim };
        let (icon_x, cy) = (x + pad + 6.0, ry + 32.0);
        match action {
            DialogAction::Power(PowerAction::Sleep) => {
                icons::moon(p, icon_x, cy, 26.0, ic, if focused { pal.surface_hi } else { pal.surface })
            }
            DialogAction::Power(PowerAction::Reboot) => icons::restart(p, icon_x, cy, 26.0, ic),
            DialogAction::Power(PowerAction::PowerOff) | DialogAction::Quit => icons::power(p, icon_x, cy, 26.0, ic),
            DialogAction::InstallUpdate => icons::chevron(p, icon_x, cy, 26.0, true, ic),
            DialogAction::WifiForget(_) | DialogAction::WifiDisconnect | DialogAction::Close => {
                icons::cross_circle(p, icon_x, cy, 26.0, ic)
            }
        }
        let weight = if focused { Weight::Bold } else { Weight::Regular };
        p.text_mid(label, x + pad + 34.0, cy, 22.0, weight, c);
    }
    chrome::hints(p, app, &[("B", app.t(T::Back)), ("A", app.t(T::Ok))]);
}

pub fn keyboard(p: &mut Painter, app: &App) {
    let Some(kb) = &app.keyboard else { return };
    let pal = p.pal;
    p.rect(0.0, 0.0, p.w, p.h, 0.0, pal.scrim);

    let rows = kb.rows();
    let unit = ((p.w - 160.0) / 11.0).min(70.0);
    let (key_h, gap) = (58.0, 8.0);
    let panel_h = 150.0 + rows.len() as f32 * (key_h + gap) + 70.0;
    let top = p.h - panel_h;
    p.rect(0.0, top, p.w, panel_h + 30.0, 26.0, pal.surface);

    // Title and field.
    p.text_mid(&kb.title, 60.0, top + 42.0, 22.0, Weight::Display, pal.text);
    let (fx, fw, fy) = (60.0, p.w - 120.0, top + 70.0);
    p.rect(fx, fy, fw, 58.0, 14.0, pal.surface_hi);
    let shown = if kb.masked { "•".repeat(kb.text.chars().count()) } else { kb.text.clone() };
    let tw = p.text_mid(&shown, fx + 20.0, fy + 29.0, 24.0, Weight::Regular, pal.text);
    if (app.time * 2.0) as i64 % 2 == 0 {
        p.rect(fx + 22.0 + tw, fy + 14.0, 2.5, 30.0, 1.0, pal.accent);
    }
    let toggle = app.t(if kb.masked { T::KbShow } else { T::KbHide });
    let toggle_w = p.measure(toggle, 18.0, Weight::Regular);
    p.text_right(toggle, fx + fw - 20.0, fy + 29.0, 18.0, Weight::Regular, pal.dim);
    icons::button_glyph(p, "X", fx + fw - 38.0 - toggle_w, fy + 29.0, 11.0, pal.dim, pal.surface_hi);

    let grid_top = fy + 80.0;
    for (r, row) in rows.iter().enumerate() {
        let ky = grid_top + r as f32 * (key_h + gap);
        for (c, key) in row.iter().enumerate() {
            let center = p.w / 2.0 + key_center(row, c) * (unit + gap);
            let kw = key.width() * (unit + gap) - gap;
            let focused = r == kb.row && c == kb.col;
            let special = !matches!(key, Key::Char(_) | Key::Space);
            let bg = if focused {
                focus_color(p)
            } else if matches!(key, Key::Done) {
                alpha(pal.accent, 0.85)
            } else if special {
                pal.surface_hi
            } else if pal.dark {
                alpha(rgb(0xffffff), 0.08)
            } else {
                rgb(0xe9ecf1)
            };
            let fg = if focused || matches!(key, Key::Done) { pal.on_accent } else { pal.text };
            p.rect(center - kw / 2.0, ky, kw, key_h, 12.0, bg);
            let cy = ky + key_h / 2.0;
            match key {
                Key::Char(ch) => {
                    p.text_center(&ch.to_string(), center, cy, 26.0, Weight::Regular, fg);
                }
                Key::Space => {
                    p.text_center(app.t(T::KbSpace), center, cy, 19.0, Weight::Regular, fg);
                }
                Key::Backspace => icons::backspace(p, center, cy, 30.0, fg),
                Key::Shift => icons::shift(p, center, cy, 26.0, kb.shift, if kb.shift { pal.accent } else { fg }),
                Key::Symbols => {
                    p.text_center(if kb.symbols { "ABC" } else { "#+=" }, center, cy, 19.0, Weight::Bold, fg);
                }
                Key::Done => {
                    p.text_center(app.t(T::KbDone), center, cy, 20.0, Weight::Bold, fg);
                }
            }
        }
    }

    // Keyboard hints.
    let hy = p.h - 34.0;
    let mut x = p.w - 60.0;
    let select = T::KbSelectOrHold.fmt(app.lang(), &[&crate::keyboard::DONE_HOLD_SECONDS.to_string()]);
    for (g, label) in [("Y", app.t(T::KbSpace)), ("B", app.t(T::Delete)), ("A", select.as_str())].iter().rev() {
        let g = chrome::glyph(app, g);
        x -= p.measure(label, 19.0, Weight::Regular);
        p.text_mid(label, x, hy, 19.0, Weight::Regular, pal.dim);
        icons::button_glyph(p, g, x - 20.0, hy, 12.0, pal.dim, pal.surface);
        x -= 60.0;
    }
}
