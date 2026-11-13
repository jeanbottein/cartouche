//! `shortcuts.vdf`: reading, writing, ownership tags, building entries.

use std::path::Path;

use super::{generate_appid, shortcuts_path, signed32};
use crate::core::app::APP_NAME;
use crate::core::config::Config;
use crate::core::models::Game;
use crate::core::paths;
use crate::core::vdf::{self, Map, Value};

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
    paths::write_atomic(path, vdf::to_binary(&root))
}

pub fn as_shortcut(value: &Value) -> Option<&Map> {
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

/// Which shortcuts are this cartridge's: its id tag, plus its label shown in
/// Steam as a tag (a collection). The id never changes with renames or moves.
pub struct Owner {
    pub tag: String,
    pub label: String,
}

impl Owner {
    pub fn of(cfg: &Config) -> Owner {
        Owner {
            tag: format!("{APP_NAME}:{}", cfg.cartridge_id()),
            label: cfg.label(),
        }
    }
}

pub fn has_ownership_tag(shortcut: &Map, owner_tag: &str) -> bool {
    tag_values(shortcut).contains(&owner_tag)
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
pub fn reindex(shortcuts: Map) -> Map {
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
    tags: &[&str],
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
        (
            "tags",
            Value::Map(Map(tags
                .iter()
                .enumerate()
                .map(|(i, tag)| (i.to_string(), text(tag)))
                .collect())),
        ),
    ];
    Map(entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect())
}

/// Names of the shortcuts this cartridge owns for one Steam user.
pub fn owned_shortcut_names(config_dir: &Path, owner_tag: &str) -> Result<Vec<String>, String> {
    let shortcuts = load_shortcuts(&shortcuts_path(config_dir))?;
    Ok(shortcuts
        .values()
        .filter_map(as_shortcut)
        .filter(|s| has_ownership_tag(s, owner_tag))
        .map(|s| appname(s).to_string())
        .collect())
}

pub(super) fn resolved_target(game: &Game) -> Option<String> {
    game.resolved.as_ref().map(|r| paths::lossy(&r.target))
}

/// A minimal shortcut, for tests across the Steam modules.
#[cfg(test)]
pub(super) fn shortcut(name: &str, exe: &str, tag: &str) -> Value {
    Value::Map(make_shortcut(name, exe, "/", "", "", &[tag]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_shortcut_matches_python_layout() {
        let golden = vdf::parse_binary(include_bytes!("../../../testdata/shortcuts.vdf")).unwrap();
        let expected = golden.get_map("shortcuts").unwrap().get_map("0").unwrap();
        let built = make_shortcut(
            "Hollow Knight",
            "/g/HK/hk.x86_64",
            "/g/HK",
            "-v",
            "/grid/1_icon.png",
            &["golden"],
        );
        assert_eq!(&built, expected);
    }

    #[test]
    fn ownership_is_by_tag() {
        let Value::Map(ours) = shortcut("A", "/a", "cartridge:1") else {
            unreachable!()
        };
        let Value::Map(other_cartridge) = shortcut("B", "/b", "cartridge:2") else {
            unreachable!()
        };
        assert!(has_ownership_tag(&ours, "cartridge:1"));
        assert!(!has_ownership_tag(&other_cartridge, "cartridge:1"));
        assert_eq!(exe_of(&ours), "/a");
    }
}
