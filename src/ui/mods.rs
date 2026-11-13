//! MODS: every mod under `mods/` with its target games and status. A turns
//! the focused mod on (applied, kept applied by the pipeline) or off
//! (originals restored); mods that can't be used say why. Listing reads game
//! files' checksums and toggling patches them, so both run on a thread.

use std::sync::mpsc::{channel, Receiver};

use macroquad::prelude::*;

use super::engine::input::Action;
use super::engine::theme;
use super::list::List;
use super::style::{self, Toast};
use super::{draw_spinner, Hint, Ui};
use crate::core::config::Config;
use crate::core::mods::{self, Mod, Status};

const ROW_H: f32 = 26.0;
const ROW_STEP: f32 = 30.0;

enum Done {
    Listed(Vec<Mod>),
    Toggled { message: String, ok: bool },
}

#[derive(Default)]
pub struct ModsScreen {
    mods: Vec<Mod>,
    list: List,
    listed: bool,
    /// A listing or a toggle running in the background.
    task: Option<Receiver<Done>>,
    toast: Toast,
}

fn status_label(status: &Status) -> (&'static str, Color) {
    match status {
        Status::On => ("ON", theme::accent()),
        Status::Off => ("OFF", theme::muted()),
        Status::Partial => ("PARTIAL", theme::secondary()),
        Status::GameNotFound => ("NO GAME", theme::muted()),
        Status::GameUpdated => ("GAME UPDATED", theme::secondary()),
        Status::Broken(_) => ("BROKEN", theme::danger()),
    }
}

/// Why a mod can't be turned on, if it can't.
fn blocked(m: &Mod) -> Option<String> {
    match &m.status {
        Status::GameNotFound => Some(format!("Game not found: {}", m.games.join(", "))),
        Status::GameUpdated => {
            Some("The game's files changed (an update?): this mod no longer matches them.".into())
        }
        Status::Broken(why) => Some(format!("Broken mod: {why}")),
        _ => None,
    }
}

fn is_on(m: &Mod) -> bool {
    m.enabled || matches!(m.status, Status::On | Status::Partial)
}

impl ModsScreen {
    pub fn is_busy(&self) -> bool {
        self.task.is_some()
    }

    fn start(&mut self, work: impl FnOnce() -> Done + Send + 'static) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        self.task = Some(rx);
    }

    fn refresh(&mut self, cfg: &Config) {
        let cfg = cfg.clone();
        self.listed = true;
        self.start(move || Done::Listed(mods::list(&cfg)));
    }

    fn toggle(&mut self, cfg: &Config) {
        let Some(m) = self.mods.get(self.list.cursor).cloned() else {
            return;
        };
        let turning_on = !is_on(&m);
        if let Some(why) = blocked(&m).filter(|_| turning_on) {
            self.toast.show(&why, theme::secondary());
            return;
        }
        let cfg = cfg.clone();
        self.start(move || {
            let (result, verb) = if turning_on {
                (mods::enable(&cfg, &m.id), "on")
            } else {
                (mods::disable(&cfg, &m.id), "off")
            };
            match result {
                Ok(()) => Done::Toggled {
                    message: format!("{} is {verb}", m.id),
                    ok: true,
                },
                Err(e) => Done::Toggled {
                    message: format!("{}: {e}", m.id),
                    ok: false,
                },
            }
        });
    }

    /// Collect a finished listing or toggle; a toggle is followed by a fresh listing.
    pub fn poll(&mut self, cfg: Option<&Config>) {
        let Some(done) = self.task.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.task = None;
        match done {
            Done::Listed(mods) => {
                self.mods = mods;
                let cursor = self.list.cursor;
                self.list.select(cursor, self.mods.len(), |_| true);
            }
            Done::Toggled { message, ok } => {
                self.toast
                    .show(&message, if ok { theme::accent() } else { theme::danger() });
                if let Some(cfg) = cfg {
                    self.refresh(cfg);
                }
            }
        }
    }

    pub fn update(&mut self, ui: &Ui, area: Rect, cfg: &Config) {
        if !self.listed {
            self.refresh(cfg);
        }
        let list = Self::list_area(area);
        let activated = self
            .list
            .update(ui, self.mods.len(), list, ROW_STEP, |_| true);
        if self.is_busy() {
            return;
        }
        if ui.action(Action::Alt) {
            self.refresh(cfg);
        } else if activated {
            self.toggle(cfg);
        }
    }

