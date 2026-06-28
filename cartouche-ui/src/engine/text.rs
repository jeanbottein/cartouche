//! Pixel-font text rendering.
//!
//! Uses VT323 (a bitmap-style TrueType, OFL) rendered with **nearest** filtering
//! at integer pixel sizes so glyphs stay crisp through the virtual-canvas upscale.
//! The vector default font looked blurry because its atlas is linear-filtered and
//! anti-aliased; this avoids both.

use macroquad::prelude::*;

/// Load the embedded VT323 pixel font with nearest-neighbor filtering.
pub fn load_pixel_font() -> Font {
    let mut font = load_ttf_font_from_bytes(include_bytes!("../../assets/VT323-Regular.ttf"))
        .expect("embedded VT323 font failed to load");
    font.set_filter(FilterMode::Nearest);
    font
}

/// Draw text in the pixel font at a baseline of `(x, y)`.
pub fn text(s: &str, x: f32, y: f32, size: u16, color: Color, font: &Font) {
    draw_text_ex(
        s,
        x,
        y,
        TextParams {
            font: Some(font),
            font_size: size,
            font_scale: 1.0,
            color,
            ..Default::default()
        },
    );
}

/// Width in pixels of `s` rendered in the pixel font at `size`.
pub fn measure(s: &str, size: u16, font: &Font) -> f32 {
    measure_text(s, Some(font), size, 1.0).width
}

/// Join `segments` with `sep`, wrapping to new lines so each stays within `max_w`.
pub fn wrap_segments(segments: &[&str], sep: &str, max_w: f32, size: u16, font: &Font) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for seg in segments {
        let trial = if cur.is_empty() {
            (*seg).to_string()
        } else {
            format!("{cur}{sep}{seg}")
        };
        if measure(&trial, size, font) > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = (*seg).to_string();
        } else {
            cur = trial;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}
