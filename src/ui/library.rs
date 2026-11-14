//! Library: a vertical list of game cards with smooth scrolling. A panel
//! beside the list (widescreen) or below it (portrait) shows the cover and the game's info
//! (year, players, platforms, install state, size, description). Covers load in the
//! background and are drawn over the presented canvas, at the window's resolution.

use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver};

use macroquad::prelude::*;

use super::covers::{self, ImageLoader};
use super::engine::input::Action;
use super::engine::theme;
use super::info::{self, Info};
use super::jobs::Jobs;
use super::launcher::{self, Session};
use super::style::{self, Toast};
use super::{draw_spinner, Hint, Ui};
use crate::core::config::Config;
use crate::core::models::{Game, Install};
use crate::core::proton::Downloads;
use crate::core::scanner;
use crate::core::{sizer, steam};

const ROW_H: f32 = 26.0;
const ROW_GAP: f32 = 4.0;
const ROW_STEP: f32 = ROW_H + ROW_GAP;
const PREVIEW_GAP: f32 = 12.0;

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
    /// Steam is installed here: otherwise its buttons and flags stay hidden.
    steam_detected: bool,
    /// Where winetricks downloads go, for Windows games.
    downloads: Downloads,
    covers: Vec<Option<Texture2D>>,
    loader: Option<ImageLoader>,
    cursor: usize,
    /// When the cursor landed on its game: its long title scrolls from then.
    focused_at: f64,
    scroll_y: f32,
    scroll_target: f32,
    pub toast: Toast,
    /// A load reading the disk in the background; the list shown stays
    /// until it arrives (events keep pointing at the games on screen).
    pending: Option<Receiver<Snapshot>>,
    /// Where cover thumbnails are cached.
    thumbs_dir: std::path::PathBuf,
}

/// What a load reads from disk, built off the UI thread: scanning reads
/// every game.json and package, and is slow on a big library on an SD card.
struct Snapshot {
    games: Vec<Game>,
    package_sizes: Vec<Option<u64>>,
    in_steam: BTreeSet<String>,
    steam_detected: bool,
    downloads: Downloads,
}

impl Snapshot {
    fn read(cfg: &Config) -> Snapshot {
        let games = cfg
            .games_dir()
            .map(|dir| scanner::load_library(&dir))
            .unwrap_or_default();
        let package_sizes = games
            .iter()
            .map(|g| Some(std::fs::metadata(g.package()?).ok()?.len()))
            .collect();
        let steam_detected = steam::detected(cfg);
        let in_steam = if steam_detected {
            steam::games_in_steam(cfg, &games)
        } else {
            BTreeSet::new()
        };
        Snapshot {
            games,
            package_sizes,
            in_steam,
            steam_detected,
            downloads: Downloads::of(cfg),
        }
    }
}

/// Where the cursor goes in a new list: on the game it was on, if still there.
fn cursor_in(games: &[Game], focused: Option<&str>) -> usize {
    focused
        .and_then(|f| games.iter().position(|g| g.folder_name == f))
        .unwrap_or(0)
        .min(games.len().saturating_sub(1))
}

impl Library {
    /// (Re)scan the games folder in the background; `poll` shows the result,
    /// keeping the cursor on the same game. A newer load replaces a pending one.
    pub fn load(&mut self, cfg: &Config) {
        let (tx, rx) = channel();
        let cfg = cfg.clone();
        self.thumbs_dir = cfg.system_dir().join("ui-thumbs");
        std::thread::spawn(move || {
            let _ = tx.send(Snapshot::read(&cfg));
        });
        self.pending = Some(rx);
    }

    fn apply(&mut self, snapshot: Snapshot) {
        let focused = self.games.get(self.cursor).map(|g| g.folder_name.clone());
        self.cursor = cursor_in(&snapshot.games, focused.as_deref());
        self.covers = vec![None; snapshot.games.len()];
        self.loader = Some(ImageLoader::spawn(covers::cover_jobs(
            &snapshot.games,
            &self.thumbs_dir,
        )));
        self.games = snapshot.games;
        self.package_sizes = snapshot.package_sizes;
        self.in_steam = snapshot.in_steam;
        self.steam_detected = snapshot.steam_detected;
        self.downloads = snapshot.downloads;
    }

    /// Still reading the library for the first time.
    fn first_load(&self) -> bool {
        self.pending.is_some() && self.games.is_empty()
    }

    /// Show a finished load, and upload covers decoded since the last frame.
    pub fn poll(&mut self) {
        if let Some(snapshot) = self.pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.pending = None;
            self.apply(snapshot);
        }
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

