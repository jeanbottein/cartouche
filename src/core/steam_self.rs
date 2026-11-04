//! Pipeline step 6c: add the cartridge itself to Steam (`STEAM_ADD_SELF=True`,
//! independent of `STEAM_EXPOSE`). The shortcut is named after the cartridge
//! (its folder name), launches this app (or `STEAM_SELF_TARGET`) and starts
//! in the cartridge folder, so the app finds its config. Its artwork is
//! `system/cover.png`, `wide.png`, `hero.png`, `logo.png` and `icon.png` when
//! present (easy to swap), else the retro art embedded in the binary.
//!
//! It carries its own tag (`<ownership tag>-self`), not the ownership tag, so
//! the cleaner never treats it as a stale game. An existing entry (same tag or
//! same exe) keeps its AppID, so artwork and playtime stay attached.

use std::path::{Path, PathBuf};

use log::{info, warn};

use super::config::Config;
use super::paths;
use super::steam::{self, raw_exe, Owner};
use super::vdf::{Map, Value};

/// Each artwork slot: the file users can drop in `system/`, the art embedded
/// as a fallback (if any), and the Steam grid suffix it is installed under.
const ART: [(&str, Option<&[u8]>, &str); 5] = [
    (
        "cover.png",
        Some(include_bytes!("../../assets/steam-art/cover.png")),
        "p.png",
    ),
    (
        "wide.png",
        Some(include_bytes!("../../assets/steam-art/wide.png")),
        ".png",
    ),
    (
        "hero.png",
        Some(include_bytes!("../../assets/steam-art/hero.png")),
        "_hero.png",
    ),
    ("logo.png", None, "_logo.png"),
    (
        "icon.png",
        Some(include_bytes!("../../assets/app-icon.png")),
        "_icon.png",
    ),
];

fn self_tag(owner: &Owner) -> String {
    format!("{}-self", owner.tag)
}

/// What the shortcut launches: `STEAM_SELF_TARGET`, else this binary (or the
/// AppImage it runs from).
pub fn self_target(cfg: &Config) -> PathBuf {
    match cfg
        .get("STEAM_SELF_TARGET")
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(target) => paths::resolve(&paths::expand(target)),
        None => super::app::launched_file()
            .map(|p| paths::resolve(&p))
            .unwrap_or_default(),
    }
}

fn find_existing(shortcuts: &Map, exe: &str, tag: &str) -> Option<String> {
    let find = |matches: &dyn Fn(&Map) -> bool| {
        shortcuts
            .0
            .iter()
            .find(|(_, v)| steam::as_shortcut(v).is_some_and(matches))
    };
    let by_tag = find(&|s| steam::tag_values(s).contains(&tag));
    let by_exe = || find(&|s| steam::exe_of(s) == exe);
    by_tag.or_else(by_exe).map(|(k, _)| k.clone())
}

/// Write the artwork into the grid folder (only files that changed): the
/// user's file from `art_dir` when there is one, else the embedded art.
/// Returns the icon path.
fn install_art(grid: &Path, appid: u32, art_dir: &Path) -> String {
    if let Err(e) = std::fs::create_dir_all(grid) {
        warn!("    Cannot create {}: {e}", grid.display());
    }
    for (file, embedded, suffix) in ART {
        let custom = std::fs::read(art_dir.join(file)).ok();
        let Some(bytes) = custom.as_deref().or(embedded) else {
            continue;
        };
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
    steam::appname(existing) == steam::appname(entry)
        && raw_exe(existing) == raw_exe(entry)
        && existing.get_str("StartDir") == entry.get_str("StartDir")
        && existing.get_str("icon").unwrap_or("") == entry.get_str("icon").unwrap_or("")
        && tags(existing) == tags(entry)
}

/// What the self shortcut is: its name and tag, where it starts, where its art lives.
struct SelfShortcut<'a> {
    name: &'a str,
    tag: &'a str,
    exe: &'a str,
    start_dir: &'a str,
    art_dir: &'a Path,
}

