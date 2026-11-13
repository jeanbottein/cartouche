//! Mod file engine. Each folder under `mods/` with a `patch.json` lists files
//! to change in a game:
//! `[{"file": "fix.bps", "target": "Game/data.pak", "method": "patch"|"replace",
//!    "target_crc32": "1A2B3C4D", "patched_crc32": "..."}]`.
//! Targets are looked up in the cartridge's `games/` and the known game
//! install locations. Applying keeps the original as `<target>.backup`;
//! restoring puts it back. What to apply is decided by `core::mods`.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::bps;
use super::config::Config;
use super::paths;

const GAME_LOCATIONS: &str = include_str!("../../assets/games_locations.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Patch {
    pub file: String,
    pub target: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub target_crc32: Option<String>,
    #[serde(default)]
    pub patched_crc32: Option<String>,
}

impl Patch {
    /// The game folder a target lives in (`Game/data.pak` -> `Game`).
    pub fn game(&self) -> &str {
        self.target
            .split(['/', '\\'])
            .find(|c| !c.is_empty())
            .unwrap_or(&self.target)
    }
}

/// Where a file of a mod stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileState {
    /// The game's own file: the mod can be applied.
    Original,
    Applied,
    /// Neither the original nor the modded file (the game was updated).
    Mismatch,
    GameNotFound,
    /// The mod itself is broken: missing file, bad checksum value, unknown method.
    Broken,
}

/// The cartridge's `games/`, then the known install directories that exist here.
pub fn game_dirs(cfg: &Config) -> Vec<PathBuf> {
    let locations: Value =
        serde_json::from_str(GAME_LOCATIONS).expect("bundled games_locations.json is valid");
    let known = ["steam_directories", "other_game_directories"]
        .iter()
        .flat_map(|group| locations[group].as_array().cloned().unwrap_or_default())
        .filter_map(|v| v.as_str().map(paths::expand_vars))
        .flat_map(|p| {
            if p.contains('*') {
                paths::glob(&p)
            } else {
                vec![PathBuf::from(p)]
            }
        });
    cfg.games_dir()
        .into_iter()
        .chain(known)
        .filter(|p| p.exists())
        .collect()
}

/// The patched file, in the first game directory that has it.
pub fn locate(patch: &Patch, game_dirs: &[PathBuf]) -> Option<PathBuf> {
    game_dirs
        .iter()
        .map(|d| d.join(&patch.target))
        .find(|p| p.exists())
}

fn crc32_of(path: &Path) -> std::io::Result<u32> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(hasher.finalize());
        }
        hasher.update(&buf[..n]);
    }
}

fn parse_crc(hex: &Option<String>) -> Result<Option<u32>, ()> {
    match hex.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        None => Ok(None),
        Some(h) => u32::from_str_radix(h.trim_start_matches("0x").trim_start_matches("0X"), 16)
            .map(Some)
            .map_err(|_| ()),
    }
}

/// A target's size and checksum, read once per status check: targets can
/// be multi-GB game archives.
#[derive(Clone, Copy)]
struct Content {
    len: u64,
    crc: u32,
}

fn content_of(path: &Path) -> std::io::Result<Content> {
    Ok(Content {
        len: std::fs::metadata(path)?.len(),
        crc: crc32_of(path)?,
    })
}

/// Sizes first, so a file of another size is never read.
fn has_content(path: &Path, content: Content) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() == content.len)
        && crc32_of(path).is_ok_and(|crc| crc == content.crc)
}

pub fn backup_of(target: &Path) -> PathBuf {
    PathBuf::from(format!("{}.backup", target.display()))
}

/// Without a backup, or still equal to it: the game's own file.
fn is_untouched(target: &Path, actual: Content) -> bool {
    let backup = backup_of(target);
    !backup.exists() || has_content(&backup, actual)
}

