//! Pipeline step 5: remove this cartridge's shortcuts for games no longer in Steam.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use log::{error, info};

use super::{
    appname, as_shortcut, exe_of, games_in_steam, has_ownership_tag, load_shortcuts, reindex,
    resolved_target, save_shortcuts, select_config_dirs, shortcuts_path, user_id, Owner,
};
use crate::core::config::Config;
use crate::core::models::Game;
use crate::core::paths;
use crate::core::vdf::Map;

// ── Step 5: remove stale shortcuts ───────────────────────────────────────

/// Remove our shortcuts whose executable matches `doomed`. Returns how many.
pub(super) fn remove_owned(
    config_dir: &Path,
    owner_tag: &str,
    doomed: impl Fn(&str) -> bool,
) -> Result<usize, String> {
    let path = shortcuts_path(config_dir);
    let shortcuts = load_shortcuts(&path)?;
    let (removed, kept): (Vec<_>, Vec<_>) = shortcuts.0.into_iter().partition(|(_, v)| {
        as_shortcut(v).is_some_and(|s| has_ownership_tag(s, owner_tag) && doomed(exe_of(s)))
    });
    if removed.is_empty() {
        return Ok(0);
    }
    for (_, shortcut) in &removed {
        info!(
            "  Removing shortcut: {}",
            as_shortcut(shortcut).map(appname).unwrap_or("")
        );
    }
    save_shortcuts(&path, reindex(Map(kept))).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(removed.len())
}

/// The shortcuts cleanup keeps: those of the games in Steam, and any in a
/// game folder the scan could not load (a game.json mid-write or with a
/// typo says nothing about whether the game is still wanted).
struct Kept {
    targets: HashSet<String>,
    unloaded_dirs: Vec<PathBuf>,
}

impl Kept {
    fn of(games: &[Game], selected: &BTreeSet<String>, games_dir: Option<&Path>) -> Kept {
        let loaded: HashSet<&Path> = games.iter().map(|g| g.dir.as_path()).collect();
        let unloaded_dirs = games_dir
            .map(paths::visible_subdirs)
            .unwrap_or_default()
            .into_iter()
            .filter(|dir| !loaded.contains(dir.as_path()))
            .collect();
        Kept {
            targets: games
                .iter()
                .filter(|g| selected.contains(&g.folder_name))
                .filter_map(resolved_target)
                .collect(),
            unloaded_dirs,
        }
    }

    fn is_stale(&self, exe: &str) -> bool {
        !self.targets.contains(exe)
            && !self
                .unloaded_dirs
                .iter()
                .any(|d| Path::new(exe).starts_with(d))
    }
}

fn remove_stale(config_dir: &Path, kept: &Kept, owner_tag: &str) -> Result<usize, String> {
    let removed = remove_owned(config_dir, owner_tag, |exe| kept.is_stale(exe))?;
    if removed > 0 {
        info!(
            "  Steam user {}: removed {removed} stale shortcut(s)",
            user_id(config_dir)
        );
    }
    Ok(removed)
}

/// Remove our shortcuts whose executable is no longer in the library;
/// returns how many were removed.
pub fn clean(games: &[Game], cfg: &Config) -> usize {
    let _files = super::files_lock();
    let kept = Kept::of(
        games,
        &games_in_steam(cfg, games),
        cfg.games_dir().as_deref(),
    );
    let owner = Owner::of(cfg);
    select_config_dirs(cfg)
        .iter()
        .filter_map(|config_dir| {
            remove_stale(config_dir, &kept, &owner.tag)
                .map_err(|e| error!("  Skipping Steam user {}: {e}", user_id(config_dir)))
                .ok()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Resolved;
    use crate::core::steam::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn stale_owned_shortcuts_are_removed_and_reindexed() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let shortcuts = Map(vec![
            ("0".into(), shortcut("Gone", "/g/gone", "cartridge:1")),
            ("1".into(), shortcut("Mine", "/g/kept", "favorite")),
            ("2".into(), shortcut("Kept", "/g/kept2", "cartridge:1")),
            ("3".into(), shortcut("Other", "/g/other", "cartridge:2")),
        ]);
        save_shortcuts(&shortcuts_path(&config_dir), shortcuts).unwrap();

        remove_stale(
            &config_dir,
            &Kept {
                targets: HashSet::from(["/g/kept2".to_string()]),
                unloaded_dirs: Vec::new(),
            },
            "cartridge:1",
        )
        .unwrap();
        let after = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let names: Vec<(&str, &str)> = after
            .0
            .iter()
            .map(|(k, v)| (k.as_str(), appname(as_shortcut(v).unwrap())))
            .collect();
        assert_eq!(names, vec![("0", "Mine"), ("1", "Kept"), ("2", "Other")]);
    }

    #[test]
    fn cleanup_keeps_shortcuts_of_game_folders_that_did_not_load() {
        let dir = TempDir::new();
        let games_dir = dir.mkdir("games");
        let loaded = dir.mkdir("games/Loaded");
        let broken = dir.mkdir("games/Broken");
        let mut game = Game::skeleton(&loaded);
        game.resolved = Some(Resolved {
            target: loaded.join("run"),
            ..Default::default()
        });
        let kept = Kept::of(
            &[game],
            &BTreeSet::from(["Loaded".into()]),
            Some(&games_dir),
        );
        let exe = |p: PathBuf| paths::lossy(&p);

        assert!(!kept.is_stale(&exe(loaded.join("run"))));
        assert!(!kept.is_stale(&exe(broken.join("run"))));
        assert!(kept.is_stale(&exe(loaded.join("old-target"))));
        assert!(kept.is_stale(&exe(games_dir.join("Deleted/run"))));
    }

    #[test]
    fn cleanup_removes_games_taken_out_of_steam() {
        let dir = TempDir::new();
        let games_dir = dir.mkdir("games");
        let folder = dir.mkdir("games/Out");
        let mut game = Game::skeleton(&folder);
        game.resolved = Some(Resolved {
            target: folder.join("run"),
            ..Default::default()
        });
        let kept = Kept::of(&[game], &BTreeSet::new(), Some(&games_dir));
        assert!(kept.is_stale(&paths::lossy(&folder.join("run"))));
    }

    #[test]
    fn removing_one_game_keeps_the_others() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let shortcuts = Map(vec![
            ("0".into(), shortcut("A", "/g/a", "cartridge:1")),
            ("1".into(), shortcut("B", "/g/b", "cartridge:1")),
            ("2".into(), shortcut("Mine", "/g/a", "favorite")),
        ]);
        save_shortcuts(&shortcuts_path(&config_dir), shortcuts).unwrap();
        assert_eq!(
            remove_owned(&config_dir, "cartridge:1", |exe| exe == "/g/a").unwrap(),
            1
        );
        let after = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let names: Vec<&str> = after
            .values()
            .filter_map(as_shortcut)
            .map(appname)
            .collect();
        assert_eq!(names, ["B", "Mine"]);
    }
}
