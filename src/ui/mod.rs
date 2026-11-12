//! The retro UI (macroquad): Library, game editor, Status (pipeline runs) and
//! Settings, all playable with a gamepad, keyboard or mouse.

mod app;
mod browser;
mod covers;
mod dialog;
mod editor;
mod engine;
mod form;
mod info;
mod jobs;
mod keyboard;
mod launcher;
mod library;
mod list;
mod mods;
mod music;
mod network;
mod settings;
mod setup;
mod status;

use std::path::{Path, PathBuf};

use macroquad::prelude::*;

use engine::canvas::{Canvas, Layout};
use engine::input::{Action, Input};
use engine::{text, theme};

/// Share of a tall screen's content height given to the panel below.
const TALL_PANEL_SHARE: f32 = 0.55;

fn window_conf() -> Conf {
    let mut conf = Conf {
        window_title: "•cartridge".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: true,
        // Start fullscreen at the display's native resolution: the canvas picks a
        // whole-number scale for it, so gamescope/SteamOS never has to rescale.
        fullscreen: true,
        ..Default::default()
    };
    // Pace frames with vsync; a sleep-based limiter fights it and causes jitter.
    conf.platform.swap_interval = Some(1);
    conf
}

/// Open the window and run the UI until the user quits.
pub fn run(cli_dir: Option<&Path>) {
    let cli_dir: Option<PathBuf> = cli_dir.map(Path::to_path_buf);
    macroquad::Window::from_config(window_conf(), async move {
        // Closing the window asks the app first (e.g. "restart Steam?").
        prevent_quit();
        let mut app = app::App::new(cli_dir);
        while !app.should_quit() {
            app.frame();
            next_frame().await;
        }
    });
}

/// Shared per-frame state every screen draws and reads input with.
pub struct Ui {
    pub canvas: Canvas,
    pub input: Input,
    pub font: Font,
}

/// One footer entry: a button badge and what it does.
pub struct Hint {
    pub key: &'static str,
    pub label: String,
    pub color: Color,
    /// Fill (0..1) for hold-to-activate hints.
    pub progress: f32,
    /// What a click on this hint does (mouse users); `None` for hold gestures.
    pub action: Option<Action>,
}

impl Hint {
    pub fn new(key: &'static str, label: impl Into<String>, color: Color) -> Hint {
        Hint {
            key,
            label: label.into(),
            color,
            progress: 0.0,
            action: None,
        }
    }
}

impl Ui {
    pub fn text(&self, s: &str, x: f32, y: f32, color: Color) {
        text::text(s, x, y, text::NORMAL, color, &self.font);
    }

    pub fn heading(&self, s: &str, x: f32, y: f32, color: Color) {
        text::text(s, x, y, text::LARGE, color, &self.font);
    }

    pub fn measure(&self, s: &str) -> f32 {
        text::measure(s, text::NORMAL, &self.font)
    }

    pub fn measure_heading(&self, s: &str) -> f32 {
        text::measure(s, text::LARGE, &self.font)
    }

    /// `s` cut with an ellipsis to fit `max_w`.
    pub fn truncate(&self, s: &str, max_w: f32) -> String {
        self.truncate_sized(s, max_w, text::NORMAL)
    }

    pub fn truncate_heading(&self, s: &str, max_w: f32) -> String {
        self.truncate_sized(s, max_w, text::LARGE)
    }

    fn truncate_sized(&self, s: &str, max_w: f32, size: u16) -> String {
        if text::measure(s, size, &self.font) <= max_w {
            return s.to_string();
        }
        let mut chars: Vec<char> = s.chars().collect();
        while !chars.is_empty()
            && text::measure(
                &format!("{}\u{2026}", chars.iter().collect::<String>()),
                size,
                &self.font,
            ) > max_w
        {
            chars.pop();
        }
        format!("{}\u{2026}", chars.iter().collect::<String>().trim_end())
    }

    /// The end of `s` with a leading ellipsis, to fit `max_w` (for paths).
    pub fn truncate_start(&self, s: &str, max_w: f32) -> String {
        if self.measure(s) <= max_w {
            return s.to_string();
        }
        let mut chars: Vec<char> = s.chars().collect();
        while !chars.is_empty()
            && self.measure(&format!("\u{2026}{}", chars.iter().collect::<String>())) > max_w
        {
            chars.remove(0);
        }
        format!("\u{2026}{}", chars.iter().collect::<String>())
    }

    /// Wrap `s` on spaces into lines no wider than `max_w`.
    pub fn wrap(&self, s: &str, max_w: f32) -> Vec<String> {
        let words: Vec<&str> = s.split(' ').collect();
        text::wrap_segments(&words, " ", max_w, text::NORMAL, &self.font)
    }

    pub fn action(&self, a: Action) -> bool {
        self.input.action(a)
    }

    /// Canvas-space position of a left click this frame.
    pub fn click(&self) -> Option<Vec2> {
        if !is_mouse_button_pressed(MouseButton::Left) {
            return None;
        }
        let (x, y) = mouse_position();
        self.canvas.screen_to_canvas(vec2(x, y))
    }

