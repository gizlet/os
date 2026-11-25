//! Font atlases holding exactly the characters the interface has shown.
//!
//! Latin text uses Inter for body text and Space Grotesk for titles (both SIL
//! Open Font License); Chinese, Japanese and Korean come from Droid Sans
//! Fallback (Apache 2.0), which has one weight. raylib reads one font file per
//! atlas, so each atlas is assembled here: glyphs below U+2E80 come from the
//! Latin face, the rest from Droid, shifted onto the Latin face's baseline.
//!
//! raylib rasterizes only the codepoints it is given, so each atlas holds the
//! characters seen so far; a new character drops the atlases, and they are
//! rebuilt on demand before the next frame.

use raylib_sys::{self as ffi, GlyphInfo, Rectangle};
use std::collections::{BTreeSet, HashMap};

static INTER_REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
static INTER_SEMIBOLD: &[u8] = include_bytes!("../fonts/Inter-SemiBold.ttf");
static SPACE_GROTESK_BOLD: &[u8] = include_bytes!("../fonts/SpaceGrotesk-Bold.ttf");
static DROID_FALLBACK: &[u8] = include_bytes!("../fonts/DroidSansFallbackFull.ttf");

/// Codepoints from here on are drawn with the CJK face.
const CJK_START: i32 = 0x2E80;
const PADDING: i32 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Weight {
    Regular,
    Bold,
    /// Titles and the clock.
    Display,
}

impl Weight {
    fn latin(self) -> &'static [u8] {
        match self {
            Weight::Regular => INTER_REGULAR,
            Weight::Bold => INTER_SEMIBOLD,
            Weight::Display => SPACE_GROTESK_BOLD,
        }
    }
}

/// The baseline as a fraction of the pixel height raylib scales a face to,
/// from the `hhea` table: ascender / (ascender - descender).
fn baseline(font: &[u8]) -> f32 {
    let u16_at = |o: usize| font.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let u32_at = |o: usize| font.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let tables = u16_at(4).unwrap_or(0) as usize;
    for i in 0..tables {
        let entry = 12 + i * 16;
        if font.get(entry..entry + 4) == Some(b"hhea") {
            let Some(offset) = u32_at(entry + 8) else { break };
            let ascender = u16_at(offset as usize + 4).unwrap_or(0) as i16 as f32;
            let descender = u16_at(offset as usize + 6).unwrap_or(0) as i16 as f32;
            if ascender - descender > 0.0 {
                return ascender / (ascender - descender);
            }
        }
    }
    0.8
}

/// The glyphs of `codepoints` from `font`, rasterized at `px`, moved out of raylib's array.
fn glyphs(font: &[u8], px: i32, codepoints: &[i32]) -> Vec<GlyphInfo> {
    if codepoints.is_empty() {
        return Vec::new();
    }
    let mut count = 0;
    unsafe {
        let ptr = ffi::LoadFontData(
            font.as_ptr(),
            font.len() as i32,
            px,
            codepoints.as_ptr(),
            codepoints.len() as i32,
            ffi::FontType::FONT_DEFAULT as i32,
            &mut count,
        );
        if ptr.is_null() {
            return Vec::new();
        }
        let out = std::slice::from_raw_parts(ptr, count.max(0) as usize).to_vec();
        // The glyph images now belong to `out`; only the array is freed.
        ffi::MemFree(ptr.cast());
        out
    }
}

/// One raylib font with the Latin face's glyphs and the CJK face's.
fn build(weight: Weight, px: i32, codepoints: &BTreeSet<i32>) -> ffi::Font {
    let (latin, cjk): (Vec<i32>, Vec<i32>) = codepoints.iter().partition(|&&c| c < CJK_START);
    let mut all = glyphs(weight.latin(), px, &latin);
    let shift = ((baseline(weight.latin()) - baseline(DROID_FALLBACK)) * px as f32).round() as i32;
    for mut g in glyphs(DROID_FALLBACK, px, &cjk) {
        g.offsetY += shift;
        all.push(g);
    }
    unsafe {
        let size = std::mem::size_of::<GlyphInfo>() * all.len().max(1);
        let array = ffi::MemAlloc(size as u32) as *mut GlyphInfo;
        std::ptr::copy_nonoverlapping(all.as_ptr(), array, all.len());
        let mut recs: *mut Rectangle = std::ptr::null_mut();
        let atlas = ffi::GenImageFontAtlas(array, &mut recs, all.len() as i32, px, PADDING, 0);
        let texture = ffi::LoadTextureFromImage(atlas);
        ffi::UnloadImage(atlas);
        ffi::SetTextureFilter(texture, ffi::TextureFilter::TEXTURE_FILTER_BILINEAR as i32);
        ffi::Font {
            baseSize: px,
            glyphCount: all.len() as i32,
            glyphPadding: PADDING,
            texture,
            recs,
            glyphs: array,
        }
    }
}

pub struct Fonts {
    codepoints: BTreeSet<i32>,
    faces: HashMap<(Weight, u32), ffi::Font>,
    dirty: bool,
}

impl Fonts {
    pub fn new<'a>(seed: impl IntoIterator<Item = &'a str>) -> Fonts {
        let mut codepoints: BTreeSet<i32> = (32..127).collect();
        codepoints.extend("…·—–“”‘’×°%•←→".chars().map(|c| c as i32));
        let mut fonts = Fonts { codepoints, faces: HashMap::new(), dirty: false };
        for s in seed {
            fonts.ensure(s);
        }
        fonts.dirty = false;
        fonts
    }

    /// Records the characters of `text`.
    pub fn ensure(&mut self, text: &str) {
        for c in text.chars() {
            if !c.is_control() && self.codepoints.insert(c as i32) {
                self.dirty = true;
            }
        }
    }

    /// Drops atlases that lack newly seen characters. Call between frames.
    pub fn rebuild_if_needed(&mut self) {
        if self.dirty {
            self.clear();
            self.dirty = false;
        }
    }

    pub fn clear(&mut self) {
        for (_, f) in self.faces.drain() {
            unsafe { ffi::UnloadFont(f) };
        }
    }

    /// The atlas for `weight` rasterized at `px` physical pixels.
    pub fn face(&mut self, weight: Weight, px: u32) -> ffi::Font {
        let px = px.clamp(6, 256);
        if let Some(f) = self.faces.get(&(weight, px)) {
            return *f;
        }
        let font = build(weight, px as i32, &self.codepoints);
        self.faces.insert((weight, px), font);
        font
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_baselines() {
        for font in [INTER_REGULAR, INTER_SEMIBOLD, SPACE_GROTESK_BOLD, DROID_FALLBACK] {
            let b = baseline(font);
            assert!(b > 0.6 && b < 0.95, "{b}");
        }
    }
}
