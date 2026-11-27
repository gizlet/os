//! Draws the launcher's state. Only animation state lives here; every decision is in `app`.

mod chrome;
mod home;
mod overlays;
mod settings;
mod store;
mod test;

use crate::app::{App, Page};
use crate::input::RawPad;
use crate::painter::Painter;
use crate::theme;
use raylib_sys as ffi;
use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};

const TILE_RADIUS_RATIO: f32 = 0.11;

/// Which version of a file an icon was loaded from: an update or a
/// reinstall writes a new file, with another time or size.
type FileId = Option<(std::time::SystemTime, u64)>;

fn file_id(path: &Path) -> FileId {
    std::fs::metadata(path).ok().and_then(|m| Some((m.modified().ok()?, m.len())))
}

/// App icons, each loaded once per version of its file.
#[derive(Default)]
pub struct Icons {
    loaded: HashMap<PathBuf, (FileId, ffi::Texture2D)>,
}

impl Icons {
    /// The icon at `path`, or `None` when it cannot be read as an image.
    /// A file changed since it was loaded is loaded again.
    pub fn get(&mut self, path: &Path) -> Option<ffi::Texture2D> {
        let id = file_id(path);
        if let Some((old_id, old)) = self.loaded.get(path) {
            if *old_id == id {
                return (old.id != 0).then_some(*old);
            }
            if old.id != 0 {
                unsafe { ffi::UnloadTexture(*old) };
            }
        }
        let tex = load(path);
        self.loaded.insert(path.to_path_buf(), (id, tex));
        (tex.id != 0).then_some(tex)
    }
}

impl Icons {
    /// Unloads every icon whose path is not in `paths`, as when an app is
    /// removed or an update names another icon file.
    pub fn keep_only(&mut self, paths: &[PathBuf]) {
        self.loaded.retain(|path, (_, tex)| {
            let keep = paths.contains(path);
            if !keep && tex.id != 0 {
                unsafe { ffi::UnloadTexture(*tex) };
            }
            keep
        });
    }
}

fn load(path: &Path) -> ffi::Texture2D {
    let empty = ffi::Texture2D { id: 0, width: 0, height: 0, mipmaps: 0, format: 0 };
    let Ok(c) = CString::new(path.to_string_lossy().as_bytes()) else {
        return empty;
    };
    unsafe {
        let mut image = ffi::LoadImage(c.as_ptr());
        if !ffi::IsImageValid(image) {
            ffi::UnloadImage(image);
            return empty;
        }
        let side = image.width.min(image.height);
        if image.width != image.height {
            let crop = ffi::Rectangle {
                x: ((image.width - side) / 2) as f32,
                y: ((image.height - side) / 2) as f32,
                width: side as f32,
                height: side as f32,
            };
            ffi::ImageCrop(&mut image, crop);
        }
        let size = side.clamp(128, 512);
        if image.width != size || image.height != size {
            ffi::ImageResize(&mut image, size, size);
        }
        let rgba = ffi::PixelFormat::PIXELFORMAT_UNCOMPRESSED_R8G8B8A8 as i32;
        ffi::ImageFormat(&mut image, rgba);
        if !ffi::IsImageValid(image) || image.format != rgba || image.width != size || image.height != size {
            ffi::UnloadImage(image);
            return empty;
        }
        let size = size as usize;
        let pixels = std::slice::from_raw_parts_mut(image.data.cast::<ffi::Color>(), size * size);
        round_corners(pixels, size);
        let mut tex = ffi::LoadTextureFromImage(image);
        ffi::UnloadImage(image);
        if tex.id != 0 {
            // Icons are drawn far smaller than they are stored.
            ffi::GenTextureMipmaps(&mut tex);
            ffi::SetTextureFilter(tex, ffi::TextureFilter::TEXTURE_FILTER_TRILINEAR as i32);
        }
        tex
    }
}

fn round_corners(pixels: &mut [ffi::Color], size: usize) {
    round_rect_corners(pixels, size, size, size as f32 * TILE_RADIUS_RATIO);
}

/// Fades the corners of a `width`×`height` image outside a rounded rectangle of `radius`.
fn round_rect_corners(pixels: &mut [ffi::Color], width: usize, height: usize, radius: f32) {
    let corner = (radius.ceil() as usize).min(width / 2).min(height / 2);
    for row in 0..corner {
        for column in 0..corner {
            let dx = radius - column as f32 - 0.5;
            let dy = radius - row as f32 - 0.5;
            let coverage = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
            let opposite_row = height - 1 - row;
            let opposite_column = width - 1 - column;
            for index in [
                row * width + column,
                row * width + opposite_column,
                opposite_row * width + column,
                opposite_row * width + opposite_column,
            ] {
                pixels[index].a = (pixels[index].a as f32 * coverage).round() as u8;
            }
        }
    }
}

/// How Store art is cut: a square icon, or a frame of an aspect ratio whose
/// corners are rounded by a share of its height, darkened for text when `scrim`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Icon,
    Frame { aspect_milli: u32, radius_milli: u32, scrim: bool },
}