fn replace_state(
    source: &Path,
    target: &Path,
    expected: Option<u32>,
    actual: Content,
) -> FileState {
    if has_content(source, actual) {
        FileState::Applied
    } else if expected == Some(actual.crc) || (expected.is_none() && is_untouched(target, actual)) {
        FileState::Original
    } else {
        FileState::Mismatch
    }
}

fn patch_state(
    target: &Path,
    expected: Option<u32>,
    patched: Option<u32>,
    actual: Content,
) -> FileState {
    if patched == Some(actual.crc) {
        return FileState::Applied;
    }
    match expected {
        Some(original) if original == actual.crc => FileState::Original,
        Some(_) => FileState::Mismatch,
        None if is_untouched(target, actual) => FileState::Original,
        // Modified since the backup, with no checksum to tell: assume ours.
        None if patched.is_none() => FileState::Applied,
        None => FileState::Mismatch,
    }
}

/// The state of one `patch.json` entry, given where its target is (if found).
pub fn state(patch: &Patch, folder: &Path, target: Option<&Path>) -> FileState {
    let source = folder.join(&patch.file);
    let (Ok(expected), Ok(patched)) = (
        parse_crc(&patch.target_crc32),
        parse_crc(&patch.patched_crc32),
    ) else {
        return FileState::Broken;
    };
    if !source.is_file() || !matches!(patch.method.as_str(), "patch" | "replace") {
        return FileState::Broken;
    }
    let Some(target) = target else {
        return FileState::GameNotFound;
    };
    let Ok(actual) = content_of(target) else {
        return FileState::Broken;
    };
    match patch.method.as_str() {
        "replace" => replace_state(&source, target, expected, actual),
        _ => patch_state(target, expected, patched, actual),
    }
}

/// Apply one entry to a target in `Original` state: back the original up
/// (refreshed, it may come from an older game version), then patch or replace.
pub fn apply(patch: &Patch, folder: &Path, target: &Path) -> Result<(), String> {
    let source = folder.join(&patch.file);
    let fail = |e: String| format!("{}: {e}", target.display());
    paths::copy_with_mtime(target, &backup_of(target)).map_err(|e| fail(e.to_string()))?;
    match patch.method.as_str() {
        "replace" => paths::copy_with_mtime(&source, target).map_err(|e| fail(e.to_string())),
        _ => apply_bps(&source, target).map_err(fail),
    }
}

/// Patch via a temporary file, then move it over the target.
fn apply_bps(patch_file: &Path, target: &Path) -> Result<(), String> {
    let patched = PathBuf::from(format!("{}.patched", target.display()));
    let result = std::fs::read(patch_file)
        .and_then(|patch| Ok((patch, std::fs::read(target)?)))
        .map_err(|e| e.to_string())
        .and_then(|(patch, source)| bps::apply(&patch, &source))
        .and_then(|out| std::fs::write(&patched, out).map_err(|e| e.to_string()))
        .and_then(|_| std::fs::rename(&patched, target).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_file(&patched);
    }
    result
}

/// Put the original back (the backup moves over the target).
pub fn restore(target: &Path) -> Result<(), String> {
    let backup = backup_of(target);
    if !backup.exists() {
        return Err(format!("{}: no backup to restore", target.display()));
    }
    std::fs::rename(&backup, target).map_err(|e| format!("{}: {e}", target.display()))
}

/// Folders under `root` (itself included) that contain a `patch.json`.
pub fn patch_folders(root: &Path) -> Vec<PathBuf> {
    let nested = paths::sorted_entries(root)
        .into_iter()
        .map(|n| root.join(n))
        .filter(|p| p.is_dir() && !paths::is_symlink(p))
        .flat_map(|p| patch_folders(&p));
    let own = root
        .join("patch.json")
        .is_file()
        .then(|| root.to_path_buf());
    own.into_iter().chain(nested).collect()
}

