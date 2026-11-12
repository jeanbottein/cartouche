//! Game editor: title, SteamGridDB ID, notes, targets, save paths and artwork
//! of one game, with auto-detection, artwork fetching and `game.json` saving.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use macroquad::prelude::*;

use super::covers::{self, ImageLoader};
use super::dialog::{self, Dialog};
use super::engine::input::Action;
use super::engine::theme;
use super::form::{Event as FormEvent, Field, Form};
use super::{open_external, Hint, Ui};
use crate::core::config::Config;
use crate::core::models::{Game, ImageSlot, Images, Install, SavePath, Target};
use crate::core::platform::{arch_tag, os_tag};
use crate::core::sgdb::{self, SearchHit};
use crate::core::{detector, paths, persister, scanner};

const OS_OPTIONS: &[&str] = &["linux", "windows", "macos", "android", "web", "any"];
const ARCH_OPTIONS: &[&str] = &["x64", "arm64", "any"];
/// Matches offered when searching SteamGridDB (what fits in a dialog).
const MAX_MATCHES: usize = 8;

pub enum Event {
    Closed,
    /// `game.json` was written; the library should reload.
    Saved,
    /// Create a package of this game (one file to share or back up).
    Pack,
}

enum Pending {
    DeleteImage(ImageSlot),
    Discard,
    /// SteamGridDB search results to pick from.
    PickMatch(Vec<SearchHit>),
    /// Replace the game's metadata and artwork with this match?
    ConfirmMatch(SearchHit),
}

type FetchResult = Result<Images, String>;

/// `Celeste (2018)`, marked when it is the game's current match.
fn match_label(hit: &SearchHit, current: Option<i64>) -> String {
    let name = hit.name.as_deref().unwrap_or("?");
    let year = hit.year.map(|y| format!(" ({y})")).unwrap_or_default();
    let mark = if current == Some(hit.id) {
        " - current"
    } else {
        ""
    };
    format!("{name}{year}{mark}")
}

pub struct Editor {
    game: Game,
    saved_json: String,
    cfg: Config,
    form: Form,
    dialog: Option<(Pending, Dialog)>,
    fetch: Option<Receiver<FetchResult>>,
    search: Option<Receiver<Vec<SearchHit>>>,
    message: Option<(String, Color)>,
    art: [Option<Texture2D>; 5],
    art_loader: Option<ImageLoader>,
}

/// The game as it would be saved: rows without a target / path are dropped.
fn cleaned(game: &Game) -> Game {
    let mut game = game.clone();
    game.targets.retain(|t| !t.target.trim().is_empty());
    game.save_paths.retain(|s| !s.path.trim().is_empty());
    game
}

fn slot_label(slot: ImageSlot) -> String {
    let name = slot.name();
    name[..1].to_uppercase() + &name[1..]
}

