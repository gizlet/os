//! The Store: its home (a hero card, updates and shelves), a gizlet's page,
//! search results, and what is installing. Art fades in over the gizlet's own
//! colour; a card opens into its page.

use super::chrome::{self, BOTTOM_H, MARGIN, TOP_H};
use super::home::{focus_color, focus_ring};
use super::{approach, ease_out, Shape, Ui, TILE_RADIUS_RATIO};
use crate::app::{store_error, App, Page, GRID_COLUMNS, GRID_PAGE_SIZE};
use crate::fonts::Weight;
use crate::i18n::{human_size, T};
use crate::icons;
use crate::painter::Painter;
use crate::storefront::{DetailFocus, Load, Offer, RowKind};
use crate::theme::{alpha, mix, rgb};
use gizlet_launcher::store::{Error, Progress, Summary};
use raylib_sys::Color;

const WHITE: Color = rgb(0xffffff);
const SHELF_TILE: f32 = 124.0;
const TILE_GAP: f32 = 26.0;
/// The lists' names above the list shown.
const TAB_H: f32 = 34.0;
/// A list: its names, a gap, the tiles, and a name and a line under each.
const LIST_H: f32 = TAB_H + 14.0 + SHELF_TILE + 58.0;
/// How long a list takes to slide in when L or R changes it.
const TAB_SLIDE: f64 = 0.25;
const HEADER_H: f32 = 300.0;
const EXPAND: f32 = 0.3;

/// A rectangle: x, y, width, height.
pub type Rect = (f32, f32, f32, f32);

fn tint(a: f32) -> Color {
    alpha(WHITE, a)
}

fn base_color(p: &Painter, g: &Summary) -> Color {
    g.rgb().map_or(p.pal.surface_hi, |[r, g, b]| Color { r, g, b, a: 255 })
}


/// A gizlet's tile: its colour and initial until its icon fades in over them.
fn tile(ui: &mut Ui, p: &mut Painter, app: &App, g: &Summary, x: f32, y: f32, size: f32) {
    let pal = p.pal;
    let radius = size * TILE_RADIUS_RATIO;
    let base = base_color(p, g);
    p.shadow(x, y, size, size, radius, 10.0, 0.5);
    p.rect(x, y, size, size, radius, base);
    let art = app.store.art_path(g.device_icon_url.as_deref()).and_then(|path| ui.art.get(path, Shape::Icon, app.time));
    let shown = art.map_or(0.0, |(_, fade)| ease_out(fade));
    if shown < 1.0 {
        if g.is_service() {
            icons::chip(p, x + size / 2.0, y + size / 2.0, size * 0.48, alpha(WHITE, 1.0 - shown), base);
        } else {
            let initial: String = g.name_for(app.lang_code()).chars().take(1).collect();
            p.text_center(&initial, x + size / 2.0, y + size / 2.0, size * 0.46, Weight::Display, tint(1.0 - shown));
        }
    }
    if let Some((texture, _)) = art {
        p.texture(texture, x, y, size, size, tint(shown));
    }
    p.outline(x, y, size, size, radius, 2.0, alpha(pal.line, 0.5));
}

/// An installed or update badge at a tile's top right corner.
fn badge(p: &mut Painter, app: &App, g: &Summary, x: f32, y: f32, size: f32) {
    let pal = p.pal;
    let (cx, cy, r) = (x + size - 14.0, y + 14.0, 13.0);
    if app.store.update_for(&g.id).is_some() && app.installed_kind(&g.id).is_some() {
        p.circle(cx, cy, r + 2.0, alpha(rgb(0x000000), 0.35));
        p.circle(cx, cy, r, pal.accent);
        // An arrow up: a newer version.
        let (t, s) = (2.4, 6.0);
        p.line(cx, cy + s, cx, cy - s, t, pal.on_accent);
        p.line(cx - s * 0.8, cy - s * 0.2, cx, cy - s, t, pal.on_accent);
        p.line(cx + s * 0.8, cy - s * 0.2, cx, cy - s, t, pal.on_accent);
    } else if app.installed_kind(&g.id).is_some() {
        p.circle(cx, cy, r + 2.0, alpha(rgb(0x000000), 0.35));
        p.circle(cx, cy, r, pal.success);
        icons::check(p, cx, cy, 15.0, WHITE);
    }
}

fn stars(p: &mut Painter, g: &Summary, x: f32, cy: f32, size: f32, c: Color) -> f32 {
    let Some(average) = g.rating.average.filter(|_| g.rating.count > 0) else { return 0.0 };
    icons::star(p, x + size * 0.45, cy, size * 0.45, rgb(0xffc23d));
    let label = format!("{average:.1}");
    let w = p.text_mid(&label, x + size + 4.0, cy, size, Weight::Bold, c);
    size + 4.0 + w
}

fn kind_label(app: &App, g: &Summary) -> &'static str {
    match (g.is_service(), g.category.as_deref()) {
        (true, _) => app.t(T::StoreService),
        (false, Some("game")) => app.t(T::StoreGame),
        _ => app.t(T::StoreApp),
    }
}

/// A small rounded label.
fn chip(p: &mut Painter, text: &str, x: f32, cy: f32, bg: Color, fg: Color) -> f32 {
    let w = p.measure(text, 16.0, Weight::Bold) + 24.0;
    p.rect(x, cy - 14.0, w, 28.0, 14.0, bg);
    p.text_center(text, x + w / 2.0, cy, 16.0, Weight::Bold, fg);
    w
}

