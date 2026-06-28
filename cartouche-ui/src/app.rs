//! Application shell: owns shared state (canvas, input, font, config, library),
//! the top tab bar, global display toggles, and per-screen update/draw dispatch.
//!
//! Library layout: a vertical list of game "boxes" (title on each), scrolling
//! top-to-bottom with smooth pixel/mouse-wheel scrolling. In widescreen a large
//! cover preview sits beside the list; in square mode it sits above it. Covers are
//! loaded on a background thread and drawn at full window resolution over the pixel
//! canvas, so photos stay crisp while the UI/font keep their retro look.

use macroquad::prelude::*;

use crate::bridge::config::Config;
use crate::bridge::library::{self, CoverLoader, GameEntry};
use crate::engine::canvas::{AspectMode, Canvas};
use crate::engine::input::{Action, Input};
use crate::engine::{text, theme};

const MARGIN: f32 = 24.0;
const HEADER_H: f32 = 52.0;
const CONTENT_TOP: f32 = HEADER_H + 2.0;

const ROW_H: f32 = 38.0;
const ROW_GAP: f32 = 6.0;
const ROW_STEP: f32 = ROW_H + ROW_GAP;

const STATUS_ACTIONS: [&str; 5] = ["Run All", "Parse", "Backup", "Steam Sync", "Cancel"];

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Library = 0,
    Status = 1,
    Settings = 2,
}

impl Tab {
    fn next(self) -> Tab {
        match self {
            Tab::Library => Tab::Status,
            Tab::Status => Tab::Settings,
            Tab::Settings => Tab::Library,
        }
    }
    fn prev(self) -> Tab {
        match self {
            Tab::Library => Tab::Settings,
            Tab::Status => Tab::Library,
            Tab::Settings => Tab::Status,
        }
    }
}

pub struct App {
    canvas: Canvas,
    input: Input,
    font: Font,
    tab: Tab,
    fullscreen: bool,

    config: Option<Config>,
    games: Vec<GameEntry>,
    loader: Option<CoverLoader>,

    lib_cursor: i32,
    scroll_y: f32,
    scroll_target: f32,
    status_cursor: i32,
    status_msg: String,
}

impl App {
    pub fn new() -> App {
        let config = Config::load();

        let mut games = Vec::new();
        let mut loader = None;
        if let Some(cfg) = &config {
            if let Some(dir) = cfg.games_dir() {
                games = library::scan_games(&dir);
                let cache = cfg.cartouche_dir.join("ui-thumbs");
                loader = Some(CoverLoader::spawn(&games, cache));
            }
        }

        App {
            canvas: Canvas::new(AspectMode::Wide),
            input: Input::new(),
            font: text::load_pixel_font(),
            tab: Tab::Library,
            fullscreen: false,
            config,
            games,
            loader,
            lib_cursor: 0,
            scroll_y: 0.0,
            scroll_target: 0.0,
            status_cursor: 0,
            status_msg: String::new(),
        }
    }

    pub fn frame(&mut self) {
        // Pull in any covers decoded on the background thread.
        if let Some(loader) = &mut self.loader {
            loader.poll(&mut self.games);
            if !loader.loading() {
                self.loader = None;
            }
        }

        self.input.update();
        self.handle_global();

        match self.tab {
            Tab::Library => self.update_library(),
            Tab::Status => self.update_status(),
            Tab::Settings => {}
        }

        // Pixel canvas. Screen content first; the header/footer are drawn last with
        // an opaque background so they mask any list rows scrolled past the edges.
        self.canvas.begin();
        match self.tab {
            Tab::Library => self.draw_library(),
            Tab::Status => self.draw_status(),
            Tab::Settings => self.draw_settings(),
        }
        self.draw_header();
        self.canvas.end();
        self.canvas.present();

        // Full-resolution cover preview drawn over the upscaled canvas.
        if self.tab == Tab::Library {
            self.overlay_cover();
        }
    }

    // --- Global controls -----------------------------------------------------

