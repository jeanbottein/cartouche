//! Pixel-font text rendering.
//!
//! Uses VT323 (a bitmap-style TrueType, OFL). Its outlines sit on a 40-unit grid
//! (1000 units/em), so glyphs only land on whole pixels at 25 px: one font pixel =
//! one canvas pixel. Any other size makes strokes uneven, so glyphs are always
//! rasterized at that native size and enlarged by whole-number `font_scale` with
//! **nearest** filtering — only [`NORMAL`] (×1) and [`LARGE`] (×2) are used.
//! The atlas still carries faint anti-aliasing from tiny outline notches, so text
//! is drawn through a material that thresholds coverage to fully on/off.

use std::cell::OnceCell;

use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
use macroquad::prelude::*;

const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying lowp vec2 uv;
varying lowp vec4 color;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

const FRAGMENT: &str = r#"#version 100
varying lowp vec4 color;
varying lowp vec2 uv;

uniform sampler2D Texture;

void main() {
    lowp vec4 texel = texture2D(Texture, uv);
    if (texel.a < 0.5) {
        discard;
    }
    gl_FragColor = vec4(color.rgb * texel.rgb, color.a);
}"#;

/// Native VT323 size: one font pixel per canvas pixel.
const NATIVE: u16 = 25;
/// Body text (×1).
pub const NORMAL: u16 = NATIVE;
/// Headings (×2).
pub const LARGE: u16 = NATIVE * 2;

/// Whole-number enlargement for a requested size (multiples of [`NATIVE`]).
fn scale_for(size: u16) -> f32 {
    (size as f32 / NATIVE as f32).round().max(1.0)
}

thread_local! {
    static HARD_EDGE: OnceCell<Material> = const { OnceCell::new() };
}

/// Lazily-built material that turns anti-aliased glyph coverage into hard pixels.
fn with_hard_edge(draw: impl FnOnce()) {
    HARD_EDGE.with(|cell| {
        let material = cell.get_or_init(|| {
            load_material(
                ShaderSource::Glsl {
                    vertex: VERTEX,
                    fragment: FRAGMENT,
                },
                MaterialParams {
                    pipeline_params: PipelineParams {
                        color_blend: Some(BlendState::new(
                            Equation::Add,
                            BlendFactor::Value(BlendValue::SourceAlpha),
                            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                        )),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect("pixel text shader failed to compile")
        });
        gl_use_material(material);
        draw();
        gl_use_default_material();
    });
}

/// Load the embedded VT323 pixel font with nearest-neighbor filtering.
pub fn load_pixel_font() -> Font {
    let mut font = load_ttf_font_from_bytes(include_bytes!("../../assets/VT323-Regular.ttf"))
        .expect("embedded VT323 font failed to load");
    font.set_filter(FilterMode::Nearest);
    font
}

/// Draw text in the pixel font at a baseline of `(x, y)`. `size` should be
/// [`NORMAL`] or [`LARGE`].
pub fn text(s: &str, x: f32, y: f32, size: u16, color: Color, font: &Font) {
    // Snap the baseline to whole canvas pixels so glyphs land on the pixel grid.
    let (x, y) = (x.round(), y.round());
    with_hard_edge(|| {
        draw_text_ex(
            s,
            x,
            y,
            TextParams {
                font: Some(font),
                font_size: NATIVE,
                font_scale: scale_for(size),
                color,
                ..Default::default()
            },
        );
    });
}

/// Width in pixels of `s` rendered in the pixel font at `size`.
pub fn measure(s: &str, size: u16, font: &Font) -> f32 {
    measure_text(s, Some(font), NATIVE, scale_for(size)).width
}

/// Join `segments` with `sep`, wrapping to new lines so each stays within `max_w`.
pub fn wrap_segments(
    segments: &[&str],
    sep: &str,
    max_w: f32,
    size: u16,
    font: &Font,
) -> Vec<String> {
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
