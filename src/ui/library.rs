//! Library: a vertical list of game "boxes" with smooth scrolling. In
//! widescreen a panel beside the list shows the cover and the game's info
//! (year, players, platforms, install state, size, description). Covers load in the
//! background and are drawn at full window resolution over the pixel canvas,
//! so photos stay crisp while the UI keeps its retro look.

use macroquad::prelude::*;

use super::covers::{self, ImageLoader};
use super::engine::canvas::AspectMode;
use super::engine::input::Action;
use super::engine::theme;
use super::launcher::{self, Session};
use super::{draw_spinner, stroke_rect, Hint, Ui};
use crate::core::config::Config;
use crate::core::models::{Game, Install};
use crate::core::scanner::{self, Compatibility};
use crate::core::sizer;

const ROW_H: f32 = 26.0;
const ROW_GAP: f32 = 4.0;
const ROW_STEP: f32 = ROW_H + ROW_GAP;
const PREVIEW_GAP: f32 = 12.0;
const LINE: f32 = 20.0;
/// Share of the side panel's height given to the cover.
const COVER_SHARE: f32 = 0.5;
/// How long a message stays on screen, in seconds.
const TOAST_SECS: f64 = 4.0;

pub enum Event {
    Launched(Session),
    Edit(usize),
}

#[derive(Default)]
pub struct Library {
    pub games: Vec<Game>,
    /// Package file size per game (read once per load, not every frame).
    package_sizes: Vec<Option<u64>>,
    covers: Vec<Option<Texture2D>>,
    loader: Option<ImageLoader>,
    cursor: usize,
    scroll_y: f32,
    scroll_target: f32,
    /// Message, color, expiry time.
    toast: Option<(String, Color, f64)>,
}

impl Library {
    /// (Re)scan the games folder, keeping the cursor on the same game.
    pub fn load(&mut self, cfg: &Config) {
        let focused = self.games.get(self.cursor).map(|g| g.folder_name.clone());
        self.games = cfg
            .games_dir()
            .map(|dir| scanner::load_library(&dir))
            .unwrap_or_default();
        self.package_sizes = self
            .games
            .iter()
            .map(|g| Some(std::fs::metadata(g.package()?).ok()?.len()))
            .collect();
        self.covers = vec![None; self.games.len()];
        self.loader = Some(ImageLoader::spawn(covers::cover_jobs(
            &self.games,
            &cfg.app_dir().join("ui-thumbs"),
        )));
        self.cursor = focused
            .and_then(|f| self.games.iter().position(|g| g.folder_name == f))
            .unwrap_or(0)
            .min(self.games.len().saturating_sub(1));
    }

    pub fn toast(&mut self, message: &str, color: Color) {
        self.toast = Some((message.to_string(), color, get_time() + TOAST_SECS));
    }

    /// Upload covers decoded since the last frame.
    pub fn poll(&mut self) {
        let Some(loader) = &mut self.loader else {
            return;
        };
        for (index, tex) in loader.poll() {
            if let Some(slot) = self.covers.get_mut(index) {
                *slot = Some(tex);
            }
        }
        if !loader.loading() {
            self.loader = None;
        }
    }

    /// (list rect, cover preview rect in widescreen).
    fn layout(&self, ui: &Ui, area: Rect) -> (Rect, Option<Rect>) {
        if ui.canvas.mode == AspectMode::Square {
            return (area, None);
        }
        let list_w = (area.w * 0.5).floor();
        let px = area.x + list_w + PREVIEW_GAP;
        (
            Rect::new(area.x, area.y, list_w, area.h),
            Some(Rect::new(px, area.y, area.right() - px, area.h)),
        )
    }

    /// (cover, info) halves of the side panel.
    fn split_panel(panel: Rect) -> (Rect, Rect) {
        let cover_h = (panel.h * COVER_SHARE).floor();
        (
            Rect::new(panel.x, panel.y, panel.w, cover_h),
            Rect::new(
                panel.x,
                panel.y + cover_h + 8.0,
                panel.w,
                panel.h - cover_h - 8.0,
            ),
        )
    }

    fn max_scroll(&self, list: Rect) -> f32 {
        (self.games.len() as f32 * ROW_STEP - ROW_GAP - list.h).max(0.0)
    }

