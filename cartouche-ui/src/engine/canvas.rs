//! Virtual canvas + scaling (ENGINE-1) and aspect modes (ENGINE-2).
//!
//! Everything is drawn into a fixed low-resolution offscreen render target, then
//! blitted to the window with **nearest-neighbor** filtering at an **integer**
//! scale (letterboxed) so the retro pixel look stays crisp at any window size.
//!
//! Two aspect modes:
//! * `Wide`   — 854×480 (16:9), the default.
//! * `Square` — 480×480 (1:1). Narrower than widescreen; layouts reflow to use the
//!   square area (content stacks vertically instead of spreading wide).

use macroquad::prelude::*;

use super::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AspectMode {
    Wide,
    Square,
}

impl AspectMode {
    /// Logical (pre-scale) canvas size for this mode.
    pub fn base_size(self) -> (f32, f32) {
        match self {
            AspectMode::Wide => (854.0, 480.0),
            AspectMode::Square => (480.0, 480.0),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AspectMode::Wide => "Wide 854x480",
            AspectMode::Square => "Square 480x480",
        }
    }
}

pub struct Canvas {
    pub mode: AspectMode,
    /// When true, snap to whole-number pixel scales (crispest, but leaves borders
    /// when the window isn't an exact multiple of the canvas). When false (default),
    /// scale to fit the window — still nearest-neighbor, just fills the space.
    pub integer_scale: bool,
    target: RenderTarget,
    w: f32,
    h: f32,
}

impl Canvas {
    pub fn new(mode: AspectMode) -> Self {
        let (w, h) = mode.base_size();
        let target = render_target(w as u32, h as u32);
        target.texture.set_filter(FilterMode::Nearest);
        Self {
            mode,
            integer_scale: false,
            target,
            w,
            h,
        }
    }

    pub fn size(&self) -> (f32, f32) {
        (self.w, self.h)
    }

    pub fn toggle_integer_scale(&mut self) {
        self.integer_scale = !self.integer_scale;
    }

    pub fn scale_label(&self) -> &'static str {
        if self.integer_scale {
            "integer"
        } else {
            "fit"
        }
    }

    pub fn toggle_aspect(&mut self) {
        self.set_aspect(match self.mode {
            AspectMode::Wide => AspectMode::Square,
            AspectMode::Square => AspectMode::Wide,
        });
    }

    pub fn set_aspect(&mut self, mode: AspectMode) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        let (w, h) = mode.base_size();
        self.w = w;
        self.h = h;
        self.target = render_target(w as u32, h as u32);
        self.target.texture.set_filter(FilterMode::Nearest);
    }

    /// Integer scale + centered offset used to place the canvas in the window.
    fn layout(&self) -> (f32, Vec2) {
        let sw = screen_width();
        let sh = screen_height();
        let mut scale = (sw / self.w).min(sh / self.h);
        // Optional crisp mode: snap to whole pixels (but never below 1x so a small
        // window still shows the canvas instead of cropping it).
        if self.integer_scale && scale >= 1.0 {
            scale = scale.floor();
        }
        let dw = self.w * scale;
        let dh = self.h * scale;
        let offset = vec2((sw - dw) / 2.0, (sh - dh) / 2.0);
        (scale, offset)
    }

    /// Convert a window-space point (e.g. the mouse) into canvas space, or `None`
    /// if it falls in the letterbox.
    pub fn screen_to_canvas(&self, p: Vec2) -> Option<Vec2> {
        let (scale, offset) = self.layout();
        let local = (p - offset) / scale;
        if local.x >= 0.0 && local.y >= 0.0 && local.x <= self.w && local.y <= self.h {
            Some(local)
        } else {
            None
        }
    }

    /// Begin drawing into the virtual canvas (origin top-left, y down).
    pub fn begin(&self) {
        set_camera(&Camera2D {
            zoom: vec2(2.0 / self.w, 2.0 / self.h),
            target: vec2(self.w / 2.0, self.h / 2.0),
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
        let (scale, offset) = self.layout();
        draw_texture_ex(
            &self.target.texture,
            offset.x,
            offset.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(self.w * scale, self.h * scale)),
                ..Default::default()
            },
        );
    }
}
