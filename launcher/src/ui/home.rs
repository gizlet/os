//! Home: the shelf of installed apps above the system buttons.

use super::chrome::{self, MARGIN};
use super::{approach, Ui};
use crate::app::{App, HomeButton, HomeRow};
use crate::fonts::Weight;
use crate::i18n::T;
use crate::icons;
use crate::painter::Painter;
use crate::theme::{alpha, mix};
use raylib_sys::Color;

const TILE: f32 = 256.0;
const CORNER: f32 = 0.11;
const GAP: f32 = 22.0;
const TILES_Y: f32 = 182.0;
const BUTTON_D: f32 = 76.0;
const BUTTON_GAP: f32 = 30.0;

/// The pulsing focus color; it settles midway as the pulse fades.
pub fn focus_color(p: &Painter) -> Color {
    let wave = (p.time * 3.2).sin() as f32 / 2.0;
    mix(p.pal.accent, p.pal.accent_hi, (0.5 + wave * p.pulse) * 0.75)
}

/// The focus ring around a rounded rectangle, with a short pop when focus arrives.
pub fn focus_ring(p: &Painter, x: f32, y: f32, w: f32, h: f32, radius: f32, pop: f32) {
    let gap = 5.0 + pop * 4.0;
    let thick = 5.0;
    let c = focus_color(p);
    if gap > 0.0 {
        // Cover the shadow inside the gap so the ring reads as separate.
        let bg = mix(p.pal.bg_top, p.pal.bg_bottom, 0.5);
        p.outline(x - gap, y - gap, w + 2.0 * gap, h + 2.0 * gap, radius + gap, gap + 0.5, bg);
    }
    let (x, y, w, h, r) = (
        x - gap - thick,
        y - gap - thick,
        w + 2.0 * (gap + thick),
        h + 2.0 * (gap + thick),
        radius + gap + thick,
    );
    p.outline(x - 4.0, y - 4.0, w + 8.0, h + 8.0, r + 4.0, 6.0, alpha(c, 0.18));
    p.outline(x, y, w, h, r, thick, c);
}

pub fn draw(ui: &mut Ui, p: &mut Painter, app: &App, dt: f32) {
    let pal = p.pal;
    chrome::brand(p, app);
    chrome::status(p, app);
    let on_tiles = app.home_row == HomeRow::Tiles;

    // The library changed: free the icons of apps that are gone or renamed them.
    let paths: Vec<_> = app.apps.iter().filter_map(|a| a.manifest.icon.as_ref().map(|i| a.dir.join(i))).collect();
    if paths != ui.icon_paths {
        ui.icons.keep_only(&paths);
        ui.icon_paths = paths;
    }
    if app.apps.is_empty() {
        empty_shelf(p, app, on_tiles);
    } else {
        shelf(ui, p, app, on_tiles, dt);
    }

    // System buttons, centered.
    let n = HomeButton::ALL.len() as f32;
    let row_w = n * BUTTON_D + (n - 1.0) * BUTTON_GAP;
    let cy = TILES_Y + TILE + 94.0;
    let pop = ui.button_pop.update((1, app.home_button), dt);
    for (i, b) in HomeButton::ALL.iter().enumerate() {
        let cx = p.w / 2.0 - row_w / 2.0 + BUTTON_D / 2.0 + i as f32 * (BUTTON_D + BUTTON_GAP);
        let focused = !on_tiles && app.home_button == i;
        let r = BUTTON_D / 2.0;
        p.shadow(cx - r, cy - r, BUTTON_D, BUTTON_D, r, 8.0, 0.5);
        p.circle(cx, cy, r, pal.surface);
        let (hole, s) = (pal.surface, 38.0);
        match b {
            HomeButton::Library => icons::grid(p, cx, cy, s * 0.9, pal.accent),
            HomeButton::Store => icons::bag(p, cx, cy, s, pal.warm, hole),
            HomeButton::ButtonTest => icons::gamepad(p, cx, cy, s * 1.05, pal.success, hole),
            HomeButton::Settings => icons::gear(p, cx, cy, s, pal.dim, hole),
            HomeButton::Power => icons::power(p, cx, cy, s * 0.95, pal.dim),
        }
        if focused {
            let g = 5.0 + pop * 3.0;
            let c = focus_color(p);
            p.ring(cx, cy, r + g, r + g + 5.0, 0.0, 360.0, c);
            p.ring(cx, cy, r + g + 5.0, r + g + 10.0, 0.0, 360.0, alpha(c, 0.18));
            p.text_center(app.t(b.title()), cx, cy + r + 34.0, 20.0, Weight::Regular, pal.text);
        }
    }

    chrome::hints(p, app, &[("A", app.t(T::Ok))]);
}