fn fields(game: &Game) -> Vec<Field> {
    let dir = &game.dir;
    let sgdb = game
        .steamgriddb_id
        .map(|id| id.to_string())
        .unwrap_or_default();
    let mut out = vec![
        Field::section(&format!("EDIT  {}", game.folder_name)),
        Field::text("title", "Title", &game.title)
            .with_help("Name shown in the library and in Steam."),
        Field::text("sgdb", "SteamGridDB ID", &sgdb)
            .with_help("Numeric game ID on steamgriddb.com, used to fetch artwork."),
        Field::text("notes", "Notes", &game.notes),
        Field::button(
            "open_sgdb",
            "Open SteamGridDB page",
            if sgdb.is_empty() {
                "set an ID first"
            } else {
                ""
            },
            theme::BLUE,
        ),
        Field::button("find_sgdb", "Find match on SteamGridDB", "by title", theme::BLUE).with_help(
            "Search SteamGridDB for the title above and pick the right game: its title, year, ID and artwork replace these (asks first).",
        ),
        Field::section("TARGETS"),
    ];
    for (i, t) in game.targets.iter().enumerate() {
        let n = i + 1;
        out.extend([
            Field::choice(&format!("t{i}.os"), &format!("#{n} OS"), &t.os, OS_OPTIONS),
            Field::choice(
                &format!("t{i}.arch"),
                &format!("#{n} Arch"),
                &t.arch,
                ARCH_OPTIONS,
            ),
            Field::path(
                &format!("t{i}.target"),
                &format!("#{n} Executable"),
                &t.target,
                false,
                dir,
            )
            .with_help("Relative to the game folder."),
            Field::path(
                &format!("t{i}.start"),
                &format!("#{n} Start in"),
                &t.start_in,
                true,
                dir,
            )
            .with_help(
                "Working folder, relative to the game folder (empty: the executable's folder).",
            ),
            Field::text(
                &format!("t{i}.opts"),
                &format!("#{n} Launch options"),
                &t.launch_options,
            ),
            Field::button(
                &format!("t{i}.del"),
                &format!("Remove target #{n}"),
                "",
                theme::RED,
            ),
        ]);
    }
    out.extend([
        Field::button("add_target", "Add target", "", theme::ACCENT),
        Field::button("detect_targets", "Auto-detect targets", "", theme::BLUE),
        Field::section("SAVE PATHS"),
    ]);
    for (i, s) in game.save_paths.iter().enumerate() {
        let n = i + 1;
        out.extend([
            Field::choice(&format!("s{i}.os"), &format!("#{n} OS"), &s.os, OS_OPTIONS),
            Field::path(&format!("s{i}.path"), &format!("#{n} Folder"), &s.path, true, dir).with_help(
                "Windows paths (%APPDATA%, C:/...) map into the Proton prefix on Linux. Variables: ${steamappid}, ${proton_c}.",
            ),
            Field::button(&format!("s{i}.open"), &format!("Open save folder #{n}"), "", theme::BLUE),
            Field::button(&format!("s{i}.del"), &format!("Remove save path #{n}"), "", theme::RED),
        ]);
    }
    out.extend([
        Field::button("add_save", "Add save path", "", theme::ACCENT),
        Field::button(
            "detect_saves",
            "Auto-detect saves (Proton)",
            "",
            theme::BLUE,
        ),
        Field::section("ARTWORK"),
    ]);
    for (i, &slot) in ImageSlot::ALL.iter().enumerate() {
        let note = match (game.images.get(slot), game.image_path(slot)) {
            (Some(name), Some(path)) if path.is_file() => name.to_string(),
            (Some(name), _) => format!("{name} (missing)"),
            _ => "(none)".to_string(),
        };
        out.push(
            Field::button(&format!("art.{i}"), &slot_label(slot), &note, theme::TEXT)
                .with_help("Select to remove this image."),
        );
    }
    out.extend([Field::button(
        "fetch_art",
        "Fetch all artwork",
        "replaces existing",
        theme::BLUE,
    )
    .with_help(
        "Download every image from SteamGridDB (needs an API key and the SteamGridDB ID).",
    )]);
    if game.install == Install::Installed {
        out.push(
            Field::button("pack", "Create a package", "one file", theme::AMBER).with_help(
                "Pack this game into games/<name>.cartridge.tar, to share or back it up.",
            ),
        );
    }
    out.extend([
        Field::section(""),
        Field::button("save", "Save", "", theme::ACCENT),
    ]);
    out
}

/// `t3.os` → (3, "os").
fn row_field(id: &str, prefix: char) -> Option<(usize, &str)> {
    let (index, field) = id.strip_prefix(prefix)?.split_once('.')?;
    Some((index.parse().ok()?, field))
}

impl Editor {
    /// The game's folder name (which game this editor is about).
    pub fn folder_name(&self) -> &str {
        &self.game.folder_name
    }

    pub fn new(game: &Game, cfg: &Config) -> Editor {
        let mut editor = Editor {
            game: game.clone(),
            saved_json: cleaned(game).to_json(),
            cfg: cfg.clone(),
            form: Form::new(fields(game)),
            dialog: None,
            fetch: None,
            search: None,
            message: None,
            art: Default::default(),
            art_loader: None,
        };
        editor.load_art();
        editor
    }

    fn load_art(&mut self) {
        self.art = Default::default();
        self.art_loader = Some(ImageLoader::spawn(covers::artwork_jobs(&self.game)));
    }

    fn is_dirty(&self) -> bool {
        cleaned(&self.game).to_json() != self.saved_json
    }

    fn say(&mut self, message: &str, color: Color) {
        self.message = Some((message.to_string(), color));
    }

