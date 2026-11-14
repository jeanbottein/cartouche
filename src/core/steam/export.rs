//! Pipeline step 6: create/update shortcuts and their artwork; add/remove one game.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use log::{error, info, warn};

use super::{
    appid_of, appname, as_shortcut, compat_tool, exe_of, games_in_steam, grid_dir,
    has_ownership_tag, load_shortcuts, make_shortcut, next_index, remove_owned, resolved_target,
    save_selection, save_shortcuts, select_config_dirs, set_compat_tools, shortcuts_path, signed32,
    user_id, CompatTool, Owner,
};
use crate::core::config::Config;
use crate::core::models::{Game, ImageSlot};
use crate::core::vdf::Value;
use crate::core::{paths, proton};

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

/// Copy `.cartridge/` artwork into Steam's grid folder under the AppID names.
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
    if let Some(name) = game.images.get(ImageSlot::Header) {
        install_wide_capsule(&game.data_dir().join(name), grid, appid);
    }
}

/// The wide capsule must be the wide header, never the vertical cover that
/// older versions copied under the same name: replace whatever sits there
/// unless it already is this file (same modification time).
fn install_wide_capsule(src: &Path, grid: &Path, appid: u32) {
    let dest = grid.join(format!("{appid}{}", extension(&src.to_string_lossy())));
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if !src.is_file() || (dest.is_file() && modified(&dest) == modified(src)) {
        return;
    }
    for other in ["png", "jpg", "jpeg", "webp"] {
        // Ignore failures: the file may not exist.
        let _ = std::fs::remove_file(grid.join(format!("{appid}.{other}")));
    }
    if let Err(e) = paths::copy_with_mtime(src, &dest) {
        warn!(
            "    Failed to copy artwork {} -> {}: {e}",
            src.display(),
            dest.display()
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
fn export_to_user(
    config_dir: &Path,
    games: &[(&Game, u32)],
    owner: &Owner,
) -> Result<(usize, usize), String> {
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
        let managed = managed_launch_options(game);
        let entry = |options: &str| {
            let mut entry = make_shortcut(
                &game.title,
                &target,
                &start_in,
                options,
                &icon,
                &[&owner.tag, &owner.label],
            );
            entry.insert("appid", Value::Int32(signed32(appid)));
            entry
        };

        // (key, launch options to keep, changed?) of the shortcut we already
        // own for this exe. Options set in Steam stay unless the game's own
        // Proton environment manages them.
        let owned = shortcuts.0.iter().find_map(|(k, v)| {
            let s = as_shortcut(v)
                .filter(|s| has_ownership_tag(s, &owner.tag) && exe_of(s) == target)?;
            let current = s.get_str("LaunchOptions").unwrap_or("");
            let options = managed.clone().unwrap_or_else(|| current.to_string());
            let wanted = entry(&options);
            let changed = appname(s) != game.title
                || options != current
                || ["appid", "icon", "StartDir", "tags"]
                    .iter()
                    .any(|key| s.get(key) != wanted.get(key));
            Some((k.clone(), options, changed))
        });
        match owned {
            Some((_, _, false)) => {}
            Some((key, options, true)) => {
                shortcuts.insert(&key, Value::Map(entry(&options)));
                updated += 1;
            }
            None => {
                let key = next_index(&shortcuts);
                let options = managed.unwrap_or_else(|| resolved.launch_options.clone());
                shortcuts.insert(&key, Value::Map(entry(&options)));
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

/// Launch options cartridge owns on a shortcut: those of a Windows game with
/// a Proton environment (`KEY=value %command% args`). Otherwise the
/// shortcut's own options are left as they are.
fn managed_launch_options(game: &Game) -> Option<String> {
    let resolved = game.resolved.as_ref().filter(|r| r.os == "windows")?;
    (!game.proton.env.is_empty())
        .then(|| proton::shortcut_launch_options(&game.proton.env, &resolved.launch_options))
}

/// Write shortcuts (and Proton for Windows games) for `with_targets` into
/// each Steam user's config. Returns (added, updated).
fn export_games(
    cfg: &Config,
    config_dirs: &[PathBuf],
    with_targets: &[(&Game, u32)],
) -> (usize, usize) {
    let owner = Owner::of(cfg);
    let counts = config_dirs
        .iter()
        .filter_map(|dir| {
            export_to_user(dir, with_targets, &owner)
                .map_err(|e| error!("  Skipping Steam user {}: {e}", user_id(dir)))
                .ok()
        })
        .fold((0, 0), |(a, u), (da, du)| (a + da, u + du));
    let windows: Vec<(&Game, u32)> = with_targets
        .iter()
        .filter(|(g, _)| g.resolved.as_ref().is_some_and(|r| r.os == "windows"))
        .copied()
        .collect();
    if windows.is_empty() {
        return counts;
    }
    let installed = proton::Installed::read();
    let wanted: Vec<(u32, CompatTool)> = windows
        .iter()
        .map(|&(game, appid)| (appid, compat_tool(game, installed.as_ref())))
        .collect();
    config_dirs.iter().for_each(|dir| {
        set_compat_tools(&wanted, dir);
    });
    counts
}

fn with_appid(game: &Game) -> Option<(&Game, u32)> {
    appid_of(game).map(|appid| (game, appid))
}

/// Pipeline step 6: create/update the shortcuts of the games the user put in
/// Steam, then set Proton for the Windows ones. Returns how many shortcuts
/// were added or updated.
pub fn export(games: &[Game], cfg: &Config) -> usize {
    let selected = games_in_steam(cfg, games);
    let with_targets = one_per_appid(
        games
            .iter()
            .filter(|g| selected.contains(&g.folder_name))
            .filter_map(with_appid)
            .collect(),
    );
    if with_targets.is_empty() {
        info!("No games to keep in Steam");
        return 0;
    }
    let (added, updated) = export_games(cfg, &select_config_dirs(cfg), &with_targets);
    if added + updated > 0 {
        info!("Steam export complete: {added} added, {updated} updated");
    } else {
        info!("Steam shortcuts already up to date");
    }
    added + updated
}

/// The other games of `library` with `game`'s AppID: a copied game folder
/// keeps its game.json, so its `steam_appid`. Only one of them can be in
/// Steam (two shortcuts with one AppID would share a Proton prefix).
fn duplicates_of<'a>(game: &Game, library: &'a [Game]) -> Vec<&'a Game> {
    let Some(appid) = appid_of(game) else {
        return Vec::new();
    };
    library
        .iter()
        .filter(|g| g.folder_name != game.folder_name && appid_of(g) == Some(appid))
        .collect()
}

/// The first game of each AppID; copies are left out of Steam (logged).
fn one_per_appid(games: Vec<(&Game, u32)>) -> Vec<(&Game, u32)> {
    let mut seen = HashSet::new();
    games
        .into_iter()
        .filter(|&(game, appid)| {
            let first = seen.insert(appid);
            if !first {
                warn!(
                    "{}: same Steam AppID as another game (a copy?), left out of Steam",
                    game.folder_name
                );
            }
            first
        })
        .collect()
}

/// Put one game in Steam now, and remember it. Its copies (same AppID) are
/// taken out: turning one on turns the others off.
pub fn add_game(cfg: &Config, library: &[Game], game: &Game) -> Result<(), String> {
    let entry = with_appid(game).ok_or("No executable found yet: Edit, Auto-detect targets")?;
    let config_dirs = select_config_dirs(cfg);
    if config_dirs.is_empty() {
        return Err("Steam not found on this machine".into());
    }
    let mut selected = games_in_steam(cfg, library);
    for copy in duplicates_of(game, library) {
        remove_shortcuts(cfg, copy)?;
        selected.remove(&copy.folder_name);
    }
    export_games(cfg, &config_dirs, &[entry]);
    selected.insert(game.folder_name.clone());
    save_selection(cfg, selected)
}

/// Take one game out of Steam now (only this cartridge's shortcut), and remember it.
pub fn remove_game(cfg: &Config, library: &[Game], game: &Game) -> Result<(), String> {
    let mut selected = games_in_steam(cfg, library);
    remove_shortcuts(cfg, game)?;
    selected.remove(&game.folder_name);
    save_selection(cfg, selected)
}

/// Remove this cartridge's shortcut of `game` for every Steam user.
fn remove_shortcuts(cfg: &Config, game: &Game) -> Result<(), String> {
    let Some(target) = resolved_target(game) else {
        return Ok(());
    };
    let owner = Owner::of(cfg);
    for dir in select_config_dirs(cfg) {
        remove_owned(&dir, &owner.tag, |exe| exe == target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Resolved;
    use crate::core::steam::*;

    /// A game folder `name` running `run`, with a stored AppID.
    fn installed(name: &str, appid: u32) -> Game {
        let mut game = Game::skeleton(&Path::new("/g").join(name));
        game.resolved = Some(Resolved {
            target: Path::new("/g").join(name).join("run"),
            ..Default::default()
        });
        game.steam_appid = Some(appid);
        game
    }

    #[test]
    fn copies_of_a_game_are_the_others_with_its_appid() {
        let library = [
            installed("Celeste", 7),
            installed("Celeste copy", 7),
            installed("Other", 8),
        ];
        let names: Vec<&str> = duplicates_of(&library[0], &library)
            .iter()
            .map(|g| g.folder_name.as_str())
            .collect();
        assert_eq!(names, ["Celeste copy"]);
        assert!(duplicates_of(&library[2], &library).is_empty());
    }

    #[test]
    fn only_the_first_game_of_an_appid_gets_a_shortcut() {
        let library = [installed("A", 7), installed("B", 7), installed("C", 8)];
        let kept: Vec<&str> = one_per_appid(library.iter().filter_map(with_appid).collect())
            .iter()
            .map(|(g, _)| g.folder_name.as_str())
            .collect();
        assert_eq!(kept, ["A", "C"]);
    }
    use crate::core::testutil::TempDir;

    #[test]
    fn wide_capsule_replaces_a_cover_copy_left_by_older_versions() {
        let dir = TempDir::new();
        let grid = dir.path().join("grid");
        std::fs::create_dir_all(&grid).unwrap();
        let header = dir.path().join("header.jpg");
        std::fs::write(&header, "wide").unwrap();
        std::fs::write(grid.join("7.png"), "vertical cover").unwrap();

        install_wide_capsule(&header, &grid, 7);

        assert!(!grid.join("7.png").exists());
        assert_eq!(std::fs::read_to_string(grid.join("7.jpg")).unwrap(), "wide");
        install_wide_capsule(&header, &grid, 7);
        assert_eq!(std::fs::read_to_string(grid.join("7.jpg")).unwrap(), "wide");
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
        let mut owner = Owner {
            tag: "cartridge:1".into(),
            label: "Potato-games".into(),
        };
        let export = |owner: &Owner, game: &Game| {
            export_to_user(&config_dir, &[(game, appid)], owner).unwrap()
        };

        assert_eq!(export(&owner, &game), (1, 0));
        assert_eq!(export(&owner, &game), (0, 0));
        game.title = "Celeste DX".into();
        assert_eq!(export(&owner, &game), (0, 1));
        // Renaming the cartridge folder updates the visible tag, same shortcut.
        owner.label = "Potatoes".into();
        assert_eq!(export(&owner, &game), (0, 1));
        let saved = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let entry = as_shortcut(saved.values().next().unwrap()).unwrap();
        assert_eq!(tag_values(entry), vec!["cartridge:1", "Potatoes"]);
        assert_eq!(
            load_shortcuts(&shortcuts_path(&config_dir))
                .unwrap()
                .0
                .len(),
            1
        );
    }

    #[test]
    fn a_renamed_game_keeps_its_stored_appid() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Celeste"));
        game.resolved = Some(Resolved {
            target: "/g/celeste".into(),
            ..Default::default()
        });
        game.steam_appid = appid_of(&game);
        let before = game.steam_appid.unwrap();
        game.title = "Celeste DX".into();
        assert_eq!(appid_of(&game), Some(before));

        let owner = Owner {
            tag: "cartridge:1".into(),
            label: "games".into(),
        };
        export_to_user(&config_dir, &[with_appid(&game).unwrap()], &owner).unwrap();
        let saved = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let entry = as_shortcut(saved.values().next().unwrap()).unwrap();
        assert_eq!(entry.get_int("appid"), Some(i64::from(signed32(before))));
        assert_eq!(appname(entry), "Celeste DX");
    }

    #[test]
    fn export_updates_a_changed_start_folder() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Celeste"));
        game.resolved = Some(Resolved {
            target: "/g/celeste".into(),
            start_in: "/g".into(),
            ..Default::default()
        });
        let owner = Owner {
            tag: "cartridge:1".into(),
            label: "games".into(),
        };
        let appid = generate_appid(&game.title, "/g/celeste");
        export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap();

        game.resolved.as_mut().unwrap().start_in = "/g/bin".into();
        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap(),
            (0, 1)
        );
        let saved = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let entry = as_shortcut(saved.values().next().unwrap()).unwrap();
        assert_eq!(entry.get_str("StartDir"), Some("\"/g/bin\""));
    }

    #[test]
    fn windows_shortcuts_carry_the_game_environment() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Rayman"));
        game.title = "Rayman".into();
        game.resolved = Some(Resolved {
            target: "/g/rayman.exe".into(),
            start_in: "/g".into(),
            launch_options: "-w".into(),
            os: "windows".into(),
        });
        let appid = generate_appid("Rayman", "/g/rayman.exe");
        let owner = Owner {
            tag: "cartridge:1".into(),
            label: "games".into(),
        };
        let options = || {
            let saved = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
            let entry = as_shortcut(saved.values().next().unwrap()).unwrap().clone();
            entry.get_str("LaunchOptions").unwrap().to_string()
        };

        export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap();
        assert_eq!(options(), "-w");
        game.proton
            .env
            .insert("PROTON_USE_WINED3D".into(), "1".into());
        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap(),
            (0, 1)
        );
        assert_eq!(options(), "PROTON_USE_WINED3D=1 %command% -w");
    }

    #[test]
    fn launch_options_set_in_steam_survive_updates() {
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
        let owner = Owner {
            tag: "cartridge:1".into(),
            label: "games".into(),
        };
        let path = shortcuts_path(&config_dir);
        export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap();
        let mut saved = load_shortcuts(&path).unwrap();
        if let Some((_, Value::Map(entry))) = saved.0.first_mut() {
            entry.insert("LaunchOptions", Value::Str("gamemoderun %command%".into()));
        }
        save_shortcuts(&path, saved).unwrap();

        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap(),
            (0, 0)
        );
        game.title = "Celeste DX".into();
        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap(),
            (0, 1)
        );
        let saved = load_shortcuts(&path).unwrap();
        let entry = as_shortcut(saved.values().next().unwrap()).unwrap();
        assert_eq!(
            entry.get_str("LaunchOptions"),
            Some("gamemoderun %command%")
        );
    }
}