pub fn home(ui: &mut Ui, p: &mut Painter, app: &App, dt: f32) {
    let pal = p.pal;
    chrome::page_title(p, app.t(T::Store));
    let left = chrome::status(p, app);
    if app.store.saved {
        let note = app.t(T::StoreSaved);
        let w = p.measure(note, 16.0, Weight::Regular) + 26.0;
        let x = (left - 24.0 - w).max(MARGIN + 140.0);
        p.rect(x, TOP_H / 2.0 - 15.0, w, 30.0, 15.0, alpha(pal.warm, 0.18));
        p.text_center(note, x + w / 2.0, TOP_H / 2.0, 16.0, Weight::Regular, pal.warm);
    }
    let mut hints = vec![("B", app.t(T::Back)), ("X", app.t(T::StoreLink)), ("Y", app.t(T::Search))];
    let store = &app.store;
    if store.rows.is_empty() {
        state_card(p, app, &store.home);
        if matches!(store.home, Load::Failed(_)) {
            let retry = if matches!(store.home, Load::Failed(Error::Offline)) { T::StoreWifi } else { T::StoreRetry };
            hints.push(("A", app.t(retry)));
        }
        chrome::hints(p, app, &hints);
        job_pill(p, app);
        return;
    }

    // Everything fits, nothing scrolls: the hero card, and under it the list L and R choose.
    let (top, bottom) = (TOP_H + 10.0, p.h - BOTTOM_H - 12.0);
    let lists = store.lists();
    let pop = ui.tile_pop.update((30 + store.row as u8, store.cols[store.row]), dt);
    if lists.start > 0 {
        let h = if lists.is_empty() { (bottom - top).min(380.0) } else { bottom - top - LIST_H - 20.0 };
        hero(ui, p, app, &store.rows[0].cards, store.cols[0], top, h, store.row == 0, pop);
    }
    if !lists.is_empty() {
        let y = if lists.start > 0 { bottom - LIST_H } else { top };
        let on = store.row == store.tab;
        tabs(p, app, y, on);
        if ui.store_tab.0 != store.tab {
            ui.store_tab = (store.tab, app.time);
        }
        shelf(ui, p, app, store.tab, y + TAB_H + 14.0, on, pop);
    }
    hints.push(("A", app.t(if store.update_all_focused() { T::StoreUpdateAll } else { T::Open })));
    chrome::hints(p, app, &hints);
    job_pill(p, app);
}

/// What the home shows without lists: loading, offline or unreachable.
fn state_card(p: &mut Painter, app: &App, home: &Load<gizlet_launcher::store::Featured>) {
    let pal = p.pal;
    let w = 560.0f32.min(p.w - MARGIN * 2.0);
    let (x, y, h) = ((p.w - w) / 2.0, TOP_H + 70.0, 300.0);
    p.rect(x, y, w, h, 28.0, alpha(pal.surface, 0.75));
    p.outline(x, y, w, h, 28.0, 2.0, alpha(pal.line, 0.8));
    let cx = x + w / 2.0;
    let (title, body) = match home {
        Load::Loading => {
            icons::spinner(p, cx, y + 100.0, 22.0, 5.0, pal.accent);
            (app.t(T::StoreLoading).to_string(), String::new())
        }
        Load::Failed(Error::Offline) => {
            icons::wifi(p, cx, y + 100.0, 72.0, 0, pal.dim, alpha(pal.dim, 0.3));
            (app.t(T::StoreOffline).to_string(), app.t(T::StoreOfflineBody).to_string())
        }
        Load::Failed(error) => {
            icons::bag(p, cx, y + 100.0, 72.0, alpha(pal.dim, 0.8), pal.surface);
            (app.t(T::StoreUnreachable).to_string(), error.to_string())
        }
        Load::Ready(_) => {
            icons::bag(p, cx, y + 100.0, 72.0, alpha(pal.dim, 0.8), pal.surface);
            (app.t(T::StoreNoResults).to_string(), String::new())
        }
    };
    p.text_center(&title, cx, y + 178.0, 26.0, Weight::Display, pal.text);
    for (i, line) in p.wrap(&body, w - 80.0, 18.0, Weight::Regular).iter().take(3).enumerate() {
        p.text_center(line, cx, y + 218.0 + i as f32 * 26.0, 18.0, Weight::Regular, pal.dim);
    }
}

