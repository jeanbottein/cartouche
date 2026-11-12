//! Virtual canvas, scaling and layouts.
//!
//! Everything is drawn into a low-resolution offscreen render target, then
//! blitted to the window with **nearest-neighbor** filtering.
//!
//! Resolution independent: the canvas is measured in design pixels, never in
//! screen pixels. It is the smallest canvas of its shape that covers the
//! window, scaled up to fill it (×2.5 at 1080p, ×5 at 4K), so the UI looks the
//! same at any resolution. Windows smaller than that get a 1:1 canvas instead,
//! so text stays legible.
//!
//! Two shapes:
//! * wide — [`DESIGN_HEIGHT`] tall, at least 3:2, as wide as the window
//!   (16:10 → 691×432, 16:9 → 768×432): side panels beside the content.
//! * 4:3  — [`NARROW_WIDTH`] wide, at least 4:3, as tall as the window
//!   (4:3 → 576×432, 9:16 → 576×1024): content stacks; portrait screens
//!   extend it downwards, with room for the panel below the content.
//!
//! `WIDESCREEN` picks the shape: auto (wide when the window is 3:2 or wider),
//! on or off. A window of another shape gets black bars.

use macroquad::prelude::*;

use super::theme;

/// Canvas height (in canvas pixels) the layout is designed for.
pub const DESIGN_HEIGHT: f32 = 432.0;
/// Width of the 4:3 canvas.
const NARROW_WIDTH: f32 = DESIGN_HEIGHT * 4.0 / 3.0;
/// Narrowest wide canvas (3:2); auto picks wide from this window shape on.
const WIDE_MIN_ASPECT: f32 = 1.5;
/// A 4:3 canvas at least this tall has room for a panel below the content.
const TALL_HEIGHT: f32 = 720.0;

/// The `WIDESCREEN` setting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Widescreen {
    Auto,
    On,
    Off,
}

impl Widescreen {
    /// From `config.txt` (any case); unset or unknown means auto.
    pub fn parse(value: Option<&str>) -> Widescreen {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("on") => Widescreen::On,
            Some("off") => Widescreen::Off,
            _ => Widescreen::Auto,
        }
    }
}

/// How screens arrange themselves on the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// Panels beside the content.
    Wide,
    /// 4:3: the content only.
    Narrow,
    /// 4:3 extended vertically: panels below the content.
    Tall,
}

/// Canvas size, screen pixels per canvas pixel and layout for a window.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Fit {
    w: f32,
    h: f32,
    scale: f32,
    layout: Layout,
}

/// Fit a canvas to a window of `pw`×`ph` physical pixels.
fn fit_window((pw, ph): (f32, f32), widescreen: Widescreen) -> Fit {
    let wide = match widescreen {
        Widescreen::Auto => pw >= ph * WIDE_MIN_ASPECT,
        Widescreen::On => true,
        Widescreen::Off => false,
    };
    // (narrowest width, widest width, tallest height) of the shape.
    let (min_w, max_w, max_h) = if wide {
        (
            DESIGN_HEIGHT * WIDE_MIN_ASPECT,
            f32::INFINITY,
            DESIGN_HEIGHT,
        )
    } else {
        (NARROW_WIDTH, NARROW_WIDTH, f32::INFINITY)
    };
    let scale = (pw / min_w).min(ph / DESIGN_HEIGHT).max(1.0);
    // The epsilon keeps e.g. 1024 / (1024 / 648) from flooring to 647.
    let w = (pw / scale + 1e-3).floor().min(max_w).max(1.0);
    let h = (ph / scale + 1e-3).floor().min(max_h).max(1.0);
    let layout = match (wide, h >= TALL_HEIGHT) {
        (true, _) => Layout::Wide,
        (false, true) => Layout::Tall,
        (false, false) => Layout::Narrow,
    };
    Fit {
        w,
        h,
        scale,
        layout,
    }
}

pub struct Canvas {
    widescreen: Widescreen,
    target: RenderTarget,
    fit: Fit,
    /// Physical screen size the canvas was last fitted to.
    fitted_to: (f32, f32),
}

impl Canvas {
    pub fn new(widescreen: Widescreen) -> Self {
        let mut canvas = Self {
            widescreen,
            target: new_target(1.0, 1.0),
            fit: Fit {
                w: 1.0,
                h: 1.0,
                scale: 1.0,
                layout: Layout::Narrow,
            },
            fitted_to: (0.0, 0.0),
        };
        canvas.fit();
        canvas
    }

    pub fn size(&self) -> (f32, f32) {
        (self.fit.w, self.fit.h)
    }

    pub fn layout(&self) -> Layout {
        self.fit.layout
    }

    pub fn set_widescreen(&mut self, widescreen: Widescreen) {
        if widescreen == self.widescreen {
            return;
        }
        self.widescreen = widescreen;
        self.fitted_to = (0.0, 0.0); // force a refit
        self.fit();
    }

    /// Switch between the wide and 4:3 layouts, until the setting changes.
    pub fn toggle_aspect(&mut self) {
        self.set_widescreen(match self.fit.layout {
            Layout::Wide => Widescreen::Off,
            Layout::Narrow | Layout::Tall => Widescreen::On,
        });
    }

