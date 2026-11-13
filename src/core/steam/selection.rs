//! Which games the user put in Steam (`system/steam.json`).

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use super::{
    as_shortcut, exe_of, has_ownership_tag, load_shortcuts, resolved_target, shortcuts_path,
    userdata_config_dirs, Owner,
};
use crate::core::config::Config;
use crate::core::models::{to_pretty_json, Game};
use crate::core::paths;

const SELECTION_FILE: &str = "steam.json";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Selection {
    /// Game folder names.
    games: BTreeSet<String>,
}

fn selection_path(cfg: &Config) -> PathBuf {
    cfg.app_dir().join(SELECTION_FILE)
}

fn load_selection(cfg: &Config) -> Option<BTreeSet<String>> {
    let text = std::fs::read_to_string(selection_path(cfg)).ok()?;
    serde_json::from_str::<Selection>(&text)
        .ok()
        .map(|s| s.games)
}

pub(super) fn save_selection(cfg: &Config, games: BTreeSet<String>) -> Result<(), String> {
    let path = selection_path(cfg);
    paths::write_atomic(&path, to_pretty_json(&Selection { games }))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The games that already have one of our shortcuts (first run, before
/// `steam.json` exists).
fn games_with_shortcuts(cfg: &Config, games: &[Game]) -> BTreeSet<String> {
    let owner = Owner::of(cfg);
    let exes: HashSet<String> = userdata_config_dirs()
        .iter()
        .filter_map(|dir| load_shortcuts(&shortcuts_path(dir)).ok())
        .flat_map(|shortcuts| {
            shortcuts
                .values()
                .filter_map(as_shortcut)
                .filter(|s| has_ownership_tag(s, &owner.tag))
                .map(|s| exe_of(s).to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    games
        .iter()
        .filter(|g| resolved_target(g).is_some_and(|t| exes.contains(&t)))
        .map(|g| g.folder_name.clone())
        .collect()
}

/// This cartridge's games the user put in Steam.
pub fn games_in_steam(cfg: &Config, games: &[Game]) -> BTreeSet<String> {
    load_selection(cfg).unwrap_or_else(|| games_with_shortcuts(cfg, games))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::testutil::TempDir;

    #[test]
    fn the_selection_is_remembered_in_system() {
        let dir = TempDir::new();
        dir.write("system/config.txt", b"");
        let cfg = Config::load(&dir.path().join("system/config.txt"));
        assert!(load_selection(&cfg).is_none());
        save_selection(&cfg, BTreeSet::from(["Celeste".to_string()])).unwrap();
        assert_eq!(
            games_in_steam(&cfg, &[]),
            BTreeSet::from(["Celeste".to_string()])
        );
    }
}
