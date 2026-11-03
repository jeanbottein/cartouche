//! Pipeline step 0: idempotent data migrations, safe to run every time.
//!
//! * `launch_manifest`  — legacy `launch_manifest.json` → `.cartouche/game.json`
//! * `save_paths_format` — nested `savePaths` blocks → flat `{os, path}` list
//! * `arch_x64`          — target arch `x86_64` → `x64`

use std::path::{Path, PathBuf};

use log::{error, info, warn};
use serde_json::{json, Map, Value};

use super::app::data_dir_name;
use super::models::{folder_name_of, to_pretty_json, GAME_JSON};
use super::paths;

const OLD_MANIFEST: &str = "launch_manifest.json";
const MIGRATED_SUFFIX: &str = ".migrated";

fn game_json_of(game_dir: &Path) -> PathBuf {
    game_dir.join(data_dir_name()).join(GAME_JSON)
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| error!("Failed to read {}: {e}", path.display()))
        .ok()?;
    serde_json::from_str(&text)
        .map_err(|e| error!("Failed to read {}: {e}", path.display()))
        .ok()
}

fn write_json(path: &Path, value: &Value) -> bool {
    std::fs::write(path, to_pretty_json(value))
        .map_err(|e| error!("Failed to write {}: {e}", path.display()))
        .is_ok()
}

/// Rewrite one game's `game.json` with `change`; `change` returns false to skip.
fn rewrite_game_json(game_dir: &Path, change: impl Fn(&mut Map<String, Value>) -> bool) -> bool {
    let path = game_json_of(game_dir);
    if !path.exists() {
        return false;
    }
    let Some(Value::Object(mut data)) = read_json(&path) else {
        return false;
    };
    if !change(&mut data) {
        return false;
    }
    let written = write_json(&path, &Value::Object(data));
    if written {
        info!("Migrated {}", path.display());
    }
    written
}

// ── launch_manifest.json ─────────────────────────────────────────────────

