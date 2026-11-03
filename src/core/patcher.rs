//! Pipeline step 8: apply mods from `PATCHES_PATH`. Each folder with a
//! `patch.json` lists patches:
//! `[{"file": "fix.bps", "target": "Game/data.pak", "method": "patch"|"replace",
//!    "target_crc32": "1A2B3C4D", "patched_crc32": "..."}]`.
//! Targets are looked up under the known game install locations; the original
//! is kept as `<target>.backup`.

use std::io::Read;
use std::path::{Path, PathBuf};

use log::{error, info, warn};
use serde::Deserialize;
use serde_json::Value;

use super::bps;
use super::config::Config;
use super::paths;

const GAME_LOCATIONS: &str = include_str!("../../assets/games_locations.json");

#[derive(Deserialize)]
struct Patch {
    file: String,
    target: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    target_crc32: Option<String>,
    #[serde(default)]
    patched_crc32: Option<String>,
}

/// Known game install directories that exist on this machine.
fn game_dirs() -> Vec<PathBuf> {
    let locations: Value =
        serde_json::from_str(GAME_LOCATIONS).expect("bundled games_locations.json is valid");
    ["steam_directories", "other_game_directories"]
        .iter()
        .flat_map(|group| locations[group].as_array().cloned().unwrap_or_default())
        .filter_map(|v| v.as_str().map(paths::expand_vars))
        .flat_map(|p| {
            if p.contains('*') {
                paths::glob(&p)
            } else {
                vec![PathBuf::from(p)]
            }
        })
        .filter(|p| p.exists())
        .collect()
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

fn parse_crc(hex: &Option<String>, label: &str) -> Result<Option<u32>, ()> {
    match hex.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        None => Ok(None),
        Some(h) => u32::from_str_radix(h.trim_start_matches("0x").trim_start_matches("0X"), 16)
            .map(Some)
            .map_err(|_| error!("❌ Invalid {label} hex value: {h:?}")),
    }
}

fn same_content(a: &Path, b: &Path) -> bool {
    matches!((crc32_of(a), crc32_of(b)), (Ok(x), Ok(y)) if x == y)
}

fn backup_of(target: &Path) -> PathBuf {
    PathBuf::from(format!("{}.backup", target.display()))
}

fn replace_file(source: &Path, target: &Path) {
    match paths::copy_with_mtime(source, target) {
        Ok(()) => info!("✅ {} replaced", target.display()),
        Err(e) => error!("❌ Failed to replace {}: {e}", target.display()),
    }
}

fn ensure_backup(target: &Path, backup: &Path) -> bool {
    if backup.exists() {
        return true;
    }
    paths::copy_with_mtime(target, backup)
        .map_err(|e| error!("❌ Cannot back up {}: {e}", target.display()))
        .is_ok()
}

/// `replace` method: swap in the mod's file, keeping the original as backup.
fn apply_replacement(source: &Path, target: &Path) {
    let backup = backup_of(target);
    if !backup.exists() {
        if ensure_backup(target, &backup) {
            replace_file(source, target);
        }
    } else if same_content(target, &backup) {
        replace_file(source, target);
    } else if same_content(target, source) {
        info!("✅ {} already replaced", target.display());
    } else {
        error!(
            "❌ {} backup exists but target file differ from patch",
            target.display()
        );
    }
}

/// Patch via a temporary file, then move it over the target.
fn apply_bps(patch_file: &Path, target: &Path) {
    let patched = PathBuf::from(format!("{}.patched", target.display()));
    let result = std::fs::read(patch_file)
        .and_then(|patch| Ok((patch, std::fs::read(target)?)))
        .map_err(|e| e.to_string())
        .and_then(|(patch, source)| bps::apply(&patch, &source))
        .and_then(|out| std::fs::write(&patched, out).map_err(|e| e.to_string()))
        .and_then(|_| std::fs::rename(&patched, target).map_err(|e| e.to_string()));
    match result {
        Ok(()) => info!("✅ {} patched", target.display()),
        Err(e) => {
            error!("❌ BPS apply failed: {e}");
            let _ = std::fs::remove_file(&patched);
        }
    }
}

/// `patch` method: BPS-patch the target unless it was already modified.
fn apply_patch_method(patch: &Patch, source: &Path, target: &Path) {
    let backup = backup_of(target);
    if backup.exists() && !same_content(target, &backup) {
        let Ok(patched_crc) = parse_crc(&patch.patched_crc32, "patched_crc32") else {
            return;
        };
        if patched_crc.is_some() && patched_crc == crc32_of(target).ok() {
            info!("✅ {} already patched", target.display());
        } else {
            error!(
                "❌ {} backup exists but target file differ from patch",
                target.display()
            );
        }
        return;
    }
    if ensure_backup(target, &backup) {
        apply_bps(source, target);
    }
}

enum Status {
    AlreadyPatched,
    Ready,
    Mismatch,
}