    /// (list rect, cover and info panel rect when there is room).
    fn layout(&self, ui: &Ui, area: Rect) -> (Rect, Option<Rect>) {
        ui.split(area, 0.5, PREVIEW_GAP)
    }

    fn max_scroll(&self, list: Rect) -> f32 {
        (self.games.len() as f32 * ROW_STEP - ROW_GAP - list.h).max(0.0)
    }

    /// `updating`: the autopilot is writing game.json files and Steam
    /// shortcuts, so editing and Steam changes wait.
    pub fn update(&mut self, ui: &Ui, area: Rect, jobs: &Jobs, updating: bool) -> Option<Event> {
        if self.games.is_empty() {
            return None;
        }
        let n = self.games.len();
        let (list, _) = self.layout(ui, area);
        let was_on = self.cursor;
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

        if self.cursor != was_on {
            self.focused_at = get_time();
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

        if updating && !busy && (ui.action(Action::Edit) || ui.action(Action::Alt)) {
            self.toast
                .show("Updating the library, one moment...", theme::secondary());
            return None;
        }
        if ui.action(Action::Edit) {
            return match (busy, installed) {
                (true, _) => Some(Event::Cancel(id)),
                (false, true) => Some(Event::Edit(self.cursor)),
                (false, false) => {
                    self.toast
                        .show("Packaged game: unpack it to edit.", theme::secondary());
                    None
                }
            };
        }
        if ui.action(Action::Alt) && self.steam_detected && installed && !busy {
            return Some(Event::Steam(self.cursor));
        }
        if !launch {
            return None;
        }
        if busy {
            self.toast
                .show("Wait for the transfer to finish.", theme::secondary());
            return None;
        }
        if !installed {
            return Some(Event::Unpack(self.cursor));
        }
        match launcher::launch(&self.games[self.cursor], &self.downloads) {
            Ok(session) => Some(Event::Launched(session)),
            Err(message) => {
                self.toast.show(&message, theme::secondary());
                None
            }
        }
    }

    pub fn draw(&self, ui: &Ui, area: Rect, jobs: &Jobs) {
        if self.first_load() {
            draw_spinner(area.x + 6.0, area.y + 24.0, 6.0, theme::accent());
            ui.text(
                "Loading games...",
                area.x + 20.0,
                area.y + 30.0,
                theme::muted(),
            );
            return;
        }
        if self.games.is_empty() {
            ui.text("No games found.", area.x, area.y + 30.0, theme::text());
            ui.text(
                &ui.truncate(
                    "Put each game in its own folder in games/: it shows up here by itself.",
                    area.w,
                ),
                area.x,
                area.y + 54.0,
                theme::muted(),
            );
            return;
        }
        let (list, preview) = self.layout(ui, area);
        self.draw_rows(ui, list, jobs, preview.is_none());
        if let Some(panel) = preview {
            let focused = &self.games[self.cursor];
            let info = Info {
                title: &focused.title,
                year: focused.year,
                players: &focused.players,
                targets: &focused.targets,
                status: self.install_line(),
                description: &focused.description,
                cover_loading: self.covers[self.cursor].is_none()
                    && focused.images.cover.is_some()
                    && self.loader.is_some(),
            };
            info::draw(
                ui,
                &info::split_panel(panel),
                &info,
                jobs.for_game(&focused.folder_name),
            );
        }
        self.draw_loading(ui, list);
        self.toast.draw(ui, list);
    }

    /// `with_sizes`: show each game's size on its row (no panel shows it).
    fn draw_rows(&self, ui: &Ui, list: Rect, jobs: &Jobs, with_sizes: bool) {
        let n = self.games.len();
        let first = (self.scroll_y / ROW_STEP).floor().max(0.0) as usize;
        let last = (((self.scroll_y + list.h) / ROW_STEP).ceil() as usize).min(n);
        for i in first..last {
            // Whole canvas pixels keep 1px borders crisp at sub-pixel scroll offsets.
            let y = (list.y - self.scroll_y + i as f32 * ROW_STEP).round();
            let focused = i == self.cursor;
            let frame = Rect::new(list.x, y, list.w, ROW_H);
            let stripe = if focused {
                theme::accent()
            } else {
                theme::secondary()
            };
            style::card(frame, stripe, focused);
            let flag_w = if self.is_in_steam(&self.games[i]) {
                style::steam_flag(frame, focused);
                8.0
            } else {
                0.0
            };
            let (tag, tag_color) = self.row_tag(i, with_sizes);
            let tag_w = if tag.is_empty() {
                0.0
            } else {
                let chip_x = list.right() - flag_w - 6.0 - style::chip_width(ui, &tag);
                style::chip(ui, &tag, chip_x, y + 18.0, tag_color) + 10.0
            };
            let title_w = list.w - 18.0 - flag_w - tag_w;
            let title = &self.games[i].title;
            let title = if focused {
                ui.ticker(title, title_w, get_time() - self.focused_at)
            } else {
                ui.truncate(title, title_w)
            };
            ui.text(
                &title,
                list.x + 12.0,
                y + 18.0,
                if focused {
                    theme::text()
                } else {
                    theme::muted()
                },
            );
            if let Some(job) = jobs.for_game(&self.games[i].folder_name) {
                let track = Rect::new(list.x + 8.0, y + ROW_H - 5.0, list.w - 16.0, 3.0);
                style::bar(track, job.fraction(), theme::secondary());
            }
        }
        // Mask partial rows that spilled below the list edge (a focus glow
        // reaches a few pixels further).
        let (w, h) = ui.canvas.size();
        style::backdrop(Rect::new(0.0, list.bottom() + 1.0, w, h - list.bottom()), h);
    }

    /// Whether the game is one the user put in Steam.
    pub fn is_in_steam(&self, game: &Game) -> bool {
        self.in_steam.contains(&game.folder_name)
    }

    /// Short marker on a row (`PKG`) and its color; with the size when
    /// there is no side panel. (Being in Steam is a flag, not a tag.)
    fn row_tag(&self, index: usize, with_size: bool) -> (String, Color) {
        let game = &self.games[index];
        let (tag, color) = match game.install {
            Install::Packaged { .. } => ("PKG", theme::secondary()),
            // A kept package shows only in the side panel, to keep the list calm.
            _ => ("", theme::muted()),
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
        draw_spinner(cx, cy, 6.0, theme::accent());
        let label = format!("{done}/{total}");
        ui.text(
            &label,
            cx - 12.0 - ui.measure(&label),
            cy + 5.0,
            theme::muted(),
        );
    }

    /// Full-resolution cover of the focused game (after the canvas is presented).
    pub fn overlay(&self, ui: &Ui, area: Rect) {
        let (_, Some(panel)) = self.layout(ui, area) else {
            return;
        };
        if let Some(Some(tex)) = self.covers.get(self.cursor) {
            info::draw_cover(ui, panel, tex);
        }
    }

    pub fn hints(&self, ui: &Ui, jobs: &Jobs) -> Vec<Hint> {
        let Some(game) = self.games.get(self.cursor) else {
            return Vec::new();
        };
        if jobs.for_game(&game.folder_name).is_some() {
            return vec![ui.hint(Action::Edit, "Cancel", theme::danger())];
        }
        if !game.is_installed() {
            return vec![ui.hint(Action::Confirm, "Unpack", theme::accent())];
        }
        let steam = if self.is_in_steam(game) {
            "Remove from Steam"
        } else {
            "Add to Steam"
        };
        let mut hints = vec![
            ui.hint(Action::Confirm, "Play", theme::accent()),
            ui.hint(Action::Edit, "Edit", theme::info()),
        ];
        if self.steam_detected {
            hints.push(ui.hint(Action::Alt, steam, theme::info()));
        }
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    fn games(names: &[&str]) -> Vec<Game> {
        names
            .iter()
            .map(|n| Game::skeleton(std::path::Path::new(n)))
            .collect()
    }

    #[test]
    fn the_cursor_stays_on_its_game_when_the_list_changes() {
        let list = games(&["A", "B", "C"]);
        assert_eq!(cursor_in(&list, Some("C")), 2);
        assert_eq!(cursor_in(&list, Some("Gone")), 0);
        assert_eq!(cursor_in(&list, None), 0);
        assert_eq!(cursor_in(&[], Some("A")), 0);
    }

    #[test]
    fn a_load_shows_once_the_background_read_arrives() {
        let dir = TempDir::new();
        dir.write("system/config.txt", b"");
        dir.mkdir("games/Celeste");
        let cfg = Config::load(&dir.path().join("system/config.txt"));
        let mut library = Library::default();

        library.load(&cfg);
        assert!(library.first_load());
        let snapshot = library.pending.take().unwrap().recv().unwrap();
        library.apply(snapshot);
        assert!(!library.first_load());
        assert_eq!(library.games[0].folder_name, "Celeste");
    }
}