fn str_field<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Old `savePath` (string or list of strings/objects) → flat `savePaths`.
fn convert_save_path(old: &Value) -> Vec<Value> {
    let entry_of = |e: &Value| match e {
        Value::String(s) => Some(json!({"os": "", "path": s})),
        Value::Object(_) => {
            let path = ["path", "savePath", "value"]
                .iter()
                .map(|k| str_field(e, k))
                .find(|p| !p.is_empty())
                .unwrap_or("");
            Some(json!({"os": str_field(e, "os"), "path": path}))
        }
        _ => None,
    };
    let entries: Vec<Value> = match old {
        Value::Array(list) => list.iter().filter_map(entry_of).collect(),
        Value::String(_) => entry_of(old).into_iter().collect(),
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .filter(|e| !str_field(e, "path").trim().is_empty())
        .collect()
}

fn build_game_json(old: &Value, game_dir: &Path) -> Value {
    let title = old
        .get("title")
        .cloned()
        .unwrap_or_else(|| json!(folder_name_of(game_dir)));
    let targets = match (old.get("targets"), old.get("target")) {
        (Some(targets), _) => targets.clone(),
        (None, Some(_)) => json!([{
            "os": str_field(old, "os"),
            "arch": str_field(old, "arch"),
            "target": str_field(old, "target"),
            "startIn": str_field(old, "startIn"),
            "launchOptions": str_field(old, "launchOptions"),
        }]),
        (None, None) => json!([]),
    };
    let mut game = json!({
        "title": title,
        "targets": targets,
        "savePaths": convert_save_path(old.get("savePath").unwrap_or(&Value::Null)),
        "images": {},
    });
    if let Some(id) = old.get("steamgriddb_id") {
        game["steamgriddb_id"] = id.clone();
    }
    game
}

fn migrate_manifest(game_dir: &Path) -> bool {
    let old_path = game_dir.join(OLD_MANIFEST);
    if !old_path.is_file() {
        return false;
    }
    let new_path = game_json_of(game_dir);
    let migrated_path = PathBuf::from(format!("{}{MIGRATED_SUFFIX}", old_path.display()));
    if new_path.is_file() {
        if !migrated_path.exists() {
            let _ = std::fs::rename(&old_path, &migrated_path);
        }
        return false;
    }
    let Some(old) = read_json(&old_path) else {
        return false;
    };
    if let Err(e) = std::fs::create_dir_all(new_path.parent().unwrap()) {
        error!("Failed to write {}: {e}", new_path.display());
        return false;
    }
    if !write_json(&new_path, &build_game_json(&old, game_dir)) {
        return false;
    }
    if let Err(e) = std::fs::rename(&old_path, &migrated_path) {
        warn!("Could not rename {}: {e}", old_path.display());
    }
    info!(
        "Migrated {}: {OLD_MANIFEST} -> {}/{GAME_JSON}",
        folder_name_of(game_dir),
        data_dir_name()
    );
    true
}

/// Folders holding a legacy manifest: the game folder and any nested folder.
fn manifest_dirs(dir: &Path) -> Vec<PathBuf> {
    let data_dir = data_dir_name();
    let nested = paths::sorted_entries(dir)
        .into_iter()
        .filter(|n| *n != data_dir)
        .map(|n| dir.join(n))
        .filter(|p| p.is_dir() && !paths::is_symlink(p))
        .flat_map(|p| manifest_dirs(&p));
    let own = dir.join(OLD_MANIFEST).is_file().then(|| dir.to_path_buf());
    own.into_iter().chain(nested).collect()
}

fn migrate_launch_manifests(games_dir: &Path) -> usize {
    let count = paths::visible_subdirs(games_dir)
        .iter()
        .flat_map(|game_dir| manifest_dirs(game_dir))
        .filter(|dir| migrate_manifest(dir))
        .count();
    if count > 0 {
        info!(
            "Migration complete: {count} game(s) migrated to {}/ format",
            data_dir_name()
        );
    }
    count
}

// ── savePaths format ─────────────────────────────────────────────────────

fn has_os_and_path(v: &Value) -> bool {
    v.get("os").is_some() && v.get("path").is_some()
}

fn flatten_save_paths(data: &mut Map<String, Value>) -> bool {
    let Some(Value::Array(entries)) = data.get("savePaths") else {
        return false;
    };
    if entries.is_empty() || has_os_and_path(&entries[0]) {
        return false;
    }
    let flat: Vec<Value> = entries
        .iter()
        .filter(|e| e.is_object())
        .flat_map(|e| {
            match e
                .get("paths")
                .and_then(Value::as_array)
                .filter(|p| !p.is_empty())
            {
                Some(nested) => nested
                    .iter()
                    .filter(|p| has_os_and_path(p))
                    .map(|p| json!({"os": p["os"], "path": p["path"]}))
                    .collect(),
                None if has_os_and_path(e) => vec![e.clone()],
                None => Vec::new(),
            }
        })
        .collect();
    if flat.is_empty() {
        return false;
    }
    data.insert("savePaths".into(), Value::Array(flat));
    true
}

// ── arch x86_64 → x64 ────────────────────────────────────────────────────

fn rename_x86_64(data: &mut Map<String, Value>) -> bool {
    let Some(Value::Array(targets)) = data.get_mut("targets") else {
        return false;
    };
    let legacy: Vec<&mut Value> = targets
        .iter_mut()
        .filter(|t| t.get("arch").and_then(Value::as_str) == Some("x86_64"))
        .collect();
    let changed = !legacy.is_empty();
    legacy.into_iter().for_each(|t| t["arch"] = json!("x64"));
    changed
}

fn migrate_each_game(games_dir: &Path, change: fn(&mut Map<String, Value>) -> bool) -> usize {
    paths::visible_subdirs(games_dir)
        .iter()
        .filter(|dir| rewrite_game_json(dir, change))
        .count()
}

/// Run every migration in order; returns (name, migrated count).
pub fn run_all(games_dir: &Path) -> Vec<(&'static str, usize)> {
    if !games_dir.is_dir() {
        return Vec::new();
    }
    let results = vec![
        ("launch_manifest", migrate_launch_manifests(games_dir)),
        (
            "save_paths_format",
            migrate_each_game(games_dir, flatten_save_paths),
        ),
        ("arch_x64", migrate_each_game(games_dir, rename_x86_64)),
    ];
    results
        .iter()
        .for_each(|(name, count)| info!("Migration '{name}': {count} item(s)"));
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    fn read(path: PathBuf) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn legacy_manifest_becomes_game_json() {
        let dir = TempDir::new();
        dir.write(
            "Game/launch_manifest.json",
            br#"{"title": "Old", "target": "run.sh", "os": "linux", "savePath": "~/saves", "steamgriddb_id": 5}"#,
        );
        assert_eq!(migrate_launch_manifests(dir.path()), 1);
        let game = read(dir.path().join("Game/.cartouche/game.json"));
        assert_eq!(game["targets"][0]["target"], "run.sh");
        assert_eq!(game["savePaths"], json!([{"os": "", "path": "~/saves"}]));
        assert_eq!(game["steamgriddb_id"], 5);
        assert!(dir
            .path()
            .join("Game/launch_manifest.json.migrated")
            .exists());
        assert_eq!(migrate_launch_manifests(dir.path()), 0);
    }

    #[test]
    fn nested_save_paths_are_flattened_once() {
        let dir = TempDir::new();
        dir.write(
            "Game/.cartouche/game.json",
            br#"{"title": "G", "savePaths": [{"name": "saves", "paths": [{"os": "linux", "path": "/a"}, {"os": "windows", "path": "C:/b"}]}]}"#,
        );
        assert_eq!(migrate_each_game(dir.path(), flatten_save_paths), 1);
        assert_eq!(
            read(dir.path().join("Game/.cartouche/game.json"))["savePaths"],
            json!([{"os": "linux", "path": "/a"}, {"os": "windows", "path": "C:/b"}])
        );
        assert_eq!(migrate_each_game(dir.path(), flatten_save_paths), 0);
    }

    #[test]
    fn x86_64_arch_becomes_x64_keeping_key_order() {
        let dir = TempDir::new();
        dir.write(
            "Game/.cartouche/game.json",
            br#"{"title": "G", "targets": [{"os": "linux", "arch": "x86_64", "target": "a"}]}"#,
        );
        assert_eq!(migrate_each_game(dir.path(), rename_x86_64), 1);
        let text = std::fs::read_to_string(dir.path().join("Game/.cartouche/game.json")).unwrap();
        assert!(text.find("\"title\"").unwrap() < text.find("\"targets\"").unwrap());
        assert!(text.contains("\"arch\": \"x64\""));
    }
}
