//! Mods the user controls: each folder under `mods/` with a `patch.json` is a
//! mod, turned on or off from the MODS tab (or by hand). On applies every file
//! of the mod (originals kept as backups), off restores them.
//!
//! `system/mods.json` remembers which mods the user turned on, so pipeline
//! step 8 (`run`) re-applies only those, e.g. after a game update replaced a
//! patched file. Without that file yet, whatever is applied counts as on.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use log::{info, warn};
use serde::{Deserialize, Serialize};

use super::config::Config;
use super::models::to_pretty_json;
use super::patcher::{self, FileState, Patch};
use super::{paths, unique};

const STATE_FILE: &str = "mods.json";

/// A mod as the MODS tab shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    On,
    Off,
    /// Some files applied, some not (e.g. a game update reverted one).
    Partial,
    GameNotFound,
    /// The game's files are neither original nor modded: probably updated.
    GameUpdated,
    /// The mod itself can't be used (bad patch.json, missing file...).
    Broken(String),
}

#[derive(Clone, Debug)]
pub struct Mod {
    /// Folder relative to `mods/`, with `/` separators.
    pub id: String,
    folder: PathBuf,
    /// Game folders the mod changes.
    pub games: Vec<String>,
    pub status: Status,
    /// The user turned it on (the pipeline keeps it applied).
    pub enabled: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    enabled: BTreeSet<String>,
}

fn state_path(cfg: &Config) -> PathBuf {
    cfg.system_dir().join(STATE_FILE)
}