/// The large card: a gizlet's cover, name and description; Left and Right go through them.
#[allow(clippy::too_many_arguments)]
fn hero(ui: &mut Ui, p: &mut Painter, app: &App, cards: &[Summary], index: usize, y: f32, h: f32, focused: bool, pop: f32) {
    let Some(g) = cards.get(index) else { return };
    let pal = p.pal;
    let (x, w) = (MARGIN, p.w - MARGIN * 2.0);
    let radius = 26.0;
    if ui.hero.0 != index {
        ui.hero = (index, app.time);
    }
    let switched = ease_out(((app.time - ui.hero.1) / 0.35) as f32);
    let base = base_color(p, g);
    p.shadow(x, y, w, h, radius, 16.0, 0.6);
    p.rect(x, y, w, h, radius, mix(base, pal.bg_bottom, 0.35));
    let cover = app.store.art_path(g.device_cover_url.as_deref());
    let shape = Shape::frame(w / h, radius / h, true);
    let mut covered = false;
    if let Some((texture, fade)) = cover.and_then(|path| ui.art.get(path, shape, app.time)) {
        p.texture(texture, x, y, w, h, tint(ease_out(fade).min(switched)));
        covered = true;
    }
    if !covered {
        // No cover: the gizlet's icon, large, at the right.
        let size = h * 0.62;
        tile(ui, p, app, g, x + w - size - 48.0, y + (h - size) / 2.0, size);
    }
    if focused {
        focus_ring(p, x, y, w, h, radius, pop);
    }

    // Name, kind, description and rating, sliding in when the card changes.
    let tx = x + 40.0 + (1.0 - switched) * 24.0;
    let text_w = w * 0.56;
    let a = switched;
    let mut cx = tx;
    cx += chip(p, kind_label(app, g), cx, y + 46.0, tint(0.2 * a), tint(a)) + 10.0;
    if app.installed_kind(&g.id).is_some() {
        chip(p, app.t(T::StoreInstalledChip), cx, y + 46.0, alpha(pal.success, 0.85 * a), tint(a));
    }
    let name = p.fit(g.name_for(app.lang_code()), text_w, 42.0, Weight::Display);
    p.text_mid(&name, tx, y + 104.0, 42.0, Weight::Display, tint(a));
    let description = g.description_for(app.lang_code()).unwrap_or("");
    for (i, line) in p.wrap(description, text_w, 20.0, Weight::Regular).iter().take(3).enumerate() {
        p.text_mid(line, tx, y + 152.0 + i as f32 * 28.0, 20.0, Weight::Regular, tint(0.86 * a));
    }
    stars(p, g, tx, y + h - 40.0, 20.0, tint(a));

    // Which card of how many.
    if cards.len() > 1 {
        let n = cards.len() as f32;
        let dots_w = (n - 1.0) * 18.0;
        let dx = x + w - 40.0 - dots_w;
        for i in 0..cards.len() {
            let on = i == index;
            p.circle(dx + i as f32 * 18.0, y + h - 30.0, if on { 5.5 } else { 4.0 }, tint(if on { 1.0 } else { 0.45 }));
        }
        if focused {
            icons::chevron(p, x + w - 26.0, y + h / 2.0, 22.0, true, tint(0.7));
            if index > 0 {
                icons::chevron(p, x + 20.0, y + h / 2.0, 22.0, false, tint(0.7));
            }
        }
    }
    if focused {
        ui.card_rect = Some((x, y, w, h));
    }
}

/// The title of a list on the home page.
fn list_title(app: &App, row: &crate::storefront::Row) -> String {
    match row.kind {
        RowKind::Updates => app.t(T::StoreUpdates).to_string(),
        _ => localized_title(app, row),
    }
}

/// The lists' names in a strip, the one shown underlined, between the L and R
/// that go through them. Names shrink to fit; past that, the strip keeps the
/// shown one in view.
fn tabs(p: &mut Painter, app: &App, y: f32, focused: bool) {
    let pal = p.pal;
    let store = &app.store;
    let lists = store.lists();
    let cy = y + TAB_H / 2.0 - 2.0;
    let hole = mix(pal.bg_bottom, pal.bg_top, 0.5);
    let updates = store.updates.iter().filter(|u| u.has_update()).count().to_string();
    let badge_w = p.measure(&updates, 15.0, Weight::Bold).max(8.0) + 16.0;
    let many = lists.len() > 1;
    let (lg, rg) = (chrome::glyph(app, "L").to_string(), chrome::glyph(app, "R").to_string());
    let (lw, rw) = (icons::pill_width(p, &lg, 26.0), icons::pill_width(p, &rg, 26.0));
    let (left, right) = if many { (MARGIN + lw + 18.0, p.w - MARGIN - rw - 18.0) } else { (MARGIN, p.w - MARGIN) };
    let gap = 30.0;
    let names: Vec<(String, bool)> = lists.clone().map(|r| (list_title(app, &store.rows[r]), store.rows[r].kind == RowKind::Updates)).collect();
    let widths = |p: &mut Painter, size: f32| -> Vec<f32> {
        names.iter().map(|(name, updates)| p.measure(name, size, Weight::Display) + if *updates { badge_w + 10.0 } else { 0.0 }).collect()
    };
    let mut size = 23.0;
    let mut ws = widths(p, size);
    while size > 17.0 && ws.iter().sum::<f32>() + gap * (ws.len() as f32 - 1.0) > right - left {
        size -= 1.0;
        ws = widths(p, size);
    }
    let shown = store.tab - lists.start;
    let end_of_shown: f32 = ws[..=shown].iter().sum::<f32>() + gap * shown as f32;
    let shift = (end_of_shown - (right - left)).max(0.0);
    p.clip(left - 4.0, y - 6.0, right - left + 8.0, TAB_H + 12.0);
    let mut x = left - shift;
    for (i, ((name, is_updates), w)) in names.iter().zip(&ws).enumerate() {
        let on = i == shown;
        let c = match (on, focused) {
            (true, true) => focus_color(p),
            (true, false) => pal.text,
            _ => alpha(pal.dim, 0.85),
        };
        let tw = p.text_mid(name, x, cy, size, Weight::Display, c);
        if *is_updates {
            let bx = x + tw + 10.0;
            p.rect(bx, cy - 12.0, badge_w, 24.0, 12.0, pal.accent);
            p.text_center(&updates, bx + badge_w / 2.0, cy, 15.0, Weight::Bold, pal.on_accent);
        }
        if on {
            p.rect(x, y + TAB_H - 4.0, *w, 4.0, 2.0, if focused { focus_color(p) } else { alpha(pal.text, 0.5) });
        }
        x += w + gap;
    }
    p.unclip();
    if many {
        let first = store.tab == lists.start;
        let last = store.tab + 1 == lists.end;
        icons::pill_glyph(p, &lg, MARGIN + lw / 2.0, cy, 26.0, alpha(pal.dim, if first { 0.35 } else { 0.9 }), hole);
        icons::pill_glyph(p, &rg, p.w - MARGIN - rw / 2.0, cy, 26.0, alpha(pal.dim, if last { 0.35 } else { 0.9 }), hole);
    }
}