    fn rebuild(&mut self) {
        self.form.set_fields(fields(&self.game));
    }

    fn save(&mut self) -> Option<Event> {
        let mut game = cleaned(&self.game);
        if let Err(e) = persister::write_game_json(&mut game) {
            self.say(&format!("Error: {e}"), theme::RED);
            return None;
        }
        log::info!("Saved {}", game.game_json_path().display());
        self.saved_json = game.to_json();
        self.game = game;
        self.rebuild();
        self.say("Saved.", theme::ACCENT);
        Some(Event::Saved)
    }

    fn apply_change(&mut self, id: &str) {
        let value = self.form.value(id).to_string();
        match id {
            "title" if !value.trim().is_empty() => self.game.title = value.trim().to_string(),
            "title" => self.rebuild(),
            "sgdb" if value.is_empty() => self.game.steamgriddb_id = None,
            "sgdb" => match value.parse::<i64>() {
                Ok(n) => self.game.steamgriddb_id = Some(n),
                Err(_) => {
                    self.say("The SteamGridDB ID must be a number.", theme::RED);
                    self.rebuild();
                }
            },
            "notes" => self.game.notes = value,
            _ => self.apply_row_change(id, value),
        }
        self.rebuild();
    }

    fn apply_row_change(&mut self, id: &str, value: String) {
        if let Some((i, field)) = row_field(id, 't') {
            let Some(t) = self.game.targets.get_mut(i) else {
                return;
            };
            match field {
                "os" => t.os = value,
                "arch" => t.arch = value,
                "target" => t.target = value,
                "start" => t.start_in = value,
                "opts" => t.launch_options = value,
                _ => {}
            }
        } else if let Some((i, field)) = row_field(id, 's') {
            let Some(s) = self.game.save_paths.get_mut(i) else {
                return;
            };
            match field {
                "os" => s.os = value,
                "path" => s.path = value,
                _ => {}
            }
        }
    }

    /// Absolute target picked from the targets being edited.
    fn current_exe(&self) -> Option<PathBuf> {
        scanner::pick_target(&self.game.targets)
            .map(|t| scanner::resolve_target(&self.game.dir, t).target)
    }

    fn detect_targets(&mut self) {
        let detected = detector::collect_targets(&self.game.dir);
        if detected.is_empty() {
            return self.say("No executables detected.", theme::AMBER);
        }
        let before = self.game.targets.len();
        for t in detected {
            let known = self
                .game
                .targets
                .iter()
                .any(|e| (&e.os, &e.arch, &e.target) == (&t.os, &t.arch, &t.target));
            if !known {
                self.game.targets.push(t);
            }
        }
        match self.game.targets.len() - before {
            0 => self.say("No new targets found.", theme::AMBER),
            n => self.say(&format!("Detected {n} new target(s)."), theme::ACCENT),
        }
        self.rebuild();
    }

    fn detect_saves(&mut self) {
        let Some(exe) = self.current_exe() else {
            return self.say("No executable found to get the app ID.", theme::AMBER);
        };
        let found = detector::detect_proton_save_paths(&self.game.title, &exe);
        if found.is_empty() {
            return self.say("No saves found in the Proton prefix.", theme::AMBER);
        }
        let before = self.game.save_paths.len();
        for sp in found {
            if !self
                .game
                .save_paths
                .iter()
                .any(|e| e.path.trim() == sp.path)
            {
                self.game.save_paths.push(sp);
            }
        }
        match self.game.save_paths.len() - before {
            0 => self.say("Save path(s) already listed.", theme::AMBER),
            n => self.say(&format!("Detected {n} new save path(s)."), theme::ACCENT),
        }
        self.rebuild();
    }

    fn open_save_folder(&mut self, index: usize) {
        let Some(entry) = self.game.save_paths.get(index).cloned() else {
            return;
        };
        let vars = self
            .current_exe()
            .map(|exe| scanner::save_path_vars(&self.game.title, &exe))
            .unwrap_or_default();
        let entry = SavePath {
            os: if entry.os.is_empty() {
                os_tag().into()
            } else {
                entry.os
            },
            ..entry
        };
        let any_os = [entry.os.as_str()];
        let Some(path) = scanner::resolve_save_entry(&entry, &any_os, &self.game.dir, &vars) else {
            return;
        };
        if !path.exists() {
            return self.say(
                &format!("Not found on disk: {}", path.display()),
                theme::AMBER,
            );
        }
        match open_external(&paths::lossy(&path)) {
            Ok(()) => self.say("Opened the file browser.", theme::ACCENT),
            Err(e) => self.say(&e, theme::RED),
        }
    }

