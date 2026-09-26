//! Sora (Google Fonts, OFL-1.1) atlas renderer — **not** the Minecraft default font.
//!
//! Atlases are pre-baked PNGs + JSON under `assets/` and embedded at compile time
//! so the injected DLL needs no external font files at runtime.

use crate::emoji;
use crate::gl;
use crate::theme;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Bold,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Sm, // ~14px
    Md, // ~18px
    Lg, // ~24px titles
}

#[derive(Debug, Deserialize)]
struct AtlasMeta {
    ascent: i32,
    line_height: i32,
    atlas_w: i32,
    atlas_h: i32,
    glyphs: HashMap<String, GlyphMeta>,
    #[allow(dead_code)]
    size_px: i32,
    #[allow(dead_code)]
    descent: i32,
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct GlyphMeta {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    xoff: i32,
    yoff: i32,
    adv: f32,
}

struct Atlas {
    tex: u32,
    meta: AtlasMeta,
    glyphs: HashMap<char, GlyphMeta>,
}

static ATLASES: OnceCell<Mutex<HashMap<&'static str, Atlas>>> = OnceCell::new();

fn atlases() -> &'static Mutex<HashMap<&'static str, Atlas>> {
    ALTASES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn style_key(weight: Weight, size: Size) -> &'static str {
    match (weight, size) {
        (Weight::Regular, Size::Sm) => "reg14",
        (Weight::Regular, Size::Md) => "reg18",
        (Weight::Regular, Size::Lg) => "bold24",
        (Weight::Bold, Size::Sm) => "bold18",
        (Weight::Bold, Size::Md) => "bold18",
        (Weight::Bold, Size::Lg) => "bold24",
    }
}

/// Map legacy `scale` knobs (1.0 / 1.15 / 1.25 / 1.4) onto Sora size+weight.
fn scale_to_style(scale: f32) -> (Weight, Size) {
    if scale >= 1.35 {
        (Weight::Bold, Size::Lg)
    } else if scale >= 1.15 {
        (Weight::Bold, Size::Md)
    } else if scale <= 1.0 {
        (Weight::Regular, Size::Sm)
    } else {
        (Weight::Regular, Size::Md)
    }
}

fn load_atlas(id: &'static str) -> Option<()> {
    let (png, json): (&[u8], &[u8]) = match id {
        "reg14" => (
            include_bytes!("../assets/sora_reg_14.png"),
            include_bytes!("../assets/sora_reg_14.json"),
        ),
        "reg18" => (
            include_bytes!("../assets/sora_reg_18.png"),
            include_bytes!("../assets/sora_reg_18.json"),
        ),
        "bold18" => (
            include_bytes!("../assets/sora_bold_18.png"),
            include_bytes!("../assets/sora_bold_18.json"),
        ),
        "bold24" => (
            include_bytes!("../assets/sora_bold_24.png"),
            include_bytes!("../assets/sora_bold_24.json"),
        ),
        _ => return None,
    };
    let meta: AtlasMeta = serde_json::from_slice(json).ok()?;
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = (img.width() as i32, img.height() as i32);
    let tex = gl::create_rgba_texture(w, h, &img.into_raw());
    if tex == 0 {
        return None;
    }
    let mut glyphs = HashMap::new();
    for (k, v) in &meta.glyphs {
        if let Some(ch) = k.chars().next() {
            glyphs.insert(ch, *v);
        }
    }
    atlases().lock().insert(id, Atlas { tex, meta, glyphs });
    Some(())
}

fn with_atlas<R>(weight: Weight, size: Size, f: impl FnOnce(&Atlas) -> R) -> Option<R> {
    let id = style_key(weight, size);
    {
        let g = atlases().lock();
        if let Some(a) = g.get(id) {
            return Some(f(a));
        }
    }
    load_atlas(id)?;
    let g = atlases().lock();
    g.get(id).map(f)
}

pub fn line_height(scale: f32) -> f32 {
    let (w, s) = scale_to_style(scale);
    with_atlas(w, s, |a| a.meta.line_height as f32).unwrap_or(20.0)
}

fn emoji_advance(size: Size) -> f32 {
    // Match the line box so emoji sits comfortably next to Sora glyphs.
    match size {
        Size::Sm => 16.0,
        Size::Md => 20.0,
        Size::Lg => 26.0,
    }
}

fn measure_ws(text: &str, weight: Weight, size: Size) -> f32 {
    let emoji_adv = emoji_advance(size);
    with_atlas(weight, size, |a| {
        let mut w = 0.0f32;
        for ch in text.chars() {
            if ch == '\n' {
                break;
            }
            // Skip VS16 — it's a modifier, zero advance when following emoji.
            if ch == '\u{FE0F}' {
                continue;
            }
            if emoji::has_glyph(ch) {
                w += emoji_adv;
                continue;
            }
            if let Some(g) = a.glyphs.get(&ch) {
                w += g.adv;
            } else if emoji::is_emoji_char(ch) {
                // Unknown emoji: reserve a diamond slot.
                w += emoji_adv;
            } else if let Some(g) = a.glyphs.get(&'?') {
                w += g.adv;
            } else {
                w += a.meta.size_px as f32 * 0.5;
            }
        }
        w
    })
    .unwrap_or(text.len() as f32 * 8.0)
}

/// Public measure — keeps the old `(text, scale)` signature used across `ui.rs`.
pub fn measure(text: &str, scale: f32) -> f32 {
    let (w, s) = scale_to_style(scale);
    measure_ws(text, w, s)
}

/// Draw text. `scale` selects Sora size/weight. Always draws a 1px MC-style shadow.
pub fn draw(
    text: &str,
    x: f32,
    y: f32,
    scale: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    max_w: f32,
) -> f32 {
    let (weight, size) = scale_to_style(scale);
    let sh = theme::TEXT_SHADOW;
    let _ = draw_glyphs(
        text,
        x + 1.0,
        y + 1.0,
        weight,
        size,
        sh.0,
        sh.1,
        sh.2,
        sh.3 * a,
        max_w,
    );
    draw_glyphs(text, x, y, weight, size, r, g, b, a, max_w)
}

pub fn draw_fit(
    text: &str,
    x: f32,
    y: f32,
    scale: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    max_w: f32,
) {
    if max_w <= 0.0 {
        draw(text, x, y, scale, r, g, b, a, 0.0);
        return;
    }
    if measure(text, scale) <= max_w {
        draw(text, x, y, scale, r, g, b, a, 0.0);
        return;
    }
    let ell = "...";
    let ell_w = measure(ell, scale);
    if ell_w >= max_w {
        draw(ell, x, y, scale, r, g, b, a, max_w);
        return;
    }
    let mut end = text.len();
    while end > 0 {
        if text.is_char_boundary(end) {
            let slice = &text[..end];
            if measure(slice, scale) + ell_w <= max_w {
                let w = draw(slice, x, y, scale, r, g, b, a, 0.0);
                draw(ell, x + w, y, scale, r, g, b, a, 0.0);
                return;
            }
        }
        end -= 1;
    }
    draw(ell, x, y, scale, r, g, b, a, max_w);
}

fn draw_glyphs(
    text: &str,
    x: f32,
    y: f32,
    weight: Weight,
    size: Size,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    max_w: f32,
) -> f32 {
    let emoji_adv = emoji_advance(size);
    with_atlas(weight, size, |atlas| {
        let limit = if max_w > 0.0 { x + max_w } else { f32::MAX };
        let mut cx = x;
        let baseline = y + atlas.meta.ascent as f32;
        // Vertically centre emoji in the line box.
        let emoji_y = y + ((atlas.meta.line_height as f32 - emoji_adv) * 0.5).max(0.0);
        for ch in text.chars() {
            if ch == '\n' {
                break;
            }
            if ch == '\u{FE0F}' {
                continue;
            }
            // ── Emoji path ────────────────────────────────────────────
            if emoji::has_glyph(ch) {
                if cx + emoji_adv > limit + 0.5 {
                    break;
                }
                let _ = emoji::draw(ch, cx, emoji_y, emoji_adv);
                cx += emoji_adv;
                continue;
            }
            if emoji::is_emoji_char(ch) && !ch.is_ascii() {
                // Missing curated glyph — draw a small gold diamond placeholder.
                if cx + emoji_adv > limit + 0.5 {
                    break;
                }
                let d = emoji_adv * 0.55;
                let dx = cx + (emoji_adv - d) * 0.5;
                let dy = emoji_y + (emoji_adv - d) * 0.5;
                gl::fill_rect(dx, dy, d, d, 0.95, 0.75, 0.2, a * 0.85);
                cx += emoji_adv;
                continue;
            }

            // ── Sora path ─────────────────────────────────────────────
            let gm = atlas
                .glyphs
                .get(&ch)
                .or_else(|| atlas.glyphs.get(&'?'))
                .copied();
            let Some(gm) = gm else {
                cx += atlas.meta.size_px as f32 * 0.45;
                continue;
            };
            if cx + gm.adv > limit + 0.5 {
                break;
            }
            if gm.w > 0 && gm.h > 0 {
                let dx = cx + gm.xoff as f32;
                let dy = baseline + gm.yoff as f32;
                gl::textured_rect_uv(
                    atlas.tex,
                    dx,
                    dy,
                    gm.w as f32,
                    gm.h as f32,
                    gm.x as f32 / atlas.meta.atlas_w as f32,
                    gm.y as f32 / atlas.meta.atlas_h as f32,
                    (gm.x + gm.w) as f32 / atlas.meta.atlas_w as f32,
                    (gm.y + gm.h) as f32 / atlas.meta.atlas_h as f32,
                    r,
                    g,
                    b,
                    a,
                );
            }
            cx += gm.adv;
        }
        cx - x
    })
    .unwrap_or(0.0)
}

pub fn wrap(text: &str, scale: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    if text.is_empty() {
        return lines;
    }
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if measure(&candidate, scale) > max_w && !line.is_empty() {
            lines.push(line);
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
