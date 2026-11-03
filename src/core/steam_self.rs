//! Pipeline step 6c: add Cartouche itself to Steam (`STEAM_ADD_SELF=True`,
//! independent of `STEAM_EXPOSE`). The shortcut is named `.cartouche`, gets
//! the retro artwork embedded in the binary, launches this binary (or
//! `STEAM_SELF_TARGET`) and starts in the workspace root, so the app finds its
//! config.
//!
//! It carries its own tag (`cartouche-self`), not the ownership tag, so the
//! cleaner never treats it as a stale game. An existing entry (same tag or same
//! exe) keeps its AppID, so artwork and playtime stay attached.

use std::path::{Path, PathBuf};

use log::{info, warn};

use super::app::app_name;
use super::config::Config;
use super::paths;
use super::steam::{self, raw_exe};
use super::vdf::{Map, Value};

/// Embedded artwork and the Steam grid suffix it is installed under.
const ART: [(&[u8], &str); 4] = [
    (include_bytes!("../../assets/steam-art/cover.png"), "p.png"),
    (include_bytes!("../../assets/steam-art/wide.png"), ".png"),
    (
        include_bytes!("../../assets/steam-art/hero.png"),
        "_hero.png",
    ),
    (include_bytes!("../../assets/app-icon.png"), "_icon.png"),
];

fn self_tag() -> String {
    format!("{}-self", app_name())
}

fn self_name() -> String {
    format!(".{}", app_name())
}

/// What the shortcut launches: `STEAM_SELF_TARGET`, else this binary.
pub fn self_target(cfg: &Config) -> PathBuf {
    match cfg
        .get("STEAM_SELF_TARGET")
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(target) => paths::resolve(&paths::expand(target)),
        None => std::env::current_exe()
            .map(|p| paths::resolve(&p))
            .unwrap_or_default(),
    }
}

fn find_existing(shortcuts: &Map, exe: &str) -> Option<String> {
    let tag = self_tag();
    let shortcut_of = |v: &Value| {
        if let Value::Map(m) = v {
            Some(m.clone())
        } else {
            None
        }
    };
    let by_tag = shortcuts.0.iter().find(|(_, v)| {
        shortcut_of(v).is_some_and(|s| steam::tag_values(&s).contains(&tag.as_str()))
    });
    let by_exe = || {
        shortcuts
            .0
            .iter()
            .find(|(_, v)| shortcut_of(v).is_some_and(|s| steam::exe_of(&s) == exe))
    };
    by_tag.or_else(by_exe).map(|(k, _)| k.clone())
}

/// Write the artwork into the grid folder (only files that changed). Returns the icon path.
fn install_art(grid: &Path, appid: u32) -> String {
    if let Err(e) = std::fs::create_dir_all(grid) {
        warn!("    Cannot create {}: {e}", grid.display());
    }
    for (bytes, suffix) in ART {
        let dest = grid.join(format!("{appid}{suffix}"));
        if std::fs::read(&dest).is_ok_and(|current| current == bytes) {
            continue;
        }
        if let Err(e) = std::fs::write(&dest, bytes) {
            warn!("    Failed to write artwork {}: {e}", dest.display());
        }
    }
    paths::lossy(&grid.join(format!("{appid}_icon.png")))
}

fn is_unchanged(existing: &Map, entry: &Map) -> bool {
    let tags = |m: &Map| m.get("tags").cloned();
    steam::appname(existing) == self_name()
        && raw_exe(existing) == raw_exe(entry)
        && existing.get_str("StartDir") == entry.get_str("StartDir")
        && existing.get_str("icon").unwrap_or("") == entry.get_str("icon").unwrap_or("")
        && tags(existing) == tags(entry)
}

