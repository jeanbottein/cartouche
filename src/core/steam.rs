//! Steam integration: non-Steam shortcuts in `userdata/<id>/config/shortcuts.vdf`
//! (pipeline steps 5 and 6), their artwork in the `grid/` folder, and Proton
//! compatibility mappings in `config/config.vdf`.
//!
//! Shortcuts we own carry the ownership tag (the app name; the legacy
//! "gamer-sidekick" is still recognised), so other non-Steam games are never
//! touched.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use log::{error, info, warn};

use super::app::app_name;
use super::config::Config;
use super::models::{Game, ImageSlot};
use super::paths;
use super::vdf::{self, Map, Value};

pub const PROTON_PREFIX: &str = "~/.local/share/Steam/steamapps/compatdata/{appid}/pfx/drive_c";
const DEFAULT_COMPAT_PRIORITY: &str = "250";

pub fn proton_prefix(appid: u32) -> String {
    PROTON_PREFIX.replace("{appid}", &appid.to_string())
}

/// Stable non-Steam shortcut AppID (what Steam itself computes).
pub fn generate_appid(name: &str, exe: &str) -> u32 {
    crc32fast::hash(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

/// `shortcuts.vdf` stores AppIDs as signed 32-bit ints.
pub fn signed32(appid: u32) -> i32 {
    appid as i32
}

// ── Steam user directories ───────────────────────────────────────────────

/// Every `userdata/<id>/config` directory (deduplicated across symlinked roots).
pub fn userdata_config_dirs() -> Vec<PathBuf> {
    let home = paths::home_dir();
    let bases = [
        home.join(".steam/steam/userdata"),
        home.join(".local/share/Steam/userdata"),
    ];
    let mut seen = HashSet::new();
    bases
        .iter()
        .flat_map(|base| {
            paths::sorted_entries(base)
                .into_iter()
                .map(move |uid| base.join(uid).join("config"))
        })
        .filter(|dir| dir.is_dir() && seen.insert(paths::resolve(dir)))
        .collect()
}

/// User config dirs, narrowed to `STEAM_USERID` when set.
pub fn select_config_dirs(cfg: &Config) -> Vec<PathBuf> {
    let dirs = userdata_config_dirs();
    if dirs.is_empty() {
        warn!("No Steam userdata directories found");
        return dirs;
    }
    let Some(uid) = cfg
        .get("STEAM_USERID")
        .map(str::trim)
        .filter(|u| !u.is_empty())
    else {
        return dirs;
    };
    let needle = format!("/{uid}/");
    let filtered: Vec<PathBuf> = dirs
        .into_iter()
        .filter(|d| paths::lossy(d).contains(&needle))
        .collect();
    if filtered.is_empty() {
        warn!("STEAM_USERID={uid} not found");
    }
    filtered
}

/// User config dirs to sync games into; empty unless `STEAM_EXPOSE=True`.
pub fn exposed_config_dirs(cfg: &Config) -> Vec<PathBuf> {
    if cfg.is_true("STEAM_EXPOSE") {
        select_config_dirs(cfg)
    } else {
        Vec::new()
    }
}

pub fn user_id(config_dir: &Path) -> String {
    config_dir
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn shortcuts_path(config_dir: &Path) -> PathBuf {
    config_dir.join("shortcuts.vdf")
}

pub fn grid_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("grid")
}

// ── shortcuts.vdf ────────────────────────────────────────────────────────

/// The shortcut entries of a `shortcuts.vdf` (empty when the file is missing).
pub fn load_shortcuts(path: &Path) -> Result<Map, String> {
    if !path.exists() {
        return Ok(Map::default());
    }
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root = vdf::parse_binary(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(root.get_map("shortcuts").cloned().unwrap_or_default())
}

pub fn save_shortcuts(path: &Path, shortcuts: Map) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let root = Map(vec![("shortcuts".to_string(), Value::Map(shortcuts))]);
    std::fs::write(path, vdf::to_binary(&root))
}

fn as_shortcut(value: &Value) -> Option<&Map> {
    match value {
        Value::Map(m) => Some(m),
        _ => None,
    }
}

pub fn tag_values(shortcut: &Map) -> Vec<&str> {
    shortcut
        .get_map("tags")
        .map(|tags| {
            tags.values()
                .filter_map(|v| {
                    if let Value::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn has_ownership_tag(shortcut: &Map) -> bool {
    let owners = [app_name(), "cartouche", "gamer-sidekick"];
    tag_values(shortcut).iter().any(|t| owners.contains(t))
}

pub fn appname(shortcut: &Map) -> &str {
    [shortcut.get_str("AppName"), shortcut.get_str("appname")]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .unwrap_or("")
}

/// The raw `Exe` value (quoted, as Steam stores it).
pub fn raw_exe(shortcut: &Map) -> &str {
    shortcut
        .get_str("Exe")
        .or_else(|| shortcut.get_str("exe"))
        .unwrap_or("")
}

pub fn exe_of(shortcut: &Map) -> &str {
    raw_exe(shortcut).trim_matches('"')
}

pub fn next_index(shortcuts: &Map) -> String {
    shortcuts
        .keys()
        .filter_map(|k| k.parse::<u64>().ok())
        .max()
        .map_or(0, |m| m + 1)
        .to_string()
}

/// Renumber entries "0", "1", ... keeping their order.
fn reindex(shortcuts: Map) -> Map {
    let mut entries = shortcuts.0;
    entries.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(0));
    Map(entries
        .into_iter()
        .enumerate()
        .map(|(i, (_, v))| (i.to_string(), v))
        .collect())
}

/// A shortcut entry in the layout Steam writes.
pub fn make_shortcut(
    name: &str,
    exe: &str,
    start_dir: &str,
    launch_options: &str,
    icon: &str,
    tag: &str,
) -> Map {
    let int = Value::Int32;
    let text = |s: &str| Value::Str(s.to_string());
    let entries = vec![
        ("appid", int(signed32(generate_appid(name, exe)))),
        ("AppName", text(name)),
        ("Exe", text(&format!("\"{exe}\""))),
        ("StartDir", text(&format!("\"{start_dir}\""))),
        ("icon", text(icon)),
        ("ShortcutPath", text("")),
        ("LaunchOptions", text(launch_options)),
        ("IsHidden", int(0)),
        ("AllowDesktopConfig", int(1)),
        ("AllowOverlay", int(1)),
        ("OpenVR", int(0)),
        ("Devkit", int(0)),
        ("DevkitGameID", text("")),
        ("DevkitOverrideAppID", int(0)),
        ("LastPlayTime", int(0)),
        ("tags", Value::Map(Map(vec![("0".to_string(), text(tag))]))),
    ];
    Map(entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect())
}

/// Names of the shortcuts we own for one Steam user.
pub fn owned_shortcut_names(config_dir: &Path) -> Result<Vec<String>, String> {
    let shortcuts = load_shortcuts(&shortcuts_path(config_dir))?;
    Ok(shortcuts
        .values()
        .filter_map(as_shortcut)
        .filter(|s| has_ownership_tag(s))
        .map(|s| appname(s).to_string())
        .collect())
}

fn resolved_target(game: &Game) -> Option<String> {
    game.resolved.as_ref().map(|r| paths::lossy(&r.target))
}

// ── Step 5: remove stale shortcuts ───────────────────────────────────────

fn remove_stale(config_dir: &Path, valid_targets: &HashSet<String>) -> Result<(), String> {
    let path = shortcuts_path(config_dir);
    let shortcuts = load_shortcuts(&path)?;
    let (stale, kept): (Vec<_>, Vec<_>) = shortcuts.0.into_iter().partition(|(_, v)| {
        as_shortcut(v).is_some_and(|s| has_ownership_tag(s) && !valid_targets.contains(exe_of(s)))
    });
    if stale.is_empty() {
        return Ok(());
    }
    for (_, shortcut) in &stale {
        info!(
            "  Removing stale shortcut: {}",
            as_shortcut(shortcut).map(appname).unwrap_or("")
        );
    }
    save_shortcuts(&path, reindex(Map(kept))).map_err(|e| format!("{}: {e}", path.display()))?;
    info!(
        "  Steam user {}: removed {} stale shortcut(s)",
        user_id(config_dir),
        stale.len()
    );
    Ok(())
}

/// Remove our shortcuts whose executable is no longer in the library.
pub fn clean(games: &[Game], cfg: &Config) {
    let valid_targets: HashSet<String> = games.iter().filter_map(resolved_target).collect();
    for config_dir in exposed_config_dirs(cfg) {
        if let Err(e) = remove_stale(&config_dir, &valid_targets) {
            error!("  Skipping Steam user {}: {e}", user_id(&config_dir));
        }
    }
}

// ── Step 6: create / update shortcuts ────────────────────────────────────

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

fn copy_if_missing(src: &Path, dest: &Path) {
    if dest.is_file() || !src.is_file() {
        return;
    }
    if let Err(e) = paths::copy_with_mtime(src, dest) {
        warn!(
            "    Failed to copy artwork {} -> {}: {e}",
            src.display(),
            dest.display()
        );
    }
}

/// Copy `.cartouche/` artwork into Steam's grid folder under the AppID names.
fn copy_artwork(game: &Game, grid: &Path, appid: u32) {
    if let Err(e) = std::fs::create_dir_all(grid) {
        warn!("    Cannot create {}: {e}", grid.display());
        return;
    }
    let slots = [
        (ImageSlot::Cover, format!("{appid}p")),
        (ImageSlot::Hero, format!("{appid}_hero")),
        (ImageSlot::Logo, format!("{appid}_logo")),
        (ImageSlot::Icon, format!("{appid}_icon")),
        // The cover doubles as the horizontal grid image.
        (ImageSlot::Cover, appid.to_string()),
    ];
    for (slot, prefix) in slots {
        let Some(name) = game.images.get(slot) else {
            continue;
        };
        copy_if_missing(
            &game.data_dir().join(name),
            &grid.join(format!("{prefix}{}", extension(name))),
        );
    }
}

fn icon_path(game: &Game, grid: &Path, appid: u32) -> String {
    game.images
        .icon
        .as_deref()
        .map(|icon| paths::lossy(&grid.join(format!("{appid}_icon{}", extension(icon)))))
        .unwrap_or_default()
}

/// Add or refresh shortcuts for one Steam user. Returns (added, updated).
fn export_to_user(config_dir: &Path, games: &[(&Game, u32)]) -> Result<(usize, usize), String> {
    let path = shortcuts_path(config_dir);
    let grid = grid_dir(config_dir);
    let mut shortcuts = load_shortcuts(&path)?;
    let (mut added, mut updated) = (0, 0);

    for &(game, appid) in games {
        let Some(resolved) = &game.resolved else {
            continue;
        };
        let target = paths::lossy(&resolved.target);
        let start_in = paths::lossy(&resolved.start_in);
        let icon = icon_path(game, &grid, appid);
        let entry = || {
            make_shortcut(
                &game.title,
                &target,
                &start_in,
                &resolved.launch_options,
                &icon,
                app_name(),
            )
        };

        let owned_key = shortcuts
            .0
            .iter()
            .find(|(_, v)| {
                as_shortcut(v).is_some_and(|s| has_ownership_tag(s) && exe_of(s) == target)
            })
            .map(|(k, v)| (k.clone(), as_shortcut(v).cloned().unwrap_or_default()));
        match owned_key {
            Some((key, existing)) => {
                if appname(&existing) != game.title
                    || existing.get_str("icon").unwrap_or("") != icon
                {
                    shortcuts.insert(&key, Value::Map(entry()));
                    updated += 1;
                }
            }
            None => {
                let key = next_index(&shortcuts);
                shortcuts.insert(&key, Value::Map(entry()));
                added += 1;
            }
        }
        copy_artwork(game, &grid, appid);
    }

    if added + updated > 0 {
        save_shortcuts(&path, shortcuts).map_err(|e| format!("{}: {e}", path.display()))?;
        info!(
            "  Steam user {}: +{added} added, ~{updated} updated",
            user_id(config_dir)
        );
    }
    Ok((added, updated))
}

/// Create/update shortcuts for every game with a target, then set Proton for
/// Windows games.
pub fn export(games: &[Game], cfg: &Config) {
    let config_dirs = exposed_config_dirs(cfg);
    let with_targets: Vec<(&Game, u32)> = games
        .iter()
        .filter_map(|g| resolved_target(g).map(|t| (g, generate_appid(&g.title, &t))))
        .collect();
    if config_dirs.is_empty() || with_targets.is_empty() {
        return;
    }

    let (added, updated) = config_dirs
        .iter()
        .filter_map(|dir| {
            export_to_user(dir, &with_targets)
                .map_err(|e| error!("  Skipping Steam user {}: {e}", user_id(dir)))
                .ok()
        })
        .fold((0, 0), |(a, u), (da, du)| (a + da, u + du));
    if added + updated > 0 {
        info!("Steam export complete: {added} added, {updated} updated");
    } else {
        info!("Steam shortcuts already up to date");
    }

    let tool = cfg
        .get("PROTON_VERSION")
        .unwrap_or("proton_experimental")
        .trim()
        .to_string();
    let windows_appids: Vec<u32> = with_targets
        .iter()
        .filter(|(g, _)| g.resolved.as_ref().is_some_and(|r| r.os == "windows"))
        .map(|&(_, appid)| appid)
        .collect();
    if !windows_appids.is_empty() {
        config_dirs.iter().for_each(|dir| {
            set_compat_tools(&windows_appids, &tool, dir);
        });
    }
}

// ── Step 6b: Proton for Windows games ────────────────────────────────────

/// Steam root from a `userdata/<id>/config` path.
fn steam_root(config_dir: &Path) -> Option<PathBuf> {
    let root = config_dir.parent()?.parent()?.parent()?;
    root.join("config").is_dir().then(|| root.to_path_buf())
}

/// Map each AppID without a compat tool to `tool` in `config.vdf`. Returns how many were added.
pub fn set_compat_tools(appids: &[u32], tool: &str, config_dir: &Path) -> usize {
    let Some(root) = steam_root(config_dir) else {
        warn!(
            "Could not determine Steam root from {}",
            config_dir.display()
        );
        return 0;
    };
    let path = root.join("config").join("config.vdf");
    if !path.is_file() {
        warn!("config.vdf not found at {}/config/", root.display());
        return 0;
    }
    let mut data = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|t| vdf::parse_text(&t))
    {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to read {}: {e}", path.display());
            return 0;
        }
    };

    let mapping = [
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    ]
    .iter()
    .fold(&mut data, |map, key| map.map_entry_ignore_case(key));
    let missing: Vec<String> = appids
        .iter()
        .map(u32::to_string)
        .filter(|id| !mapping.contains(id))
        .collect();
    for appid in &missing {
        let entry = [
            ("name", tool),
            ("config", ""),
            ("priority", DEFAULT_COMPAT_PRIORITY),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), Value::Str(v.to_string())))
        .collect();
        mapping.insert(appid, Value::Map(Map(entry)));
    }
    if missing.is_empty() {
        return 0;
    }
    if let Err(e) = std::fs::write(&path, vdf::to_text(&data)) {
        error!("Failed to write {}: {e}", path.display());
        return 0;
    }
    info!(
        "Set {tool} for {} Windows game(s) in config.vdf",
        missing.len()
    );
    missing.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Resolved;
    use crate::core::testutil::TempDir;

    fn shortcut(name: &str, exe: &str, tag: &str) -> Value {
        Value::Map(make_shortcut(name, exe, "/", "", "", tag))
    }

    #[test]
    fn appid_matches_python_generate_appid() {
        assert_eq!(
            generate_appid("Hollow Knight", "/g/Hollow Knight/hollow_knight.exe"),
            3770780903
        );
        assert_eq!(
            generate_appid("Hollow Knight", "/g/HK/hk.x86_64"),
            2664893048
        );
        assert_eq!(signed32(2664893048), -1630074248);
    }

    #[test]
    fn new_shortcut_matches_python_layout() {
        let golden = vdf::parse_binary(include_bytes!("../../testdata/shortcuts.vdf")).unwrap();
        let expected = golden.get_map("shortcuts").unwrap().get_map("0").unwrap();
        let built = make_shortcut(
            "Hollow Knight",
            "/g/HK/hk.x86_64",
            "/g/HK",
            "-v",
            "/grid/1_icon.png",
            "golden",
        );
        assert_eq!(&built, expected);
    }

    #[test]
    fn ownership_is_by_tag() {
        let Value::Map(ours) = shortcut("A", "/a", "gamer-sidekick") else {
            unreachable!()
        };
        let Value::Map(theirs) = shortcut("B", "/b", "favorite") else {
            unreachable!()
        };
        assert!(has_ownership_tag(&ours));
        assert!(!has_ownership_tag(&theirs));
        assert_eq!(exe_of(&ours), "/a");
    }

    #[test]
    fn stale_owned_shortcuts_are_removed_and_reindexed() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let shortcuts = Map(vec![
            ("0".into(), shortcut("Gone", "/g/gone", "cartouche")),
            ("1".into(), shortcut("Mine", "/g/kept", "favorite")),
            ("2".into(), shortcut("Kept", "/g/kept2", "cartouche")),
        ]);
        save_shortcuts(&shortcuts_path(&config_dir), shortcuts).unwrap();

        remove_stale(&config_dir, &HashSet::from(["/g/kept2".to_string()])).unwrap();
        let after = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let names: Vec<(&str, &str)> = after
            .0
            .iter()
            .map(|(k, v)| (k.as_str(), appname(as_shortcut(v).unwrap())))
            .collect();
        assert_eq!(names, vec![("0", "Mine"), ("1", "Kept")]);
    }

    #[test]
    fn export_adds_once_then_updates_renamed_games() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Celeste"));
        game.title = "Celeste".into();
        game.resolved = Some(Resolved {
            target: "/g/celeste".into(),
            start_in: "/g".into(),
            ..Default::default()
        });
        let appid = generate_appid("Celeste", "/g/celeste");

        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)]).unwrap(),
            (1, 0)
        );
        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)]).unwrap(),
            (0, 0)
        );
        game.title = "Celeste DX".into();
        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)]).unwrap(),
            (0, 1)
        );
        assert_eq!(
            load_shortcuts(&shortcuts_path(&config_dir))
                .unwrap()
                .0
                .len(),
            1
        );
    }

    #[test]
    fn compat_tools_are_added_once_case_insensitively() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("Steam/userdata/1/config");
        let vdf_path = dir.write(
            "Steam/config/config.vdf",
            b"\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"Other\"\t\t\"1\"\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        assert_eq!(set_compat_tools(&[123, 456], "proton_9", &config_dir), 2);
        assert_eq!(set_compat_tools(&[123], "proton_9", &config_dir), 0);
        let data = vdf::parse_text(&std::fs::read_to_string(vdf_path).unwrap()).unwrap();
        let steam = data
            .get_map("InstallConfigStore")
            .unwrap()
            .get_map("Software")
            .unwrap()
            .get_map("valve")
            .unwrap()
            .get_map("Steam")
            .unwrap();
        assert_eq!(steam.get_str("Other"), Some("1"));
        let mapping = steam.get_map("CompatToolMapping").unwrap();
        assert_eq!(
            mapping.get_map("456").unwrap().get_str("name"),
            Some("proton_9")
        );
    }
}