    /// Button label for `a` on the device used last.
    pub fn key(&self, a: Action) -> &'static str {
        let pad = self.input.gamepad_last();
        match a {
            Action::Confirm => {
                if pad {
                    "A"
                } else {
                    "ENTER"
                }
            }
            Action::Back => {
                if pad {
                    "B"
                } else {
                    "ESC"
                }
            }
            Action::Edit => "X",
            Action::Alt => {
                if pad {
                    "Y"
                } else {
                    "H"
                }
            }
            Action::Menu => "START",
            Action::TabPrev | Action::TabNext => {
                if pad {
                    "LB RB"
                } else {
                    "Q E"
                }
            }
            Action::Left | Action::Right => "< >",
            _ => "",
        }
    }

    pub fn hint(&self, a: Action, label: impl Into<String>, color: Color) -> Hint {
        Hint {
            action: Some(a),
            ..Hint::new(self.key(a), label, color)
        }
    }

    /// `area` split into the main content and its panel (cover, info, art):
    /// beside it in the wide layout, the content taking `main_share` of the
    /// width; below it on tall 4:3 screens; no panel on plain 4:3.
    pub fn split(&self, area: Rect, main_share: f32, gap: f32) -> (Rect, Option<Rect>) {
        match self.canvas.layout() {
            Layout::Wide => split_beside(area, main_share, gap),
            Layout::Tall => split_below(area, 1.0 - TALL_PANEL_SHARE, gap),
            Layout::Narrow => (area, None),
        }
    }

    /// Draw a texture letterboxed into a canvas-space rect at full window
    /// resolution (call after the canvas has been presented).
    pub fn draw_image_fit(&self, tex: &Texture2D, area: Rect) {
        let (scale, offset) = self.canvas.present_transform();
        let ar = tex.width() / tex.height();
        let (w, h) = if ar > area.w / area.h {
            (area.w, area.w / ar)
        } else {
            (area.h * ar, area.h)
        };
        let x = area.x + (area.w - w) / 2.0;
        let y = area.y + (area.h - h) / 2.0;
        draw_texture_ex(
            tex,
            offset.x + x * scale,
            offset.y + y * scale,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(w * scale, h * scale)),
                ..Default::default()
            },
        );
    }
}

fn split_beside(area: Rect, main_share: f32, gap: f32) -> (Rect, Option<Rect>) {
    let main_w = (area.w * main_share).floor();
    let panel_x = area.x + main_w + gap;
    (
        Rect::new(area.x, area.y, main_w, area.h),
        Some(Rect::new(panel_x, area.y, area.right() - panel_x, area.h)),
    )
}

fn split_below(area: Rect, main_share: f32, gap: f32) -> (Rect, Option<Rect>) {
    let main_h = (area.h * main_share).floor();
    let panel_y = area.y + main_h + gap;
    (
        Rect::new(area.x, area.y, area.w, main_h),
        Some(Rect::new(area.x, panel_y, area.w, area.bottom() - panel_y)),
    )
}

/// A crisp `th`-pixel border as four filled rects (no edge centering or
/// anti-aliasing, unlike `draw_rectangle_lines`).
pub fn stroke_rect(r: Rect, th: f32, color: Color) {
    draw_rectangle(r.x, r.y, r.w, th, color);
    draw_rectangle(r.x, r.y + r.h - th, r.w, th, color);
    draw_rectangle(r.x, r.y, th, r.h, color);
    draw_rectangle(r.x + r.w - th, r.y, th, r.h, color);
}

/// An opaque framed panel (modals, cards).
pub fn panel(r: Rect, border: Color) {
    draw_rectangle(r.x, r.y, r.w, r.h, theme::BG);
    stroke_rect(r, 2.0, border);
}

/// A classic fading-dot spinner centered at `(cx, cy)`.
pub fn draw_spinner(cx: f32, cy: f32, r: f32, color: Color) {
    const DOTS: usize = 8;
    let head = (get_time() * 8.0) as f32 % DOTS as f32;
    for i in 0..DOTS {
        let angle = i as f32 / DOTS as f32 * std::f32::consts::TAU;
        let fade = 1.0 - (head - i as f32).rem_euclid(DOTS as f32) / DOTS as f32;
        draw_circle(
            cx + angle.cos() * r,
            cy + angle.sin() * r,
            r * 0.22,
            Color::new(color.r, color.g, color.b, fade),
        );
    }
}

/// A rect centered in `outer`, at most `w`×`h`.
pub fn centered(outer: Rect, w: f32, h: f32) -> Rect {
    let (w, h) = (w.min(outer.w), h.min(outer.h));
    Rect::new(
        (outer.x + (outer.w - w) / 2.0).round(),
        (outer.y + (outer.h - h) / 2.0).round(),
        w,
        h,
    )
}

/// Open a file, folder or URL with the desktop's default handler.
pub fn open_external(target: &str) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            std::thread::spawn(move || child.wait()); // reap, never a zombie
        })
        .map_err(|e| format!("Could not open {target}: {e}"))
}