    /// The list, leaving room for the focused mod's details below.
    fn list_area(area: Rect) -> Rect {
        Rect::new(area.x, area.y, area.w, area.h - 56.0)
    }

    pub fn draw(&self, ui: &Ui, area: Rect, cfg: Option<&Config>) {
        if self.mods.is_empty() {
            self.draw_empty(ui, area, cfg);
            return;
        }
        let list = Self::list_area(area);
        for (i, r) in self.list.visible(self.mods.len(), list, ROW_STEP) {
            self.draw_row(ui, i, Rect::new(r.x, r.y, r.w, ROW_H));
        }
        self.draw_details(ui, Rect::new(area.x, list.bottom() + 6.0, area.w, 50.0));
        self.draw_busy(ui, area);
        self.toast.draw(ui, list);
    }

    fn draw_row(&self, ui: &Ui, i: usize, frame: Rect) {
        let m = &self.mods[i];
        let focused = i == self.list.cursor;
        let (label, color) = status_label(&m.status);
        style::card(frame, color, focused);
        let label_w = style::chip_width(ui, label) + 12.0;
        style::chip(
            ui,
            label,
            frame.right() - label_w + 6.0,
            frame.y + 18.0,
            color,
        );
        let games = m.games.join(", ");
        let games_w = (frame.w * 0.35).min(ui.measure(&games) + 12.0);
        ui.text(
            &ui.truncate(&games, games_w - 12.0),
            frame.right() - label_w - games_w,
            frame.y + 18.0,
            theme::muted(),
        );
        let name_w = frame.w - 18.0 - label_w - games_w;
        let name_color = if focused {
            theme::text()
        } else {
            theme::muted()
        };
        ui.text(
            &ui.truncate(&m.id, name_w),
            frame.x + 12.0,
            frame.y + 18.0,
            name_color,
        );
    }

    fn draw_details(&self, ui: &Ui, area: Rect) {
        let Some(m) = self.mods.get(self.list.cursor) else {
            return;
        };
        let detail = blocked(m).unwrap_or_else(|| match m.status {
            Status::On => "Applied. Turning it off restores the game's original files.".into(),
            Status::Partial => {
                "Partly applied (a game update?): turn it on again to finish.".into()
            }
            _ => "Turning it on patches the game; the originals are kept to restore them.".into(),
        });
        let mut y = area.y + 16.0;
        for line in ui.wrap(&detail, area.w).into_iter().take(2) {
            ui.text(&line, area.x, y, theme::muted());
            y += 20.0;
        }
    }

    fn draw_empty(&self, ui: &Ui, area: Rect, cfg: Option<&Config>) {
        let y = area.y + 30.0;
        if self.is_busy() {
            draw_spinner(area.x + 6.0, y - 5.0, 6.0, theme::accent());
            ui.text("Reading mods...", area.x + 20.0, y, theme::text());
            return;
        }
        ui.text("No mods.", area.x, y, theme::text());
        let folder = cfg.map_or("mods/".into(), |c| c.mods_dir().display().to_string());
        let help = [
            "Put each mod in its own folder in".to_string(),
            folder,
            "with a patch.json listing the files it changes.".to_string(),
        ];
        for (i, line) in help.iter().enumerate() {
            let text = ui.truncate_start(line, area.w);
            ui.text(&text, area.x, y + 30.0 + i as f32 * 20.0, theme::muted());
        }
    }

    fn draw_busy(&self, ui: &Ui, area: Rect) {
        if !self.is_busy() {
            return;
        }
        let (cx, cy) = (area.right() - 10.0, area.bottom() - 10.0);
        draw_spinner(cx, cy, 6.0, theme::accent());
        let label = "working...";
        ui.text(
            label,
            cx - 12.0 - ui.measure(label),
            cy + 5.0,
            theme::muted(),
        );
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        let refresh = ui.hint(Action::Alt, "Refresh", theme::info());
        let Some(m) = self.mods.get(self.list.cursor).filter(|_| !self.is_busy()) else {
            return vec![refresh];
        };
        let toggle = match (is_on(m), blocked(m)) {
            (true, _) => Some(ui.hint(Action::Confirm, "Turn off", theme::secondary())),
            (false, None) => Some(ui.hint(Action::Confirm, "Turn on", theme::accent())),
            (false, Some(_)) => None,
        };
        toggle.into_iter().chain([refresh]).collect()
    }
}
