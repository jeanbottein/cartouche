//! Application identity, the cartridge folder and its `system/` workspace.
//!
//! The app is `cartridge`, shown as `•cartridge` ("dot cartridge"): every game
//! keeps its metadata in a `.cartridge/` folder, and a portable folder holding
//! the app, `system/` and the games is a cartridge. Names are fixed; renaming
//! the binary, the app bundle or the folder changes nothing but the label,
//! which is the cartridge folder's name.

use std::path::{Path, PathBuf};

/// Identifier: binary, Steam ownership prefix, user agent.
pub const APP_NAME: &str = "cartridge";
/// How the app is written in text: "dot cartridge".
pub const DISPLAY_NAME: &str = "•cartridge";

/// The macOS bundle holding `exe` (`X.app/Contents/MacOS/exe`), if any.
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    is_bundle.then(|| bundle.to_path_buf())
}

/// macOS runs an unsigned, freshly downloaded app from a hidden read-only
/// copy ("App Translocation"): nothing next to it is visible or writable.
pub fn is_translocated(path: &Path) -> bool {
    path.to_string_lossy().contains("/AppTranslocation/")
}

/// Per-game metadata folder (`<game>/.cartridge/game.json`): part of the game
/// and package format.
pub const GAME_DATA_DIR: &str = ".cartridge";

/// The workspace folder next to the app: `system/config.txt`, caches, thumbnails.
pub const WORKSPACE_DIR: &str = "system";

/// The cartridge's fixed folders, next to `system/` (no settings: symlink
/// one elsewhere if needed).
pub const GAMES_DIR: &str = "games";
pub const MODS_DIR: &str = "mods";
pub const SAVES_COPIES_DIR: &str = "saves-copies";
pub const SAVES_LINKS_DIR: &str = "saves-links";
pub const FOLDERS: [&str; 4] = [GAMES_DIR, MODS_DIR, SAVES_COPIES_DIR, SAVES_LINKS_DIR];

/// Marks a `system/` folder as ours even before `config.txt` exists (e.g.
/// freshly unpacked from a release archive).
pub const WORKSPACE_README: &str = "README.txt";
const README_SIGNATURE: &str = DISPLAY_NAME;
/// The cartridge's stable identity, in `system/`: Steam shortcuts are owned by
/// this id, so renaming or moving the folder never orphans them, and two
/// cartridges on one machine don't clean up each other's games.
const CARTRIDGE_ID_FILE: &str = "cartridge-id";

pub fn data_dir_name() -> String {
    GAME_DATA_DIR.to_string()
}

/// The file the user launched: the `.AppImage` when running from one (its
/// runtime sets `APPIMAGE`), the `.app` bundle on macOS, else the binary
/// itself. Inside an AppImage the binary lives in a temporary read-only mount,
/// and inside a bundle in `Contents/MacOS/`: useless for portable files or a
/// Steam shortcut.
pub fn launched_file() -> Option<PathBuf> {
    launched_from(std::env::var_os("APPIMAGE"), std::env::current_exe().ok())
}

fn launched_from(appimage: Option<std::ffi::OsString>, exe: Option<PathBuf>) -> Option<PathBuf> {
    appimage
        .filter(|a| !a.is_empty())
        .map(PathBuf::from)
        .or_else(|| exe.map(|e| app_bundle(&e).unwrap_or(e)))
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

fn workspace_in(parent: &Path) -> Option<PathBuf> {
    let system = parent.join(WORKSPACE_DIR);
    is_workspace(&system).then_some(system)
}

/// The cartridge's label: its folder's name (`Potato-games/system/` ->
/// "Potato-games"). Shown in the app and as the Steam self-shortcut name.
pub fn label(workspace: &Path) -> String {
    workspace
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| DISPLAY_NAME.to_string())
}

fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    // Two independently seeded hashes: unpredictable enough for an identity.
    let part = || {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        hasher.write_u32(std::process::id());
        hasher.finish()
    };
    format!("{:016x}{:016x}", part(), part())
}

/// The id in `app_dir/cartridge-id`, created on first use.
pub fn cartridge_id(app_dir: &Path) -> String {
    let path = app_dir.join(CARTRIDGE_ID_FILE);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let existing = existing.trim();
    if !existing.is_empty() {
        return existing.to_string();
    }
    let id = new_id();
    if let Err(e) = std::fs::write(&path, format!("{id}\n")) {
        log::warn!("Cannot save {}: {e}", path.display());
    }
    id
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
        dir.write(
            "system/README.txt",
            "•cartridge workspace: settings...".as_bytes(),
        );
        assert!(is_workspace(&system));
        let other = dir.mkdir("other/system");
        dir.write("other/system/config.txt", b"");
        assert!(is_workspace(&other));
    }

    #[test]
    fn the_workspace_is_system_next_to_the_app_only() {
        let dir = TempDir::new();
        dir.write(".cartridge/config.txt", b"");
        assert_eq!(workspace_in(dir.path()), None);
        dir.write("system/config.txt", b"");
        assert_eq!(workspace_in(dir.path()), Some(dir.path().join("system")));
    }

    #[test]
    fn the_label_is_the_cartridge_folder_name() {
        assert_eq!(label(Path::new("/games/Potato-games")), "Potato-games");
        assert_eq!(label(Path::new("/")), DISPLAY_NAME);
    }

    #[test]
    fn the_cartridge_id_is_created_once_and_kept() {
        let dir = TempDir::new();
        let id = cartridge_id(dir.path());
        assert_eq!(id.len(), 32);
        assert_eq!(cartridge_id(dir.path()), id);
        let other = TempDir::new();
        assert_ne!(cartridge_id(other.path()), id);
    }

    #[test]
    fn an_appimage_stands_in_for_the_mounted_binary() {
        let exe = Some(PathBuf::from("/tmp/.mount_abc/usr/bin/cartridge"));
        assert_eq!(
            launched_from(Some("/games/Cartridge.AppImage".into()), exe.clone()),
            Some(PathBuf::from("/games/Cartridge.AppImage"))
        );
        assert_eq!(launched_from(None, exe.clone()), exe);
        assert_eq!(launched_from(Some("".into()), exe.clone()), exe);
    }

    #[test]
    fn a_mac_bundle_stands_in_for_its_binary() {
        let exe = PathBuf::from("/Games/Cartridge.app/Contents/MacOS/cartridge");
        assert_eq!(
            launched_from(None, Some(exe)),
            Some(PathBuf::from("/Games/Cartridge.app"))
        );
        let loose = PathBuf::from("/G/Contents/MacOS/cartridge");
        assert_eq!(launched_from(None, Some(loose.clone())), Some(loose));
    }

    #[test]
    fn translocated_paths_are_recognised() {
        assert!(is_translocated(Path::new(
            "/private/var/folders/x/T/AppTranslocation/ABC/d/Cartridge.app"
        )));
        assert!(!is_translocated(Path::new("/Users/me/Games/Cartridge.app")));
    }
}
