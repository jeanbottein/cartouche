//! Settings: every `config.txt` key, grouped. Each change is written straight
//! to `config.txt`, touching only that line (comments and order are kept).

use std::collections::BTreeMap;

use macroquad::prelude::*;

use super::engine::theme;
use super::form::{Event as FormEvent, Field, Form};
use super::{Hint, Ui};
use crate::core::config::{self, Config};
use crate::core::steam;

/// Settings that mean nothing without Steam.
const STEAM_KEYS: [&str; 3] = [SELF_SHORTCUT, "STEAM_USERID", "PROTON_VERSION"];
/// Labelled with the cartridge folder's name, as it appears in Steam.
const SELF_SHORTCUT: &str = "STEAM_ADD_SELF";
const STRATEGIES: &[&str] = &["backup", "sync", "restore"];

enum Kind {
    Text,
    Secret,
    Flag(bool),
    Choice(&'static [&'static str]),
    /// A Steam account on this machine; empty means all of them.
    SteamUser,
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

const SCHEMA: [(&str, &[Setting]); 4] = [
    ("GENERAL", &[
        s(SELF_SHORTCUT, "in Steam", Kind::Flag(false), "Add this cartridge to Steam under its folder name, with retro artwork (or your cover.png, hero.png... in system/). Applied right away."),
        s("SAVESCOPY_STRATEGY", "Save strategy", Kind::Choice(STRATEGIES), "backup: copy saves into saves-copies/. restore: copy them back over the saves, every run (careful!)."),
        s("MUSIC_ENABLED", "Background music", Kind::Flag(true), "Shuffle the audio files in the workspace's system/ folder (mp3, flac, ogg, wav, m4a). Fades out while a game runs."),
    ]),
    ("STEAM & ARTWORK", &[
        s("STEAM_USERID", "Steam user", Kind::SteamUser, "Only update this Steam user. Default: all users."),
        s("PROTON_VERSION", "Proton version", Kind::Text, "Compatibility tool for Windows games, e.g. proton_experimental."),
        s("STEAMGRIDDB_API_KEY", "SteamGridDB API key", Kind::Secret, "For automatic artwork: steamgriddb.com/profile/preferences/api"),
        s("STEAMGRIDDB_NSFW", "Artwork: NSFW", Kind::Flag(false), "Include adult content in artwork results."),
        s("STEAMGRIDDB_HUMOR", "Artwork: humor", Kind::Flag(false), "Include humor content in artwork results."),
        s("STEAMGRIDDB_EPILEPSY", "Artwork: epilepsy", Kind::Flag(false), "Include epilepsy-triggering content in artwork results."),
        s("MANIFEST_EXPORT", "Export manifests", Kind::Flag(true), "Write manifests.json for Steam ROM Manager."),
    ]),
    ("SHARING", &[
        s("SHARE_ENABLED", "Share games on the LAN", Kind::Flag(false), "Let other •cartridge machines on the network see and copy your games."),
        s("SHARE_PORT", "Sharing port", Kind::Text, "TCP + UDP port (default 47800). Open it in the firewall if peers can't see this machine."),
        s("SHARE_PEERS", "Extra peers", Kind::Text, "host[:port], comma-separated: machines to list even when broadcast can't reach them."),
    ]),
    ("DEBUG", &[
        s("DEBUG_NO_STEAM", "Pretend Steam is missing", Kind::Flag(false), "Hide the Steam buttons and dim the Steam settings, as on a machine without Steam. Interface only."),
    ]),
];

fn steam_user_options() -> Vec<(String, String)> {
    std::iter::once((String::new(), "All users".to_string()))
        .chain(steam::users())
        .collect()
}

fn fields(cfg: &Config) -> Vec<Field> {
    let raw = cfg.raw();
    let value = |key: &str| raw.get(key).map_or("", String::as_str);
    let steam_found = steam::detected(cfg);
    SCHEMA
        .iter()
        .flat_map(|(category, settings)| {
            let fields = settings.iter().map(|s| {
                let v = value(s.key);
                let label = if s.key == SELF_SHORTCUT {
                    format!("{} {}", cfg.label(), s.label)
                } else {
                    s.label.to_string()
                };
                let label = label.as_str();
                let field = match s.kind {
                    Kind::Text => Field::text(s.key, label, v),
                    Kind::Secret => Field::secret(s.key, label, v),
                    Kind::Flag(default) => Field::flag(s.key, label, v, default),
                    Kind::Choice(options) => Field::choice(s.key, label, v, options),
                    Kind::SteamUser => Field::choice_labelled(s.key, label, v, steam_user_options()),
                };
                field
                    .with_help(s.help)
                    .disabled_if(!steam_found && STEAM_KEYS.contains(&s.key))
            });
            std::iter::once(Field::section(category)).chain(fields)
        })
        .collect()
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

    fn form_area(area: Rect) -> Rect {
        Rect::new(area.x, area.y, area.w, area.h - 22.0)
    }

    /// Show a message under the form (e.g. what applying a setting did).
    pub fn note(&mut self, message: &str, color: Color) {
        self.message = Some((message.to_string(), color));
    }

    /// The key saved to `config.txt`, if any (the app reloads it).
    pub fn update(&mut self, ui: &mut Ui, area: Rect, cfg: &Config) -> Option<String> {
        let FormEvent::Changed(id) = self.form.update(ui, Settings::form_area(area))? else {
            return None;
        };
        let value = self.form.value(&id).to_string();
        match config::update_file(&cfg.path, &BTreeMap::from([(id.clone(), value)])) {
            Ok(()) => {
                self.message = Some((format!("Saved {id}."), theme::ACCENT));
                Some(id)
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
