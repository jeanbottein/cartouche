//! Cartouche retro UI — M0 walking skeleton.
//!
//! Proves out the foundation: a window that renders to a 480p virtual canvas,
//! scales crisply (wide + square aspect), draws on a **pure black** background,
//! and is navigated with gamepad / keyboard / mouse via the unified input layer.
//! A demo grid + cursor stands in for the real Library screen (M1).

mod engine;

use engine::canvas::{AspectMode, Canvas};
use engine::input::{Action, Input};
use engine::text;
use engine::theme;
use macroquad::prelude::*;

fn window_conf() -> Conf {
    let mut conf = Conf {
        window_title: "Cartouche".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: true,
        ..Default::default()
    };
    // Pace frames with vsync (typically 60fps; up to the panel's refresh rate).
    // A manual sleep-based limiter fights vsync and causes 40-50fps jitter, so we
    // let the GPU pace us. A configurable fps cap lands with the Settings screen.
    conf.platform.swap_interval = Some(1);
    conf
}

#[macroquad::main(window_conf)]
async fn main() {
    let mut canvas = Canvas::new(AspectMode::Wide);
    let mut input = Input::new();
    let font = text::load_pixel_font();

    // Demo state: a cursor over a reflowing grid, plus a "selected" cell.
    let mut cursor: i32 = 0;
    let mut selected: Option<i32> = None;
    let mut fullscreen = false;
    let mut last_mouse = mouse_position();

    loop {
        input.update();

        let (cols, rows) = grid_dims(&canvas);
        let count = cols * rows;

        // --- Display controls ---
        // F: toggle fullscreen (borderless on Linux/miniquad).
        if is_key_pressed(KeyCode::F) {
            fullscreen = !fullscreen;
            macroquad::window::set_fullscreen(fullscreen);
        }
        // I: toggle integer (crisp) vs fit scaling.
        if is_key_pressed(KeyCode::I) {
            canvas.toggle_integer_scale();
        }

        // --- Update ---
        if input.action(Action::ToggleAspect) || input.action(Action::Menu) {
            canvas.toggle_aspect();
            cursor = cursor.min(count - 1);
        }
        if input.action(Action::Right) {
            cursor = (cursor + 1).min(count - 1);
        }
        if input.action(Action::Left) {
            cursor = (cursor - 1).max(0);
        }
        if input.action(Action::Down) {
            cursor = (cursor + cols).min(count - 1);
        }
        if input.action(Action::Up) {
            cursor = (cursor - cols).max(0);
        }
        if input.action(Action::Confirm) {
            selected = Some(cursor);
        }
        if input.action(Action::Back) {
            selected = None;
        }

        // Mouse: hover moves the cursor only when the pointer actually moves, so it
        // doesn't fight keyboard/gamepad navigation; click always selects.
        let mouse = mouse_position();
        let mouse_moved = (mouse.0 - last_mouse.0).abs() > 0.5 || (mouse.1 - last_mouse.1).abs() > 0.5;
        last_mouse = mouse;
        if let Some(hover) = hovered_cell(&canvas, cols, rows) {
            if mouse_moved {
                cursor = hover;
            }
            if is_mouse_button_pressed(MouseButton::Left) {
                cursor = hover;
                selected = Some(hover);
            }
        }

        // --- Draw into the virtual canvas ---
        canvas.begin();
        draw_demo(&canvas, &font, cols, rows, cursor, selected);
        canvas.end();

        // --- Blit to window ---
        canvas.present();

        next_frame().await;
    }
}

/// Demo grid sizing — fixed-ish cell size so Square mode yields more rows,
/// demonstrating layout reflow (ENGINE-2).
fn grid_dims(canvas: &Canvas) -> (i32, i32) {
    let (w, h) = canvas.size();
    let cols = ((w - MARGIN * 2.0 + GAP) / (CELL + GAP)).floor() as i32;
    let rows = ((h - GRID_TOP - MARGIN + GAP) / (CELL + GAP)).floor() as i32;
    (cols.max(1), rows.max(1))
}