impl Shape {
    pub fn frame(aspect: f32, radius: f32, scrim: bool) -> Shape {
        Shape::Frame { aspect_milli: (aspect * 1000.0) as u32, radius_milli: (radius * 1000.0) as u32, scrim }
    }
}

/// Store art: icons, covers and screenshots, loaded once per shape and
/// unloaded when not drawn for a while. Each knows when it arrived, to fade in.
#[derive(Default)]
pub struct ArtCache {
    loaded: HashMap<(PathBuf, Shape), (ffi::Texture2D, f64, f64)>,
}

/// How long new art takes to fade in, and how long art not drawn stays loaded.
const ART_FADE: f64 = 0.3;
const ART_KEEP: f64 = 20.0;

impl ArtCache {
    /// The art at `path` in `shape`, and how far it has faded in.
    pub fn get(&mut self, path: &Path, shape: Shape, now: f64) -> Option<(ffi::Texture2D, f32)> {
        let key = (path.to_path_buf(), shape);
        if let Some((tex, born, used)) = self.loaded.get_mut(&key) {
            *used = now;
            return (tex.id != 0).then(|| (*tex, ((now - *born) / ART_FADE).min(1.0) as f32));
        }
        let tex = match shape {
            Shape::Icon => load(path),
            Shape::Frame { aspect_milli, radius_milli, scrim } => {
                load_frame(path, aspect_milli as f32 / 1000.0, radius_milli as f32 / 1000.0, scrim)
            }
        };
        self.loaded.insert(key, (tex, now, now));
        (tex.id != 0).then_some((tex, 0.0))
    }

    /// Art still fading in.
    pub fn fading(&self, now: f64) -> bool {
        self.loaded.values().any(|(tex, born, _)| tex.id != 0 && now - born < ART_FADE)
    }

    pub fn prune(&mut self, now: f64) {
        self.loaded.retain(|_, (tex, _, used)| {
            let keep = now - *used < ART_KEEP;
            if !keep && tex.id != 0 {
                unsafe { ffi::UnloadTexture(*tex) };
            }
            keep
        });
    }
}

impl Drop for ArtCache {
    fn drop(&mut self) {
        for (tex, _, _) in self.loaded.values().filter(|(t, _, _)| t.id != 0) {
            unsafe { ffi::UnloadTexture(*tex) };
        }
    }
}

/// An image cut to `aspect` around its center, at most 960 wide, with its
/// corners rounded by `radius` of its height, and darkened to the left and
/// bottom for text over it when `scrim`.
fn load_frame(path: &Path, aspect: f32, radius: f32, scrim: bool) -> ffi::Texture2D {
    let empty = ffi::Texture2D { id: 0, width: 0, height: 0, mipmaps: 0, format: 0 };
    let Ok(c) = CString::new(path.to_string_lossy().as_bytes()) else {
        return empty;
    };
    unsafe {
        let mut image = ffi::LoadImage(c.as_ptr());
        if !ffi::IsImageValid(image) || image.width < 2 || image.height < 2 {
            ffi::UnloadImage(image);
            return empty;
        }
        let (w, h) = (image.width as f32, image.height as f32);
        let crop = if w / h > aspect {
            let cw = (h * aspect).round();
            ffi::Rectangle { x: ((w - cw) / 2.0).floor(), y: 0.0, width: cw, height: h }
        } else {
            let ch = (w / aspect).round();
            ffi::Rectangle { x: 0.0, y: ((h - ch) / 2.0).floor(), width: w, height: ch }
        };
        ffi::ImageCrop(&mut image, crop);
        if image.width > 960 {
            let height = (960.0 / aspect).round() as i32;
            ffi::ImageResize(&mut image, 960, height.max(1));
        }
        let rgba = ffi::PixelFormat::PIXELFORMAT_UNCOMPRESSED_R8G8B8A8 as i32;
        ffi::ImageFormat(&mut image, rgba);
        if !ffi::IsImageValid(image) || image.format != rgba {
            ffi::UnloadImage(image);
            return empty;
        }
        let (width, height) = (image.width as usize, image.height as usize);
        let pixels = std::slice::from_raw_parts_mut(image.data.cast::<ffi::Color>(), width * height);
        if scrim {
            darken_for_text(pixels, width, height);
        }
        round_rect_corners(pixels, width, height, radius * height as f32);
        let mut tex = ffi::LoadTextureFromImage(image);
        ffi::UnloadImage(image);
        if tex.id != 0 {
            ffi::GenTextureMipmaps(&mut tex);
            ffi::SetTextureFilter(tex, ffi::TextureFilter::TEXTURE_FILTER_TRILINEAR as i32);
        }
        tex
    }
}

