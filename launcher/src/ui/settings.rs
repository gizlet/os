//! Settings: categories in a sidebar, the focused category's rows beside it.

use super::chrome::{self, BOTTOM_H, MARGIN, TOP_H};
use super::home::{focus_color, focus_ring};
use super::{approach, Ui};
use crate::app::{App, Category, Item, ItemId, ItemValue};
use crate::fonts::Weight;
use crate::i18n::T;
use crate::icons;
use crate::painter::Painter;
use crate::theme::{alpha, mix, rgb};
use raylib_sys::Color;

const SIDEBAR_W: f32 = 330.0;
const CAT_H: f32 = 62.0;
const ROW_H: f32 = 66.0;
const PAD: f32 = 22.0;
const BODY_SIZE: f32 = 18.0;
const BODY_LINE: f32 = 27.0;

fn body_lines(p: &mut Painter, body: &str, w: f32) -> Vec<String> {
    p.wrap(body, w - PAD * 2.0 - 46.0, BODY_SIZE, Weight::Regular).into_iter().take(5).collect()
}

fn row_height(p: &mut Painter, item: &Item, w: f32) -> f32 {
    match &item.value {
        ItemValue::Section => 54.0,
        ItemValue::Notice { body } if !body.is_empty() => 84.0 + body_lines(p, body, w).len() as f32 * BODY_LINE,
        ItemValue::Notice { .. } => 70.0,
        ItemValue::Bar { .. } => 96.0,
        _ => ROW_H,
    }
}

fn category_icon(p: &Painter, cat: Category, cx: f32, cy: f32, s: f32, c: Color, hole: Color) {
    match cat {
        Category::Network => icons::wifi(p, cx, cy + s * 0.04, s, 4, c, c),
        Category::Ssh => icons::terminal(p, cx, cy, s, c, hole),
        Category::Display => icons::sun(p, cx, cy, s, c),
        Category::Sound => icons::speaker(p, cx, cy, s, c),
        Category::Battery => icons::battery(p, cx - s * 0.42, cy, s * 0.42, 70, false, c, c),
        Category::Controls => icons::gamepad(p, cx, cy, s * 1.1, c, hole),
        Category::Language => icons::globe(p, cx, cy, s, c),
        Category::System => icons::chip(p, cx, cy, s, c, hole),
    }
}

pub fn draw(ui: &mut Ui, p: &mut Painter, app: &App, dt: f32) {
    let pal = p.pal;
    let focus = app.focus.clone();
    let in_content = focus.in_content;

    // Sidebar.
    let side_bg = if pal.dark { alpha(rgb(0x000000), 0.16) } else { alpha(rgb(0xffffff), 0.5) };
    p.rect(0.0, 0.0, SIDEBAR_W, p.h - BOTTOM_H, 0.0, side_bg);
    chrome::page_title(p, app.t(T::Settings));
    chrome::status(p, app);

    let list_top = TOP_H + 14.0;
    let target_y = list_top + focus.category as f32 * CAT_H;
    ui.sidebar_y = if ui.sidebar_y == 0.0 { target_y } else { approach(ui.sidebar_y, target_y, 22.0, dt) };
    let hl_x = MARGIN * 0.5;
    let hl_w = SIDEBAR_W - MARGIN * 0.5 - 18.0;
    if in_content {
        p.rect(hl_x, ui.sidebar_y + 4.0, hl_w, CAT_H - 8.0, 12.0, alpha(pal.surface_hi, 0.6));
    } else {
        p.rect(hl_x, ui.sidebar_y + 4.0, hl_w, CAT_H - 8.0, 12.0, pal.surface_hi);
        p.rect(hl_x + 8.0, ui.sidebar_y + 18.0, 5.0, CAT_H - 36.0, 2.5, focus_color(p));
    }
    for (i, cat) in Category::ALL.iter().enumerate() {
        let cy = list_top + i as f32 * CAT_H + CAT_H / 2.0;
        let selected = i == focus.category;
        let c = match (selected, in_content) {
            (true, true) => pal.accent,
            (true, false) => pal.text,
            _ => pal.dim,
        };
        let hole = if selected { pal.surface_hi } else { mix(pal.bg_top, pal.bg_bottom, 0.4) };
        category_icon(p, *cat, hl_x + 44.0, cy, 26.0, c, hole);
        let weight = if selected { Weight::Bold } else { Weight::Regular };
        p.text_mid(app.t(cat.title()), hl_x + 76.0, cy, 22.0, weight, c);
    }

    // Rows of the focused category.
    let cat = app.focused_category();
    let items = app.settings_items(cat);
    let left = SIDEBAR_W + 40.0;
    let right = p.w - MARGIN;
    let width = right - left;
    let top = TOP_H + 10.0;
    let bottom = p.h - BOTTOM_H - 6.0;

    let mut ys = Vec::with_capacity(items.len());
    let mut y = 0.0;
    for item in &items {
        ys.push(y);
        y += row_height(p, item, width);
    }
    let total = y;
    let sel = focus.item.min(items.len().saturating_sub(1));
    let mut target = if in_content { ui.settings_scroll } else { 0.0 };
    if in_content && !items.is_empty() {
        // Keep the focused row, and the heading above it, in view.
        let mut fy = ys[sel];
        if sel > 0 && items[sel - 1].value == ItemValue::Section {
            fy = ys[sel - 1];
        }
        let fb = ys[sel] + row_height(p, &items[sel], width);
        let view = bottom - top;
        if fy - target < 0.0 {
            target = fy;
        }
        if fb - target > view {
            target = fb - view;
        }
        target = target.clamp(0.0, (total - view).max(0.0));
    }
    ui.settings_scroll = approach(ui.settings_scroll, target, 18.0, dt);
    let pop = ui.list_pop.update((3 + focus.category as u8, if in_content { sel } else { usize::MAX }), dt);

    p.clip(SIDEBAR_W, top - 6.0, p.w - SIDEBAR_W, bottom - top + 12.0);
    for (i, item) in items.iter().enumerate() {
        let y = top + ys[i] - ui.settings_scroll;
        let h = row_height(p, item, width);
        if y > bottom + 20.0 || y + h < top - 20.0 {
            continue;
        }
        let focused = in_content && i == sel && item.focusable();
        draw_row(p, app, item, left, y, width, h, focused, pop);
    }
    p.unclip();

    let mut hints: Vec<(&str, &str)> = vec![("B", app.t(T::Back))];
    if in_content {
        match items.get(sel).map(|i| (&i.id, &i.value)) {
            Some((_, ItemValue::Slider(_))) | Some((ItemId::Theme, _)) => hints.push(("←→", app.t(T::Change))),
            Some((ItemId::None, _)) | None => {}
            Some((ItemId::WifiNetwork(_), ItemValue::Network { status: Some(_), .. })) => {
                hints.push(("X", app.t(T::WifiForget)));
                hints.push(("A", app.t(T::Select)));
            }
            _ => hints.push(("A", app.t(T::Select))),
        }
        if cat == Category::Network && matches!(app.snapshot.as_ref().map(|s| &s.wifi), Some(Ok(w)) if w.on && w.adapter) {
            hints.insert(1, ("Y", app.t(T::Refresh)));
        }
    } else {
        hints.push(("A", app.t(T::Select)));
    }
    chrome::hints(p, app, &hints);
}