/// Add or refresh the self shortcut for one Steam user. Returns true if `shortcuts.vdf` changed.
fn register_for_user(config_dir: &Path, me: &SelfShortcut) -> Result<bool, String> {
    let (name, exe) = (me.name, me.exe);
    let path = steam::shortcuts_path(config_dir);
    let mut shortcuts = steam::load_shortcuts(&path)?;
    let key = find_existing(&shortcuts, exe, me.tag);
    let existing = key.as_ref().and_then(|k| shortcuts.get_map(k)).cloned();
    let kept_appid = existing
        .as_ref()
        .and_then(|e| e.get_int("appid"))
        .map(|id| id as u32)
        .filter(|&id| id != 0);
    let appid = kept_appid.unwrap_or_else(|| steam::generate_appid(name, exe));
    let key = key.unwrap_or_else(|| steam::next_index(&shortcuts));

    let icon = install_art(&steam::grid_dir(config_dir), appid, me.art_dir);
    let mut entry = steam::make_shortcut(name, exe, me.start_dir, "", &icon, &[me.tag]);
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
        name
    );
    Ok(true)
}

/// Add/refresh the cartridge's own shortcut when `STEAM_ADD_SELF=True`.
pub fn register(cfg: &Config) {
    if !cfg.is_true("STEAM_ADD_SELF") {
        return;
    }
    let config_dirs = steam::select_config_dirs(cfg);
    if config_dirs.is_empty() {
        return;
    }
    let owner = Owner::of(cfg);
    let exe = self_target(cfg);
    // A path that exists: a binary/AppImage file or a macOS .app bundle.
    if !exe.exists() {
        warn!(
            "{} shortcut skipped: target not found ({})",
            owner.label,
            exe.display()
        );
        return;
    }
    let (exe, start_dir) = (paths::lossy(&exe), paths::lossy(&cfg.workspace()));
    let tag = self_tag(&owner);
    let art_dir = cfg.app_dir();
    let me = SelfShortcut {
        name: &owner.label,
        tag: &tag,
        exe: &exe,
        start_dir: &start_dir,
        art_dir: &art_dir,
    };
    let changed = config_dirs
        .iter()
        .filter(|dir| {
            register_for_user(dir, &me)
                .map_err(|e| warn!("  Skipping Steam user {}: {e}", steam::user_id(dir)))
                .unwrap_or(false)
        })
        .count();
    if changed == 0 {
        info!("{} shortcut already up to date", owner.label);
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
        let art_dir = dir.mkdir("ws/system");
        let me = |exe| SelfShortcut {
            name: "Potato-games",
            tag: "cartridge:1-self",
            exe,
            start_dir: "/ws",
            art_dir: &art_dir,
        };
        assert!(register_for_user(&config_dir, &me("/opt/cartridge")).unwrap());
        assert!(!register_for_user(&config_dir, &me("/opt/cartridge")).unwrap());

        let path = steam::shortcuts_path(&config_dir);
        let mut shortcuts = steam::load_shortcuts(&path).unwrap();
        let first = shortcuts.get_map("0").unwrap().clone();
        let mut played = first.clone();
        played.insert("LastPlayTime", Value::Int32(99));
        shortcuts.insert("0", Value::Map(played));
        steam::save_shortcuts(&path, shortcuts).unwrap();

        // Moving the binary updates the same entry.
        assert!(register_for_user(&config_dir, &me("/new/cartridge")).unwrap());
        let after = steam::load_shortcuts(&path).unwrap();
        assert_eq!(after.0.len(), 1);
        let entry = after.get_map("0").unwrap();
        assert_eq!(entry.get_int("appid"), first.get_int("appid"));
        assert_eq!(entry.get_int("LastPlayTime"), Some(99));
        assert_eq!(steam::exe_of(entry), "/new/cartridge");
        assert!(!steam::has_ownership_tag(entry, "cartridge:1"));
        assert_eq!(steam::appname(entry), "Potato-games");

        let appid = first.get_int("appid").unwrap() as u32;
        let grid = steam::grid_dir(&config_dir);
        assert!(grid.join(format!("{appid}_hero.png")).is_file());
        assert!(!grid.join(format!("{appid}_logo.png")).exists());
    }

    #[test]
    fn art_from_system_replaces_the_embedded_art() {
        let dir = TempDir::new();
        let art_dir = dir.mkdir("system");
        dir.write("system/hero.png", b"my hero");
        dir.write("system/logo.png", b"my logo");
        let grid = dir.mkdir("grid");
        install_art(&grid, 5, &art_dir);
        assert_eq!(std::fs::read(grid.join("5_hero.png")).unwrap(), b"my hero");
        assert_eq!(std::fs::read(grid.join("5_logo.png")).unwrap(), b"my logo");
        assert!(std::fs::read(grid.join("5p.png"))
            .unwrap()
            .starts_with(b"\x89PNG"));
    }
}