    /// Download the match's artwork in the background, over `base` (the images
    /// kept where the match has none).
    fn start_fetch(&mut self, base: Images) {
        let Some(id) = self.game.steamgriddb_id else {
            return self.say("No SteamGridDB ID set.", theme::AMBER);
        };
        if sgdb::api_key(&self.cfg).is_none() {
            return self.say("No API key configured (Settings).", theme::AMBER);
        }
        let (tx, rx) = channel();
        let (cfg, mut game) = (self.cfg.clone(), self.game.clone());
        game.images = base;
        std::thread::spawn(move || {
            let result = sgdb::fetch_all_images(id, &cfg).map(|(found, urls)| {
                for slot in ImageSlot::ALL {
                    if let Some(name) = found.get(slot) {
                        *game.images.slot_mut(slot) = Some(name.to_string());
                    }
                }
                game.artwork_urls = urls;
                persister::download_all_images(&mut game);
                game.images
            });
            let _ = tx.send(result);
        });
        self.fetch = Some(rx);
        self.say("Fetching artwork...", theme::MUTED);
    }

    fn poll_fetch(&mut self) -> Option<Event> {
        let result = self.fetch.as_ref()?.try_recv().ok()?;
        self.fetch = None;
        match result {
            Ok(images) => {
                self.game.images = images;
                self.load_art();
                let saved = self.save();
                self.say("Artwork updated.", theme::ACCENT);
                saved
            }
            Err(message) => {
                self.say(&message, theme::AMBER);
                None
            }
        }
    }