    pub fn update(&mut self, ui: &Ui, area: Rect) -> Option<Event> {
        if self.games.is_empty() {
            return None;
        }
        let n = self.games.len();
        let (list, _) = self.layout(ui, area);
        let mut nav = false;
        if ui.action(Action::Down) {
            self.cursor = (self.cursor + 1).min(n - 1);
            nav = true;
        }
        if ui.action(Action::Up) {
            self.cursor = self.cursor.saturating_sub(1);
            nav = true;
        }
        // Mouse wheel: free, fluid scrolling (independent of the cursor).
        self.scroll_target -= mouse_wheel().1 * 50.0;

        // A click selects a row; clicking the focused row launches it.
        let mut launch = ui.action(Action::Confirm);
        if let Some(p) = ui.click().filter(|p| list.contains(*p)) {
            let index = ((p.y - list.y + self.scroll_y) / ROW_STEP).floor() as usize;
            if index < n {
                launch |= index == self.cursor;
                self.cursor = index;
            }
        }

        if nav {
            let top = self.cursor as f32 * ROW_STEP;
            self.scroll_target = self.scroll_target.min(top).max(top + ROW_H - list.h);
        }
        self.scroll_target = self.scroll_target.clamp(0.0, self.max_scroll(list));
        let t = (get_frame_time() * 14.0).min(1.0);
        self.scroll_y += (self.scroll_target - self.scroll_y) * t;

        let edit = ui.action(Action::Edit);
        if (edit || launch) && !self.games[self.cursor].is_installed() {
            self.toast("Packaged game: unpack it to play or edit.", theme::AMBER);
            return None;
        }
        if edit {
            return Some(Event::Edit(self.cursor));
        }
        if !launch {
            return None;
        }
        match launcher::launch(&self.games[self.cursor]) {
            Ok(session) => Some(Event::Launched(session)),
            Err(message) => {
                self.toast(&message, theme::AMBER);
                None
            }
        }
    }

    pub fn draw(&self, ui: &Ui, area: Rect) {
        if self.games.is_empty() {
            ui.text("No games found.", area.x, area.y + 30.0, theme::TEXT);
            ui.text(
                &ui.truncate(
                    "Set the games folder in SETTINGS, then run the pipeline in STATUS.",
                    area.w,
                ),
                area.x,
                area.y + 54.0,
                theme::MUTED,
            );
            return;
        }
        let (list, preview) = self.layout(ui, area);
        self.draw_rows(ui, list);
        if let Some(panel) = preview {
            let (cover, info) = Self::split_panel(panel);
            let focused = &self.games[self.cursor];
            if self.covers[self.cursor].is_none()
                && focused.images.cover.is_some()
                && self.loader.is_some()
            {
                ui.text(
                    "loading...",
                    cover.center().x - 40.0,
                    cover.center().y,
                    theme::MUTED,
                );
            }
            self.draw_info(ui, info);
        }
        self.draw_loading(ui, list);
        self.draw_toast(ui, list);
    }

    fn draw_rows(&self, ui: &Ui, list: Rect) {
        let n = self.games.len();
        let first = (self.scroll_y / ROW_STEP).floor().max(0.0) as usize;
        let last = (((self.scroll_y + list.h) / ROW_STEP).ceil() as usize).min(n);
        for i in first..last {
            // Whole canvas pixels keep 1px borders crisp at sub-pixel scroll offsets.
            let y = (list.y - self.scroll_y + i as f32 * ROW_STEP).round();
            let focused = i == self.cursor;
            if focused {
                draw_rectangle(list.x, y, list.w, ROW_H, theme::PANEL);
            }
            // Cartridge-style accent stripe on the leading edge.
            draw_rectangle(
                list.x,
                y,
                5.0,
                ROW_H,
                if focused { theme::ACCENT } else { theme::AMBER },
            );
            let (border, th) = if focused {
                (theme::ACCENT, 2.0)
            } else {
                (theme::BORDER, 1.0)
            };
            stroke_rect(Rect::new(list.x, y, list.w, ROW_H), th, border);
            let tag = self.row_tag(i, ui.canvas.mode == AspectMode::Square);
            let tag_w = if tag.is_empty() {
                0.0
            } else {
                ui.measure(&tag) + 12.0
            };
            ui.text(&tag, list.right() - tag_w, y + 18.0, theme::AMBER);
            let title = ui.truncate(&self.games[i].title, list.w - 18.0 - tag_w);
            ui.text(
                &title,
                list.x + 12.0,
                y + 18.0,
                if focused { theme::TEXT } else { theme::MUTED },
            );
        }
        // Mask partial rows that spilled below the list edge.
        let (w, h) = ui.canvas.size();
        draw_rectangle(0.0, list.bottom(), w, h - list.bottom(), theme::BG);
    }

    /// Short install marker on a row; with the size when there is no side panel.
    fn row_tag(&self, index: usize, with_size: bool) -> String {
        let game = &self.games[index];
        let tag = match game.install {
            Install::Installed => "",
            Install::Packaged { .. } => "PKG",
            Install::InstalledWithPackage { .. } => "+PKG",
        };
        match game.size.filter(|_| with_size) {
            Some(size) if tag.is_empty() => sizer::human(size),
            Some(size) => format!("{tag} {}", sizer::human(size)),
            None => tag.to_string(),
        }
    }