/// The shelf without apps: one wide card that says how to get some.
fn empty_shelf(p: &mut Painter, app: &App, on_tiles: bool) {
    let pal = p.pal;
    let w = (TILE * 2.0 + GAP).min(p.w - MARGIN * 2.0);
    let x = (p.w - w) / 2.0;
    let y = TILES_Y;
    if on_tiles {
        p.text_mid(app.t(T::Library), x, y - 32.0, 28.0, Weight::Display, focus_color(p));
    }
    p.rect(x, y, w, TILE, TILE * CORNER, alpha(pal.surface, 0.7));
    p.outline(x, y, w, TILE, TILE * CORNER, 2.0, alpha(pal.line, 0.8));
    icons::gamepad(p, x + w / 2.0, y + TILE * 0.33, 84.0, alpha(pal.dim, 0.8), pal.surface);
    p.text_center(app.t(T::EmptyTitle), x + w / 2.0, y + TILE * 0.6, 24.0, Weight::Display, pal.text);
    let lines = p.wrap(app.t(T::EmptyBody), w - 60.0, 18.0, Weight::Regular);
    for (i, line) in lines.iter().take(2).enumerate() {
        p.text_center(line, x + w / 2.0, y + TILE * 0.74 + i as f32 * 26.0, 18.0, Weight::Regular, pal.dim);
    }
    if on_tiles {
        focus_ring(p, x, y, w, TILE, TILE * CORNER, 0.0);
    }
}

/// Installed apps as square tiles in one row, scrolled to keep focus in view.
fn shelf(ui: &mut Ui, p: &mut Painter, app: &App, on_tiles: bool, dt: f32) {
    let pal = p.pal;
    let n = app.apps.len();
    let step = TILE + GAP;
    let row_w = n as f32 * step - GAP;
    let view_w = p.w - MARGIN * 2.0;
    // Centered when the row fits; otherwise the focused tile stays in view.
    let target = if row_w <= view_w {
        (p.w - row_w) / 2.0
    } else {
        let focus_x = app.home_tile as f32 * step;
        MARGIN - (focus_x + TILE / 2.0 - view_w / 2.0).clamp(0.0, row_w - view_w)
    };
    ui.shelf_x = if ui.shelf_x == 0.0 { target } else { approach(ui.shelf_x, target, 14.0, dt) };
    let y = TILES_Y;
    let pop = ui.tile_pop.update((2, app.home_tile), dt);
    let lang = app.lang_code();

    p.text_mid(app.t(T::Library), MARGIN, y - 32.0, 28.0, Weight::Display, if on_tiles { focus_color(p) } else { pal.dim });
    for (i, a) in app.apps.iter().enumerate() {
        let x = ui.shelf_x + i as f32 * step;
        if x + TILE < 0.0 || x > p.w {
            continue;
        }
        let radius = TILE * CORNER;
        p.shadow(x, y, TILE, TILE, radius, 10.0, 0.5);
        let base = a.manifest.rgb().map_or(pal.surface, |[r, g, b]| Color { r, g, b, a: 255 });
        p.rect(x, y, TILE, TILE, radius, base);
        match a.icon().and_then(|path| ui.icons.get(&path)) {
            // Square icons fill the tile inside its rounded corners.
            Some(tex) => {
                let inset = radius * 0.3;
                p.texture(tex, x + inset, y + inset, TILE - 2.0 * inset, TILE - 2.0 * inset, Color { r: 255, g: 255, b: 255, a: 255 });
            }
            None => {
                let name = a.manifest.name_for(lang);
                let initial: String = name.chars().take(1).collect();
                p.text_center(&initial, x + TILE / 2.0, y + TILE / 2.0, 120.0, Weight::Display, pal.text);
            }
        }
        p.outline(x, y, TILE, TILE, radius, 2.0, alpha(pal.line, 0.5));
        let focused = on_tiles && app.home_tile == i;
        if focused {
            focus_ring(p, x, y, TILE, TILE, radius, pop);
            let name = p.fit(a.manifest.name_for(lang), TILE + GAP * 2.0, 22.0, Weight::Display);
            p.text_center(&name, x + TILE / 2.0, y + TILE + 30.0, 22.0, Weight::Display, pal.text);
        }
    }
}