/// The entries of a mod folder's `patch.json`.
pub fn read_patches(folder: &Path) -> Result<Vec<Patch>, String> {
    let path = folder.join("patch.json");
    std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    const PATCH: &[u8] = include_bytes!("../../testdata/patch.bps");
    const SOURCE: &[u8] = include_bytes!("../../testdata/src.bin");
    const TARGET: &[u8] = include_bytes!("../../testdata/tgt.bin");

    fn patch(method: &str) -> Patch {
        Patch {
            file: "fix".into(),
            target: "Game/data".into(),
            method: method.into(),
            target_crc32: None,
            patched_crc32: None,
        }
    }

    #[test]
    fn bps_patch_applies_and_restores() {
        let dir = TempDir::new();
        let mods = dir.mkdir("mods/fix-it");
        dir.write("mods/fix-it/fix", PATCH);
        let target = dir.write("games/Game/data", SOURCE);
        let mut p = patch("patch");
        p.patched_crc32 = Some(format!("{:08X}", crc32fast::hash(TARGET)));

        assert_eq!(state(&p, &mods, Some(&target)), FileState::Original);
        apply(&p, &mods, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), TARGET);
        assert_eq!(std::fs::read(backup_of(&target)).unwrap(), SOURCE);
        assert_eq!(state(&p, &mods, Some(&target)), FileState::Applied);

        restore(&target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), SOURCE);
        assert!(!backup_of(&target).exists());
        assert_eq!(state(&p, &mods, Some(&target)), FileState::Original);
    }

    #[test]
    fn replacement_applies_and_restores() {
        let dir = TempDir::new();
        let mods = dir.mkdir("mods");
        dir.write("mods/fix", b"new");
        let target = dir.write("games/Game/data", b"old");
        let p = patch("replace");

        assert_eq!(state(&p, &mods, Some(&target)), FileState::Original);
        apply(&p, &mods, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(state(&p, &mods, Some(&target)), FileState::Applied);
        restore(&target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }

    #[test]
    fn an_updated_game_is_a_mismatch() {
        let dir = TempDir::new();
        let mods = dir.mkdir("mods");
        dir.write("mods/fix", b"new");
        let target = dir.write("games/Game/data", b"v2 of the game");
        let mut p = patch("replace");
        p.target_crc32 = Some("DEADBEEF".into());
        assert_eq!(state(&p, &mods, Some(&target)), FileState::Mismatch);
    }

    #[test]
    fn missing_games_and_broken_mods_are_told_apart() {
        let dir = TempDir::new();
        let mods = dir.mkdir("mods");
        dir.write("mods/fix", b"new");
        assert_eq!(
            state(&patch("replace"), &mods, None),
            FileState::GameNotFound
        );
        assert_eq!(state(&patch("teleport"), &mods, None), FileState::Broken);
        let mut bad_crc = patch("replace");
        bad_crc.target_crc32 = Some("zz".into());
        assert_eq!(state(&bad_crc, &mods, None), FileState::Broken);
        let mut no_file = patch("replace");
        no_file.file = "gone".into();
        assert_eq!(state(&no_file, &mods, None), FileState::Broken);
    }

    #[test]
    fn crc_values_accept_hex_prefix_and_spaces() {
        assert_eq!(
            parse_crc(&Some(" 0x1A2B3C4D ".into())),
            Ok(Some(0x1A2B3C4D))
        );
        assert_eq!(parse_crc(&Some("".into())), Ok(None));
        assert!(parse_crc(&Some("zz".into())).is_err());
    }

    #[test]
    fn finds_nested_patch_folders() {
        let dir = TempDir::new();
        dir.write("a/patch.json", b"[]");
        dir.write("b/c/patch.json", b"[]");
        dir.write("d/readme", b"");
        assert_eq!(
            patch_folders(dir.path()),
            vec![dir.path().join("a"), dir.path().join("b/c")]
        );
    }

    #[test]
    fn the_game_is_the_first_folder_of_the_target() {
        assert_eq!(patch("x").game(), "Game");
    }
}