/// The list shown: its icons in one row, made smaller to fit across, sliding
/// in from the side L or R came from.
#[allow(clippy::too_many_arguments)]
fn shelf(ui: &mut Ui, p: &mut Painter, app: &App, r: usize, ty: f32, focused: bool, pop: f32) {
    let pal = p.pal;
    let row = &app.store.rows[r];
    let updates = row.kind == RowKind::Updates;
    let slots = row.slots();
    let view_w = p.w - MARGIN * 2.0;
    let size = SHELF_TILE.min((view_w + TILE_GAP) / slots.max(1) as f32 - TILE_GAP);
    let step = size + TILE_GAP;
    let col = app.store.cols[r];
    let slide = 1.0 - ease_out(((app.time - ui.store_tab.1) / TAB_SLIDE) as f32);
    let x0 = MARGIN + slide * 40.0;
    for slot in 0..slots {
        let x = x0 + slot as f32 * step;
        let on = focused && slot == col;
        let label_c = if on { pal.text } else { pal.dim };
        let Some(g) = row.card(slot) else {
            // "Update all": a button the size of a tile.
            let radius = size * TILE_RADIUS_RATIO;
            p.shadow(x, ty, size, size, radius, 10.0, 0.5);
            p.rect(x, ty, size, size, radius, pal.accent);
            icons::restart(p, x + size / 2.0, ty + size / 2.0, size * 0.42, pal.on_accent);
            if on {
                focus_ring(p, x, ty, size, size, radius, pop);
            }
            let label = p.fit(app.t(T::StoreUpdateAll), step - 6.0, 18.0, Weight::Regular);
            p.text_center(&label, x + size / 2.0, ty + size + 24.0, 18.0, Weight::Regular, label_c);
            continue;
        };
        tile(ui, p, app, g, x, ty, size);
        badge(p, app, g, x, ty, size);
        if on {
            focus_ring(p, x, ty, size, size, size * TILE_RADIUS_RATIO, pop);
            ui.card_rect = Some((x, ty, size, size));
        }
        let name = p.fit(g.name_for(app.lang_code()), step - 6.0, 18.0, if on { Weight::Bold } else { Weight::Regular });
        p.text_center(&name, x + size / 2.0, ty + size + 24.0, 18.0, if on { Weight::Bold } else { Weight::Regular }, label_c);
        // Below the name: the new version for an update, else the rating or kind.
        let meta_y = ty + size + 48.0;
        if updates {
            let installed = app.store.update_for(&g.id).map(|u| u.installed.clone()).unwrap_or_default();
            let line = format!("{installed} → {}", g.version.as_deref().unwrap_or(""));
            let line = p.fit(&line, step - 6.0, 15.0, Weight::Regular);
            p.text_center(&line, x + size / 2.0, meta_y, 15.0, Weight::Regular, pal.accent);
        } else if g.rating.count > 0 {
            let w = 15.0 + 4.0 + p.measure(&format!("{:.1}", g.rating.average.unwrap_or(0.0)), 15.0, Weight::Bold);
            stars(p, g, x + size / 2.0 - w / 2.0, meta_y, 15.0, pal.dim);
        } else {
            p.text_center(kind_label(app, g), x + size / 2.0, meta_y, 15.0, Weight::Regular, alpha(pal.dim, 0.8));
        }
    }
}

fn localized_title(app: &App, row: &crate::storefront::Row) -> String {
    let lang = app.lang_code();
    let base = lang.split('-').next().unwrap_or(lang);
    row.titles.iter().find(|(k, _)| k.split('-').next() == Some(base)).map_or_else(|| row.title.clone(), |(_, v)| v.clone())
}

/// What is installing, over every Store page: a name, a bar and how far.
fn job_pill(p: &mut Painter, app: &App) {
    let pal = p.pal;
    let Some(job) = &app.store.job else { return };
    // The gizlet's own page shows it on its button.
    if app.page() == Page::StoreDetail && app.store.detail.as_ref().is_some_and(|d| d.repo == job.repo) {
        return;
    }
    let (label, share) = job_label(app, job.removing(), job.progress);
    let waiting = app.store.queue.len();
    let text = if waiting > 0 { format!("{} · {label} · +{waiting}", job.name) } else { format!("{} · {label}", job.name) };
    let w = (p.measure(&text, 17.0, Weight::Regular) + 70.0).min(p.w - MARGIN * 2.0);
    let (x, y, h) = (p.w - MARGIN - w, p.h - BOTTOM_H - 62.0, 44.0);
    p.shadow(x, y, w, h, 22.0, 12.0, 0.6);
    p.rect(x, y, w, h, 22.0, pal.surface_hi);
    if let Some(share) = share {
        p.rect(x, y, w * share, h, 22.0, alpha(pal.accent, 0.35));
    }
    icons::spinner(p, x + 24.0, y + h / 2.0, 9.0, 3.0, pal.accent);
    let text = p.fit(&text, w - 56.0, 17.0, Weight::Regular);
    p.text_mid(&text, x + 44.0, y + h / 2.0, 17.0, Weight::Regular, pal.text);
}

fn job_label(app: &App, removing: bool, progress: Option<Progress>) -> (String, Option<f32>) {
    match (removing, progress) {
        (true, _) => (app.t(T::StoreRemoving).into(), None),
        (false, Some(Progress::Downloading(done, total))) if total > 0 => {
            let share = done as f32 / total as f32;
            (T::StoreDownloading.fmt(app.lang(), &[&((share * 100.0) as u32).to_string()]), Some(share))
        }
        (false, Some(Progress::Installing)) => (app.t(T::StoreInstalling).into(), Some(1.0)),
        _ => (T::StoreDownloading.fmt(app.lang(), &["0"]), Some(0.0)),
    }
}

