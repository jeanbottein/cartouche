//! Step 6b: the Proton build of Windows games' shortcuts, in Steam's `config.vdf`.

use std::path::Path;

use log::{error, info, warn};

use super::steam_root;
use crate::core::models::Game;
use crate::core::vdf::{self, Map, Value};
use crate::core::{paths, proton};

/// The Proton build for a Windows game's shortcut: the one its game.json
/// names when it is installed here, else the default (only set when the
/// shortcut has none). A build missing here is never forced on Steam.
pub(super) fn compat_tool(game: &Game, installed: Option<&proton::Installed>) -> CompatTool {
    let default = installed
        .and_then(proton::Installed::default_tool)
        .map_or_else(|| proton::FALLBACK_TOOL.to_string(), |t| t.name.clone());
    let pinned = game.proton.version.trim();
    if pinned.is_empty() {
        return CompatTool::Default(default);
    }
    match installed.and_then(|i| i.find(pinned)) {
        Some(tool) => CompatTool::Pinned(tool.name.clone()),
        None => {
            warn!(
                "{}: {pinned} is not installed, Steam keeps its build",
                game.title
            );
            CompatTool::Default(default)
        }
    }
}

// ── Step 6b: Proton for Windows games ────────────────────────────────────

const DEFAULT_COMPAT_PRIORITY: &str = "250";

/// The Proton build wanted for a Windows game's shortcut.
#[derive(Clone, Debug, PartialEq)]
pub enum CompatTool {
    /// The build used when the game names none: set only when the shortcut
    /// has none, so a build picked in Steam stays.
    Default(String),
    /// Named in the game's game.json: always set.
    Pinned(String),
}

/// Map AppIDs to their Proton build in `config.vdf`. Returns how many changed.
pub fn set_compat_tools(wanted: &[(u32, CompatTool)], config_dir: &Path) -> usize {
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
    let changes: Vec<(String, &str)> = wanted
        .iter()
        .filter_map(|(appid, tool)| {
            let id = appid.to_string();
            let current = mapping.get_map(&id).and_then(|m| m.get_str("name"));
            match tool {
                CompatTool::Default(_) if mapping.contains(&id) => None,
                CompatTool::Pinned(name) if current == Some(name.as_str()) => None,
                CompatTool::Default(name) | CompatTool::Pinned(name) => Some((id, name.as_str())),
            }
        })
        .collect();
    for (appid, tool) in &changes {
        let entry = [
            ("name", *tool),
            ("config", ""),
            ("priority", DEFAULT_COMPAT_PRIORITY),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), Value::Str(v.to_string())))
        .collect();
        mapping.insert(appid, Value::Map(Map(entry)));
    }
    if changes.is_empty() {
        return 0;
    }
    if let Err(e) = paths::write_atomic(&path, vdf::to_text(&data)) {
        error!("Failed to write {}: {e}", path.display());
        return 0;
    }
    info!(
        "Set Proton for {} Windows game(s) in config.vdf",
        changes.len()
    );
    changes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::steam::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn compat_tools_are_added_once_case_insensitively() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("Steam/userdata/1/config");
        let vdf_path = dir.write(
            "Steam/config/config.vdf",
            b"\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"Other\"\t\t\"1\"\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let default = |appid| (appid, CompatTool::Default("proton_9".into()));
        assert_eq!(
            set_compat_tools(&[default(123), default(456)], &config_dir),
            2
        );
        assert_eq!(set_compat_tools(&[default(123)], &config_dir), 0);
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

    #[test]
    fn a_game_pinned_build_replaces_the_shortcut_build() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("Steam/userdata/1/config");
        dir.write("Steam/config/config.vdf", b"\"InstallConfigStore\" { }");
        let default = [(123, CompatTool::Default("proton_9".into()))];
        let pinned = [(123, CompatTool::Pinned("proton_10".into()))];
        assert_eq!(set_compat_tools(&default, &config_dir), 1);
        assert_eq!(set_compat_tools(&pinned, &config_dir), 1);
        assert_eq!(set_compat_tools(&pinned, &config_dir), 0);
        assert_eq!(set_compat_tools(&default, &config_dir), 0);
    }

    fn windows_game_pinned(version: &str) -> Game {
        let mut game = Game::skeleton(Path::new("/g/Rayman"));
        game.proton.version = version.into();
        game
    }

    #[test]
    fn only_an_installed_pinned_build_is_forced_on_steam() {
        let steam = TempDir::new();
        steam.write("steamapps/common/Proton 10.0/proton", b"");
        let installed = proton::Installed::read_from(steam.path());
        let tool = |version| compat_tool(&windows_game_pinned(version), Some(&installed));
        assert_eq!(tool("Proton 10.0"), CompatTool::Pinned("proton_10".into()));
        assert_eq!(
            tool("GE-Proton9-20"),
            CompatTool::Default("proton_10".into())
        );
        assert_eq!(tool(""), CompatTool::Default("proton_10".into()));
        assert_eq!(
            compat_tool(&windows_game_pinned("proton_9"), None),
            CompatTool::Default(proton::FALLBACK_TOOL.into())
        );
    }
}
