//! Text rendering at the screen's resolution.
//!
//! Positions and sizes are in design pixels; glyphs are rasterized at that
//! size × the canvas scale, so text gets sharper on bigger screens without
//! moving. Every typeface sits on the same baselines: VT323 fills its 14 px
//! cap height, the smooth faces are a touch smaller (12.6 px caps) so they
//! breathe in the same rows.
//!
//! * Pixel (VT323): a bitmap-style TrueType whose outlines sit on a 40-unit
//!   grid. Its faint anti-aliasing is thresholded to fully on/off pixels.
//! * Smooth (Barlow): anti-aliased; medium weight for body
//!   text, semibold for headings.

use std::cell::OnceCell;

use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
use macroquad::prelude::*;

/// macroquad's standard vertex shader, for custom materials.
pub(super) const VERTEX: &str = r#"#version 100
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Typeface {
    Pixel,
    Barlow,
}

#[derive(Clone, Copy)]
pub enum Size {
    /// Button hints (0.8 × body).
    Small,
    /// Body text.
    Normal,
    /// Panel titles (1.4 × body).
    Title,
    /// Headings (twice the body size).
    Large,
}

/// The fonts of one typeface, loaded.
pub struct Fonts {
    typeface: Typeface,
    body: Font,
    heading: Font,
}

impl Fonts {
    pub fn load(typeface: Typeface) -> Fonts {
        let (body, heading) = match typeface {
            Typeface::Pixel => {
                let mut font = load(include_bytes!("../../../assets/VT323-Regular.ttf"));
                font.set_filter(FilterMode::Nearest);
                (font.clone(), font)
            }
            Typeface::Barlow => (
                load(include_bytes!("../../../assets/Barlow-Medium.ttf")),
                load(include_bytes!("../../../assets/Barlow-SemiBold.ttf")),
            ),
        };
        Fonts {
            typeface,
            body,
            heading,
        }
    }

    pub fn typeface(&self) -> Typeface {
        self.typeface
    }

    fn font(&self, size: Size) -> &Font {
        match size {
            Size::Small | Size::Normal => &self.body,
            Size::Title | Size::Large => &self.heading,
        }
    }

    /// Font size in design pixels: VT323 at its native 25 px (one font pixel
    /// per design pixel), the others at 18 px.
    fn design_px(&self, size: Size) -> f32 {
        let body = match self.typeface {
            Typeface::Pixel => 25.0,
            Typeface::Barlow => 18.0,
        };
        match size {
            Size::Small => body * 0.8,
            Size::Normal => body,
            Size::Title => body * 1.4,
            Size::Large => body * 2.0,
        }
    }

    /// Parameters drawing `size` text at `scale` screen pixels per design pixel.
    fn params(&self, size: Size, scale: f32, color: Color) -> TextParams<'_> {
        let px = self.design_px(size);
        let raster = (px * scale).round().max(1.0);
        TextParams {
            font: Some(self.font(size)),
            font_size: raster as u16,
            font_scale: px / raster,
            color,
            ..Default::default()
        }
    }
}

fn load(bytes: &[u8]) -> Font {
    load_ttf_font_from_bytes(bytes).expect("embedded font failed to load")
}

thread_local! {
    static HARD_EDGE: OnceCell<Material> = const { OnceCell::new() };
}

/// Draw with a material that turns anti-aliased glyph coverage into hard pixels.
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

/// `v` design pixels, snapped to whole screen pixels.
fn snap(v: f32, scale: f32) -> f32 {
    (v * scale).round() / scale
}

/// Draw text with its baseline at `(x, y)`, `scale` screen pixels per design pixel.
pub fn text(s: &str, x: f32, y: f32, size: Size, color: Color, fonts: &Fonts, scale: f32) {
    // Glyphs land on whole screen pixels: sharp, and no shimmer while scrolling.
    let (x, y) = (snap(x, scale), snap(y, scale));
    let params = fonts.params(size, scale, color);
    match fonts.typeface {
        Typeface::Pixel => with_hard_edge(|| {
            draw_text_ex(s, x, y, params);
        }),
        _ => {
            draw_text_ex(s, x, y, params);
        }
    }
}

/// Width of `s` in design pixels.
pub fn measure(s: &str, size: Size, fonts: &Fonts, scale: f32) -> f32 {
    let params = fonts.params(size, scale, WHITE);
    measure_text(s, params.font, params.font_size, params.font_scale).width
}

/// Join `segments` with `sep`, wrapping to new lines so each stays within
/// `max_w` as measured by `width`.
pub fn wrap_segments(
    segments: &[&str],
    sep: &str,
    max_w: f32,
    width: impl Fn(&str) -> f32,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for seg in segments {
        let trial = if cur.is_empty() {
            (*seg).to_string()
        } else {
            format!("{cur}{sep}{seg}")
        };
        if width(&trial) > max_w && !cur.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn wrap_breaks_between_segments() {
        assert_eq!(
            wrap_segments(&["aa", "bb", "cc"], " ", 5.0, chars),
            vec!["aa bb", "cc"]
        );
    }

    #[test]
    fn wrap_keeps_an_overlong_segment_on_its_own_line() {
        assert_eq!(
            wrap_segments(&["aaaaaaa", "b"], " ", 5.0, chars),
            vec!["aaaaaaa", "b"]
        );
    }
}
