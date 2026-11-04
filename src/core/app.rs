//! Application identity and the `.cartouche/` workspace directory.
//!
//! The app name comes from the binary filename — everything before the first
//! `-` or `.` — so `cartouche-linux-x86_64` is still "cartouche", and renaming
//! the binary rebrands the app (its `.<name>/` dir, Steam tags, ...).
//! In an AppImage the name comes from the `.AppImage` file instead (the inner
//! binary is always `cartouche`), lowercased so `Cartouche.AppImage` keeps
//! using `.cartouche/`.

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

/// Name of the per-game and per-workspace data directory: `.cartouche`.
pub fn data_dir_name() -> String {
    format!(".{}", app_name())
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

/// A workspace dir, not a per-game data dir (those hold a `game.json`).
fn is_app_dir(p: &Path) -> bool {
    p.is_dir() && !p.join("game.json").exists()
}

/// Locate the `.cartouche/` config dir. First match wins:
/// 1. `cli_dir/.cartouche/` (explicit argument)
/// 2. next to the binary (portable mode)
/// 3. the current directory
pub fn find_app_dir(cli_dir: Option<&Path>) -> Option<PathBuf> {
    let name = data_dir_name();
    let cwd = std::env::current_dir().unwrap_or_default();
    cli_dir
        .map(Path::to_path_buf)
        .into_iter()
        .chain([exe_dir(), cwd])
        .map(|dir| dir.join(&name))
        .find(|p| is_app_dir(p))
}

#[cfg(test)]
mod tests {
    use super::*;

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