/// A gizlet's page: its cover, what it is, what can be done, and more about it.
pub fn detail(ui: &mut Ui, p: &mut Painter, app: &App, dt: f32) {
    let pal = p.pal;
    let Some(view) = &app.store.detail else { return };
    let Some(g) = view.summary() else {
        // A link opened: nothing to show but that it is loading or failed.
        chrome::page_title(p, app.t(T::Store));
        chrome::status(p, app);
        failed_or_loading(p, app, &view.data);
        chrome::hints(p, app, &[("B", app.t(T::Back))]);
        return;
    };
    let lang = app.lang_code();
    let installed = app.installed_kind(&g.id);
    let offers = app.store_offers();
    let data = view.data.ready();
    let shots = data.map_or(&[][..], |d| d.device_screenshots.as_slice());

    // The card the page opened from grows into the header.
    let opened = ui.expand.map_or(1.0, |(_, since)| ease_out(((app.time - since) / EXPAND as f64) as f32));
    let header: Rect = (0.0, 0.0, p.w, HEADER_H);
    let from = ui.expand.map_or(header, |(rect, _)| rect);
    let lerp = |a: f32, b: f32| a + (b - a) * opened;
    let (hx, hy, hw, hh) = (lerp(from.0, header.0), lerp(from.1, header.1), lerp(from.2, header.2), lerp(from.3, header.3));
    let content = opened;

    let scroll_target = match view.focus {
        DetailFocus::Actions(_) => 0.0,
        DetailFocus::Shots(_) => 250.0,
        // Far enough to show the end of the description and the controls.
        DetailFocus::About => (if shots.is_empty() { 250.0f32 } else { 520.0 }).max(ui.detail_end - (p.h - BOTTOM_H - 16.0)),
    };
    ui.detail_y = approach(ui.detail_y, scroll_target, 12.0, dt);
    let sy = -ui.detail_y;

    // Header: the cover under a scrim; without one, the icon blown up behind
    // it, which reads as a soft backdrop in the gizlet's colours.
    let base = base_color(p, g);
    let header_y = hy + sy;
    p.rect(hx, header_y, hw, hh, 22.0 * (1.0 - opened), mix(base, pal.bg_bottom, 0.45));
    let shape = Shape::frame(p.w / HEADER_H, 0.0, true);
    let backdrop = match app.store.art_path(g.device_cover_url.as_deref()) {
        Some(cover) => ui.art.get(cover, shape, app.time).map(|(t, fade)| (t, ease_out(fade))),
        None => app.store.art_path(g.device_icon_url.as_deref()).and_then(|icon| ui.art.get(icon, shape, app.time)).map(|(t, fade)| (t, ease_out(fade) * 0.55)),
    };
    if let Some((texture, shown)) = backdrop {
        p.texture(texture, hx, header_y, hw, hh, tint(shown));
    }
    if opened >= 1.0 {
        p.gradient_v(0.0, header_y + hh - 110.0, p.w, 110.0, alpha(pal.bg_top, 0.0), mix(pal.bg_top, pal.bg_bottom, 0.25));
    }

    let a = content;
    let fg = alpha(pal.text, a);
    let dim = alpha(pal.dim, a);
    // Icon, name and what it is.
    let icon = 116.0;
    let (ix, iy) = (MARGIN, header_y + HEADER_H - icon * 0.62);
    if a > 0.01 {
        tile(ui, p, app, g, ix, iy, icon);
        let nx = ix + icon + 28.0;
        let name = p.fit(g.name_for(lang), p.w - nx - MARGIN, 40.0, Weight::Display);
        p.text_mid(&name, nx, header_y + HEADER_H - 46.0, 40.0, Weight::Display, tint(a));
        let by = data.map(|d| d.owner.clone()).filter(|o| !o.is_empty()).unwrap_or_else(|| owner_of(&view.repo));
        let version = g.version.as_deref().map(|v| format!(" · {v}")).unwrap_or_default();
        let line = format!("{by} · {}{version}", kind_label(app, g));
        p.text_mid(&line, nx, header_y + HEADER_H + 4.0, 19.0, Weight::Regular, dim);
        // Chips: unlisted, service, and where the installed copy came from.
        let mut cx = nx;
        let cy = header_y + HEADER_H + 40.0;
        if g.is_unlisted() {
            cx += chip(p, app.t(T::StoreUnlisted), cx, cy, alpha(pal.warm, 0.2 * a), alpha(pal.warm, a)) + 10.0;
        }
        if g.is_service() {
            cx += chip(p, app.t(T::StoreService), cx, cy, alpha(pal.accent, 0.2 * a), alpha(pal.accent, a)) + 10.0;
        }
        if let Some(license) = &g.license {
            chip(p, license, cx, cy, alpha(pal.surface_hi, a), dim);
        }
    }

    // What can be done: the first is the main button, and shows progress.
    let ay = header_y + HEADER_H + 78.0;
    let mut ax = MARGIN;
    let action_pop = ui.list_pop.update((40, match view.focus { DetailFocus::Actions(i) => i, _ => 99 }), dt);
    if offers.is_empty() && data.is_some() && installed.is_none() {
        p.text_mid(app.t(T::StoreNoPackage), ax, ay + 28.0, 20.0, Weight::Regular, dim);
    }
    for (i, offer) in offers.iter().enumerate() {
        let primary = i == 0;
        let (label, share) = match offer {
            Offer::Install => (app.t(T::StoreInstall).to_string(), None),
            Offer::Update(v) => (T::StoreUpdateTo.fmt(app.lang(), &[v]), None),
            Offer::Open => (app.t(T::Open).to_string(), None),
            Offer::Manage => (app.t(T::StoreManage).to_string(), None),
            Offer::Uninstall => (app.t(T::StoreUninstall).to_string(), None),
            Offer::Queued => (app.t(T::StoreQueued).to_string(), None),
            Offer::Busy(progress) => {
                let removing = app.store.job.as_ref().is_some_and(|j| j.removing());
                job_label(app, removing, *progress)
            }
        };
        let w = (p.measure(&label, 21.0, Weight::Bold) + 56.0).max(if primary { 210.0 } else { 150.0 });
        let h = 56.0;
        let (bg, text_c) = if primary && !matches!(offer, Offer::Uninstall) { (pal.accent, pal.on_accent) } else { (pal.surface_hi, pal.text) };
        let danger = matches!(offer, Offer::Uninstall);
        if let Some(share) = share {
            p.rect(ax, ay, w, h, h / 2.0, alpha(pal.accent, 0.25 * a));
            p.rect(ax, ay, (w * share).max(h), h, h / 2.0, alpha(pal.accent, a));
        } else {
            p.rect(ax, ay, w, h, h / 2.0, alpha(bg, a));
        }
        let text_c = if share.is_some() { pal.on_accent } else if danger { pal.danger } else { text_c };
        p.text_center(&label, ax + w / 2.0, ay + h / 2.0, 21.0, Weight::Bold, alpha(text_c, a));
        if view.focus == DetailFocus::Actions(i) && view.viewer.is_none() {
            focus_ring(p, ax, ay, w, h, h / 2.0, action_pop);
        }
        ax += w + 18.0;
    }
    if app.store.other_source(installed) {
        let note = p.fit(app.t(T::StoreOtherSource), p.w - ax - MARGIN, 17.0, Weight::Regular);
        p.text_mid(&note, ax + 6.0, ay + 28.0, 17.0, Weight::Regular, dim);
    } else if g.is_unlisted() {
        let note = p.fit(app.t(T::StoreUnlistedNote), p.w - ax - MARGIN, 17.0, Weight::Regular);
        p.text_mid(&note, ax + 6.0, ay + 28.0, 17.0, Weight::Regular, dim);
    }

    // Facts in a strip.
    let fy = ay + 96.0;
    let size = data.and_then(|d| d.package_here()).and_then(|p| p.size).map(human_size);
    let rating = g.rating.average.filter(|_| g.rating.count > 0).map(|v| format!("★ {v:.1}  ({})", g.rating.count));
    let facts: Vec<(T, String)> = vec![
        (T::StoreRating, rating.unwrap_or_else(|| app.t(T::StoreNoRating).into())),
        (T::StoreVersion, g.version.clone().unwrap_or_else(|| "—".into())),
        (T::StoreSize, size.unwrap_or_else(|| "—".into())),
        (T::StoreDownloads, compact(g.downloads)),
    ];
    let col_w = (p.w - MARGIN * 2.0) / facts.len() as f32;
    p.rect(MARGIN, fy, p.w - MARGIN * 2.0, 84.0, 18.0, alpha(pal.surface, 0.7 * a));
    for (i, (label, value)) in facts.iter().enumerate() {
        let cx = MARGIN + col_w * (i as f32 + 0.5);
        p.text_center(app.t(*label), cx, fy + 26.0, 15.0, Weight::Regular, dim);
        let value = p.fit(value, col_w - 20.0, 23.0, Weight::Display);
        p.text_center(&value, cx, fy + 56.0, 23.0, Weight::Display, fg);
        if i > 0 {
            p.rect(MARGIN + col_w * i as f32, fy + 18.0, 1.5, 48.0, 0.0, alpha(pal.line, 0.8 * a));
        }
    }

    // Screenshots, as the handheld shows them.
    let mut y = fy + 124.0;
    if !shots.is_empty() {
        p.text_mid(app.t(T::StoreScreenshots), MARGIN, y, 24.0, Weight::Display, if matches!(view.focus, DetailFocus::Shots(_)) { focus_color(p) } else { fg });
        let (sw, sh) = (288.0, 216.0);
        let step = sw + 22.0;
        let focus = match view.focus { DetailFocus::Shots(i) => i, _ => 0 };
        let row_w = shots.len() as f32 * step - 22.0;
        let view_w = p.w - MARGIN * 2.0;
        let target = MARGIN - (focus as f32 * step + sw / 2.0 - view_w / 2.0).clamp(0.0, (row_w - view_w).max(0.0));
        ui.shots_x = if ui.shots_x.is_nan() { target } else { approach(ui.shots_x, target, 14.0, dt) };
        let pop = ui.button_pop.update((41, focus), dt);
        for (i, url) in shots.iter().enumerate() {
            let x = ui.shots_x + i as f32 * step;
            let sy = y + 30.0;
            if x + sw < 0.0 || x > p.w {
                continue;
            }
            p.rect(x, sy, sw, sh, 16.0, alpha(pal.surface, a));
            let art = app.store.art_path(Some(url)).and_then(|path| ui.art.get(path, Shape::frame(4.0 / 3.0, 16.0 / sh, false), app.time));
            match art {
                Some((texture, fade)) => p.texture(texture, x, sy, sw, sh, tint(ease_out(fade) * a)),
                None => icons::spinner(p, x + sw / 2.0, sy + sh / 2.0, 14.0, 3.0, alpha(pal.dim, 0.6 * a)),
            }
            if view.focus == DetailFocus::Shots(i) && view.viewer.is_none() {
                focus_ring(p, x, sy, sw, sh, 16.0, pop);
            }
        }
        y += 30.0 + sh + 46.0;
    } else {
        ui.shots_x = f32::NAN;
    }

    // About and controls side by side.
    let about_c = if view.focus == DetailFocus::About { focus_color(p) } else { fg };
    let half = (p.w - MARGIN * 2.0 - 40.0) / 2.0;
    p.text_mid(app.t(T::StoreAbout), MARGIN, y, 24.0, Weight::Display, about_c);
    let description = g.description_for(lang).unwrap_or("");
    let mut ty = y + 38.0;
    for line in p.wrap(description, half, 19.0, Weight::Regular).iter().take(8) {
        p.text_mid(line, MARGIN, ty, 19.0, Weight::Regular, fg);
        ty += 28.0;
    }
    if !g.tags.is_empty() {
        let mut tx = MARGIN;
        for tag in g.tags.iter().take(6) {
            let label = format!("#{tag}");
            let w = p.measure(&label, 15.0, Weight::Regular) + 20.0;
            if tx + w > MARGIN + half {
                break;
            }
            p.rect(tx, ty - 4.0, w, 26.0, 13.0, alpha(pal.surface_hi, a));
            p.text_center(&label, tx + w / 2.0, ty + 9.0, 15.0, Weight::Regular, dim);
            tx += w + 8.0;
        }
    }
    if let Some(controls) = data.map(|d| &d.controls).filter(|c| !c.is_empty()) {
        let cx = MARGIN + half + 40.0;
        p.text_mid(app.t(T::StoreControls), cx, y, 24.0, Weight::Display, about_c);
        let hole = mix(pal.bg_bottom, pal.bg_top, 0.5);
        for (i, control) in controls.iter().take(8).enumerate() {
            let cy = y + 42.0 + i as f32 * 38.0;
            let label = control_glyph(app, &control.button);
            if label.chars().count() == 1 {
                icons::button_glyph(p, &label, cx + 15.0, cy, 15.0, alpha(pal.text, a), hole);
            } else {
                let gw = icons::pill_width(p, &label, 28.0);
                icons::pill_glyph(p, &label, cx + gw / 2.0, cy, 28.0, alpha(pal.text, a), hole);
            }
            let action = p.fit(control.action_for(lang), half - 90.0, 19.0, Weight::Regular);
            p.text_mid(&action, cx + 78.0, cy, 19.0, Weight::Regular, fg);
        }
    }
    if let Load::Failed(error) = &view.data {
        p.text_mid(&store_error(app.lang(), error), MARGIN, y + 38.0, 18.0, Weight::Regular, pal.danger);
    }
    let controls = data.map_or(0, |d| d.controls.len().min(8));
    let end = (if g.tags.is_empty() { ty } else { ty + 26.0 }).max(y + 42.0 + controls as f32 * 38.0);
    ui.detail_end = end + ui.detail_y;
    // The hint bar covers what scrolls under it.
    p.rect(0.0, p.h - BOTTOM_H, p.w, BOTTOM_H, 0.0, pal.bg_bottom);

    // Scrolled: the status bar gets a backdrop, fading in as the header leaves.
    let covered = (ui.detail_y / 80.0).clamp(0.0, 1.0);
    if covered > 0.0 {
        let top = mix(pal.bg_top, pal.bg_bottom, 0.1);
        p.rect(0.0, 0.0, p.w, TOP_H - 10.0, 0.0, alpha(top, covered));
        p.gradient_v(0.0, TOP_H - 10.0, p.w, 30.0, alpha(top, covered), alpha(top, 0.0));
    }
    chrome::status(p, app);

    let mut hints = vec![("B", app.t(T::Back))];
    let action = match view.focus {
        DetailFocus::Actions(i) => offers.get(i).and_then(|o| offer_verb(app, o)),
        DetailFocus::Shots(_) => Some(app.t(T::StoreFullScreen).to_string()),
        DetailFocus::About => None,
    };
    if let Some(action) = &action {
        hints.push(("A", action));
    }
    chrome::hints(p, app, &hints);
    job_pill(p, app);

    if let Some(index) = view.viewer {
        viewer(ui, p, app, shots, index);
    }
}