    /// Install state and sizes of the focused game, e.g. `Installed · 1.2 GB`.
    fn install_line(&self) -> String {
        let game = &self.games[self.cursor];
        let size = |s: Option<u64>| s.map_or("? B".to_string(), sizer::human);
        let package = size(self.package_sizes[self.cursor]);
        match &game.install {
            Install::Installed => format!("Installed · {}", size(game.size)),
            Install::Packaged { .. } => {
                format!("Package {package} · {} unpacked", size(game.size))
            }
            Install::InstalledWithPackage { verified, .. } => format!(
                "Installed {} · Package {package}{}",
                size(game.size),
                if *verified { "" } else { " (different)" }
            ),
        }
    }

    fn compatibility_color(compatibility: Compatibility) -> Color {
        match compatibility {
            Compatibility::Native => theme::ACCENT,
            Compatibility::Proton => theme::BLUE,
            Compatibility::Unsupported => theme::RED,
            Compatibility::Unknown => theme::MUTED,
        }
    }

    fn draw_info(&self, ui: &Ui, area: Rect) {
        let game = &self.games[self.cursor];
        let mut y = area.y + 24.0;
        ui.heading(
            &ui.truncate_heading(&game.title, area.w),
            area.x,
            y,
            theme::TEXT,
        );
        y += LINE + 8.0;

        let players = match game.players.trim() {
            "" => None,
            "1" => Some("1 player".to_string()),
            n => Some(format!("{n} players")),
        };
        let facts: Vec<String> = game
            .year
            .map(|y| y.to_string())
            .into_iter()
            .chain(players)
            .collect();
        if !facts.is_empty() {
            ui.text(
                &ui.truncate(&facts.join(" · "), area.w),
                area.x,
                y,
                theme::MUTED,
            );
            y += LINE;
        }

        let compatibility = scanner::compatibility(&game.targets);
        let platforms = scanner::platforms(&game.targets).join(" · ");
        let label = compatibility.label();
        ui.text(label, area.x, y, Self::compatibility_color(compatibility));
        let label_w = ui.measure(label) + 12.0;
        ui.text(
            &ui.truncate(&platforms, area.w - label_w),
            area.x + label_w,
            y,
            theme::MUTED,
        );
        y += LINE;

        ui.text(
            &ui.truncate(&self.install_line(), area.w),
            area.x,
            y,
            theme::TEXT,
        );
        y += LINE + 6.0;

        let room = ((area.bottom() - y) / LINE).floor().max(0.0) as usize;
        let lines = ui.wrap(game.description.trim(), area.w);
        let shown = lines.len().min(room);
        for (i, line) in lines.iter().take(shown).enumerate() {
            let text = if i + 1 == shown && shown < lines.len() {
                ui.truncate(&format!("{line}…"), area.w)
            } else {
                line.clone()
            };
            ui.text(&text, area.x, y + i as f32 * LINE, theme::MUTED);
        }
    }

    fn draw_loading(&self, ui: &Ui, list: Rect) {
        let Some(loader) = &self.loader else { return };
        let (done, total) = loader.progress();
        let (cx, cy) = (list.right() - 10.0, list.bottom() - 10.0);
        draw_spinner(cx, cy, 6.0, theme::ACCENT);
        let label = format!("{done}/{total}");
        ui.text(
            &label,
            cx - 12.0 - ui.measure(&label),
            cy + 5.0,
            theme::MUTED,
        );
    }

    fn draw_toast(&self, ui: &Ui, list: Rect) {
        let Some((message, color, until)) = &self.toast else {
            return;
        };
        if get_time() > *until {
            return;
        }
        let y = list.bottom() - 26.0;
        draw_rectangle(list.x, y, list.w, 24.0, theme::BG);
        stroke_rect(Rect::new(list.x, y, list.w, 24.0), 1.0, *color);
        ui.text(
            &ui.truncate(message, list.w - 16.0),
            list.x + 8.0,
            y + 18.0,
            *color,
        );
    }

    /// Full-resolution cover of the focused game (after the canvas is presented).
    pub fn overlay(&self, ui: &Ui, area: Rect) {
        let (_, Some(panel)) = self.layout(ui, area) else {
            return;
        };
        let (preview, _) = Self::split_panel(panel);
        if let Some(Some(tex)) = self.covers.get(self.cursor) {
            ui.draw_image_fit(
                tex,
                Rect::new(
                    preview.x + 4.0,
                    preview.y + 4.0,
                    preview.w - 8.0,
                    preview.h - 8.0,
                ),
            );
        }
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        if self.games.is_empty() || !self.games[self.cursor].is_installed() {
            return Vec::new();
        }
        vec![
            ui.hint(Action::Confirm, "Play", theme::ACCENT),
            ui.hint(Action::Edit, "Edit", theme::BLUE),
        ]
    }
}
