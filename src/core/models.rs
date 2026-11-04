//! In-memory game model and the `.cartridge/game.json` schema (version 2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::app::data_dir_name;

pub const GAME_JSON: &str = "game.json";
pub const SCHEMA_VERSION: u32 = 2;

/// `null` reads as an empty string (hand-edited files).
fn nullable<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(scalar_text(Value::deserialize(d)?))
}

/// Any JSON scalar as text, so a hand-edited `"arch": 64` doesn't make the
/// whole file unreadable.
fn scalar_text(value: Value) -> String {
    match value {
        Value::String(s) => s,
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// `null` and `""` both mean "no image".
fn optional_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Some(scalar_text(Value::deserialize(d)?)).filter(|s| !s.is_empty()))
}

/// A platform-specific executable of a game; paths are relative to the game dir.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    #[serde(default, deserialize_with = "nullable")]
    pub os: String,
    #[serde(default, deserialize_with = "nullable")]
    pub arch: String,
    #[serde(default, deserialize_with = "nullable")]
    pub target: String,
    #[serde(default, rename = "startIn", deserialize_with = "nullable")]
    pub start_in: String,
    #[serde(default, rename = "launchOptions", deserialize_with = "nullable")]
    pub launch_options: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavePath {
    #[serde(default, deserialize_with = "nullable")]
    pub os: String,
    #[serde(default, deserialize_with = "nullable")]
    pub path: String,
}

/// Artwork filenames stored in `.cartridge/`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Images {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_name"
    )]
    pub cover: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_name"
    )]
    pub icon: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_name"
    )]
    pub hero: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_name"
    )]
    pub logo: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_name"
    )]
    pub header: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSlot {
    Cover,
    Icon,
    Hero,
    Logo,
    Header,
}

impl ImageSlot {
    pub const ALL: [ImageSlot; 5] = [
        ImageSlot::Cover,
        ImageSlot::Icon,
        ImageSlot::Hero,
        ImageSlot::Logo,
        ImageSlot::Header,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ImageSlot::Cover => "cover",
            ImageSlot::Icon => "icon",
            ImageSlot::Hero => "hero",
            ImageSlot::Logo => "logo",
            ImageSlot::Header => "header",
        }
    }

    /// SteamGridDB artwork type that fills this slot.
    pub fn art_type(self) -> &'static str {
        match self {
            ImageSlot::Cover => "poster",
            ImageSlot::Icon => "icon",
            ImageSlot::Hero => "hero",
            ImageSlot::Logo => "logo",
            ImageSlot::Header => "grid",
        }
    }
}

impl Images {
    pub fn get(&self, slot: ImageSlot) -> Option<&str> {
        self.slot(slot).as_deref()
    }

    fn slot(&self, slot: ImageSlot) -> &Option<String> {
        match slot {
            ImageSlot::Cover => &self.cover,
            ImageSlot::Icon => &self.icon,
            ImageSlot::Hero => &self.hero,
            ImageSlot::Logo => &self.logo,
            ImageSlot::Header => &self.header,
        }
    }

    pub fn slot_mut(&mut self, slot: ImageSlot) -> &mut Option<String> {
        match slot {
            ImageSlot::Cover => &mut self.cover,
            ImageSlot::Icon => &mut self.icon,
            ImageSlot::Hero => &mut self.hero,
            ImageSlot::Logo => &mut self.logo,
            ImageSlot::Header => &mut self.header,
        }
    }
}

/// The target picked for this machine, with absolute paths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub target: PathBuf,
    pub start_in: PathBuf,
    pub launch_options: String,
    /// OS of the picked target (lowercase).
    pub os: String,
}

/// SteamGridDB artwork URLs by art type (`grid`, `poster`, `hero`, `logo`, `icon`).
pub type ArtworkUrls = BTreeMap<String, Option<String>>;

/// Where a game lives on disk. A package (`Foo.cartridge.tar`) and the folder
/// `Foo/` are the same game: the scanner merges them into one entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Install {
    /// Only `Foo/`.
    #[default]
    Installed,
    /// Only `Foo.cartridge.tar`: not runnable, read-only until unpacked.
    Packaged { package: PathBuf },
    /// Both; `verified` when `Foo/` was unpacked from this very package.
    InstalledWithPackage { package: PathBuf, verified: bool },
}