    fn handle_global(&mut self) {
        if is_key_pressed(KeyCode::F) {
            self.fullscreen = !self.fullscreen;
            macroquad::window::set_fullscreen(self.fullscreen);
        }
        if is_key_pressed(KeyCode::I) {
            self.canvas.toggle_integer_scale();
        }
        if self.input.action(Action::ToggleAspect) {
            self.canvas.toggle_aspect();
        }

        if is_key_pressed(KeyCode::Key1) {
            self.tab = Tab::Library;
        }
        if is_key_pressed(KeyCode::Key2) {
            self.tab = Tab::Status;
        }
        if is_key_pressed(KeyCode::Key3) {
            self.tab = Tab::Settings;
        }
        if self.input.action(Action::TabNext) {
            self.tab = self.tab.next();
        }
        if self.input.action(Action::TabPrev) {
            self.tab = self.tab.prev();
        }
    }

    // --- Library -------------------------------------------------------------

    /// (list rect, optional cover-preview rect). The preview only exists in
    /// widescreen; in square mode the list uses the full height.
    fn library_layout(&self) -> (Rect, Option<Rect>) {
        let (w, _) = self.canvas.size();
        let top = CONTENT_TOP;
        let h = self.content_bottom() - top;
        if self.canvas.mode == AspectMode::Wide {
            let list_w = ((w - 2.0 * MARGIN) * 0.5).floor();
            let list = Rect::new(MARGIN, top, list_w, h);
            let px = MARGIN + list_w + 16.0;
            let preview = Rect::new(px, top, w - px - MARGIN, h);
            (list, Some(preview))
        } else {
            (Rect::new(MARGIN, top, w - 2.0 * MARGIN, h), None)
        }
    }

    fn max_scroll(&self, list: Rect) -> f32 {
        let total = self.games.len() as f32 * ROW_STEP - ROW_GAP;
        (total - list.h).max(0.0)
    }

    fn update_library(&mut self) {
        let n = self.games.len() as i32;
        if n == 0 {
            return;
        }
        let (list, _) = self.library_layout();

        let mut nav = false;
        if self.input.action(Action::Down) {
            self.lib_cursor = (self.lib_cursor + 1).min(n - 1);
            nav = true;
        }
        if self.input.action(Action::Up) {
            self.lib_cursor = (self.lib_cursor - 1).max(0);
            nav = true;
        }

        // Mouse wheel: free, fluid scrolling (independent of the cursor).
        let wheel = mouse_wheel().1;
        if wheel != 0.0 {
            self.scroll_target -= wheel * 50.0;
        }

        // Mouse click selects a row.
        if is_mouse_button_pressed(MouseButton::Left) {
            let (mx, my) = mouse_position();
            if let Some(p) = self.canvas.screen_to_canvas(vec2(mx, my)) {
                if list.contains(p) {
                    let idx = ((p.y - list.y + self.scroll_y) / ROW_STEP).floor() as i32;
                    if idx >= 0 && idx < n {
                        self.lib_cursor = idx;
                    }
                }
            }
        }

        // Keep the cursor in view when navigating with keys/pad.
        if nav {
            let cursor_top = self.lib_cursor as f32 * ROW_STEP;
            let cursor_bottom = cursor_top + ROW_H;
            if cursor_top < self.scroll_target {
                self.scroll_target = cursor_top;
            }
            if cursor_bottom > self.scroll_target + list.h {
                self.scroll_target = cursor_bottom - list.h;
            }
        }

        self.scroll_target = self.scroll_target.clamp(0.0, self.max_scroll(list));

        // Smoothly approach the target for fluid scrolling.
        let t = (get_frame_time() * 14.0).min(1.0);
        self.scroll_y += (self.scroll_target - self.scroll_y) * t;
    }

    fn draw_library(&self) {
        if self.games.is_empty() {
            text::text(
                "No games found.",
                MARGIN,
                CONTENT_TOP + 40.0,
                28,
                theme::TEXT,
                &self.font,
            );
            return;
        }

        let (list, preview) = self.library_layout();
        self.draw_game_list(list);
        if let Some(preview) = preview {
            self.draw_preview_panel(preview);
        }
        self.draw_loading_corner(list);
    }