/// What A does to an offer, for its hint.
fn offer_verb(app: &App, offer: &Offer) -> Option<String> {
    Some(match offer {
        Offer::Install => app.t(T::StoreInstall).into(),
        Offer::Update(_) => app.t(T::StoreUpdate).into(),
        Offer::Open => app.t(T::Open).into(),
        Offer::Manage => app.t(T::StoreManage).into(),
        Offer::Uninstall => app.t(T::StoreUninstall).into(),
        Offer::Busy(_) | Offer::Queued => return None,
    })
}

/// A screenshot over the whole screen.
fn viewer(ui: &mut Ui, p: &mut Painter, app: &App, shots: &[String], index: usize) {
    let pal = p.pal;
    p.rect(0.0, 0.0, p.w, p.h, 0.0, rgb(0x05060a));
    let (max_w, max_h) = (p.w - 120.0, p.h - 150.0);
    let (w, h) = if max_w / max_h > 4.0 / 3.0 { (max_h * 4.0 / 3.0, max_h) } else { (max_w, max_w * 3.0 / 4.0) };
    let (x, y) = ((p.w - w) / 2.0, 40.0);
    if let Some(url) = shots.get(index) {
        let art = app.store.art_path(Some(url)).and_then(|path| ui.art.get(path, Shape::frame(4.0 / 3.0, 12.0 / 216.0, false), app.time));
        match art {
            Some((texture, _)) => p.texture(texture, x, y, w, h, WHITE),
            None => icons::spinner(p, p.w / 2.0, y + h / 2.0, 20.0, 4.0, pal.accent),
        }
    }
    let n = shots.len();
    for i in 0..n {
        let on = i == index;
        p.circle(p.w / 2.0 + (i as f32 - (n as f32 - 1.0) / 2.0) * 20.0, y + h + 30.0, if on { 6.0 } else { 4.0 }, tint(if on { 1.0 } else { 0.4 }));
    }
    if index > 0 {
        icons::chevron(p, x - 30.0, y + h / 2.0, 30.0, false, tint(0.8));
    }
    if index + 1 < n {
        icons::chevron(p, x + w + 30.0, y + h / 2.0, 30.0, true, tint(0.8));
    }
    chrome::hints(p, app, &[("B", app.t(T::Close))]);
}

