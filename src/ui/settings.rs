//! Settings: every `config.txt` key, grouped, plus custom `BACKUP_*` /
//! `RUN_AFTER_*` entries. Each change is written straight to `config.txt`,
//! touching only that line (comments and order are kept).

use std::collections::BTreeMap;

use macroquad::prelude::*;

use super::engine::theme;
use super::form::{Event as FormEvent, Field, Form};
use super::{Hint, Ui};
use crate::core::config::{self, Config};

const STRATEGIES: &[&str] = &["backup", "sync", "restore"];
const NEW_ENTRY: &str = "+new";

enum Kind {
    Text,
    Secret,
    Flag(bool),
    Choice(&'static [&'static str]),
    Path,
}

struct Setting {
    key: &'static str,
    label: &'static str,
    kind: Kind,
    help: &'static str,
}

const fn s(key: &'static str, label: &'static str, kind: Kind, help: &'static str) -> Setting {
    Setting {
        key,
        label,
        kind,
        help,
    }
}

const SCHEMA: [(&str, &[Setting]); 5] = [
    ("GENERAL", &[
        s("MACHINE_NAME", "Machine name", Kind::Text, "Prefixes the config backup in the save copies folder."),
        s("PERSIST_DATA", "Persist game data", Kind::Flag(true), "Write .cartouche/ folders (game.json, artwork) in game folders."),
        s("MUSIC_ENABLED", "Background music", Kind::Flag(true), "Shuffle the audio files in the workspace's system/ folder (mp3, flac, ogg, wav, m4a). Fades out while a game runs."),
    ]),
    ("PATHS", &[
        s("FREEGAMES_PATH", "Games folder", Kind::Path, "Where your DRM-free games are stored. Relative paths are relative to the workspace."),
        s("PATCHES_PATH", "Patches folder", Kind::Path, "Mods and patches (folders with a patch.json)."),
    ]),
    ("SAVES", &[
        s("SAVESCOPY_PATH", "Save copies", Kind::Path, "Where save backups are stored."),
        s("SAVESCOPY_STRATEGY", "Strategy", Kind::Choice(STRATEGIES), "backup: copy saves to the backup. restore: copy the backup over the saves, every run (careful!)."),
        s("SAVESLINK_PATH", "Save links", Kind::Path, "Folder of symlinks to the live save folders, for Syncthing & co."),
    ]),
    ("STEAM & ARTWORK", &[
        s("STEAM_EXPOSE", "Add games to Steam", Kind::Flag(false), "Add games as non-Steam shortcuts, with artwork."),
        s("STEAM_ADD_SELF", "Add Cartouche to Steam", Kind::Flag(false), "Add this app to Steam as '.cartouche', with retro artwork."),
        s("STEAM_USERID", "Steam user ID", Kind::Text, "Only update this Steam user (empty: all users)."),
        s("PROTON_VERSION", "Proton version", Kind::Text, "Compatibility tool for Windows games, e.g. proton_experimental."),
        s("STEAMGRIDDB_API_KEY", "SteamGridDB API key", Kind::Secret, "For automatic artwork: steamgriddb.com/profile/preferences/api"),
        s("STEAMGRIDDB_NSFW", "Artwork: NSFW", Kind::Flag(false), "Include adult content in artwork results."),
        s("STEAMGRIDDB_HUMOR", "Artwork: humor", Kind::Flag(false), "Include humor content in artwork results."),
        s("STEAMGRIDDB_EPILEPSY", "Artwork: epilepsy", Kind::Flag(false), "Include epilepsy-triggering content in artwork results."),
        s("MANIFEST_EXPORT", "Export manifests", Kind::Flag(true), "Write manifests.json for Steam ROM Manager."),
        s("MANIFEST_PATH", "Manifest file", Kind::Path, "Custom manifests.json path (default: in the games folder)."),
    ]),
    ("SHARING", &[
        s("SHARE_ENABLED", "Share games on the LAN", Kind::Flag(false), "Let other Cartouche machines on the network see and copy your games."),
        s("SHARE_PORT", "Sharing port", Kind::Text, "TCP + UDP port (default 47800). Open it in the firewall if peers can't see this machine."),
        s("SHARE_PEERS", "Extra peers", Kind::Text, "host[:port], comma-separated: machines to list even when broadcast can't reach them."),
    ]),
];