    fn draw_game_list(&self, list: Rect) {
        let n = self.games.len() as i32;
        let first = ((self.scroll_y / ROW_STEP).floor() as i32).max(0);
        let last = (((self.scroll_y + list.h) / ROW_STEP).ceil() as i32).min(n);

        for i in first..last {
            // Snap to whole canvas pixels so 1px borders stay crisp and don't
            // shimmer or vanish at sub-pixel scroll offsets.
            let y = (list.y - self.scroll_y + i as f32 * ROW_STEP).round();
            let focused = i == self.lib_cursor;

            if focused {
                draw_rectangle(list.x, y, list.w, ROW_H, theme::PANEL);
            }
            // Cartridge-style accent stripe on the leading edge.
            let stripe = if focused { theme::ACCENT } else { theme::AMBER };
            draw_rectangle(list.x, y, 6.0, ROW_H, stripe);
            let (border, th) = if focused {
                (theme::ACCENT, 2.0)
            } else {
                (theme::BORDER, 1.0)
            };
            stroke_rect(list.x, y, list.w, ROW_H, th, border);

            let color = if focused { theme::TEXT } else { theme::MUTED };
            let title = self.truncate(&self.games[i as usize].title, list.w - 22.0, 24);
            text::text(&title, list.x + 16.0, y + 26.0, 24, color, &self.font);
        }

        // Mask any partial rows that spilled below the list edge.
        let (_, h) = self.canvas.size();
        let bottom = list.y + list.h;
        draw_rectangle(0.0, bottom, self.canvas.size().0, h - bottom, theme::BG);
    }

    fn draw_preview_panel(&self, preview: Rect) {
        // Just the cover (drawn full-res in overlay_cover); show a hint while it loads.
        let game = &self.games[self.lib_cursor as usize];
        if game.cover.is_none() && game.cover_file.is_some() {
            let area = preview_cover_area(preview);
            text::text(
                "loading...",
                area.x + area.w / 2.0 - 40.0,
                area.y + area.h / 2.0,
                22,
                theme::MUTED,
                &self.font,
            );
        }
    }

    fn draw_loading_corner(&self, list: Rect) {
        let Some(loader) = &self.loader else {
            return;
        };
        let (done, total) = loader.progress();
        let cx = list.right() - 14.0;
        let cy = list.bottom() - 14.0;
        draw_spinner(cx, cy, 8.0, theme::ACCENT);
        let label = format!("{done}/{total}");
        let tw = text::measure(&label, 18, &self.font);
        text::text(&label, cx - 16.0 - tw, cy + 6.0, 18, theme::MUTED, &self.font);
    }