fn failed_or_loading(p: &mut Painter, app: &App, data: &Load<gizlet_launcher::store::Detail>) {
    let pal = p.pal;
    let cy = p.h / 2.0 - 40.0;
    match data {
        Load::Failed(error) => {
            p.text_center(app.t(T::StorePageFailed), p.w / 2.0, cy, 26.0, Weight::Display, pal.text);
            for (i, line) in p.wrap(&store_error(app.lang(), error), p.w - MARGIN * 4.0, 18.0, Weight::Regular).iter().take(3).enumerate() {
                p.text_center(line, p.w / 2.0, cy + 44.0 + i as f32 * 26.0, 18.0, Weight::Regular, pal.dim);
            }
        }
        _ => icons::spinner(p, p.w / 2.0, cy, 22.0, 5.0, pal.accent),
    }
}

/// A control's button as this handheld labels it.
fn control_glyph(app: &App, button: &str) -> String {
    match button {
        "a" | "b" => chrome::glyph(app, &button.to_ascii_uppercase()).to_string(),
        "dpad" => "D-PAD".into(),
        "start" => "START".into(),
        "select" => "SELECT".into(),
        other => other.to_ascii_uppercase(),
    }
}

fn owner_of(repo: &str) -> String {
    repo.split('/').nth(1).unwrap_or(repo).to_string()
}

