//! Application identity and the `system/` workspace directory.
//!
//! The app name comes from the binary filename — everything before the first
//! `-` or `.` — so `cartouche-linux-x86_64` is still "cartouche", and renaming
//! the binary rebrands the app (Steam tags, window title). Folder names don't
//! follow the name: every game keeps its metadata in `.cartouche/` (a format,
//! shared between machines and packages) and the workspace is `system/`.
//! In an AppImage the name comes from the `.AppImage` file instead (the inner
//! binary is always `cartouche`), lowercased so `Cartouche.AppImage` stays
//! "cartouche".

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn app_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        name_from(
            std::env::var_os("APPIMAGE").map(PathBuf::from),
            std::env::current_exe().ok(),
        )
    })
}

/// Everything before the first `-` or `.` of the launched file's name.
fn name_from(appimage: Option<PathBuf>, exe: Option<PathBuf>) -> String {
    let stem_of = |path: &Path| {
        let file = path.file_name()?.to_string_lossy().into_owned();
        let stem = file.split(['-', '.']).next()?.to_string();
        (!stem.is_empty()).then_some(stem)
    };
    let from_appimage = appimage
        .as_deref()
        .and_then(stem_of)
        .map(|s| s.to_lowercase());
    from_appimage
        .or_else(|| exe.as_deref().and_then(stem_of))
        .unwrap_or_else(|| "cartouche".into())
}

/// Per-game metadata folder (`<game>/.cartouche/game.json`). Fixed: it is
/// part of the game format, whatever the app is called.
pub const GAME_DATA_DIR: &str = ".cartouche";

/// The workspace folder next to the app: `system/config.txt`, caches, thumbnails.
pub const WORKSPACE_DIR: &str = "system";

/// Marks a `system/` folder as ours even before `config.txt` exists (e.g.
/// freshly unpacked from a release archive).
pub const WORKSPACE_README: &str = "README.txt";
const README_SIGNATURE: &str = "Cartouche";

pub fn data_dir_name() -> String {
    GAME_DATA_DIR.to_string()
}

/// The file the user launched: the `.AppImage` when running from one (its
/// runtime sets `APPIMAGE`), else the binary itself. Inside an AppImage the
/// binary lives in a temporary read-only mount, useless for portable files or
/// a Steam shortcut.
pub fn launched_file() -> Option<PathBuf> {
    launched_from(std::env::var_os("APPIMAGE"), std::env::current_exe().ok())
}

fn launched_from(appimage: Option<std::ffi::OsString>, exe: Option<PathBuf>) -> Option<PathBuf> {
    appimage
        .filter(|a| !a.is_empty())
        .map(PathBuf::from)
        .or(exe)
}

/// Directory holding the launched file (portable mode: the workspace sits
/// next to the binary or the AppImage).
pub fn exe_dir() -> PathBuf {
    launched_file()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// False on Linux without X11/Wayland (e.g. over SSH), where a window can't open.
pub fn has_display() -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|var| std::env::var_os(var).is_some_and(|v| !v.is_empty()))
}

/// `system/` is ours: it has a `config.txt`, or our README. A `system/` folder
/// that merely exists (a common name) is never taken over.
pub fn is_workspace(dir: &Path) -> bool {
    let readme = std::fs::read_to_string(dir.join(WORKSPACE_README)).unwrap_or_default();
    dir.join("config.txt").is_file() || readme.starts_with(README_SIGNATURE)
}

/// A pre-`system/` workspace: `.cartouche/` (or `.<app name>/` from a renamed
/// binary) with a `config.txt` and no `game.json` (so not a game's own folder).
fn legacy_workspace(parent: &Path) -> Option<PathBuf> {
    [GAME_DATA_DIR.to_string(), format!(".{}", app_name())]
        .into_iter()
        .map(|name| parent.join(name))
        .find(|p| p.join("config.txt").is_file() && !p.join("game.json").exists())
}