    /// Draw the focused game's cover at full window resolution inside the preview.
    fn overlay_cover(&self) {
        if self.games.is_empty() {
            return;
        }
        let Some(tex) = &self.games[self.lib_cursor as usize].cover else {
            return;
        };
        let (_, preview) = self.library_layout();
        let Some(preview) = preview else {
            return; // no preview in square mode
        };
        let area = preview_cover_area(preview);
        let (scale, offset) = self.canvas.present_transform();

        let ar = tex.width() / tex.height();
        let (fw, fh) = if ar > area.w / area.h {
            (area.w, area.w / ar)
        } else {
            (area.h * ar, area.h)
        };
        let fx = area.x + (area.w - fw) / 2.0;
        let fy = area.y + (area.h - fh) / 2.0;

        draw_texture_ex(
            tex,
            offset.x + fx * scale,
            offset.y + fy * scale,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(fw * scale, fh * scale)),
                ..Default::default()
            },
        );
    }

    fn truncate(&self, s: &str, max_w: f32, size: u16) -> String {
        if text::measure(s, size, &self.font) <= max_w {
            return s.to_string();
        }
        let mut t = s.to_string();
        while !t.is_empty() && text::measure(&format!("{t}\u{2026}"), size, &self.font) > max_w {
            t.pop();
        }
        format!("{}\u{2026}", t.trim_end())
    }

    // --- Status --------------------------------------------------------------

    fn update_status(&mut self) {
        let n = STATUS_ACTIONS.len() as i32;
        if self.input.action(Action::Down) {
            self.status_cursor = (self.status_cursor + 1).min(n - 1);
        }
        if self.input.action(Action::Up) {
            self.status_cursor = (self.status_cursor - 1).max(0);
        }
        if self.input.action(Action::Confirm) {
            let action = STATUS_ACTIONS[self.status_cursor as usize];
            self.status_msg = format!("'{action}' — pipeline run is wired in the next M1 step");
        }
    }

    fn draw_status(&self) {
        let top = HEADER_H + 24.0;
        text::text("STATUS", MARGIN, top, 40, theme::AMBER, &self.font);

        let info = format!("{} game(s) discovered", self.games.len());
        text::text(&info, MARGIN, top + 36.0, 24, theme::MUTED, &self.font);

        let mut y = top + 80.0;
        for (i, action) in STATUS_ACTIONS.iter().enumerate() {
            let focused = i as i32 == self.status_cursor;
            let color = if focused { theme::ACCENT } else { theme::TEXT };
            let prefix = if focused { "> " } else { "  " };
            text::text(&format!("{prefix}{action}"), MARGIN, y, 28, color, &self.font);
            y += 34.0;
        }

        if !self.status_msg.is_empty() {
            text::text(&self.status_msg, MARGIN, y + 12.0, 22, theme::BLUE, &self.font);
        }
    }

    // --- Settings ------------------------------------------------------------

    fn draw_settings(&self) {
        let top = HEADER_H + 24.0;
        text::text("SETTINGS", MARGIN, top, 40, theme::AMBER, &self.font);

        let cfg_line = match &self.config {
            Some(c) => format!("config: {}", c.config_path().display()),
            None => "config: not found".to_string(),
        };
        let games_line = match self.config.as_ref().and_then(|c| c.games_dir()) {
            Some(d) => format!("games:  {}", d.display()),
            None => "games:  (unset)".to_string(),
        };
        text::text(&cfg_line, MARGIN, top + 40.0, 22, theme::MUTED, &self.font);
        text::text(&games_line, MARGIN, top + 68.0, 22, theme::MUTED, &self.font);
        text::text(
            "Full settings editor lands in M3.",
            MARGIN,
            top + 110.0,
            24,
            theme::TEXT,
            &self.font,
        );
    }

    // --- Chrome (header / footer) -------------------------------------------

    fn draw_header(&self) {
        let (w, _) = self.canvas.size();
        draw_rectangle(0.0, 0.0, w, CONTENT_TOP, theme::BG);

        let tabs = ["LIBRARY", "STATUS", "SETTINGS"];
        let mut x = MARGIN;
        for (i, name) in tabs.iter().enumerate() {
            let active = i == self.tab as usize;
            let color = if active { theme::ACCENT } else { theme::MUTED };
            text::text(name, x, 36.0, 30, color, &self.font);
            let wd = text::measure(name, 30, &self.font);
            if active {
                draw_line(x, 44.0, x + wd, 44.0, 2.0, theme::ACCENT);
            }
            x += wd + 28.0;
        }
        draw_line(0.0, HEADER_H, w, HEADER_H, 1.0, theme::BORDER);
    }

    fn content_bottom(&self) -> f32 {
        let (_, h) = self.canvas.size();
        h - 8.0
    }
}

/// Draw a crisp `th`-pixel border as four filled rects (exact pixels, no edge
/// centering or anti-aliasing — unlike `draw_rectangle_lines`).
fn stroke_rect(x: f32, y: f32, w: f32, h: f32, th: f32, color: Color) {
    draw_rectangle(x, y, w, th, color); // top
    draw_rectangle(x, y + h - th, w, th, color); // bottom
    draw_rectangle(x, y, th, h, color); // left
    draw_rectangle(x + w - th, y, th, h, color); // right
}

fn preview_cover_area(preview: Rect) -> Rect {
    let pad = 6.0;
    Rect::new(
        preview.x + pad,
        preview.y + pad,
        preview.w - 2.0 * pad,
        preview.h - 2.0 * pad,
    )
}

/// A classic fading-dot spinner centered at `(cx, cy)`.
fn draw_spinner(cx: f32, cy: f32, r: f32, color: Color) {
    const DOTS: usize = 8;
    let head = (get_time() * 8.0) as f32 % DOTS as f32;
    for i in 0..DOTS {
        let ang = i as f32 / DOTS as f32 * std::f32::consts::TAU;
        let px = cx + ang.cos() * r;
        let py = cy + ang.sin() * r;
        let dist = (head - i as f32).rem_euclid(DOTS as f32);
        let alpha = 1.0 - dist / DOTS as f32;
        draw_circle(px, py, r * 0.22, Color::new(color.r, color.g, color.b, alpha));
    }
}
