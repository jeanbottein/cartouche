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
/// `max_w` as measured by `width`. Each piece is measured once: widths are
/// sums of glyph advances (no kerning), so a line's width is its pieces'.
pub fn wrap_segments(
    segments: &[&str],
    sep: &str,
    max_w: f32,
    width: impl Fn(&str) -> f32,
) -> Vec<String> {
    let sep_w = width(sep);
    let mut lines = Vec::new();
    let (mut cur, mut cur_w) = (String::new(), 0.0);
    for seg in segments {
        let seg_w = width(seg);
        if !cur.is_empty() && cur_w + sep_w + seg_w > max_w {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }
        if !cur.is_empty() {
            cur.push_str(sep);
            cur_w += sep_w;
        }
        cur.push_str(seg);
        cur_w += seg_w;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

const ELLIPSIS: char = '\u{2026}';

/// `s` cut with an ellipsis to fit `max_w` as measured by `width`.
pub fn truncate_end(s: &str, max_w: f32, width: impl Fn(&str) -> f32) -> String {
    if width(s) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let start = |n: usize| chars[..n].iter().collect::<String>();
    let kept = longest_fitting(chars.len(), |n| {
        width(&format!("{}{ELLIPSIS}", start(n))) <= max_w
    });
    format!("{}{ELLIPSIS}", start(kept).trim_end())
}

/// The end of `s` behind an ellipsis, to fit `max_w` (for paths).
pub fn truncate_start(s: &str, max_w: f32, width: impl Fn(&str) -> f32) -> String {
    if width(s) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let end = |n: usize| chars[chars.len() - n..].iter().collect::<String>();
    let kept = longest_fitting(chars.len(), |n| {
        width(&format!("{ELLIPSIS}{}", end(n))) <= max_w
    });
    format!("{ELLIPSIS}{}", end(kept))
}

/// How many leading characters of `s` a ticker drops before the rest fits
/// `max_w`.
pub fn ticker_steps(s: &str, max_w: f32, width: impl Fn(&str) -> f32) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let end = |n: usize| chars[chars.len() - n..].iter().collect::<String>();
    chars.len() - longest_fitting(chars.len(), |n| width(&end(n)) <= max_w)
}

/// The largest `n` in `0..=len` for which `fits(n)` holds, `fits` holding up
/// to some `n` and not after it. A binary search: a few measurements of a
/// text instead of one per character it is too long by.
fn longest_fitting(len: usize, fits: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, len);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// `lines` kept to `max` lines, the overflow joined onto the last one (for
/// the caller to cut with an ellipsis).
pub fn fold_lines(mut lines: Vec<String>, max: usize) -> Vec<String> {
    if max == 0 || lines.len() <= max {
        return lines;
    }
    let rest = lines.split_off(max - 1).join(" ");
    lines.push(rest);
    lines
}

/// Seconds a ticker rests at each end of its text.
const TICKER_PAUSE: f64 = 1.5;
/// Seconds per character a ticker moves.
const TICKER_STEP: f64 = 0.15;

/// Characters a ticker has dropped from the start of its text, `elapsed`
/// seconds after it started: it rests, moves one character per step until
/// the last `steps` are dropped, rests on the end, and starts over.
pub fn ticker_offset(steps: usize, elapsed: f64) -> usize {
    if steps == 0 {
        return 0;
    }
    let cycle = 2.0 * TICKER_PAUSE + steps as f64 * TICKER_STEP;
    let moving = elapsed.rem_euclid(cycle) - TICKER_PAUSE;
    if moving < 0.0 {
        return 0;
    }
    ((moving / TICKER_STEP) as usize + 1).min(steps)
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

    fn owned(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn lines_within_the_limit_are_kept() {
        assert_eq!(fold_lines(owned(&["a", "b"]), 3), owned(&["a", "b"]));
    }

    #[test]
    fn extra_lines_join_the_last_kept_one() {
        assert_eq!(
            fold_lines(owned(&["a", "b", "c", "d"]), 3),
            owned(&["a", "b", "c d"])
        );
    }

    #[test]
    fn a_ticker_that_fits_never_moves() {
        assert_eq!(ticker_offset(0, 12.0), 0);
    }

    #[test]
    fn a_ticker_rests_before_moving() {
        assert_eq!(ticker_offset(5, 0.0), 0);
        assert_eq!(ticker_offset(5, TICKER_PAUSE - 0.01), 0);
    }

    #[test]
    fn a_ticker_moves_one_character_per_step() {
        assert_eq!(ticker_offset(5, TICKER_PAUSE + 0.01), 1);
        assert_eq!(ticker_offset(5, TICKER_PAUSE + TICKER_STEP + 0.01), 2);
    }

    #[test]
    fn a_ticker_rests_on_the_end_then_starts_over() {
        let end = TICKER_PAUSE + 5.0 * TICKER_STEP;
        assert_eq!(ticker_offset(5, end + 1.0), 5);
        assert_eq!(ticker_offset(5, end + TICKER_PAUSE + 0.01), 0);
    }

    #[test]
    fn truncation_keeps_what_fits_with_the_ellipsis() {
        assert_eq!(truncate_end("abcdef", 6.0, chars), "abcdef");
        assert_eq!(truncate_end("abcdef", 4.0, chars), "abc…");
        assert_eq!(truncate_end("ab cdef", 4.0, chars), "ab…");
        assert_eq!(truncate_end("abcdef", 0.5, chars), "…");
        assert_eq!(truncate_start("/a/b/c/d", 4.0, chars), "…c/d");
        assert_eq!(truncate_start("/a/b", 4.0, chars), "/a/b");
    }

    #[test]
    fn a_ticker_steps_until_the_rest_fits() {
        assert_eq!(ticker_steps("abcdef", 6.0, chars), 0);
        assert_eq!(ticker_steps("abcdef", 4.0, chars), 2);
        assert_eq!(ticker_steps("abcdef", 0.5, chars), 6);
    }

    #[test]
    fn longest_fitting_finds_the_last_length_that_fits() {
        assert_eq!(longest_fitting(10, |n| n <= 3), 3);
        assert_eq!(longest_fitting(2, |n| n <= 3), 2);
        assert_eq!(longest_fitting(10, |_| false), 0);
        assert_eq!(longest_fitting(0, |_| true), 0);
    }

    #[test]
    fn wrap_keeps_an_overlong_segment_on_its_own_line() {
        assert_eq!(
            wrap_segments(&["aaaaaaa", "b"], " ", 5.0, chars),
            vec!["aaaaaaa", "b"]
        );
    }
}