fn in_schema(key: &str) -> bool {
    SCHEMA
        .iter()
        .any(|(_, settings)| settings.iter().any(|s| s.key == key))
}

fn fields(cfg: &Config) -> Vec<Field> {
    let raw = cfg.raw();
    let workspace = cfg.workspace();
    let value = |key: &str| raw.get(key).map_or("", String::as_str);
    let mut out = vec![Field::section(&format!("CONFIG  {}", cfg.path.display()))];
    for (category, settings) in SCHEMA {
        out.push(Field::section(category));
        out.extend(settings.iter().map(|s| {
            let v = value(s.key);
            let field = match s.kind {
                Kind::Text => Field::text(s.key, s.label, v),
                Kind::Secret => Field::secret(s.key, s.label, v),
                Kind::Flag(default) => Field::flag(s.key, s.label, v, default),
                Kind::Choice(options) => Field::choice(s.key, s.label, v, options),
                Kind::Path => Field::path(s.key, s.label, v, true, &workspace),
            };
            field.with_help(s.help)
        }));
    }
    out.push(Field::section("CUSTOM"));
    for (key, v) in raw.iter().filter(|(k, _)| !in_schema(k)) {
        let field = if config::is_path_key(key) {
            Field::path(key, key, v, true, &workspace)
        } else {
            Field::text(key, key, v)
        };
        out.push(field.with_help("Clear the value to disable this entry."));
    }
    out.push(Field::text(NEW_ENTRY, "+ Add entry", "").with_help(
        "Type KEY=value, e.g. BACKUP_shaders=~/.local/share/shaders or RUN_AFTER_01=echo done",
    ));
    out
}

/// A key the user changed; the app reloads the config afterwards.
pub struct Saved {
    pub key: String,
}

pub struct Settings {
    form: Form,
    message: Option<(String, Color)>,
}

impl Settings {
    pub fn new(cfg: &Config) -> Settings {
        Settings {
            form: Form::new(fields(cfg)),
            message: None,
        }
    }

    /// Rebuild from a reloaded config, keeping the cursor.
    pub fn refresh(&mut self, cfg: &Config) {
        self.form.set_fields(fields(cfg));
    }

    pub fn has_modal(&self) -> bool {
        self.form.has_modal()
    }

    /// The (key, value) a change writes, or why it can't.
    fn update_for(&self, id: &str) -> Result<(String, String), String> {
        let value = self.form.value(id).to_string();
        if id != NEW_ENTRY {
            return Ok((id.to_string(), value));
        }
        let (key, value) = value.split_once('=').ok_or("Use KEY=value.")?;
        let key = key.trim().to_uppercase().replace(' ', "_");
        let valid = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(format!("Invalid key '{key}'."));
        }
        Ok((key, value.trim().to_string()))
    }

    fn form_area(area: Rect) -> Rect {
        Rect::new(area.x, area.y, area.w, area.h - 22.0)
    }

    pub fn update(&mut self, ui: &mut Ui, area: Rect, cfg: &Config) -> Option<Saved> {
        let FormEvent::Changed(id) = self.form.update(ui, Settings::form_area(area))? else {
            return None;
        };
        let (key, value) = match self.update_for(&id) {
            Ok(update) => update,
            Err(message) => {
                self.message = Some((message, theme::RED));
                return None;
            }
        };
        match config::update_file(&cfg.path, &BTreeMap::from([(key.clone(), value)])) {
            Ok(()) => {
                self.message = Some((format!("Saved {key}."), theme::ACCENT));
                Some(Saved { key })
            }
            Err(e) => {
                self.message = Some((format!("Could not save: {e}"), theme::RED));
                None
            }
        }
    }

    pub fn draw(&self, ui: &Ui, area: Rect) {
        if let Some((message, color)) = &self.message {
            ui.text(
                &ui.truncate(message, area.w),
                area.x,
                area.bottom() - 4.0,
                *color,
            );
        }
        self.form.draw(ui, Settings::form_area(area));
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        self.form.hints(ui)
    }
}
