//! Home: the shelf of installed apps above the system buttons.

use super::chrome::{self, MARGIN};
use super::{approach, Ui, TILE_RADIUS_RATIO};
use crate::app::{App, HomeButton, HomeRow, Page, GRID_COLUMNS, GRID_PAGE_SIZE};
use crate::apps::{App as Package, Kind};
use crate::fonts::Weight;
use crate::i18n::T;
use crate::icons;
use crate::painter::Painter;
use crate::theme::{alpha, mix};
use raylib_sys::Color;

const TILE: f32 = 256.0;
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

    let favorites = app.favorites();
    if favorites.is_empty() {
        empty_card(p, app, T::NoFavorites, T::NoFavoritesBody, icons::gamepad);
    } else {
        shelf(ui, p, app, &favorites, on_tiles, dt);
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
            HomeButton::Apps => icons::grid(p, cx, cy, s * 0.9, pal.accent),
            HomeButton::Services => icons::chip(p, cx, cy, s * 0.9, pal.dim, hole),
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

fn empty_card(p: &mut Painter, app: &App, title: T, body: T, icon: fn(&Painter, f32, f32, f32, Color, Color)) {
    let pal = p.pal;
    let w = (TILE * 2.0 + GAP).min(p.w - MARGIN * 2.0);
    let x = (p.w - w) / 2.0;
    let y = TILES_Y;
    p.rect(x, y, w, TILE, TILE * TILE_RADIUS_RATIO, alpha(pal.surface, 0.7));
    p.outline(x, y, w, TILE, TILE * TILE_RADIUS_RATIO, 2.0, alpha(pal.line, 0.8));
    icon(p, x + w / 2.0, y + TILE * 0.33, 84.0, alpha(pal.dim, 0.8), pal.surface);
    p.text_center(app.t(title), x + w / 2.0, y + TILE * 0.6, 24.0, Weight::Display, pal.text);
    let lines = p.wrap(app.t(body), w - 60.0, 18.0, Weight::Regular);
    for (i, line) in lines.iter().take(2).enumerate() {
        p.text_center(line, x + w / 2.0, y + TILE * 0.74 + i as f32 * 26.0, 18.0, Weight::Regular, pal.dim);
    }
}

pub fn grid(ui: &mut Ui, p: &mut Painter, app: &App, dt: f32) {
    let services = app.page() == Page::Services;
    let pal = p.pal;
    chrome::page_title(p, app.t(if services { T::Services } else { T::Apps }));
    chrome::status(p, app);
    let mut hints = vec![("B", app.t(T::Back))];
    let count = app.grid_packages().count();
    if count == 0 {
        match (services, &app.services.list) {
            (true, None) => {
                icons::spinner(p, p.w / 2.0, TILES_Y + 80.0, 18.0, 4.0, pal.accent);
                p.text_center(app.t(T::ServicesReading), p.w / 2.0, TILES_Y + 142.0, 24.0, Weight::Display, pal.text);
            }
            (true, Some(Err(error))) => {
                p.text_center(app.t(T::ServicesUnavailable), p.w / 2.0, TILES_Y + 80.0, 24.0, Weight::Display, pal.text);
                for (index, line) in p.wrap(error, p.w - MARGIN * 2.0, 18.0, Weight::Regular).iter().take(4).enumerate() {
                    p.text_center(line, p.w / 2.0, TILES_Y + 125.0 + index as f32 * 26.0, 18.0, Weight::Regular, pal.dim);
                }
            }
            (true, _) => empty_card(p, app, T::NoServices, T::NoServicesBody, icons::chip),
            _ => empty_card(p, app, T::EmptyTitle, T::EmptyBody, icons::gamepad),
        }
        chrome::hints(p, app, &hints);
        return;
    }

    let tile = 108.0;
    let column_width = 168.0;
    let row_height = 168.0;
    let page = app.grid_tile / GRID_PAGE_SIZE;
    let first = page * GRID_PAGE_SIZE;
    let pages = count.div_ceil(GRID_PAGE_SIZE);
    let left = (p.w - column_width * GRID_COLUMNS as f32) / 2.0;
    let pop = ui.tile_pop.update((if services { 21 } else { 20 }, app.grid_tile), dt);
    let mut selected_starred = false;
    for (offset, package) in app.grid_packages().skip(first).take(GRID_PAGE_SIZE).enumerate() {
        let center = left + (offset % GRID_COLUMNS) as f32 * column_width + column_width / 2.0;
        let top = 118.0 + (offset / GRID_COLUMNS) as f32 * row_height;
        let x = center - tile / 2.0;
        let focused = first + offset == app.grid_tile;
        let starred = app.starred(package.id());
        package_tile(ui, p, app, package, x, top, tile);
        if focused {
            selected_starred = starred;
            focus_ring(p, x, top, tile, tile, tile * TILE_RADIUS_RATIO, pop);
        }
        if starred {
            icons::star_badge(p, x + tile - 18.0, top + 18.0, 13.0);
        }
        let name = p.fit(package.manifest.name_for(app.lang_code()), column_width - 14.0, 19.0, Weight::Regular);
        p.text_center(&name, center, top + tile + 25.0, 19.0, Weight::Regular, if focused { pal.text } else { pal.dim });
    }
    if pages > 1 {
        let current = format!("{} / {pages}", page + 1);
        p.text_center(&current, p.w / 2.0, p.h - chrome::BOTTOM_H - 18.0, 18.0, Weight::Regular, pal.dim);
        hints.push(("L/R", app.t(T::ChangePage)));
    }
    hints.push(("X", app.t(if selected_starred { T::Unstar } else { T::Star })));
    hints.push(("A", app.t(T::Open)));
    chrome::hints(p, app, &hints);
}

/// Favorites as square tiles in one row, scrolled to keep focus in view.
fn shelf(ui: &mut Ui, p: &mut Painter, app: &App, packages: &[&Package], on_tiles: bool, dt: f32) {
    let pal = p.pal;
    let n = packages.len();
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

    p.text_mid(app.t(T::Favorites), MARGIN, y - 32.0, 28.0, Weight::Display, if on_tiles { focus_color(p) } else { pal.dim });
    for (i, a) in packages.iter().enumerate() {
        let x = ui.shelf_x + i as f32 * step;
        if x + TILE < 0.0 || x > p.w {
            continue;
        }
        let radius = TILE * TILE_RADIUS_RATIO;
        package_tile(ui, p, app, a, x, y, TILE);
        let focused = on_tiles && app.home_tile == i;
        if focused {
            focus_ring(p, x, y, TILE, TILE, radius, pop);
            let name = p.fit(a.manifest.name_for(lang), TILE + GAP * 2.0, 22.0, Weight::Display);
            p.text_center(&name, x + TILE / 2.0, y + TILE + 30.0, 22.0, Weight::Display, pal.text);
        }
    }
}

fn package_tile(ui: &mut Ui, p: &mut Painter, app: &App, package: &Package, x: f32, y: f32, size: f32) {
    let pal = p.pal;
    let radius = size * TILE_RADIUS_RATIO;
    let base = package.manifest.rgb().map_or(pal.surface, |[r, g, b]| Color { r, g, b, a: 255 });
    p.shadow(x, y, size, size, radius, 10.0, 0.5);
    p.rect(x, y, size, size, radius, base);
    if let Some(texture) = package.icon().and_then(|path| ui.icons.get(&path)) {
        p.texture(texture, x, y, size, size, Color { r: 255, g: 255, b: 255, a: 255 });
    } else if package.manifest.kind() == Kind::Service {
        icons::chip(p, x + size / 2.0, y + size / 2.0, size * 0.48, pal.text, base);
    } else {
        let initial: String = package.manifest.name_for(app.lang_code()).chars().take(1).collect();
        p.text_center(&initial, x + size / 2.0, y + size / 2.0, size * 0.47, Weight::Display, pal.text);
    }
    p.outline(x, y, size, size, radius, 2.0, alpha(pal.line, 0.5));
}