fn load_state(cfg: &Config) -> Option<State> {
    let text = std::fs::read_to_string(state_path(cfg)).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_state(cfg: &Config, state: &State) -> Result<(), String> {
    paths::write_atomic(state_path(cfg), to_pretty_json(state)).map_err(|e| e.to_string())
}

/// Each entry with its target (if the game was found) and state.
fn files(
    patches: &[Patch],
    folder: &Path,
    game_dirs: &[PathBuf],
) -> Vec<(Patch, Option<PathBuf>, FileState)> {
    patches
        .iter()
        .map(|p| {
            let target = patcher::locate(p, game_dirs);
            let state = patcher::state(p, folder, target.as_deref());
            (p.clone(), target, state)
        })
        .collect()
}

fn status_of(states: &[FileState]) -> Status {
    let all = |s: FileState| states.iter().all(|&x| x == s);
    let any = |s: FileState| states.contains(&s);
    if states.is_empty() {
        Status::Broken("patch.json lists no files".into())
    } else if any(FileState::Broken) {
        Status::Broken("missing file, unknown method or bad checksum in patch.json".into())
    } else if any(FileState::GameNotFound) {
        Status::GameNotFound
    } else if any(FileState::Mismatch) {
        Status::GameUpdated
    } else if all(FileState::Applied) {
        Status::On
    } else if all(FileState::Original) {
        Status::Off
    } else {
        Status::Partial
    }
}

fn mod_id(root: &Path, folder: &Path) -> String {
    paths::lossy(&paths::relpath(folder, root)).replace('\\', "/")
}

/// Mod folders under `mods/`.
fn mod_folders(root: &Path) -> Vec<PathBuf> {
    patcher::patch_folders(root)
        .into_iter()
        .filter(|f| f != root)
        .collect()
}

fn read_mod(root: &Path, folder: PathBuf, game_dirs: &[PathBuf]) -> Mod {
    let id = mod_id(root, &folder);
    let (games, status) = match patcher::read_patches(&folder) {
        Ok(patches) => {
            let states: Vec<FileState> = files(&patches, &folder, game_dirs)
                .into_iter()
                .map(|(_, _, s)| s)
                .collect();
            let games = unique(patches.iter().map(|p| p.game().to_string()));
            (games, status_of(&states))
        }
        Err(e) => (Vec::new(), Status::Broken(e)),
    };
    Mod {
        id,
        folder,
        games,
        status,
        enabled: false,
    }
}

/// Every mod under `mods/`, with its status and whether the user turned it on.
pub fn list(cfg: &Config) -> Vec<Mod> {
    let root = cfg.mods_dir();
    if !root.is_dir() {
        return Vec::new();
    }
    let game_dirs = patcher::game_dirs(cfg);
    let mods: Vec<Mod> = mod_folders(&root)
        .into_iter()
        .map(|f| read_mod(&root, f, &game_dirs))
        .collect();
    let enabled = load_state(cfg).map(|s| s.enabled);
    mods.into_iter()
        .map(|m| Mod {
            enabled: enabled
                .as_ref()
                .map_or(m.status == Status::On, |set| set.contains(&m.id)),
            ..m
        })
        .collect()
}

/// The folder of mod `id`, without checking any mod's files.
fn find(cfg: &Config, id: &str) -> Result<PathBuf, String> {
    let root = cfg.mods_dir();
    mod_folders(&root)
        .into_iter()
        .find(|f| mod_id(&root, f) == id)
        .ok_or_else(|| format!("No mod {id}"))
}

/// Apply every file still original (others are already applied).
fn apply_all(cfg: &Config, folder: &Path) -> Result<(), String> {
    let patches = patcher::read_patches(folder)?;
    for (patch, target, state) in files(&patches, folder, &patcher::game_dirs(cfg)) {
        match (state, target) {
            (FileState::Applied, _) => {}
            (FileState::Original, Some(target)) => patcher::apply(&patch, folder, &target)?,
            (FileState::GameNotFound, _) => {
                return Err(format!("{}: game not found", patch.target))
            }
            (FileState::Mismatch, _) => {
                return Err(format!(
                    "{}: the game's file changed (updated?)",
                    patch.target
                ))
            }
            _ => return Err(format!("{}: the mod is broken", patch.target)),
        }
    }
    Ok(())
}

/// The enabled set, starting from what is applied when there's no file yet.
fn current_enabled(cfg: &Config) -> BTreeSet<String> {
    load_state(cfg).map(|s| s.enabled).unwrap_or_else(|| {
        list(cfg)
            .into_iter()
            .filter(|m| m.enabled)
            .map(|m| m.id)
            .collect()
    })
}

fn remember(cfg: &Config, id: &str, on: bool) -> Result<(), String> {
    let mut enabled = current_enabled(cfg);
    if on {
        enabled.insert(id.to_string());
    } else {
        enabled.remove(id);
    }
    save_state(cfg, &State { enabled })
}

static FILES: Mutex<()> = Mutex::new(());

/// Held while applying or restoring mod files: the MODS screen and the
/// autopilot may both do it.
fn files_lock() -> MutexGuard<'static, ()> {
    FILES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Turn a mod on: apply it, and keep it applied in later pipeline runs.
pub fn enable(cfg: &Config, id: &str) -> Result<(), String> {
    let _files = files_lock();
    apply_all(cfg, &find(cfg, id)?)?;
    remember(cfg, id, true)
}

/// Turn a mod off: restore the originals of the files it applied.
pub fn disable(cfg: &Config, id: &str) -> Result<(), String> {
    let _files = files_lock();
    let folder = find(cfg, id)?;
    let patches = patcher::read_patches(&folder)?;
    for (_, target, state) in files(&patches, &folder, &patcher::game_dirs(cfg)) {
        if let (FileState::Applied, Some(target)) = (state, target) {
            patcher::restore(&target)?;
        }
    }
    remember(cfg, id, false)
}

/// Pipeline step 8: keep the mods the user turned on applied.
pub fn run(cfg: &Config) {
    let _files = files_lock();
    let wanted: Vec<Mod> = list(cfg).into_iter().filter(|m| m.enabled).collect();
    if load_state(cfg).is_none() && !wanted.is_empty() {
        let enabled = wanted.iter().map(|m| m.id.clone()).collect();
        let _ = save_state(cfg, &State { enabled });
    }
    if wanted.is_empty() {
        info!("No mods turned on");
        return;
    }
    for m in wanted {
        if m.status == Status::On {
            info!("  {}: on", m.id);
            continue;
        }
        match apply_all(cfg, &m.folder) {
            Ok(()) => info!("  {}: applied again", m.id),
            Err(e) => warn!("  {}: {e}", m.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    /// A cartridge with one replace-mod targeting games/Game/data.
    fn cartridge(dir: &TempDir) -> Config {
        dir.write("system/config.txt", b"");
        dir.write("games/Game/data", b"old");
        dir.write("mods/Better/new", b"new");
        dir.write(
            "mods/Better/patch.json",
            br#"[{"file": "new", "target": "Game/data", "method": "replace"}]"#,
        );
        Config::load(&dir.path().join("system/config.txt"))
    }

    #[test]
    fn mods_turn_on_and_off_and_remember_it() {
        let dir = TempDir::new();
        let cfg = cartridge(&dir);
        let data = dir.path().join("games/Game/data");

        let m = &list(&cfg)[0];
        assert_eq!(
            (m.id.as_str(), &m.status, m.enabled),
            ("Better", &Status::Off, false)
        );
        assert_eq!(m.games, ["Game"]);

        enable(&cfg, "Better").unwrap();
        assert_eq!(std::fs::read(&data).unwrap(), b"new");
        assert_eq!(list(&cfg)[0].status, Status::On);
        assert!(list(&cfg)[0].enabled);

        disable(&cfg, "Better").unwrap();
        assert_eq!(std::fs::read(&data).unwrap(), b"old");
        assert_eq!(list(&cfg)[0].status, Status::Off);
        assert!(!list(&cfg)[0].enabled);
    }

    #[test]
    fn the_pipeline_reapplies_only_enabled_mods() {
        let dir = TempDir::new();
        let cfg = cartridge(&dir);
        let data = dir.path().join("games/Game/data");
        enable(&cfg, "Better").unwrap();
        // A game update puts the original file back (and drops the backup).
        std::fs::write(&data, b"old").unwrap();
        std::fs::remove_file(patcher::backup_of(&data)).unwrap();
        run(&cfg);
        assert_eq!(std::fs::read(&data).unwrap(), b"new");

        disable(&cfg, "Better").unwrap();
        run(&cfg);
        assert_eq!(std::fs::read(&data).unwrap(), b"old");
    }

    #[test]
    fn without_a_state_file_applied_mods_count_as_on() {
        let dir = TempDir::new();
        let cfg = cartridge(&dir);
        std::fs::write(dir.path().join("games/Game/data.backup"), b"old").unwrap();
        std::fs::write(dir.path().join("games/Game/data"), b"new").unwrap();
        assert!(list(&cfg)[0].enabled);
    }

    #[test]
    fn missing_games_and_broken_mods_are_reported() {
        let dir = TempDir::new();
        let cfg = cartridge(&dir);
        std::fs::remove_dir_all(dir.path().join("games/Game")).unwrap();
        dir.write("mods/Bad/patch.json", b"not json");
        let mods = list(&cfg);
        assert!(matches!(mods[0].status, Status::Broken(_)));
        assert_eq!(mods[1].status, Status::GameNotFound);
        assert!(enable(&cfg, "Better").is_err());
    }
}
