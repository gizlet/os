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
    let radius = size as f32 * TILE_RADIUS_RATIO;
    let corner = radius.ceil() as usize;
    for row in 0..corner {
        for column in 0..corner {
            let dx = radius - column as f32 - 0.5;
            let dy = radius - row as f32 - 0.5;
            let coverage = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
            let opposite_row = size - 1 - row;
            let opposite_column = size - 1 - column;
            for index in [
                row * size + column,
                row * size + opposite_column,
                opposite_row * size + column,
                opposite_row * size + opposite_column,
            ] {
                pixels[index].a = (pixels[index].a as f32 * coverage).round() as u8;
            }
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
}

impl Ui {
    /// Draws the frame. Returns true while something eases toward a new
    /// layout (the shelf, the settings list, a focus pop or a page fade), so
    /// the caller keeps drawing every frame until it settles.
    pub fn draw(&mut self, p: &mut Painter, app: &App, pad: &RawPad, dt: f32) -> bool {
        let before = (self.shelf_x, self.sidebar_y, self.settings_scroll);
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

        before != (self.shelf_x, self.sidebar_y, self.settings_scroll)
            || self.page_since < 0.2
            || self.button_pop.moving()
            || self.tile_pop.moving()
            || self.list_pop.moving()
    }
}