/// Add or refresh the self shortcut for one Steam user. Returns true if `shortcuts.vdf` changed.
fn register_for_user(config_dir: &Path, exe: &str, start_dir: &str) -> Result<bool, String> {
    let path = steam::shortcuts_path(config_dir);
    let mut shortcuts = steam::load_shortcuts(&path)?;
    let key = find_existing(&shortcuts, exe);
    let existing = key.as_ref().and_then(|k| shortcuts.get_map(k)).cloned();
    let kept_appid = existing
        .as_ref()
        .and_then(|e| e.get_int("appid"))
        .map(|id| id as u32)
        .filter(|&id| id != 0);
    let appid = kept_appid.unwrap_or_else(|| steam::generate_appid(&self_name(), exe));
    let key = key.unwrap_or_else(|| steam::next_index(&shortcuts));

    let icon = install_art(&steam::grid_dir(config_dir), appid);
    let mut entry = steam::make_shortcut(&self_name(), exe, start_dir, "", &icon, &self_tag());
    entry.insert("appid", Value::Int32(steam::signed32(appid)));
    if let Some(existing) = &existing {
        entry.insert(
            "LastPlayTime",
            existing
                .get("LastPlayTime")
                .cloned()
                .unwrap_or(Value::Int32(0)),
        );
        if is_unchanged(existing, &entry) {
            return Ok(false);
        }
    }

    shortcuts.insert(&key, Value::Map(entry));
    steam::save_shortcuts(&path, shortcuts).map_err(|e| format!("{}: {e}", path.display()))?;
    let verb = if existing.is_some() {
        "updated"
    } else {
        "added"
    };
    info!(
        "  Steam user {}: {verb} {} -> {exe}",
        steam::user_id(config_dir),
        self_name()
    );
    Ok(true)
}

/// Add/refresh the Cartouche shortcut when `STEAM_ADD_SELF=True`.
pub fn register(cfg: &Config) {
    if !cfg.is_true("STEAM_ADD_SELF") {
        return;
    }
    let config_dirs = steam::select_config_dirs(cfg);
    if config_dirs.is_empty() {
        return;
    }
    let exe = self_target(cfg);
    if !exe.is_file() {
        warn!(
            "{} shortcut skipped: target not found ({})",
            self_name(),
            exe.display()
        );
        return;
    }
    let exe = paths::lossy(&exe);
    let start_dir = paths::lossy(&cfg.workspace());
    let changed = config_dirs
        .iter()
        .filter(|dir| {
            register_for_user(dir, &exe, &start_dir)
                .map_err(|e| warn!("  Skipping Steam user {}: {e}", steam::user_id(dir)))
                .unwrap_or(false)
        })
        .count();
    if changed == 0 {
        info!("{} shortcut already up to date", self_name());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn registers_once_and_keeps_appid_and_playtime() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/7/config");
        assert!(register_for_user(&config_dir, "/opt/cartouche", "/ws").unwrap());
        assert!(!register_for_user(&config_dir, "/opt/cartouche", "/ws").unwrap());

        let path = steam::shortcuts_path(&config_dir);
        let mut shortcuts = steam::load_shortcuts(&path).unwrap();
        let first = shortcuts.get_map("0").unwrap().clone();
        let mut played = first.clone();
        played.insert("LastPlayTime", Value::Int32(99));
        shortcuts.insert("0", Value::Map(played));
        steam::save_shortcuts(&path, shortcuts).unwrap();

        // Moving the binary updates the same entry.
        assert!(register_for_user(&config_dir, "/new/cartouche", "/ws").unwrap());
        let after = steam::load_shortcuts(&path).unwrap();
        assert_eq!(after.0.len(), 1);
        let entry = after.get_map("0").unwrap();
        assert_eq!(entry.get_int("appid"), first.get_int("appid"));
        assert_eq!(entry.get_int("LastPlayTime"), Some(99));
        assert_eq!(steam::exe_of(entry), "/new/cartouche");
        assert!(!steam::has_ownership_tag(entry));

        let appid = first.get_int("appid").unwrap() as u32;
        assert!(steam::grid_dir(&config_dir)
            .join(format!("{appid}_hero.png"))
            .is_file());
    }
}