fn check_status(target: &Path, patch: &Patch) -> Status {
    let (Ok(expected), Ok(patched)) = (
        parse_crc(&patch.target_crc32, "target_crc32"),
        parse_crc(&patch.patched_crc32, "patched_crc32"),
    ) else {
        return Status::Mismatch;
    };
    let actual = match crc32_of(target) {
        Ok(crc) => crc,
        Err(e) => {
            error!("❌ Cannot read {}: {e}", target.display());
            return Status::Mismatch;
        }
    };
    if patched == Some(actual) {
        return Status::AlreadyPatched;
    }
    if expected.is_none_or(|e| e == actual) {
        return Status::Ready;
    }
    warn!(
        "❌ CRC mismatch. Expected: {}, Got: {actual:08X}",
        patch.target_crc32.as_deref().unwrap_or("")
    );
    Status::Mismatch
}

fn process(patch: &Patch, folder: &Path, game_dirs: &[PathBuf]) {
    let source = folder.join(&patch.file);
    if !source.exists() {
        error!("❌ {} does not exist", source.display());
        return;
    }
    let Some(target) = game_dirs
        .iter()
        .map(|d| d.join(&patch.target))
        .find(|p| p.exists())
    else {
        info!("❌ {} not found", patch.target);
        return;
    };
    match check_status(&target, patch) {
        Status::AlreadyPatched => info!("✅ {} already patched", target.display()),
        Status::Mismatch => warn!(
            "❌ {}: skipping patch due to CRC mismatch",
            target.display()
        ),
        Status::Ready if patch.method == "replace" => apply_replacement(&source, &target),
        Status::Ready if patch.method == "patch" => apply_patch_method(patch, &source, &target),
        Status::Ready => warn!("❌ {}: unknown method {:?}", patch.target, patch.method),
    }
}

/// Folders under `root` (itself included) that contain a `patch.json`.
fn patch_folders(root: &Path) -> Vec<PathBuf> {
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

pub fn run(cfg: &Config) {
    let Some(root) = cfg.path_of("PATCHES_PATH").filter(|p| p.is_dir()) else {
        warn!("🤖 PATCHES_PATH not configured or invalid");
        return;
    };
    info!("🤖 Looking for patches in {}", root.display());
    let folders = patch_folders(&root);
    if folders.is_empty() {
        info!("ℹ️  No patch.json files found");
        return;
    }
    let game_dirs = game_dirs();
    for folder in folders {
        info!(
            "📦 Processing {}",
            paths::lossy(&paths::relpath(&folder, &root))
        );
        let json_path = folder.join("patch.json");
        let patches: Vec<Patch> = match std::fs::read_to_string(&json_path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to read {}: {e}", json_path.display());
                continue;
            }
        };
        patches.iter().for_each(|p| process(p, &folder, &game_dirs));
    }
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
    fn bps_patch_applies_once_with_backup() {
        let dir = TempDir::new();
        let mods = dir.mkdir("mods/fix-it");
        dir.write("mods/fix-it/fix", PATCH);
        let target = dir.write("games/Game/data", SOURCE);
        let game_dirs = vec![dir.path().join("games")];
        let mut p = patch("patch");
        p.patched_crc32 = Some(format!("{:08X}", crc32fast::hash(TARGET)));

        process(&p, &mods, &game_dirs);
        assert_eq!(std::fs::read(&target).unwrap(), TARGET);
        assert_eq!(std::fs::read(backup_of(&target)).unwrap(), SOURCE);
        process(&p, &mods, &game_dirs);
        assert_eq!(std::fs::read(&target).unwrap(), TARGET);
    }

    #[test]
    fn replacement_keeps_original_and_is_idempotent() {
        let dir = TempDir::new();
        let mods = dir.mkdir("mods");
        dir.write("mods/fix", b"new");
        let target = dir.write("games/Game/data", b"old");
        let game_dirs = vec![dir.path().join("games")];

        process(&patch("replace"), &mods, &game_dirs);
        process(&patch("replace"), &mods, &game_dirs);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(backup_of(&target)).unwrap(), b"old");
    }

    #[test]
    fn crc_mismatch_skips_the_patch() {
        let dir = TempDir::new();
        let mods = dir.mkdir("mods");
        dir.write("mods/fix", b"new");
        let target = dir.write("games/Game/data", b"old");
        let mut p = patch("replace");
        p.target_crc32 = Some("DEADBEEF".into());
        process(&p, &mods, &[dir.path().join("games")]);
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }

    #[test]
    fn crc_values_accept_hex_prefix_and_spaces() {
        assert_eq!(
            parse_crc(&Some(" 0x1A2B3C4D ".into()), "t"),
            Ok(Some(0x1A2B3C4D))
        );
        assert_eq!(parse_crc(&Some("".into()), "t"), Ok(None));
        assert!(parse_crc(&Some("zz".into()), "t").is_err());
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
}
