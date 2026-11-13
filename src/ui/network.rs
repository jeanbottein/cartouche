//! Network: games shared by other •cartridge machines on the LAN. Peers are
//! found in the background (broadcast + SHARE_PEERS), their games listed in
//! one list; a game can be installed here (unpacked while it streams) or
//! downloaded as a package (to unpack later or share again).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

use macroquad::prelude::*;

use super::covers::{ImageLoader, Job as CoverJob, Source};
use super::dialog::{Dialog, Outcome};
use super::engine::input::Action;
use super::engine::theme;
use super::info::{self, Info};
use super::jobs::{Jobs, Kind};
use super::list::List;
use super::style::{self, Toast};
use super::{draw_spinner, Hint, Ui};
use crate::core::config::Config;
use crate::core::share::discovery::Peer;
use crate::core::share::{self, client, RemoteGame};
use crate::core::sizer;

const ROW_H: f32 = 26.0;
const ROW_STEP: f32 = 30.0;
const PANEL_GAP: f32 = 12.0;

pub enum Event {
    /// Copy a remote game here.
    Get {
        addr: SocketAddr,
        game: String,
        title: String,
        kind: Kind,
    },
    Cancel(String),
}

/// One remote game and where it comes from.
struct Row {
    peer: Peer,
    game: RemoteGame,
    /// Why it can't be copied here (already installed or packaged).
    conflict: Option<String>,
}

/// What a background search found: each peer and its games (or the error).
type Found = Vec<(Peer, Result<Vec<RemoteGame>, String>)>;

#[derive(Default)]
pub struct Network {
    rows: Vec<Row>,
    peers: Vec<Peer>,
    searching: Option<Receiver<Found>>,
    searched: bool,
    covers: Vec<Option<Texture2D>>,
    loader: Option<ImageLoader>,
    list: List,
    /// Open "how to copy" choice for the focused row.
    dialog: Option<Dialog>,
    pub toast: Toast,
}

fn search(cfg: &Config) -> Receiver<Found> {
    let (tx, rx) = channel();
    let cfg = cfg.clone();
    std::thread::spawn(move || {
        let found = client::find_peers(&cfg)
            .into_iter()
            .map(|peer| {
                let games = client::list(peer.addr);
                (peer, games)
            })
            .collect();
        let _ = tx.send(found);
    });
    rx
}

impl Network {
    pub fn has_modal(&self) -> bool {
        self.dialog.is_some()
    }

    fn refresh(&mut self, cfg: &Config) {
        self.searching = Some(search(cfg));
        self.searched = true;
    }

    /// Re-check which remote games are already here (after a copy finished).
    pub fn recheck_local(&mut self, games_dir: Option<&Path>) {
        for row in &mut self.rows {
            row.conflict = games_dir.and_then(|dir| client::conflict(dir, &row.game.id));
        }
    }