/// Darkens the left part and the bottom of an image, so white text over them reads.
fn darken_for_text(pixels: &mut [ffi::Color], width: usize, height: usize) {
    for row in 0..height {
        let v = row as f32 / height as f32;
        let bottom = 0.55 * ((v - 0.4) / 0.6).clamp(0.0, 1.0).powf(1.4);
        for column in 0..width {
            let u = column as f32 / width as f32;
            let left = 0.8 * ((0.75 - u) / 0.75).clamp(0.0, 1.0).powf(1.2);
            let keep = (1.0 - left) * (1.0 - bottom);
            let pixel = &mut pixels[row * width + column];
            pixel.r = (pixel.r as f32 * keep) as u8;
            pixel.g = (pixel.g as f32 * keep) as u8;
            pixel.b = (pixel.b as f32 * keep) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_mask_softens_corners_and_preserves_the_source_alpha() {
        let size = 128;
        let mut pixels = vec![ffi::Color { r: 48, g: 89, b: 150, a: 180 }; size * size];
        round_corners(&mut pixels, size);
        for index in [0, size - 1, size * (size - 1), size * size - 1] {
            assert_eq!(pixels[index].a, 0);
        }
        for index in [size / 2, size * (size / 2), size * (size / 2) + size / 2] {
            assert_eq!(pixels[index].a, 180);
        }
        assert!(pixels.iter().any(|pixel| (1..180).contains(&pixel.a)));
        assert!(pixels.iter().all(|pixel| (pixel.r, pixel.g, pixel.b) == (48, 89, 150)));
    }
}

impl Drop for Icons {
    fn drop(&mut self) {
        for (_, tex) in self.loaded.values().filter(|(_, t)| t.id != 0) {
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
    /// Updated since the last frame was drawn.
    drawn: bool,
}

impl Pop {
    pub fn update(&mut self, key: (u8, usize), dt: f32) -> f32 {
        self.drawn = true;
        if self.key != Some(key) {
            self.key = Some(key);
            self.since = 0.0;
        } else {
            self.since += dt;
        }
        1.0 - ease_out(self.since / 0.22)
    }

    /// Still popping in this frame; a pop on a page not drawn does not count.
    fn moving(&mut self) -> bool {
        std::mem::take(&mut self.drawn) && self.since < 0.22
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
    /// The installed apps' icon paths the cache was last pruned to.
    icon_paths: Vec<PathBuf>,
    shelf_x: f32,
    /// Store art, and where the Store's pages are scrolled to.
    art: ArtCache,
    /// The list shown under the Store's hero card, and since when.
    store_tab: (usize, f64),
    /// The hero card shown, and since when.
    hero: (usize, f64),
    /// The focused Store card this frame, which a page opened from it grows out of.
    card_rect: Option<store::Rect>,
    expand: Option<(store::Rect, f64)>,
    detail_y: f32,
    shots_x: f32,
}

impl Ui {
    /// Draws the frame. Returns true while something eases toward a new
    /// layout (the shelf, the settings list, a focus pop or a page fade), so
    /// the caller keeps drawing every frame until it settles.
    pub fn draw(&mut self, p: &mut Painter, app: &App, pad: &RawPad, dt: f32) -> bool {
        let before = (self.shelf_x, self.sidebar_y, self.settings_scroll, self.detail_y, self.shots_x.to_bits());
        p.pal = theme::palette(app.settings.theme);
        let paths: Vec<_> = app.apps.iter()
            .chain(app.installed_services().iter().map(|service| &service.package))
            .filter_map(|package| package.manifest.icon.as_ref().map(|icon| package.dir.join(icon)))
            .collect();
        if paths != self.icon_paths {
            self.icons.keep_only(&paths);
            self.icon_paths = paths;
        }
        let key = (Some(app.page()), app.depth());
        if key != self.page_key {
            // The first frame shows at once; later pages fade in.
            self.page_since = if self.page_key.0.is_none() { 1.0 } else { 0.0 };
            self.settings_scroll = 0.0;
            self.page_key = key;
            store::opened(self, app, app.page());
            if self.expand.is_some() {
                // The page grows out of its card instead of fading in.
                self.page_since = 1.0;
            }
        } else {
            self.page_since += dt;
        }

        chrome::background(p);
        match app.page() {
            Page::Home => home::draw(self, p, app, dt),
            Page::Apps | Page::Services => home::grid(self, p, app, dt),
            Page::ServiceDetails => settings::service_details(self, p, app, dt),
            Page::Settings => settings::draw(self, p, app, dt),
            Page::ButtonTest => test::draw(p, app, pad),
            Page::Store => store::home(self, p, app, dt),
            Page::StoreDetail => store::detail(self, p, app, dt),
            Page::StoreSearch => store::search(self, p, app, dt),
        }
        self.art.prune(app.time);

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

        before != (self.shelf_x, self.sidebar_y, self.settings_scroll, self.detail_y, self.shots_x.to_bits())
            || self.art.fading(app.time)
            || store::expanding(self, app)
            || app.time - self.store_tab.1 < 0.3
            || self.page_since < 0.2
            || self.button_pop.moving()
            || self.tile_pop.moving()
            || self.list_pop.moving()
    }
}