    /// Re-derive the canvas size and scale from the window. Cheap when nothing
    /// changed; call once per frame so window resizes / fullscreen are picked up.
    pub fn fit(&mut self) {
        let window = physical_screen();
        if window == self.fitted_to {
            return;
        }
        self.fitted_to = window;
        let fit = fit_window(window, self.widescreen);
        if (fit.w, fit.h) != self.size() {
            self.target = new_target(fit.w, fit.h);
        }
        self.fit = fit;
    }

    /// (scale, offset) mapping canvas space -> window space (macroquad's
    /// logical units), with the canvas centered on whole physical pixels. Lets
    /// callers overlay full-resolution art (e.g. covers) on the pixel canvas.
    pub fn present_transform(&self) -> (f32, Vec2) {
        let dpi = screen_dpi_scale();
        let (pw, ph) = physical_screen();
        let Fit { w, h, scale, .. } = self.fit;
        let ox = ((pw - w * scale) / 2.0).floor();
        let oy = ((ph - h * scale) / 2.0).floor();
        (scale / dpi, vec2(ox, oy) / dpi)
    }

    /// Convert a window-space point (e.g. the mouse) into canvas space, or `None`
    /// if it falls in the letterbox.
    pub fn screen_to_canvas(&self, p: Vec2) -> Option<Vec2> {
        let (scale, offset) = self.present_transform();
        let local = (p - offset) / scale;
        let (w, h) = self.size();
        (local.x >= 0.0 && local.y >= 0.0 && local.x <= w && local.y <= h).then_some(local)
    }

    /// Begin drawing into the virtual canvas (origin top-left, y down).
    pub fn begin(&self) {
        let (w, h) = self.size();
        set_camera(&Camera2D {
            zoom: vec2(2.0 / w, 2.0 / h),
            target: vec2(w / 2.0, h / 2.0),
            render_target: Some(self.target.clone()),
            ..Default::default()
        });
        clear_background(theme::BG);
    }

    /// Finish canvas drawing.
    pub fn end(&self) {
        set_default_camera();
    }

    /// Blit the virtual canvas to the window (pure-black letterbox).
    pub fn present(&self) {
        clear_background(theme::BG);
        let (scale, offset) = self.present_transform();
        let (w, h) = self.size();
        draw_texture_ex(
            &self.target.texture,
            offset.x,
            offset.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(w * scale, h * scale)),
                ..Default::default()
            },
        );
    }
}

fn new_target(w: f32, h: f32) -> RenderTarget {
    let target = render_target(w as u32, h as u32);
    target.texture.set_filter(FilterMode::Nearest);
    target
}

/// Window size in physical pixels (macroquad reports logical units under high-DPI).
fn physical_screen() -> (f32, f32) {
    let dpi = screen_dpi_scale();
    (
        (screen_width() * dpi).round(),
        (screen_height() * dpi).round(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fitted(window: (f32, f32), widescreen: Widescreen) -> (f32, f32, Layout) {
        let fit = fit_window(window, widescreen);
        (fit.w, fit.h, fit.layout)
    }

    #[test]
    fn auto_is_wide_on_16_9() {
        assert_eq!(
            fitted((1920.0, 1080.0), Widescreen::Auto),
            (768.0, 432.0, Layout::Wide)
        );
    }

    #[test]
    fn auto_is_wide_on_steam_deck() {
        assert_eq!(
            fitted((1280.0, 800.0), Widescreen::Auto),
            (691.0, 432.0, Layout::Wide)
        );
    }

    #[test]
    fn auto_is_4_3_on_4_3() {
        assert_eq!(
            fitted((1024.0, 768.0), Widescreen::Auto),
            (576.0, 432.0, Layout::Narrow)
        );
    }

    #[test]
    fn auto_extends_4_3_downwards_on_portrait() {
        assert_eq!(
            fitted((1080.0, 1920.0), Widescreen::Auto),
            (576.0, 1024.0, Layout::Tall)
        );
    }

    #[test]
    fn scale_is_the_same_at_every_resolution_of_a_shape() {
        let hd = fit_window((1920.0, 1080.0), Widescreen::Auto);
        let uhd = fit_window((3840.0, 2160.0), Widescreen::Auto);
        assert_eq!((hd.w, hd.h, hd.scale * 2.0), (uhd.w, uhd.h, uhd.scale));
    }

    #[test]
    fn off_pillarboxes_4_3_on_16_9() {
        assert_eq!(
            fitted((1920.0, 1080.0), Widescreen::Off),
            (576.0, 432.0, Layout::Narrow)
        );
    }

    #[test]
    fn on_letterboxes_3_2_on_4_3() {
        assert_eq!(
            fitted((1024.0, 768.0), Widescreen::On),
            (648.0, 432.0, Layout::Wide)
        );
    }

    #[test]
    fn small_windows_get_a_1_to_1_canvas() {
        let fit = fit_window((400.0, 300.0), Widescreen::Auto);
        assert_eq!((fit.w, fit.h, fit.scale), (400.0, 300.0, 1.0));
    }

    #[test]
    fn parse_reads_any_case_and_defaults_to_auto() {
        assert_eq!(Widescreen::parse(Some("ON")), Widescreen::On);
        assert_eq!(Widescreen::parse(Some("off")), Widescreen::Off);
        assert_eq!(Widescreen::parse(Some("maybe")), Widescreen::Auto);
        assert_eq!(Widescreen::parse(None), Widescreen::Auto);
    }
}