    /// Collect a finished search and covers decoded since the last frame.
    pub fn poll(&mut self, cfg: Option<&Config>) {
        if let Some(found) = self.searching.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.searching = None;
            self.show(found, cfg.and_then(Config::games_dir).as_deref());
        }
        if let Some(loader) = &mut self.loader {
            for (index, tex) in loader.poll() {
                if let Some(slot) = self.covers.get_mut(index) {
                    *slot = Some(tex);
                }
            }
        }
    }

    fn show(&mut self, found: Found, games_dir: Option<&Path>) {
        let focused = self.rows.get(self.list.cursor).map(|r| r.game.id.clone());
        let failed: Vec<String> = found
            .iter()
            .filter_map(|(peer, games)| games.as_ref().err().map(|_| peer.info.name.clone()))
            .collect();
        self.peers = found.iter().map(|(peer, _)| peer.clone()).collect();
        let mut rows: Vec<Row> = found
            .into_iter()
            .filter_map(|(peer, games)| Some((peer, games.ok()?)))
            .flat_map(|(peer, games)| {
                games.into_iter().map(move |game| Row {
                    peer: peer.clone(),
                    game,
                    conflict: None,
                })
            })
            .collect();
        rows.sort_by_key(|r| r.game.title.to_lowercase());
        self.rows = rows;
        self.recheck_local(games_dir);
        self.list.select(
            focused
                .and_then(|id| self.rows.iter().position(|r| r.game.id == id))
                .unwrap_or(0),
            self.rows.len(),
            |_| true,
        );
        self.covers = vec![None; self.rows.len()];
        let jobs = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.game.has_cover)
            .map(|(key, r)| CoverJob {
                key,
                source: Source::Remote {
                    addr: r.peer.addr,
                    id: r.game.id.clone(),
                },
                cache: None,
            })
            .collect();
        self.loader = Some(ImageLoader::spawn(jobs));
        if !failed.is_empty() {
            self.toast.show(
                &format!("Could not list: {}", failed.join(", ")),
                theme::danger(),
            );
        }
    }

    fn layout(ui: &Ui, area: Rect) -> (Rect, Option<Rect>) {
        ui.split(area, 0.5, PANEL_GAP)
    }

    pub fn update(&mut self, ui: &Ui, area: Rect, cfg: &Config, jobs: &Jobs) -> Option<Event> {
        if !self.searched {
            self.refresh(cfg);
        }
        if let Some(dialog) = &mut self.dialog {
            let outcome = dialog.update(ui, area);
            if !matches!(outcome, Outcome::Open) {
                self.dialog = None;
            }
            let kind = match outcome {
                Outcome::Chosen(0) => Kind::Install,
                Outcome::Chosen(1) => Kind::Download,
                _ => return None,
            };
            let row = self.rows.get(self.list.cursor)?;
            return Some(Event::Get {
                addr: row.peer.addr,
                game: row.game.id.clone(),
                title: row.game.title.clone(),
                kind,
            });
        }
        if ui.action(Action::Alt) && self.searching.is_none() {
            self.refresh(cfg);
        }
        let (list, _) = Self::layout(ui, area);
        let activated = self
            .list
            .update(ui, self.rows.len(), list, ROW_STEP, |_| true);
        let row = self.rows.get(self.list.cursor)?;
        let busy = jobs.for_game(&row.game.id).is_some();
        if ui.action(Action::Edit) && busy {
            return Some(Event::Cancel(row.game.id.clone()));
        }
        if !activated {
            return None;
        }
        if busy {
            self.toast
                .show("Already copying this game.", theme::secondary());
        } else if let Some(conflict) = row.conflict.clone() {
            self.toast.show(&conflict, theme::secondary());
        } else {
            self.dialog = Some(self.copy_dialog(row));
        }
        None
    }

    fn copy_dialog(&self, row: &Row) -> Dialog {
        let size = row.game.size.map(sizer::human).unwrap_or_default();
        let message = format!(
            "From {} {size}. Install unpacks it here, ready to play. \
             Download keeps a package, to unpack later or share again.",
            row.peer.info.name
        );
        Dialog::new(
            &format!("Copy {}", row.game.title),
            &message,
            &[
                ("Install here", theme::accent()),
                ("Download package", theme::info()),
                ("Cancel", theme::muted()),
            ],
        )
    }

    pub fn draw(&self, ui: &Ui, area: Rect, cfg: Option<&Config>, jobs: &Jobs) {
        if self.rows.is_empty() {
            self.draw_empty(ui, area, cfg);
        } else {
            let (list, panel) = Self::layout(ui, area);
            self.draw_rows(ui, list, jobs);
            if let Some(panel) = panel {
                self.draw_panel(ui, panel, jobs);
            }
            self.toast.draw(ui, list);
        }
        if let Some(dialog) = &self.dialog {
            dialog.draw(ui, area);
        }
    }

    fn draw_empty(&self, ui: &Ui, area: Rect, cfg: Option<&Config>) {
        let mut y = area.y + 30.0;
        if self.searching.is_some() {
            draw_spinner(area.x + 6.0, y - 5.0, 6.0, theme::accent());
            ui.text("Searching the network...", area.x + 20.0, y, theme::text());
            return;
        }
        let headline = if self.peers.is_empty() {
            "No other •cartridge machine found."
        } else {
            "Other machines share no games."
        };
        ui.text(headline, area.x, y, theme::text());
        y += 30.0;
        let port = cfg.map_or(share::DEFAULT_PORT, share::port);
        let help = [
            "On the other machine: SETTINGS > Share games on the LAN, or run".to_string(),
            "cartridge --serve. Both machines need the same sharing port".to_string(),
            format!("({port}, TCP + UDP) open in their firewall."),
            String::new(),
            "Not on the same network segment? Add it in SETTINGS > Extra peers.".to_string(),
        ];
        for line in help {
            ui.text(&ui.truncate(&line, area.w), area.x, y, theme::muted());
            y += 20.0;
        }
    }

    fn draw_rows(&self, ui: &Ui, list: Rect, jobs: &Jobs) {
        for (i, r) in self.list.visible(self.rows.len(), list, ROW_STEP) {
            let row = &self.rows[i];
            let focused = i == self.list.cursor;
            let frame = Rect::new(r.x, r.y, r.w, ROW_H);
            let stripe = if row.conflict.is_some() {
                theme::disabled()
            } else if focused {
                theme::accent()
            } else {
                theme::info()
            };
            style::card(frame, stripe, focused);
            let tag = if row.conflict.is_some() {
                "HERE".to_string()
            } else {
                row.peer.info.name.clone()
            };
            let tag_w = ui.measure(&tag) + 12.0;
            ui.text(&tag, frame.right() - tag_w, frame.y + 18.0, theme::muted());
            let title = ui.truncate(&row.game.title, frame.w - 18.0 - tag_w);
            let color = if focused {
                theme::text()
            } else {
                theme::muted()
            };
            ui.text(&title, frame.x + 12.0, frame.y + 18.0, color);
            if let Some(job) = jobs.for_game(&row.game.id) {
                let track = Rect::new(frame.x + 8.0, frame.y + ROW_H - 5.0, frame.w - 16.0, 3.0);
                style::bar(track, job.fraction(), theme::secondary());
            }
        }
    }

    fn status_line(row: &Row) -> String {
        let size = row
            .game
            .size
            .map(sizer::human)
            .unwrap_or_else(|| "? B".into());
        let form = if row.game.installed {
            ""
        } else {
            " · package"
        };
        let here = row.conflict.as_ref().map_or("", |_| " · already here");
        format!("On {} · {size}{form}{here}", row.peer.info.name)
    }

    fn draw_panel(&self, ui: &Ui, panel: Rect, jobs: &Jobs) {
        let Some(row) = self.rows.get(self.list.cursor) else {
            return;
        };
        let info = Info {
            title: &row.game.title,
            year: row.game.year,
            players: &row.game.players,
            targets: &row.game.targets,
            status: Self::status_line(row),
            description: &row.game.description,
            cover_loading: row.game.has_cover
                && self.covers[self.list.cursor].is_none()
                && self.loader.as_ref().is_some_and(ImageLoader::loading),
        };
        info::draw(
            ui,
            &info::split_panel(panel),
            &info,
            jobs.for_game(&row.game.id),
        );
    }

    /// Full-resolution cover of the focused game (after the canvas is presented).
    pub fn overlay(&self, ui: &Ui, area: Rect) {
        if self.dialog.is_some() {
            return;
        }
        let (_, Some(panel)) = Self::layout(ui, area) else {
            return;
        };
        if let Some(Some(tex)) = self.covers.get(self.list.cursor) {
            info::draw_cover(ui, panel, tex);
        }
    }

    pub fn hints(&self, ui: &Ui, jobs: &Jobs) -> Vec<Hint> {
        if let Some(dialog) = &self.dialog {
            return dialog.hints(ui);
        }
        let refresh = ui.hint(Action::Alt, "Refresh", theme::info());
        let Some(row) = self.rows.get(self.list.cursor) else {
            return vec![refresh];
        };
        let action = match (jobs.for_game(&row.game.id), &row.conflict) {
            (Some(_), _) => ui.hint(Action::Edit, "Cancel", theme::danger()),
            (None, None) => ui.hint(Action::Confirm, "Copy", theme::accent()),
            (None, Some(_)) => return vec![refresh],
        };
        vec![action, refresh]
    }
}
