//! Draws the launcher's state. Only animation state lives here; every decision is in `app`.

mod chrome;
mod home;
mod overlays;
mod settings;
mod test;

use crate::app::{App, Page};
use crate::input::RawPad;
use crate::painter::Painter;
use crate::theme;
use raylib_sys as ffi;
use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};

/// App icons, loaded once per file and kept until the launcher quits.
#[derive(Default)]
pub struct Icons {
    loaded: HashMap<PathBuf, ffi::Texture2D>,
}

impl Icons {
    /// The icon at `path`, or `None` when it cannot be read as an image.
    pub fn get(&mut self, path: &Path) -> Option<ffi::Texture2D> {
        let tex = *self.loaded.entry(path.to_path_buf()).or_insert_with(|| {
            let Ok(c) = CString::new(path.to_string_lossy().as_bytes()) else {
                return ffi::Texture2D { id: 0, width: 0, height: 0, mipmaps: 0, format: 0 };
            };
            unsafe {
                let mut tex = ffi::LoadTexture(c.as_ptr());
                if tex.id != 0 {
                    // Icons are drawn far smaller than they are stored.
                    ffi::GenTextureMipmaps(&mut tex);
                    ffi::SetTextureFilter(tex, ffi::TextureFilter::TEXTURE_FILTER_TRILINEAR as i32);
                }
                tex
            }
        });
        (tex.id != 0).then_some(tex)
    }
}

impl Drop for Icons {
    fn drop(&mut self) {
        for tex in self.loaded.values().filter(|t| t.id != 0) {
            unsafe { ffi::UnloadTexture(*tex) };
        }
    }
}

/// Exponential smoothing toward `target`; `rate` is roughly 1/time-constant.
pub fn approach(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    let v = current + (target - current) * (1.0 - (-rate * dt).exp());
    if (v - target).abs() < 0.05 {
        target
    } else {
        v
    }
}

/// Ease-out cubic over `0..=1`.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// A focus change "pop": 1 right after the change, easing to 0.
#[derive(Default)]
pub struct Pop {
    key: Option<(u8, usize)>,
    since: f32,
}

impl Pop {
    pub fn update(&mut self, key: (u8, usize), dt: f32) -> f32 {
        if self.key != Some(key) {
            self.key = Some(key);
            self.since = 0.0;
        } else {
            self.since += dt;
        }
        1.0 - ease_out(self.since / 0.22)
    }
}

#[derive(Default)]
pub struct Ui {
    settings_scroll: f32,
    sidebar_y: f32,
    button_pop: Pop,
    tile_pop: Pop,
    list_pop: Pop,
    page_key: (Option<Page>, usize),
    page_since: f32,
    icons: Icons,
    shelf_x: f32,
}

impl Ui {
    pub fn draw(&mut self, p: &mut Painter, app: &App, pad: &RawPad, dt: f32) {
        p.pal = theme::palette(app.settings.theme);
        let key = (Some(app.page()), app.depth());
        if key != self.page_key {
            // The first frame shows at once; later pages fade in.
            self.page_since = if self.page_key.0.is_none() { 1.0 } else { 0.0 };
            self.page_key = key;
        } else {
            self.page_since += dt;
        }

        chrome::background(p);
        match app.page() {
            Page::Home => home::draw(self, p, app, dt),
            Page::Settings => settings::draw(self, p, app, dt),
            Page::ButtonTest => test::draw(p, app, pad),
        }

        let fade = 1.0 - ease_out(self.page_since / 0.2);
        if fade > 0.0 {
            let c = theme::mix(p.pal.bg_top, p.pal.bg_bottom, 0.5);
            p.rect(0.0, 0.0, p.w, p.h, 0.0, theme::alpha(c, fade));
        }

        overlays::toasts(p, app);
        if app.dialog.is_some() {
            overlays::dialog(p, app);
        }
        if app.keyboard.is_some() {
            overlays::keyboard(p, app);
        }
    }
}
