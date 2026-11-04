//! Library: a vertical list of game "boxes" with smooth scrolling. In
//! widescreen a panel beside the list shows the cover and the game's info
//! (year, players, platforms, install state, size, description). Covers load in the
//! background and are drawn at full window resolution over the pixel canvas,
//! so photos stay crisp while the UI keeps its retro look.

use std::collections::BTreeSet;

use macroquad::prelude::*;

use super::covers::{self, ImageLoader};
use super::engine::canvas::AspectMode;
use super::engine::input::Action;
use super::engine::theme;
use super::info::{self, Info};
use super::jobs::Jobs;
use super::launcher::{self, Session};
use super::{draw_spinner, stroke_rect, Hint, Ui};
use crate::core::config::Config;
use crate::core::models::{Game, Install};
use crate::core::scanner;
use crate::core::{sizer, steam};

const ROW_H: f32 = 26.0;
const ROW_GAP: f32 = 4.0;
const ROW_STEP: f32 = ROW_H + ROW_GAP;
const PREVIEW_GAP: f32 = 12.0;
/// How long a message stays on screen, in seconds.
const TOAST_SECS: f64 = 4.0;

pub enum Event {
    Launched(Session),
    Edit(usize),
    /// Unpack the focused packaged game.
    Unpack(usize),
    /// Add the focused game to Steam, or take it out.
    Steam(usize),
    /// Cancel the transfer of this game.
    Cancel(String),
}

#[derive(Default)]
pub struct Library {
    pub games: Vec<Game>,
    /// Package file size per game (read once per load, not every frame).
    package_sizes: Vec<Option<u64>>,
    /// Folder names of the games the user put in Steam.
    in_steam: BTreeSet<String>,
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
        self.in_steam = steam::games_in_steam(cfg, &self.games);
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

    fn max_scroll(&self, list: Rect) -> f32 {
        (self.games.len() as f32 * ROW_STEP - ROW_GAP - list.h).max(0.0)
    }

    pub fn update(&mut self, ui: &Ui, area: Rect, jobs: &Jobs) -> Option<Event> {
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

        let game = &self.games[self.cursor];
        let id = game.folder_name.clone();
        let busy = jobs.for_game(&id).is_some();
        let installed = game.is_installed();

        if ui.action(Action::Edit) {
            return match (busy, installed) {
                (true, _) => Some(Event::Cancel(id)),
                (false, true) => Some(Event::Edit(self.cursor)),
                (false, false) => {
                    self.toast("Packaged game: unpack it to edit.", theme::AMBER);
                    None
                }
            };
        }
        if ui.action(Action::Alt) && installed && !busy {
            return Some(Event::Steam(self.cursor));
        }
        if !launch {
            return None;
        }
        if busy {
            self.toast("Wait for the transfer to finish.", theme::AMBER);
            return None;
        }
        if !installed {
            return Some(Event::Unpack(self.cursor));
        }
        match launcher::launch(&self.games[self.cursor]) {
            Ok(session) => Some(Event::Launched(session)),
            Err(message) => {
                self.toast(&message, theme::AMBER);
                None
            }
        }
    }

    pub fn draw(&self, ui: &Ui, area: Rect, jobs: &Jobs) {
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
        self.draw_rows(ui, list, jobs);
        if let Some(panel) = preview {
            let (cover, info_area) = info::split_panel(panel);
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
            let info = Info {
                title: &focused.title,
                year: focused.year,
                players: &focused.players,
                targets: &focused.targets,
                status: self.install_line(),
                description: &focused.description,
            };
            info::draw(ui, info_area, &info, jobs.for_game(&focused.folder_name));
        }
        self.draw_loading(ui, list);
        self.draw_toast(ui, list);
    }

