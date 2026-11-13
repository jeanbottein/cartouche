//! The look every screen shares, drawn from the current theme: backdrop,
//! cards, focus highlights, panels, wells, progress bars, toasts, chips and
//! button badges. Screens decide where things go; this decides how they look.

use macroquad::prelude::*;

use super::engine::shape::{fill_rounded, glow, gradient, stroke_rounded};
use super::engine::theme::{self, fade};
use super::Ui;

/// Footer button badges.
const BADGE_H: f32 = 16.0;

/// The background gradient of the whole canvas, painted inside `r` only
/// (opaque masks over scrolled content blend in).
pub fn backdrop(r: Rect, canvas_h: f32) {
    let t = theme::current();
    let at = |y: f32| {
        let k = (y / canvas_h.max(1.0)).clamp(0.0, 1.0);
        Color::from_vec(t.bg_top.to_vec().lerp(t.bg_bottom.to_vec(), k))
    };
    gradient(r, at(r.y), at(r.bottom()));
}

/// The colored mark on a row's leading edge: the cartridge's full-height
/// label stripe on square themes, an inset pill on rounded ones.
fn stripe(r: Rect, color: Color, square_width: f32, pill_width: f32) {
    if theme::current().radius <= 0.0 {
        draw_rectangle(r.x, r.y, square_width, r.h, color);
        return;
    }
    let inset = (r.h * 0.28).round();
    let pill = Rect::new(r.x + 3.0, r.y + inset, pill_width, r.h - 2.0 * inset);
    fill_rounded(pill, pill_width / 2.0, color);
}

/// The blue mark on a row's trailing edge for games in Steam: a full-height
/// bar inside the frame on square themes, an inset pill on rounded ones.
pub fn steam_flag(r: Rect, focused: bool) {
    let t = theme::current();
    if t.radius <= 0.0 {
        let frame = if focused { t.focus_width } else { 1.0 };
        draw_rectangle(
            r.right() - frame - 4.0,
            r.y + frame,
            4.0,
            r.h - 2.0 * frame,
            t.info,
        );
        return;
    }
    let inset = (r.h * 0.28).round();
    let pill = Rect::new(r.right() - 6.0, r.y + inset, 3.0, r.h - 2.0 * inset);
    fill_rounded(pill, 1.5, t.info);
}

/// A list card (Library, Network, Mods): a box with a colored stripe on its
/// leading edge, lifted and outlined in the accent when focused.
pub fn card(r: Rect, stripe_color: Color, focused: bool) {
    let t = theme::current();
    if focused && t.glow {
        glow(r, t.radius, 8.0, fade(t.accent, 0.25));
    }
    fill_rounded(r, t.radius, if focused { t.surface_hi } else { t.surface });
    stripe(r, stripe_color, 5.0, 3.0);
    if focused {
        stroke_rounded(r, t.radius, t.focus_width, t.accent);
    } else if t.card_outline {
        stroke_rounded(r, t.radius, 1.0, t.border);
    }
}

/// The focused row of a plain list (forms, menus, the file browser).
pub fn highlight(r: Rect, color: Color) {
    let r = Rect::new(r.x, r.y, r.w, r.h - 2.0);
    fill_rounded(r, theme::current().radius, theme::surface_hi());
    stripe(r, color, 3.0, 2.0);
}

/// A modal frame over the dimmed screen.
pub fn panel(ui: &Ui, r: Rect, border: Color) {
    let t = theme::current();
    if t.shadow.a > 0.0 {
        let (w, h) = ui.canvas.size();
        draw_rectangle(0.0, 0.0, w, h, fade(t.shadow, t.shadow.a * 0.8));
        glow(r.offset(vec2(0.0, 4.0)), t.radius, 18.0, t.shadow);
    }
    fill_rounded(r, t.radius, t.surface);
    stroke_rounded(r, t.radius, t.focus_width, border);
}

/// A plain box for content: the log, a text field, a cover.
pub fn well(r: Rect) {
    let t = theme::current();
    fill_rounded(r, t.radius, t.surface);
    stroke_rounded(r, t.radius, 1.0, t.border);
}

/// A progress bar.
pub fn bar(r: Rect, fraction: f32, color: Color) {
    let radius = theme::current().radius.min(r.h / 2.0);
    fill_rounded(r, radius, theme::border());
    let filled = (r.w * fraction.clamp(0.0, 1.0)).round();
    if filled > 0.0 {
        fill_rounded(Rect::new(r.x, r.y, filled, r.h), radius, color);
    }
}

/// How long a toast stays on screen, in seconds.
const TOAST_SECS: f64 = 4.0;

/// A short message over the bottom of a list, for a few seconds.
#[derive(Default)]
pub struct Toast(Option<(String, Color, f64)>);

impl Toast {
    pub fn show(&mut self, message: &str, color: Color) {
        self.0 = Some((message.to_string(), color, get_time() + TOAST_SECS));
    }

    pub fn draw(&self, ui: &Ui, list: Rect) {
        let Some((message, color, until)) = &self.0 else {
            return;
        };
        if get_time() > *until {
            return;
        }
        let t = theme::current();
        let r = Rect::new(list.x, list.bottom() - 26.0, list.w, 24.0);
        if t.shadow.a > 0.0 {
            glow(r.offset(vec2(0.0, 2.0)), t.radius, 10.0, t.shadow);
        }
        fill_rounded(r, t.radius, t.surface_hi);
        stroke_rounded(r, t.radius, 1.0, *color);
        ui.text(
            &ui.truncate(message, r.w - 16.0),
            r.x + 8.0,
            r.y + 18.0,
            *color,
        );
    }
}

pub fn chip_width(ui: &Ui, label: &str) -> f32 {
    ui.measure_small(label).ceil() + 10.0
}

/// A small tinted label with its text on `baseline`; returns its width.
pub fn chip(ui: &Ui, label: &str, x: f32, baseline: f32, color: Color) -> f32 {
    let w = chip_width(ui, label);
    let radius = theme::current().radius.min(4.0);
    fill_rounded(
        Rect::new(x, baseline - 13.0, w, 17.0),
        radius,
        fade(color, 0.16),
    );
    ui.small_text(label, x + 5.0, baseline, color);
    w
}

/// Width of a button badge.
pub fn badge_width(ui: &Ui, key: &str) -> f32 {
    (ui.measure_small(key).ceil() + 8.0).max(BADGE_H)
}

/// A button badge (`A`, `ENTER`...) from `top`: round for a single letter,
/// like a pad's face buttons.
pub fn badge(ui: &Ui, key: &str, x: f32, top: f32, color: Color) {
    let t = theme::current();
    let w = badge_width(ui, key);
    let radius = match (t.radius > 0.0, key.chars().count()) {
        (false, _) => 0.0,
        (true, 1) => BADGE_H / 2.0,
        (true, _) => t.radius.min(BADGE_H / 2.0),
    };
    fill_rounded(Rect::new(x, top, w, BADGE_H), radius, color);
    let text_x = x + (w - ui.measure_small(key)) / 2.0;
    ui.small_text(key, text_x, top + 13.0, t.bg_bottom);
}
