//! Twemoji-based emoji atlas (CC-BY 4.0) + shortcode map + picker data.
//!
//! Glyphs are a pre-baked PNG atlas embedded at compile time. Lookup is by
//! Unicode scalar (single codepoint emoji in our curated set). Complex ZWJ
//! sequences fall back to a missing-glyph diamond so chat never crashes.

use crate::gl;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct AtlasMeta {
    cell: i32,
    atlas_w: i32,
    atlas_h: i32,
    glyphs: HashMap<String, GlyphRect>,
    shortcodes: HashMap<String, String>,
    picker: Vec<(String, String)>, // (shortcode, char)
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct GlyphRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    #[allow(dead_code)]
    i: i32,
}

struct Atlas {
    tex: u32,
    cell: f32,
    atlas_w: f32,
    atlas_h: f32,
    /// char → UV rect
    glyphs: HashMap<char, GlyphRect>,
    /// shortcode → char
    shortcodes: HashMap<String, char>,
    /// ordered picker entries
    picker: Vec<(String, char)>,
}

static ATLAS: OnceCell<Mutex<Option<Atlas>>> = OnceCell::new();

fn slot() -> &'static Mutex<Option<Atlas>> {
    ATLAS.get_or_init(|| Mutex::new(None))
}

fn ensure_loaded() -> bool {
    let mut g = slot().lock();
    if g.is_some() {
        return true;
    }
    let json = include_bytes!("../assets/emoji_atlas.json");
    let png = include_bytes!("../assets/emoji_atlas.png");
    let meta: AtlasMeta = match serde_json::from_slice(json) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[tuffbox-overlay] emoji atlas json: {e}");
            return false;
        }
    };
    let img = match image::load_from_memory(png) {
        Ok(i) => i.to_rgba8(),
        Err(e) => {
            eprintln!("[tuffbox-overlay] emoji atlas png: {e}");
            return false;
        }
    };
    let (w, h) = (img.width() as i32, img.height() as i32);
    let tex = gl::create_rgba_texture(w, h, &img.into_raw());
    if tex == 0 {
        return false;
    }

    let mut glyphs = HashMap::new();
    for (k, v) in meta.glyphs {
        // Keys are single Unicode scalars (or a short string — take first char).
        if let Some(ch) = k.chars().next() {
            glyphs.insert(ch, v);
            // Also index the stripped form without FE0F if present in key.
        }
    }
    let mut shortcodes = HashMap::new();
    for (code, chs) in meta.shortcodes {
        if let Some(ch) = chs.chars().next() {
            shortcodes.insert(code, ch);
        }
    }
    let mut picker = Vec::with_capacity(meta.picker.len());
    for (code, chs) in meta.picker {
        if let Some(ch) = chs.chars().next() {
            picker.push((code, ch));
        }
    }

    *g = Some(Atlas {
        tex,
        cell: meta.cell as f32,
        atlas_w: meta.atlas_w as f32,
        atlas_h: meta.atlas_h as f32,
        glyphs,
        shortcodes,
        picker,
    });
    true
}

/// Resolve `:shortcode:` → emoji char.
pub fn from_shortcode(name: &str) -> Option<char> {
    if !ensure_loaded() {
        return None;
    }
    let g = slot().lock();
    g.as_ref()?.shortcodes.get(name).copied()
}

pub fn has_glyph(ch: char) -> bool {
    if !ensure_loaded() {
        return false;
    }
    let g = slot().lock();
    g.as_ref()
        .map(|a| a.glyphs.contains_key(&ch))
        .unwrap_or(false)
}

/// Pixel size of one emoji cell at 1.0 scale.
pub fn cell_size() -> f32 {
    if !ensure_loaded() {
        return 24.0;
    }
    slot().lock().as_ref().map(|a| a.cell).unwrap_or(24.0)
}

/// Draw a single emoji glyph at top-left `(x,y)` with edge length `size`.
/// Returns true if a glyph was drawn.
pub fn draw(ch: char, x: f32, y: f32, size: f32) -> bool {
    if !ensure_loaded() {
        return false;
    }
    let g = slot().lock();
    let Some(a) = g.as_ref() else {
        return false;
    };
    let Some(r) = a.glyphs.get(&ch).copied() else {
        return false;
    };
    let u0 = r.x as f32 / a.atlas_w;
    let v0 = r.y as f32 / a.atlas_h;
    let u1 = (r.x + r.w) as f32 / a.atlas_w;
    let v1 = (r.y + r.h) as f32 / a.atlas_h;
    gl::textured_rect_uv(
        a.tex, x, y, size, size, u0, v0, u1, v1, 1.0, 1.0, 1.0, 1.0,
    );
    true
}

/// Ordered picker list: `(shortcode, char)`.
pub fn picker_entries() -> Vec<(String, char)> {
    if !ensure_loaded() {
        return Vec::new();
    }
    slot()
        .lock()
        .as_ref()
        .map(|a| a.picker.clone())
        .unwrap_or_default()
}

/// True if `ch` is in the emoji / symbol ranges we care about for layout.
pub fn is_emoji_char(ch: char) -> bool {
    if has_glyph(ch) {
        return true;
    }
    let u = ch as u32;
    // Broad emoji blocks — used only to decide "try emoji, else Sora".
    matches!(u,
        0x2600..=0x27BF   // misc symbols + dingbats
        | 0x1F300..=0x1FAFF // emoji
        | 0x1F1E6..=0x1F1FF // flags
        | 0x2300..=0x23FF
        | 0x2B50 | 0x2B55
        | 0x3030 | 0x303D
        | 0x3297 | 0x3299
        | 0xFE0F            // variation selector
        | 0x200D            // ZWJ
    )
}

/// Lookup closure suitable for `textutil::expand_shortcodes`.
pub fn shortcode_lookup(name: &str) -> Option<char> {
    from_shortcode(name)
}
