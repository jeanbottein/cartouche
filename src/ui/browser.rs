//! File / folder picker that works with a gamepad.

use std::path::{Path, PathBuf};

use macroquad::prelude::*;

use super::engine::input::Action;
use super::engine::theme;
use super::list::List;
use super::{centered, style, Hint, Ui};
use crate::core::paths;

const ROW: f32 = 22.0;

pub enum Outcome {
    Browsing,
    Picked(PathBuf),
    Cancelled,
}

enum Entry {
    UseThisFolder,
    Parent,
    Dir(String),
    File(String),
}

pub struct Browser {
    title: String,
    dirs_only: bool,
    show_hidden: bool,
    dir: PathBuf,
    entries: Vec<Entry>,
    list: List,
}

/// The folder to open for `current`: itself, its parent, or the nearest existing ancestor.
fn start_dir(current: &Path) -> PathBuf {
    current
        .ancestors()
        .find(|p| p.is_dir() && p.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(paths::home_dir)
}

impl Browser {
    pub fn new(title: &str, current: &Path, dirs_only: bool) -> Browser {
        let mut browser = Browser {
            title: title.to_string(),
            dirs_only,
            show_hidden: false,
            dir: start_dir(current),
            entries: Vec::new(),
            list: List::default(),
        };
        browser.load();
        browser
    }

    fn load(&mut self) {
        let names: Vec<String> = paths::sorted_entries(&self.dir)
            .into_iter()
            .filter(|n| self.show_hidden || !n.starts_with('.'))
            .collect();
        let (dirs, files): (Vec<String>, Vec<String>) =
            names.into_iter().partition(|n| self.dir.join(n).is_dir());
        let head = self
            .dirs_only
            .then_some(Entry::UseThisFolder)
            .into_iter()
            .chain(self.dir.parent().map(|_| Entry::Parent));
        let files = files
            .into_iter()
            .filter(|_| !self.dirs_only)
            .map(Entry::File);
        self.entries = head
            .chain(dirs.into_iter().map(Entry::Dir))
            .chain(files)
            .collect();
        self.list = List::default();
    }

    fn open(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.load();
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
            let left = self
                .dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            self.open(parent);
            if let Some(i) = self
                .entries
                .iter()
                .position(|e| matches!(e, Entry::Dir(n) if Some(n) == left.as_ref()))
            {
                self.list.cursor = i;
            }
        }
    }

    fn frame(area: Rect) -> Rect {
        centered(area, area.w - 40.0, area.h - 20.0)
    }

    fn list_area(area: Rect) -> Rect {
        let f = Browser::frame(area);
        Rect::new(f.x + 10.0, f.y + 54.0, f.w - 20.0, f.h - 62.0)
    }

    pub fn update(&mut self, ui: &mut Ui, area: Rect) -> Outcome {
        if ui.action(Action::Back) {
            return Outcome::Cancelled;
        }
        if ui.action(Action::Alt) {
            self.show_hidden = !self.show_hidden;
            self.load();
        }
        if ui.action(Action::Left) {
            self.go_up();
        }
        let activated = self.list.update(
            ui,
            self.entries.len(),
            Browser::list_area(area),
            ROW,
            |_| true,
        );
        let right = ui.action(Action::Right)
            && matches!(self.entries.get(self.list.cursor), Some(Entry::Dir(_)));
        if !(activated || right) {
            return Outcome::Browsing;
        }
        match self.entries.get(self.list.cursor) {
            Some(Entry::UseThisFolder) => Outcome::Picked(self.dir.clone()),
            Some(Entry::Parent) => {
                self.go_up();
                Outcome::Browsing
            }
            Some(Entry::Dir(name)) => {
                let next = self.dir.join(name);
                self.open(next);
                Outcome::Browsing
            }
            Some(Entry::File(name)) => Outcome::Picked(self.dir.join(name)),
            None => Outcome::Browsing,
        }
    }

    pub fn draw(&self, ui: &Ui, area: Rect) {
        let frame = Browser::frame(area);
        style::panel(ui, frame, theme::accent());
        ui.text(
            &ui.truncate(&self.title, frame.w - 20.0),
            frame.x + 10.0,
            frame.y + 22.0,
            theme::secondary(),
        );
        ui.text(
            &ui.truncate_start(&paths::lossy(&self.dir), frame.w - 20.0),
            frame.x + 10.0,
            frame.y + 44.0,
            theme::muted(),
        );

        let list = Browser::list_area(area);
        for (i, r) in self.list.visible(self.entries.len(), list, ROW) {
            let focused = i == self.list.cursor;
            if focused {
                style::highlight(r, theme::accent());
            }
            let (label, color) = match &self.entries[i] {
                Entry::UseThisFolder => ("[ Use this folder ]".to_string(), theme::accent()),
                Entry::Parent => ("..".to_string(), theme::muted()),
                Entry::Dir(n) => (format!("{n}/"), theme::info()),
                Entry::File(n) => (n.clone(), theme::text()),
            };
            let color = if focused { theme::text() } else { color };
            ui.text(
                &ui.truncate(&label, r.w - 16.0),
                r.x + 8.0,
                r.y + 16.0,
                color,
            );
        }
        if self.entries.is_empty() {
            ui.text("(empty)", list.x + 8.0, list.y + 16.0, theme::muted());
        }
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        let pick = match self.entries.get(self.list.cursor) {
            Some(Entry::Dir(_)) | Some(Entry::Parent) => "Open",
            _ => "Select",
        };
        vec![
            ui.hint(Action::Confirm, pick, theme::accent()),
            ui.hint(Action::Back, "Cancel", theme::secondary()),
            ui.hint(Action::Left, "Up / Open", theme::muted()),
            ui.hint(
                Action::Alt,
                if self.show_hidden {
                    "Hide hidden"
                } else {
                    "Show hidden"
                },
                theme::muted(),
            ),
        ]
    }
}