/// 1234 as 1.2K, 2500000 as 2.5M.
fn compact(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}K", n as f64 / 1e3).replace(".0K", "K"),
        _ => format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M"),
    }
}

/// Search results as a grid of icons.
pub fn search(ui: &mut Ui, p: &mut Painter, app: &App, dt: f32) {
    let pal = p.pal;
    let Some(search) = &app.store.search else { return };
    let title = if search.query.is_empty() { app.t(T::StoreAll).to_string() } else { T::StoreResults.fmt(app.lang(), &[&search.query]) };
    let title = p.fit(&title, p.w - MARGIN * 2.0 - 260.0, 32.0, Weight::Display);
    chrome::page_title(p, &title);
    chrome::status(p, app);
    let mut hints = vec![("B", app.t(T::Back)), ("X", app.t(T::StoreLink)), ("Y", app.t(T::Search))];
    let results = match &search.results {
        Some(Load::Ready(found)) if !found.is_empty() => found,
        Some(Load::Ready(_)) => {
            p.text_center(app.t(T::StoreNoResults), p.w / 2.0, p.h / 2.0 - 40.0, 26.0, Weight::Display, pal.text);
            p.text_center(app.t(T::StoreNoResultsBody), p.w / 2.0, p.h / 2.0, 18.0, Weight::Regular, pal.dim);
            chrome::hints(p, app, &hints);
            return;
        }
        Some(Load::Failed(error)) => {
            p.text_center(&store_error(app.lang(), error), p.w / 2.0, p.h / 2.0 - 40.0, 22.0, Weight::Display, pal.text);
            chrome::hints(p, app, &hints);
            return;
        }
        _ => {
            icons::spinner(p, p.w / 2.0, p.h / 2.0 - 40.0, 22.0, 5.0, pal.accent);
            chrome::hints(p, app, &hints);
            return;
        }
    };
    let size = 108.0;
    let (column_w, row_h) = (168.0, 176.0);
    let page = search.focus / GRID_PAGE_SIZE;
    let first = page * GRID_PAGE_SIZE;
    let pages = results.len().div_ceil(GRID_PAGE_SIZE);
    let left = (p.w - column_w * GRID_COLUMNS as f32) / 2.0;
    let pop = ui.tile_pop.update((50, search.focus), dt);
    for (offset, g) in results.iter().enumerate().skip(first).take(GRID_PAGE_SIZE) {
        let slot = offset - first;
        let center = left + (slot % GRID_COLUMNS) as f32 * column_w + column_w / 2.0;
        let top = 118.0 + (slot / GRID_COLUMNS) as f32 * row_h;
        let x = center - size / 2.0;
        let focused = offset == search.focus;
        tile(ui, p, app, g, x, top, size);
        badge(p, app, g, x, top, size);
        if focused {
            focus_ring(p, x, top, size, size, size * TILE_RADIUS_RATIO, pop);
            ui.card_rect = Some((x, top, size, size));
        }
        let name = p.fit(g.name_for(app.lang_code()), column_w - 14.0, 19.0, Weight::Regular);
        p.text_center(&name, center, top + size + 25.0, 19.0, Weight::Regular, if focused { pal.text } else { pal.dim });
    }
    if pages > 1 {
        p.text_center(&format!("{} / {pages}", page + 1), p.w / 2.0, p.h - BOTTOM_H - 18.0, 18.0, Weight::Regular, pal.dim);
        hints.push(("L/R", app.t(T::ChangePage)));
    }
    hints.push(("A", app.t(T::Open)));
    chrome::hints(p, app, &hints);
    job_pill(p, app);
}

/// Notes the card the page opens from, so the page grows out of it.
pub fn opened(ui: &mut Ui, app: &App, page: Page) {
    ui.expand = None;
    ui.detail_y = 0.0;
    ui.detail_end = 0.0;
    ui.shots_x = f32::NAN;
    if page == Page::StoreDetail {
        if let Some(rect) = ui.card_rect.take() {
            ui.expand = Some((rect, app.time));
        }
    }
}

/// The page is still growing out of its card.
pub fn expanding(ui: &Ui, app: &App) -> bool {
    ui.expand.is_some_and(|(_, since)| app.time - since < EXPAND as f64)
}