#[derive(Clone, Debug, Default)]
pub struct Game {
    /// Subfolder name under `games/`.
    pub folder_name: String,
    pub dir: PathBuf,
    pub title: String,
    pub targets: Vec<Target>,
    pub save_paths: Vec<SavePath>,
    pub steamgriddb_id: Option<i64>,
    pub images: Images,
    pub notes: String,
    pub year: Option<u32>,
    /// Free text, e.g. `1`, `1-4`.
    pub players: String,
    pub description: String,
    /// Bytes of the game's own files (`.cartridge/` excluded).
    pub size: Option<u64>,
    pub install: Install,
    /// `.cartridge/game.json` exists on disk.
    pub has_cartridge: bool,
    /// In-memory data differs from disk.
    pub needs_persist: bool,
    /// Runtime-resolved, never persisted.
    pub resolved: Option<Resolved>,
    pub resolved_save_paths: Vec<PathBuf>,
    /// Transient: artwork to download on persist.
    pub artwork_urls: ArtworkUrls,
}

#[derive(Serialize)]
struct GameFileOut<'a> {
    schema_version: u32,
    title: &'a str,
    targets: &'a [Target],
    #[serde(rename = "savePaths")]
    save_paths: &'a [SavePath],
    images: &'a Images,
    #[serde(skip_serializing_if = "Option::is_none")]
    steamgriddb_id: Option<i64>,
    #[serde(skip_serializing_if = "str::is_empty")]
    notes: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    year: Option<u32>,
    #[serde(skip_serializing_if = "str::is_empty")]
    players: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    description: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GameFileIn {
    title: Value,
    targets: Value,
    #[serde(rename = "savePaths")]
    save_paths: Value,
    images: Value,
    steamgriddb_id: Value,
    notes: Value,
    year: Value,
    players: Value,
    description: Value,
    size: Value,
}

impl Game {
    /// A game folder without metadata yet (to be filled by detection).
    pub fn skeleton(dir: &Path) -> Game {
        let folder_name = folder_name_of(dir);
        Game {
            title: folder_name.clone(),
            folder_name,
            dir: dir.to_path_buf(),
            ..Default::default()
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        self.dir.join(data_dir_name())
    }

    pub fn game_json_path(&self) -> PathBuf {
        self.data_dir().join(GAME_JSON)
    }

    pub fn image_path(&self, slot: ImageSlot) -> Option<PathBuf> {
        self.images.get(slot).map(|name| self.data_dir().join(name))
    }

    /// Parse a `game.json` body for the game in `dir`.
    pub fn from_json(dir: &Path, text: &str) -> Result<Game, serde_json::Error> {
        let file: GameFileIn = serde_json::from_str(text)?;
        let folder_name = folder_name_of(dir);
        let title = file
            .title
            .as_str()
            .filter(|t| !t.is_empty())
            .map_or_else(|| folder_name.clone(), str::to_string);
        Ok(Game {
            title,
            folder_name,
            dir: dir.to_path_buf(),
            targets: entries(file.targets),
            save_paths: entries(file.save_paths),
            steamgriddb_id: as_id(&file.steamgriddb_id),
            images: serde_json::from_value(file.images).unwrap_or_default(),
            notes: file.notes.as_str().unwrap_or_default().to_string(),
            year: as_id(&file.year).and_then(|y| u32::try_from(y).ok()),
            players: scalar_text(file.players),
            description: scalar_text(file.description),
            size: as_id(&file.size).and_then(|s| u64::try_from(s).ok()),
            has_cartridge: true,
            ..Default::default()
        })
    }

    /// Serialize the persisted fields (4-space indent, like the Python writer).
    pub fn to_json(&self) -> String {
        to_pretty_json(&GameFileOut {
            schema_version: SCHEMA_VERSION,
            title: &self.title,
            targets: &self.targets,
            save_paths: &self.save_paths,
            images: &self.images,
            steamgriddb_id: self.steamgriddb_id,
            notes: &self.notes,
            year: self.year,
            players: &self.players,
            description: &self.description,
            size: self.size,
        })
    }

    pub fn is_installed(&self) -> bool {
        !matches!(self.install, Install::Packaged { .. })
    }

    pub fn package(&self) -> Option<&Path> {
        match &self.install {
            Install::Installed => None,
            Install::Packaged { package } | Install::InstalledWithPackage { package, .. } => {
                Some(package)
            }
        }
    }
}

/// The objects of a JSON list as `T` (anything else is skipped).
fn entries<T: serde::de::DeserializeOwned>(list: Value) -> Vec<T> {
    match list {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| serde_json::from_value(item).ok())
            .collect(),
        _ => Vec::new(),
    }
}