const MARGIN: f32 = 40.0;
const GRID_TOP: f32 = 96.0;
const GAP: f32 = 14.0;
const CELL: f32 = 110.0;

fn cell_rect(col: i32, row: i32) -> (f32, f32) {
    (
        MARGIN + col as f32 * (CELL + GAP),
        GRID_TOP + row as f32 * (CELL + GAP),
    )
}

fn hovered_cell(canvas: &Canvas, cols: i32, rows: i32) -> Option<i32> {
    let (mx, my) = mouse_position();
    let p = canvas.screen_to_canvas(vec2(mx, my))?;
    for row in 0..rows {
        for col in 0..cols {
            let (x, y) = cell_rect(col, row);
            if p.x >= x && p.x <= x + CELL && p.y >= y && p.y <= y + CELL {
                return Some(row * cols + col);
            }
        }
    }
    None
}

/// Join `segments` with `sep`, wrapping to new lines so each stays within `max_w`.
fn wrap_segments(segments: &[&str], sep: &str, max_w: f32, font_size: u16, font: &Font) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for seg in segments {
        let trial = if cur.is_empty() {
            (*seg).to_string()
        } else {
            format!("{cur}{sep}{seg}")
        };
        if text::measure(&trial, font_size, font) > max_w && !cur.is_empty() {
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

fn draw_demo(canvas: &Canvas, font: &Font, cols: i32, rows: i32, cursor: i32, selected: Option<i32>) {
    let (w, h) = canvas.size();

    // Title + subtitle.
    text::text("CARTOUCHE", MARGIN, 56.0, 56, theme::AMBER, font);
    text::text("M0 walking skeleton", MARGIN, 84.0, 24, theme::MUTED, font);

    // Grid cells.
    for row in 0..rows {
        for col in 0..cols {
            let index = row * cols + col;
            let (x, y) = cell_rect(col, row);

            draw_rectangle(x, y, CELL, CELL, theme::PANEL);

            let (border_col, thickness) = if Some(index) == selected {
                (theme::AMBER, 4.0)
            } else if index == cursor {
                (theme::ACCENT, 4.0)
            } else {
                (theme::BORDER, 2.0)
            };
            draw_rectangle_lines(x, y, CELL, CELL, thickness, border_col);

            let label = format!("{}", index + 1);
            text::text(&label, x + 12.0, y + 40.0, 36, theme::TEXT, font);
            if Some(index) == selected {
                text::text("OK", x + 12.0, y + CELL - 12.0, 28, theme::AMBER, font);
            }
        }
    }

    // Footer: help (word-wrapped to fit the canvas width) + status, stacked up
    // from the bottom so it never crops — important in narrow Square mode.
    let help_segments = [
        "Move: D-Pad/Stick/Arrows",
        "Confirm: A/Enter",
        "Back: B/Esc",
        "Aspect: Start/Tab",
        "Fullscreen: F",
        "Scale: I",
    ];
    let help_fs: u16 = 22;
    let line_h = help_fs as f32 + 6.0;
    let max_w = w - MARGIN * 2.0;
    let help_lines = wrap_segments(&help_segments, "   ", max_w, help_fs, font);

    let mut y = h - 12.0;
    for line in help_lines.iter().rev() {
        text::text(line, MARGIN, y, help_fs, theme::MUTED, font);
        y -= line_h;
    }
    text::text(
        &format!(
            "{}  |  scale: {}  |  {} fps",
            canvas.mode.label(),
            canvas.scale_label(),
            get_fps()
        ),
        MARGIN,
        y - 4.0,
        24,
        theme::BLUE,
        font,
    );

    // Thin frame around the whole canvas so the letterbox edge is visible.
    draw_rectangle_lines(1.0, 1.0, w - 2.0, h - 2.0, 2.0, theme::BORDER);
}