    fn draw_rows(&self, ui: &Ui, list: Rect, jobs: &Jobs) {
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
            // In Steam: a discreet blue flag inside the right edge (inside the
            // border, so it looks the same on the focused row).
            let flag_w = if self.is_in_steam(&self.games[i]) {
                draw_rectangle(
                    list.right() - th - 4.0,
                    y + th,
                    4.0,
                    ROW_H - 2.0 * th,
                    theme::BLUE,
                );
                6.0
            } else {
                0.0
            };
            let (tag, tag_color) = self.row_tag(i, ui.canvas.mode == AspectMode::Square);
            let tag_w = if tag.is_empty() {
                0.0
            } else {
                ui.measure(&tag) + 12.0
            };
            ui.text(&tag, list.right() - flag_w - tag_w, y + 18.0, tag_color);
            let title = ui.truncate(&self.games[i].title, list.w - 18.0 - flag_w - tag_w);
            ui.text(
                &title,
                list.x + 12.0,
                y + 18.0,
                if focused { theme::TEXT } else { theme::MUTED },
            );
            if let Some(job) = jobs.for_game(&self.games[i].folder_name) {
                let track = Rect::new(list.x + 6.0, y + ROW_H - 5.0, list.w - 8.0, 3.0);
                info::bar(track, job.fraction(), theme::AMBER);
            }
        }
        // Mask partial rows that spilled below the list edge.
        let (w, h) = ui.canvas.size();
        draw_rectangle(0.0, list.bottom(), w, h - list.bottom(), theme::BG);
    }

    /// Short install marker on a row; with the size when there is no side panel.
    /// Whether the game is one the user put in Steam.
    pub fn is_in_steam(&self, game: &Game) -> bool {
        self.in_steam.contains(&game.folder_name)
    }

    /// Short marker on a row (`PKG`) and its color; with the size when
    /// there is no side panel. (Being in Steam is a flag, not a tag.)
    fn row_tag(&self, index: usize, with_size: bool) -> (String, Color) {
        let game = &self.games[index];
        let (tag, color) = match game.install {
            Install::Packaged { .. } => ("PKG", theme::AMBER),
            // A kept package shows only in the side panel, to keep the list calm.
            _ => ("", theme::MUTED),
        };
        let text = match game.size.filter(|_| with_size) {
            Some(size) if tag.is_empty() => sizer::human(size),
            Some(size) => format!("{tag} {}", sizer::human(size)),
            None => tag.to_string(),
        };
        (text, color)
    }

    /// Install state and sizes of the focused game, e.g. `Installed · 1.2 GB`.
    fn install_line(&self) -> String {
        let game = &self.games[self.cursor];
        let size = |s: Option<u64>| s.map_or("? B".to_string(), sizer::human);
        let package = size(self.package_sizes[self.cursor]);
        match &game.install {
            Install::Installed => format!("Installed · {}", size(game.size)),
            Install::Packaged { .. } => format!("Packaged · {package}"),
            Install::InstalledWithPackage { verified, .. } => format!(
                "Installed {} + Package {package}{}",
                size(game.size),
                if *verified { "" } else { " (different)" }
            ),
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
        let (preview, _) = info::split_panel(panel);
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

    pub fn hints(&self, ui: &Ui, jobs: &Jobs) -> Vec<Hint> {
        let Some(game) = self.games.get(self.cursor) else {
            return Vec::new();
        };
        if jobs.for_game(&game.folder_name).is_some() {
            return vec![ui.hint(Action::Edit, "Cancel", theme::RED)];
        }
        if !game.is_installed() {
            return vec![ui.hint(Action::Confirm, "Unpack", theme::ACCENT)];
        }
        let steam = if self.is_in_steam(game) {
            "Remove from Steam"
        } else {
            "Add to Steam"
        };
        vec![
            ui.hint(Action::Confirm, "Play", theme::ACCENT),
            ui.hint(Action::Edit, "Edit", theme::BLUE),
            ui.hint(Action::Alt, steam, theme::BLUE),
        ]
    }
}