fn as_id(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse().ok())
}

pub fn folder_name_of(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// JSON with 4-space indentation.
pub fn to_pretty_json<T: Serialize>(value: &T) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut serializer)
        .expect("JSON serialization cannot fail");
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
    "schema_version": 2,
    "title": "Celeste",
    "targets": [
        {
            "os": "linux",
            "arch": "x64",
            "target": "Celeste.bin.x86_64",
            "startIn": ".",
            "launchOptions": ""
        }
    ],
    "savePaths": [
        {
            "os": "linux",
            "path": "~/.local/share/Celeste"
        }
    ],
    "images": {
        "cover": "cover.png",
        "hero": "hero.jpg"
    },
    "steamgriddb_id": 1234,
    "notes": "great"
}"#;

    #[test]
    fn round_trips_byte_identically() {
        let game = Game::from_json(Path::new("/g/Celeste"), SAMPLE).unwrap();
        assert_eq!(game.title, "Celeste");
        assert_eq!(game.targets[0].start_in, ".");
        assert_eq!(game.steamgriddb_id, Some(1234));
        assert_eq!(game.to_json(), SAMPLE);
    }

    #[test]
    fn tolerates_missing_and_null_fields() {
        let game = Game::from_json(
            Path::new("/g/Some Game"),
            r#"{"title": null, "targets": [{"os": null, "target": "run.sh"}], "images": {"cover": ""}}"#,
        )
        .unwrap();
        assert_eq!(game.title, "Some Game");
        assert_eq!(game.targets[0].os, "");
        assert_eq!(game.images.cover, None);
        assert!(game.save_paths.is_empty());
    }

    #[test]
    fn tolerates_wrong_types() {
        let game = Game::from_json(
            Path::new("/g/G"),
            r#"{"targets": [{"arch": 64, "launchOptions": 0, "target": "run"}, "junk"], "savePaths": {}, "steamgriddb_id": "12"}"#,
        )
        .unwrap();
        assert_eq!(game.targets.len(), 1);
        assert_eq!(
            (
                game.targets[0].arch.as_str(),
                game.targets[0].launch_options.as_str()
            ),
            ("64", "0")
        );
        assert!(game.save_paths.is_empty());
        assert_eq!(game.steamgriddb_id, Some(12));
    }

    #[test]
    fn omits_unset_optional_fields() {
        let game = Game::skeleton(Path::new("/g/X"));
        let json = game.to_json();
        assert!(!json.contains("steamgriddb_id"));
        assert!(!json.contains("notes"));
        assert!(json.contains("\"images\": {}"));
        assert!(!json.contains("year") && !json.contains("size"));
    }

    #[test]
    fn round_trips_info_fields() {
        let mut game = Game::skeleton(Path::new("/g/X"));
        game.year = Some(2018);
        game.players = "1-4".into();
        game.description = "A climb.".into();
        game.size = Some(123);
        let back = Game::from_json(Path::new("/g/X"), &game.to_json()).unwrap();
        assert_eq!(
            (
                back.year,
                back.players.as_str(),
                back.description.as_str(),
                back.size
            ),
            (Some(2018), "1-4", "A climb.", Some(123))
        );
    }

    #[test]
    fn info_fields_tolerate_loose_types() {
        let game = Game::from_json(
            Path::new("/g/X"),
            r#"{"year": "2018", "players": 2, "size": -1}"#,
        )
        .unwrap();
        assert_eq!(
            (game.year, game.players.as_str(), game.size),
            (Some(2018), "2", None)
        );
    }
}
