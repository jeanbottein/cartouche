//! Virtual canvas + scaling (ENGINE-1) and aspect modes (ENGINE-2).
//!
//! Everything is drawn into a low-resolution offscreen render target, then
//! blitted to the window with **nearest-neighbor** filtering.
//!
//! The canvas is always [`DESIGN_HEIGHT`] px tall, so the UI looks the same at
//! any resolution; its width follows the window's shape (4:3 → 576×432,
//! 16:9 → 768×432, 16:10 → 691×432). It is scaled to fill the window height —
//! exact ×5 at 4K, ×2.5 at 1080p. Windows shorter than that get a 1:1 canvas of
//! their own height instead, so small screens keep legible text.
//!
//! Two aspect modes:
//! * `Wide`   — fills the whole window, the default.
//! * `Square` — a square canvas centered in the screen. Layouts reflow to use the
//!   square area (content stacks vertically instead of spreading wide).

use macroquad::prelude::*;

use super::theme;

/// Canvas height (in canvas pixels) the layout is designed for.
pub const DESIGN_HEIGHT: f32 = 432.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AspectMode {
    Wide,
    Square,
}

pub struct Canvas {
    pub mode: AspectMode,
    target: RenderTarget,
    w: f32,
    h: f32,
    /// Screen pixels per canvas pixel.
    scale: f32,
    /// Physical screen size the canvas was last fitted to.
    fitted_to: (f32, f32),
}

impl Canvas {
    pub fn new(mode: AspectMode) -> Self {
        let mut canvas = Self {
            mode,
            target: new_target(1.0, 1.0),
            w: 1.0,
            h: 1.0,
            scale: 1.0,
            fitted_to: (0.0, 0.0),
        };
        canvas.fit();
        canvas
    }

    pub fn size(&self) -> (f32, f32) {
        (self.w, self.h)
    }

    pub fn toggle_aspect(&mut self) {
        self.mode = match self.mode {
            AspectMode::Wide => AspectMode::Square,
            AspectMode::Square => AspectMode::Wide,
        };
        self.fitted_to = (0.0, 0.0); // force a refit
        self.fit();
    }

    /// Re-derive the canvas size and scale from the window. Cheap when nothing
    /// changed; call once per frame so window resizes / fullscreen are picked up.
    pub fn fit(&mut self) {
        let (pw, ph) = physical_screen();
        if (pw, ph) == self.fitted_to {
            return;
        }
        self.fitted_to = (pw, ph);

        let (h, scale) = if ph >= DESIGN_HEIGHT {
            (DESIGN_HEIGHT, ph / DESIGN_HEIGHT)
        } else {
            (ph.max(1.0), 1.0)
        };
        let full_w = (pw / scale).floor().max(1.0);
        let w = match self.mode {
            AspectMode::Wide => full_w,
            AspectMode::Square => h.min(full_w),
        };

        self.scale = scale;
        if (w, h) != (self.w, self.h) {
            self.w = w;
            self.h = h;
            self.target = new_target(w, h);
        }
    }

    /// (scale, offset) in macroquad's logical window units, with the canvas
    /// centered on whole physical pixels.
    fn layout(&self) -> (f32, Vec2) {
        let dpi = screen_dpi_scale();
        let (pw, ph) = physical_screen();
        let ox = ((pw - self.w * self.scale) / 2.0).floor();
        let oy = ((ph - self.h * self.scale) / 2.0).floor();
        (self.scale / dpi, vec2(ox, oy) / dpi)
    }

    /// (scale, offset) mapping canvas space -> window space. Lets callers overlay
    /// full-resolution art (e.g. cover textures) on top of the pixel canvas.
    pub fn present_transform(&self) -> (f32, Vec2) {
        self.layout()
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