    fn start_search(&mut self) {
        let Some(key) = sgdb::api_key(&self.cfg) else {
            return self.say("No API key configured (Settings).", theme::AMBER);
        };
        let title = self.game.title.clone();
        self.say(
            &format!("Searching SteamGridDB for {title}..."),
            theme::MUTED,
        );
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(sgdb::search_games(&title, &key));
        });
        self.search = Some(rx);
    }

    /// Offer the search results once they arrive (and nothing else is open).
    fn poll_search(&mut self) {
        if self.dialog.is_some() || self.form.has_modal() {
            return;
        }
        let Some(hits) = self.search.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.search = None;
        if hits.is_empty() {
            let message = format!("No match on SteamGridDB for {}.", self.game.title);
            return self.say(&message, theme::AMBER);
        }
        self.message = None;
        let hits: Vec<SearchHit> = hits.into_iter().take(MAX_MATCHES).collect();
        let labels: Vec<String> = hits
            .iter()
            .map(|hit| match_label(hit, self.game.steamgriddb_id))
            .collect();
        let options: Vec<(&str, Color)> = labels
            .iter()
            .map(|label| (label.as_str(), theme::TEXT))
            .chain([("Cancel", theme::MUTED)])
            .collect();
        let dialog = Dialog::new(
            "SteamGridDB matches",
            &format!("Which game is {}?", self.game.title),
            &options,
        );
        self.dialog = Some((Pending::PickMatch(hits), dialog));
    }

    fn confirm_match(&mut self, hit: SearchHit) {
        let dialog = Dialog::new(
            &format!("Use {}?", match_label(&hit, None)),
            "Its title, year, SteamGridDB ID and artwork replace this game's, then the game is saved.",
            &[("Replace", theme::ACCENT), ("Cancel", theme::MUTED)],
        );
        self.dialog = Some((Pending::ConfirmMatch(hit), dialog));
    }

    /// Take the match's metadata now; its artwork replaces every image once
    /// downloaded (slots it has none for end up empty), then the game is saved.
    fn apply_match(&mut self, hit: SearchHit) {
        self.game.steamgriddb_id = Some(hit.id);
        if let Some(name) = hit.name.filter(|n| !n.trim().is_empty()) {
            self.game.title = name;
        }
        self.game.year = hit.year;
        self.rebuild();
        self.start_fetch(Images::default());
    }

    fn delete_image_file(&mut self, slot: ImageSlot) -> Option<Event> {
        if let Some(path) = self.game.image_path(slot) {
            match std::fs::remove_file(&path) {
                Ok(()) => log::info!("Deleted image file: {}", path.display()),
                Err(e) if path.exists() => log::error!("Failed to delete {}: {e}", path.display()),
                Err(_) => {}
            }
        }
        self.remove_image_entry(slot)
    }

    fn remove_image_entry(&mut self, slot: ImageSlot) -> Option<Event> {
        *self.game.images.slot_mut(slot) = None;
        self.load_art();
        self.save()
    }

    fn press(&mut self, id: &str) -> Option<Event> {
        match id {
            "open_sgdb" => {
                if let Some(n) = self.game.steamgriddb_id {
                    if let Err(e) = open_external(&format!("https://www.steamgriddb.com/game/{n}"))
                    {
                        self.say(&e, theme::RED);
                    }
                }
            }
            "add_target" => {
                self.game.targets.push(Target {
                    os: os_tag().into(),
                    arch: arch_tag().into(),
                    ..Default::default()
                });
                self.rebuild();
                self.form
                    .select_id(&format!("t{}.target", self.game.targets.len() - 1));
            }
            "add_save" => {
                self.game.save_paths.push(SavePath {
                    os: os_tag().into(),
                    path: String::new(),
                });
                self.rebuild();
                self.form
                    .select_id(&format!("s{}.path", self.game.save_paths.len() - 1));
            }
            "pack" => return Some(Event::Pack),
            "detect_targets" => self.detect_targets(),
            "detect_saves" => self.detect_saves(),
            "fetch_art" if self.fetch.is_none() => self.start_fetch(self.game.images.clone()),
            "find_sgdb" if self.fetch.is_none() && self.search.is_none() => self.start_search(),
            "save" => return self.save(),
            _ => self.press_row(id),
        }
        None
    }

    fn press_row(&mut self, id: &str) {
        if let Some((i, "del")) = row_field(id, 't') {
            self.game.targets.remove(i);
            self.rebuild();
        } else if let Some((i, field)) = row_field(id, 's') {
            match field {
                "open" => self.open_save_folder(i),
                "del" => {
                    self.game.save_paths.remove(i);
                    self.rebuild();
                }
                _ => {}
            }
        } else if let Some(i) = id
            .strip_prefix("art.")
            .and_then(|n| n.parse::<usize>().ok())
        {
            let slot = ImageSlot::ALL[i];
            if self.game.images.get(slot).is_none() {
                return;
            }
            let dialog = Dialog::new(
                &format!("Remove the {} image?", slot.name()),
                "The entry is removed from game.json. The file can be kept or deleted.",
                &[
                    ("Delete entry only", theme::AMBER),
                    ("Delete entry + file", theme::RED),
                    ("Cancel", theme::MUTED),
                ],
            );
            self.dialog = Some((Pending::DeleteImage(slot), dialog));
        }
    }

    fn update_dialog(&mut self, ui: &Ui, area: Rect) -> Option<Event> {
        let (pending, dialog) = self.dialog.as_mut()?;
        let choice = match dialog.update(ui, area) {
            dialog::Outcome::Open => return None,
            dialog::Outcome::Cancelled => None,
            dialog::Outcome::Chosen(i) => Some(i),
        };
        let pending = std::mem::replace(pending, Pending::Discard);
        self.dialog = None;
        match (pending, choice) {
            (Pending::DeleteImage(slot), Some(0)) => self.remove_image_entry(slot),
            (Pending::DeleteImage(slot), Some(1)) => self.delete_image_file(slot),
            (Pending::Discard, Some(0)) => self.save().map(|_| Event::Closed),
            (Pending::Discard, Some(1)) => Some(Event::Closed),
            (Pending::PickMatch(hits), Some(i)) => {
                if let Some(hit) = hits.into_iter().nth(i) {
                    self.confirm_match(hit);
                }
                None
            }
            (Pending::ConfirmMatch(hit), Some(0)) => {
                self.apply_match(hit);
                None
            }
            _ => None,
        }
    }

    fn layout(ui: &Ui, area: Rect) -> (Rect, Option<Rect>) {
        ui.split(Rect::new(area.x, area.y, area.w, area.h - 22.0), 0.6, 12.0)
    }

    pub fn update(&mut self, ui: &mut Ui, area: Rect) -> Option<Event> {
        if let Some(loader) = &mut self.art_loader {
            for (key, tex) in loader.poll() {
                self.art[key] = Some(tex);
            }
            if !loader.loading() {
                self.art_loader = None;
            }
        }
        if let Some(event) = self.poll_fetch() {
            return Some(event);
        }
        self.poll_search();
        if self.dialog.is_some() {
            return self.update_dialog(ui, area);
        }
        if !self.form.has_modal() && ui.action(Action::Back) {
            if !self.is_dirty() {
                return Some(Event::Closed);
            }
            let dialog = Dialog::new(
                "Unsaved changes",
                "Save the changes to game.json before closing?",
                &[
                    ("Save and close", theme::ACCENT),
                    ("Discard changes", theme::RED),
                    ("Keep editing", theme::MUTED),
                ],
            );
            self.dialog = Some((Pending::Discard, dialog));
            return None;
        }
        let (form_area, _) = Editor::layout(ui, area);
        match self.form.update(ui, form_area)? {
            FormEvent::Changed(id) => {
                self.apply_change(&id);
                None
            }
            FormEvent::Pressed(id) => self.press(&id),
        }
    }

    /// Artwork slot to preview: the focused one, else the cover.
    fn preview_slot(&self) -> usize {
        self.form
            .focused()
            .and_then(|f| f.id.strip_prefix("art."))
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    }

    pub fn draw(&self, ui: &Ui, area: Rect) {
        let (form_area, preview) = Editor::layout(ui, area);
        if let Some(preview) = preview {
            let slot = ImageSlot::ALL[self.preview_slot()];
            ui.text(&slot_label(slot), preview.x, preview.y + 18.0, theme::AMBER);
            if self.art[self.preview_slot()].is_none() {
                let note = if self.game.images.get(slot).is_some() && self.art_loader.is_some() {
                    "loading..."
                } else {
                    "no image"
                };
                ui.text(
                    note,
                    preview.center().x - ui.measure(note) / 2.0,
                    preview.center().y,
                    theme::MUTED,
                );
            }
        }
        let dirty = if self.is_dirty() {
            "  (unsaved changes)"
        } else {
            ""
        };
        let (message, color) = self.message.clone().unwrap_or_default();
        let status = if message.is_empty() {
            dirty.trim().to_string()
        } else {
            format!("{message}{dirty}")
        };
        ui.text(
            &ui.truncate(&status, area.w),
            area.x,
            area.bottom() - 4.0,
            if message.is_empty() {
                theme::AMBER
            } else {
                color
            },
        );
        self.form.draw(ui, form_area);
        if let Some((_, dialog)) = &self.dialog {
            dialog.draw(ui, area);
        }
    }

    /// Full-resolution artwork preview (after the canvas is presented).
    pub fn overlay(&self, ui: &Ui, area: Rect) {
        if self.form.has_modal() || self.dialog.is_some() {
            return;
        }
        let (_, Some(preview)) = Editor::layout(ui, area) else {
            return;
        };
        if let Some(tex) = &self.art[self.preview_slot()] {
            ui.draw_image_fit(
                tex,
                Rect::new(preview.x, preview.y + 26.0, preview.w, preview.h - 26.0),
            );
        }
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        if let Some((_, dialog)) = &self.dialog {
            return dialog.hints(ui);
        }
        let mut hints = self.form.hints(ui);
        if !self.form.has_modal() {
            hints.push(ui.hint(Action::Back, "Close", theme::AMBER));
        }
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: i64, name: Option<&str>, year: Option<u32>) -> SearchHit {
        SearchHit {
            id,
            name: name.map(str::to_string),
            year,
        }
    }

    #[test]
    fn match_label_shows_year_and_marks_the_current_match() {
        let celeste = hit(7, Some("Celeste"), Some(2018));
        assert_eq!(match_label(&celeste, None), "Celeste (2018)");
        assert_eq!(match_label(&celeste, Some(7)), "Celeste (2018) - current");
        assert_eq!(match_label(&hit(8, None, None), Some(7)), "?");
    }
}