fn draw_row(p: &mut Painter, app: &App, item: &Item, x: f32, y: f32, w: f32, h: f32, focused: bool, pop: f32) {
    let pal = p.pal;
    let cy = y + h / 2.0;
    let size = 21.0;

    match &item.value {
        ItemValue::Section => {
            p.text(&item.label, x + 4.0, y + h - 30.0, 19.0, Weight::Bold, pal.dim);
            return;
        }
        ItemValue::Notice { body } => {
            let card_h = h - 14.0;
            p.rect(x, y + 6.0, w, card_h, 16.0, alpha(pal.surface, 0.8));
            if body.is_empty() {
                p.text_mid(&item.label, x + PAD, y + 6.0 + card_h / 2.0, size, Weight::Regular, pal.dim);
                return;
            }
            let iy = y + 6.0 + 38.0;
            p.circle(x + PAD + 16.0, iy, 16.0, alpha(pal.warm, 0.2));
            p.text_center("!", x + PAD + 16.0, iy, 22.0, Weight::Bold, pal.warm);
            p.text_mid(&item.label, x + PAD + 46.0, iy, 22.0, Weight::Bold, pal.text);
            for (i, line) in body_lines(p, body, w).iter().enumerate() {
                p.text(line, x + PAD + 46.0, iy + 22.0 + i as f32 * BODY_LINE, BODY_SIZE, Weight::Regular, pal.dim);
            }
            return;
        }
        ItemValue::Busy => {
            icons::spinner(p, x + PAD + 10.0, cy, 11.0, 3.0, pal.accent);
            p.text_mid(&item.label, x + PAD + 34.0, cy, size, Weight::Regular, pal.dim);
            return;
        }
        ItemValue::Bar { value, fraction, warn } => {
            let (lx, bw) = (x + PAD, w - PAD * 2.0);
            p.text_mid(&item.label, lx, y + 30.0, size, Weight::Regular, pal.text);
            p.text_right(value, lx + bw, y + 30.0, size, Weight::Regular, pal.dim);
            p.rect(lx, y + 56.0, bw, 14.0, 7.0, alpha(pal.line, 0.9));
            let c = if *warn { pal.danger } else { pal.accent };
            p.rect(lx, y + 56.0, (bw * fraction.clamp(0.0, 1.0)).max(14.0), 14.0, 7.0, c);
            return;
        }
        _ => {}
    }

    if focused {
        p.rect(x, y + 3.0, w, h - 6.0, 14.0, pal.surface_hi);
        focus_ring(p, x + 6.0, y + 9.0, w - 12.0, h - 18.0, 8.0, pop * 0.5 - 3.0);
    } else {
        p.rect(x + PAD, y + h - 1.0, w - PAD * 2.0, 1.0, 0.0, alpha(pal.line, 0.6));
    }

    let label_x = x + PAD;
    let right = x + w - PAD;
    match &item.value {
        ItemValue::Info(v) => {
            let lw = p.measure(&item.label, size, Weight::Regular);
            p.text_mid(&item.label, label_x, cy, size, Weight::Regular, pal.text);
            let v = p.fit(v, (right - label_x - lw - 30.0).max(60.0), size, Weight::Regular);
            p.text_right(&v, right, cy, size, Weight::Regular, pal.dim);
        }
        ItemValue::Button => {
            let c = if item.id == ItemId::WifiDisconnect { pal.danger } else { pal.accent };
            p.text_mid(&item.label, label_x, cy, size, Weight::Bold, c);
            icons::chevron(p, right - 8.0, cy, 22.0, true, alpha(pal.dim, 0.8));
        }
        ItemValue::Toggle(on) => {
            p.text_mid(&item.label, label_x, cy, size, Weight::Regular, pal.text);
            if item.id == ItemId::SwapAb {
                let lw = p.measure(&item.label, size, Weight::Regular);
                let hint = p.fit(app.t(T::SwapAbBody), (w - lw - 140.0).max(80.0), 16.0, Weight::Regular);
                p.text_mid(&hint, label_x + lw + 16.0, cy + 1.0, 16.0, Weight::Regular, pal.faint);
            }
            switch(p, right - 54.0, cy, *on);
        }
        ItemValue::Slider(v) => {
            p.text_mid(&item.label, label_x, cy, size, Weight::Regular, pal.text);
            let bw = (w * 0.42).min(340.0);
            let bx = right - bw;
            p.text_right(&format!("{}%", (v * 100.0).round() as i32), bx - 18.0, cy, 20.0, Weight::Regular, pal.dim);
            p.rect(bx, cy - 4.0, bw, 8.0, 4.0, alpha(pal.line, 0.9));
            p.rect(bx, cy - 4.0, bw * v, 8.0, 4.0, pal.accent);
            let kx = bx + bw * v;
            p.circle(kx, cy, if focused { 14.0 } else { 11.0 }, if focused { focus_color(p) } else { pal.text });
            p.circle(kx, cy, 5.0, pal.surface_hi);
        }
        ItemValue::Radio(selected) => {
            let weight = if *selected { Weight::Bold } else { Weight::Regular };
            p.text_mid(&item.label, label_x, cy, size, weight, pal.text);
            if *selected {
                icons::check(p, right - 12.0, cy, 24.0, pal.accent);
            }
        }
        ItemValue::Choice(v) => {
            p.text_mid(&item.label, label_x, cy, size, Weight::Regular, pal.text);
            let vw = p.measure(v, size, Weight::Bold);
            p.text_mid(v, right - 22.0 - vw, cy, size, Weight::Bold, pal.accent);
            if focused {
                icons::chevron(p, right - vw - 40.0, cy, 18.0, false, pal.dim);
                icons::chevron(p, right - 6.0, cy, 18.0, true, pal.dim);
            }
        }
        ItemValue::Network { signal, secured, status } => {
            let bars = 1 + (signal / 26).min(3);
            icons::wifi(p, label_x + 14.0, cy, 26.0, bars, pal.text, alpha(pal.text, 0.22));
            let name_x = label_x + 44.0;
            let mut name_right = right;
            if *secured {
                icons::lock(p, right - 10.0, cy, 20.0, pal.dim);
                name_right -= 30.0;
            }
            if let Some(s) = status {
                let c = if s == app.t(T::WifiConnected) { pal.success } else { pal.dim };
                let sw = p.measure(s, 18.0, Weight::Regular);
                p.text_mid(s, name_right - sw, cy, 18.0, Weight::Regular, c);
                if s == app.t(T::WifiConnecting) {
                    icons::spinner(p, name_right - sw - 18.0, cy, 9.0, 2.5, pal.accent);
                    name_right -= 24.0;
                }
                name_right -= sw + 20.0;
            }
            let name = p.fit(&item.label, name_right - name_x, size, Weight::Regular);
            p.text_mid(&name, name_x, cy, size, Weight::Regular, pal.text);
        }
        _ => {}
    }
}

fn switch(p: &Painter, x: f32, cy: f32, on: bool) {
    let pal = p.pal;
    let (w, h) = (54.0, 30.0);
    p.rect(x, cy - h / 2.0, w, h, h / 2.0, if on { pal.accent } else { mix(pal.line, pal.dim, 0.35) });
    let kx = if on { x + w - h / 2.0 } else { x + h / 2.0 };
    p.circle(kx, cy, h / 2.0 - 4.0, rgb(0xffffff));
}
