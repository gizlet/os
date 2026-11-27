//! The background, status bar, page titles and button hints shared by every page.

use crate::app::App;
use crate::clock;
use crate::fonts::Weight;
use crate::icons;
use crate::painter::Painter;
use crate::settings::Language;
use crate::theme::{alpha, mix};

pub const TOP_H: f32 = 92.0;
pub const BOTTOM_H: f32 = 72.0;
pub const MARGIN: f32 = 64.0;

pub fn background(p: &mut Painter) {
    let pal = p.pal;
    p.gradient_v(0.0, 0.0, p.w, p.h, pal.bg_top, pal.bg_bottom);
    // A faint glow behind the content keeps large dark areas from looking flat.
    let glow = alpha(pal.accent, if pal.dark { 0.09 } else { 0.1 });
    p.circle_gradient(p.w * 0.5, -p.h * 0.15, p.h * 0.95, glow, alpha(pal.accent, 0.0));
}

/// The controller button for a hint. Hints name the confirm button "A" and
/// the back button "B"; with A/B swapped they are the other physical buttons.
pub fn glyph<'a>(app: &App, g: &'a str) -> &'a str {
    match (app.settings.swap_ab, g) {
        (true, "A") => "B",
        (true, "B") => "A",
        _ => g,
    }
}

/// The clock, Wi-Fi, ssh and battery at the top right. Returns the left edge used.
pub fn status(p: &mut Painter, app: &App) -> f32 {
    let pal = p.pal;
    let cy = TOP_H / 2.0;
    let mut x = p.w - MARGIN;
    let snap = app.snapshot.as_ref();

    if let Some(Ok(b)) = snap.map(|s| &s.battery) {
        let h = 18.0;
        x -= h * 2.0 + 6.0;
        let fill = if b.charging() {
            pal.success
        } else if b.percent <= 15 {
            pal.danger
        } else {
            pal.text
        };
        icons::battery(p, x, cy, h, b.percent, b.charging(), pal.text, fill);
        if app.settings.battery_percent {
            let label = format!("{}%", b.percent);
            x -= p.measure(&label, 21.0, Weight::Regular) + 10.0;
            p.text_mid(&label, x, cy, 21.0, Weight::Regular, pal.text);
        }
        x -= 26.0;
    }

    if let Some(Ok(w)) = snap.map(|s| &s.wifi) {
        if w.on && w.adapter {
            let bars = match (w.link.ssid.is_some(), w.link.signal) {
                (false, _) => 0,
                (true, Some(s)) => 1 + (s / 30).min(3),
                (true, None) => 4,
            };
            x -= 28.0;
            icons::wifi(p, x + 14.0, cy, 28.0, bars, pal.text, alpha(pal.text, 0.25));
            x -= 22.0;
        }
    }

    if let Some(Ok(s)) = snap.map(|s| &s.ssh) {
        if s.running {
            x -= 28.0;
            icons::terminal(p, x + 14.0, cy, 28.0, pal.success, mix(pal.bg_top, pal.bg_bottom, 0.3));
            x -= 22.0;
        }
    }

    let time = clock::now().time(app.settings.clock_24h);
    x -= p.measure(&time, 26.0, Weight::Display);
    p.text_mid(&time, x, cy, 26.0, Weight::Display, pal.text);
    x
}

/// Gizlet's mark and today's date at the top left.
pub fn brand(p: &mut Painter, app: &App) {
    let pal = p.pal;
    let cy = TOP_H / 2.0;
    icons::logo(p, MARGIN + 20.0, cy, 40.0, pal.accent, pal.on_accent);
    let x = MARGIN + 54.0;
    p.text_mid("Gizlet", x, cy - 11.0, 23.0, Weight::Display, pal.text);
    let date = clock::now().date(app.lang() == Language::ZhCn);
    p.text_mid(&date, x, cy + 14.0, 17.0, Weight::Regular, pal.dim);
}

/// A page title at the top left.
pub fn page_title(p: &mut Painter, title: &str) {
    let pal = p.pal;
    p.text_mid(title, MARGIN, TOP_H / 2.0 + 2.0, 32.0, Weight::Display, pal.text);
}

pub fn divider(p: &mut Painter, y: f32) {
    let pal = p.pal;
    p.rect(MARGIN * 0.5, y, p.w - MARGIN, 1.5, 0.0, mix(pal.line, pal.bg_bottom, 0.2));
}

/// Button hints along the bottom right, listed left to right, and the controllers on the left.
pub fn hints(p: &mut Painter, app: &App, items: &[(&str, &str)]) {
    let pal = p.pal;
    let top = p.h - BOTTOM_H;
    divider(p, top);
    let cy = top + BOTTOM_H / 2.0;
    let hole = mix(pal.bg_bottom, pal.bg_top, 0.5);

    icons::gamepad(p, MARGIN + 18.0, cy, 34.0, pal.dim, hole);
    if !app.gamepads.is_empty() {
        p.text_mid(&format!("× {}", app.gamepads.len()), MARGIN + 44.0, cy, 18.0, Weight::Regular, pal.dim);
    }

    let (size, r) = (21.0, 13.0);
    let mut x = p.w - MARGIN;
    for (g, label) in items.iter().rev() {
        let g = glyph(app, g);
        x -= p.measure(label, size, Weight::Regular);
        p.text_mid(label, x, cy, size, Weight::Regular, pal.text);
        x -= 8.0;
        if g.chars().count() == 1 && g.chars().all(|c| c.is_ascii_alphabetic()) {
            icons::button_glyph(p, g, x - r, cy, r, pal.text, hole);
            x -= r * 2.0;
        } else {
            let gw = icons::pill_width(p, g, r * 2.0);
            icons::pill_glyph(p, g, x - gw / 2.0, cy, r * 2.0, pal.text, hole);
            x -= gw;
        }
        x -= 30.0;
    }
}