/// The workspace in `parent`, moving a legacy `.cartouche/` to `system/` once.
/// The legacy folder is kept as is when an unrelated `system/` exists.
fn workspace_in(parent: &Path) -> Option<PathBuf> {
    let system = parent.join(WORKSPACE_DIR);
    if is_workspace(&system) {
        return Some(system);
    }
    let legacy = legacy_workspace(parent)?;
    if system.exists() {
        log::warn!(
            "Keeping {}: {} exists and isn't a Cartouche workspace",
            legacy.display(),
            system.display()
        );
        return Some(legacy);
    }
    match std::fs::rename(&legacy, &system) {
        Ok(()) => {
            log::info!(
                "Moved workspace {} -> {}",
                legacy.display(),
                system.display()
            );
            Some(system)
        }
        Err(e) => {
            log::warn!(
                "Cannot move {} to {}: {e}",
                legacy.display(),
                system.display()
            );
            Some(legacy)
        }
    }
}

/// Locate the workspace dir (`system/`). First match wins:
/// 1. `cli_dir/system/` (explicit argument)
/// 2. next to the binary / AppImage (portable mode)
/// 3. the current directory
pub fn find_app_dir(cli_dir: Option<&Path>) -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    cli_dir
        .map(Path::to_path_buf)
        .into_iter()
        .chain([exe_dir(), cwd])
        .find_map(|dir| workspace_in(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn system_counts_only_with_config_or_our_readme() {
        let dir = TempDir::new();
        let system = dir.mkdir("system");
        assert!(!is_workspace(&system));
        dir.write("system/README.txt", b"Some other project");
        assert!(!is_workspace(&system));
        dir.write("system/README.txt", b"Cartouche workspace: settings...");
        assert!(is_workspace(&system));
        let other = dir.mkdir("other/system");
        dir.write("other/system/config.txt", b"");
        assert!(is_workspace(&other));
    }

    #[test]
    fn a_legacy_workspace_moves_to_system_once() {
        let dir = TempDir::new();
        dir.write(".cartouche/config.txt", b"FREEGAMES_PATH=games\n");
        let found = workspace_in(dir.path()).unwrap();
        assert_eq!(found, dir.path().join("system"));
        assert!(found.join("config.txt").is_file());
        assert!(!dir.path().join(".cartouche").exists());
        assert_eq!(workspace_in(dir.path()), Some(found));
    }

    #[test]
    fn an_unrelated_system_folder_is_never_merged() {
        let dir = TempDir::new();
        dir.write(".cartouche/config.txt", b"");
        dir.write("system/notes.txt", b"not ours");
        assert_eq!(
            workspace_in(dir.path()),
            Some(dir.path().join(".cartouche"))
        );
        assert!(dir.path().join("system/notes.txt").is_file());
    }

    #[test]
    fn a_game_data_folder_is_not_a_workspace() {
        let dir = TempDir::new();
        dir.write(".cartouche/config.txt", b"");
        dir.write(".cartouche/game.json", b"{}");
        assert_eq!(workspace_in(dir.path()), None);
        assert!(!dir.path().join("system").exists());
    }

    #[test]
    fn an_appimage_stands_in_for_the_mounted_binary() {
        let exe = Some(PathBuf::from("/tmp/.mount_abc/usr/bin/cartouche"));
        assert_eq!(
            launched_from(Some("/games/Cartouche.AppImage".into()), exe.clone()),
            Some(PathBuf::from("/games/Cartouche.AppImage"))
        );
        assert_eq!(launched_from(None, exe.clone()), exe);
        assert_eq!(launched_from(Some("".into()), exe.clone()), exe);
    }

    #[test]
    fn the_name_follows_the_launched_file() {
        let name = |appimage: Option<&str>, exe: &str| {
            name_from(appimage.map(PathBuf::from), Some(PathBuf::from(exe)))
        };
        // Plain binary: exact case, as before.
        assert_eq!(name(None, "/g/cartouche-linux-x86_64"), "cartouche");
        assert_eq!(name(None, "/g/MyGames"), "MyGames");
        // AppImage: its file name, lowercased; the inner binary is ignored.
        let inner = "/tmp/.mount_x/usr/bin/cartouche";
        assert_eq!(name(Some("/g/Cartouche.AppImage"), inner), "cartouche");
        assert_eq!(
            name(Some("/g/cartouche-linux-x86_64.AppImage"), inner),
            "cartouche"
        );
        assert_eq!(name(Some("/g/MyGames.AppImage"), inner), "mygames");
        // Nothing usable: the default.
        assert_eq!(name_from(None, None), "cartouche");
        assert_eq!(name(Some("/g/.AppImage"), inner), "cartouche");
    }
}
