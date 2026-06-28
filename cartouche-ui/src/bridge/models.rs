//! Serde models mirroring `.cartouche/game.json` (schema_version 2) — see the
//! Python `lib/models.py` / `lib/persister.py`. Field names use the JSON casing
//! (`startIn`, `launchOptions`, `savePaths`) via `rename`. Everything is optional
//! / defaulted so a partially-written file still parses.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GameJson {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub targets: Vec<Target>,
    #[serde(default, rename = "savePaths")]
    pub save_paths: Vec<SavePath>,
    #[serde(default)]
    pub images: Images,
    #[serde(default)]
    pub steamgriddb_id: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Target {
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub target: String,
    #[serde(default, rename = "startIn")]
    pub start_in: String,
    #[serde(default, rename = "launchOptions")]
    pub launch_options: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SavePath {
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Images {
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub hero: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub header: Option<String>,
}
