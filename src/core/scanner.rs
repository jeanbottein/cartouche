//! Pipeline step 1: read every game folder under `games/` into memory
//! and resolve the target and save paths for the current machine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use log::{error, info, warn};

use super::models::{folder_name_of, Game, Install, Resolved, SavePath, Target};
use super::package;
use super::paths;
use super::platform::{arch_tag, os_tag};
use super::steam;

/// Narrow `pool` by `key`: exact match, else blank/`any`, else keep everything.
fn filter_by<'a>(pool: Vec<&'a Target>, key: fn(&Target) -> &str, value: &str) -> Vec<&'a Target> {
    let exact: Vec<_> = pool
        .iter()
        .copied()
        .filter(|t| key(t).eq_ignore_ascii_case(value))
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let any: Vec<_> = pool
        .iter()
        .copied()
        .filter(|t| key(t).trim().is_empty() || key(t).eq_ignore_ascii_case("any"))
        .collect();
    if any.is_empty() {
        pool
    } else {
        any
    }
}

/// Best target for the current OS, then arch.
pub fn pick_target(targets: &[Target]) -> Option<&Target> {
    let pool: Vec<&Target> = targets
        .iter()
        .filter(|t| !t.target.trim().is_empty())
        .collect();
    let pool = filter_by(pool, |t| &t.os, os_tag());
    filter_by(pool, |t| &t.arch, arch_tag()).first().copied()
}

/// How a game runs on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compatibility {
    Native,
    /// Windows build on Linux.
    Proton,
    Unsupported,
    /// No targets yet.
    Unknown,
}

impl Compatibility {
    pub fn label(self) -> &'static str {
        match self {
            Compatibility::Native => "Native",
            Compatibility::Proton => "Proton",
            Compatibility::Unsupported => "Not supported",
            Compatibility::Unknown => "Unknown",
        }
    }
}

fn is_any(tag: &str) -> bool {
    tag.trim().is_empty() || tag.eq_ignore_ascii_case("any")
}

fn runs_on(target: &Target, os: &str, arch: &str) -> bool {
    (is_any(&target.os) || target.os.eq_ignore_ascii_case(os))
        && (is_any(&target.arch) || target.arch.eq_ignore_ascii_case(arch))
}

pub fn compatibility(targets: &[Target]) -> Compatibility {
    let runnable: Vec<&Target> = targets
        .iter()
        .filter(|t| !t.target.trim().is_empty())
        .collect();
    if runnable.is_empty() {
        Compatibility::Unknown
    } else if runnable.iter().any(|t| runs_on(t, os_tag(), arch_tag())) {
        Compatibility::Native
    } else if os_tag() == "linux" && runnable.iter().any(|t| runs_on(t, "windows", arch_tag())) {
        Compatibility::Proton
    } else {
        Compatibility::Unsupported
    }
}

fn os_label(os: &str) -> String {
    match os.trim().to_lowercase().as_str() {
        "" | "any" => "Any".into(),
        "linux" => "Linux".into(),
        "windows" => "Windows".into(),
        "macos" => "macOS".into(),
        other => other.to_string(),
    }
}

/// Distinct `OS arch` labels of the targets, e.g. `["Linux x64", "Windows x64"]`.
pub fn platforms(targets: &[Target]) -> Vec<String> {
    targets
        .iter()
        .filter(|t| !t.target.trim().is_empty())
        .map(|t| {
            let arch = if is_any(&t.arch) { "" } else { t.arch.trim() };
            format!("{} {arch}", os_label(&t.os)).trim_end().to_string()
        })
        .fold(Vec::new(), |mut acc, label| {
            if !acc.contains(&label) {
                acc.push(label);
            }
            acc
        })
}

/// Absolute paths for `target`; `startIn` defaults to the executable's folder.
pub fn resolve_target(game_dir: &Path, target: &Target) -> Resolved {
    let start_in = if target.start_in.is_empty() {
        Path::new(&target.target)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    } else {
        PathBuf::from(&target.start_in)
    };
    Resolved {
        target: paths::normpath(&game_dir.join(&target.target)),
        start_in: paths::normpath(&game_dir.join(start_in)),
        launch_options: target.launch_options.clone(),
        os: target.os.to_lowercase(),
    }
}

/// `${steamappid}` / `${proton_c}` for a game's save paths: the AppID Steam gives
/// its non-Steam shortcut, which is also the Proton prefix folder name.
pub fn save_path_vars(title: &str, target: &Path) -> HashMap<String, String> {
    if title.is_empty() {
        return HashMap::new();
    }
    let appid = steam::generate_appid(title, &paths::lossy(target));
    HashMap::from([
        ("steamappid".to_string(), appid.to_string()),
        ("proton_c".to_string(), steam::proton_prefix(appid)),
    ])
}

/// Map Windows locations (`%APPDATA%`, `C:/`, ...) into the Proton prefix.
pub fn translate_windows_to_proton(path: &str, proton_c: &str) -> String {
    let user = format!("{proton_c}/users/steamuser");
    let rules = [
        ("%USERPROFILE%", user.clone()),
        ("%APPDATA%", format!("{user}/AppData/Roaming")),
        ("%LOCALAPPDATA%", format!("{user}/AppData/Local")),
        ("%PUBLIC%", format!("{proton_c}/users/Public")),
        ("%DOCUMENTS%", format!("{user}/Documents")),
        ("C:/", format!("{proton_c}/")),
    ];
    rules
        .iter()
        .fold(path.replace('\\', "/"), |acc, (from, to)| {
            paths::replace_ignore_case(&acc, from, to)
        })
}

/// Substitute custom vars, expand `~` and env vars, anchor relative paths to the game dir.
pub fn resolve_save_path(
    raw: &str,
    game_dir: &Path,
    vars: &HashMap<String, String>,
) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    let substituted = paths::substitute(raw, |name| vars.get(name).cloned());
    Some(paths::normpath(&game_dir.join(paths::expand(&substituted))))
}

/// Resolve one save entry for this machine; `None` when it targets another OS.
pub fn resolve_save_entry(
    entry: &SavePath,
    valid_os: &[&str],
    game_dir: &Path,
    vars: &HashMap<String, String>,
) -> Option<PathBuf> {
    let entry_os = entry.os.trim().to_lowercase();
    if !entry_os.is_empty() && entry_os != "any" && !valid_os.contains(&entry_os.as_str()) {
        return None;
    }
    let raw = match vars.get("proton_c") {
        Some(proton_c) if os_tag() == "linux" && entry_os == "windows" => {
            translate_windows_to_proton(&entry.path, proton_c)
        }
        _ => entry.path.clone(),
    };
    resolve_save_path(&raw, game_dir, vars)
}

/// Fill the runtime fields (best target, save paths) for the current platform.
pub fn resolve_runtime(game: &mut Game) {
    game.resolved = pick_target(&game.targets).map(|t| resolve_target(&game.dir, t));
    let vars = game
        .resolved
        .as_ref()
        .map(|r| save_path_vars(&game.title, &r.target))
        .unwrap_or_default();
    let target_os = game
        .resolved
        .as_ref()
        .map(|r| r.os.as_str())
        .unwrap_or_default();
    let valid_os = [os_tag(), target_os];

    game.resolved_save_paths = game.save_paths.iter().fold(Vec::new(), |mut acc, entry| {
        if let Some(path) = resolve_save_entry(entry, &valid_os, &game.dir, &vars) {
            if !acc.contains(&path) {
                acc.push(path);
            }
        }
        acc
    });
}

/// Load `.cartridge/game.json` from `dir`: a skeleton when it doesn't exist,
/// `None` when it exists but can't be read (so it is never overwritten).
pub fn load_game(dir: &Path) -> Option<Game> {
    let path = Game::skeleton(dir).game_json_path();
    if !path.is_file() {
        return Some(Game::skeleton(dir));
    }
    let skipped = |e: String| {
        error!(
            "Skipping {}: cannot read {} ({e})",
            folder_name_of(dir),
            path.display()
        )
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| skipped(e.to_string()))
        .ok()?;
    let mut game = Game::from_json(dir, &text)
        .map_err(|e| skipped(e.to_string()))
        .ok()?;
    resolve_runtime(&mut game);
    Some(game)
}

/// A package not unpacked yet: read-only metadata, nothing resolved (it can't
/// run). `None` when the package can't be read.
pub fn load_package(package: &Path, games_dir: &Path) -> Option<Game> {
    let (manifest, game_json) = package::read_meta(package)
        .map_err(|e| error!("Skipping {}: {e}", package.display()))
        .ok()?;
    let dir = games_dir.join(&manifest.folder_name);
    let mut game = game_json
        .and_then(|text| Game::from_json(&dir, &text).ok())
        .unwrap_or_else(|| Game {
            title: manifest.title.clone(),
            ..Game::skeleton(&dir)
        });
    game.size = game.size.or(Some(manifest.size));
    game.install = Install::Packaged {
        package: package.to_path_buf(),
    };
    Some(game)
}

/// Attach a package to the installed game it belongs to.
fn with_package(mut game: Game, package: &Path) -> Game {
    let verified = package::read_meta(package)
        .is_ok_and(|(m, _)| package::unpacked_from(&game.dir) == Some(m.id));
    game.install = Install::InstalledWithPackage {
        package: package.to_path_buf(),
        verified,
    };
    game
}

/// Every visible subfolder of `games_dir` as a game (a skeleton when it has no
/// metadata yet), with runtime fields resolved, plus packages (`Foo.cartridge.tar`)
/// merged into their folder when it exists. Silent: the UI reloads often.
pub fn load_library(games_dir: &Path) -> Vec<Game> {
    let installed: Vec<Game> = paths::visible_subdirs(games_dir)
        .iter()
        .filter_map(|dir| load_game(dir))
        .collect();
    let packages = package::list(games_dir);
    let package_of = |game: &Game| package::package_path(games_dir, &game.folder_name);
    let standalone: Vec<Game> = packages
        .iter()
        .filter(|p| !installed.iter().any(|g| package_of(g) == **p))
        .filter_map(|p| load_package(p, games_dir))
        .collect();
    let mut games: Vec<Game> = installed
        .into_iter()
        .map(|game| match package_of(&game) {
            p if packages.contains(&p) => with_package(game, &p),
            _ => game,
        })
        .chain(standalone)
        .collect();
    // Stable sort keeps the installed folder first if a renamed package
    // holds the same game; dedup then drops the package entry.
    games.sort_by(|a, b| a.folder_name.cmp(&b.folder_name));
    games.dedup_by(|a, b| a.folder_name == b.folder_name);
    games
}

/// Pipeline step 1: [`load_library`] with a log line per game.
pub fn scan(games_dir: &Path) -> Vec<Game> {
    if !games_dir.is_dir() {
        warn!("No games folder at {}", games_dir.display());
        return Vec::new();
    }
    info!("Scanning for games in {}", games_dir.display());
    let games = load_library(games_dir);
    for game in &games {
        if !game.is_installed() {
            info!("  Packaged: {}", game.title);
        } else if game.has_cartridge {
            info!("  Loaded: {}", game.title);
        } else {
            info!("  Discovered: {} (needs detection)", game.folder_name);
        }
    }
    info!("Found {} game(s)", games.len());
    games
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    fn t(os: &str, arch: &str, target: &str) -> Target {
        Target {
            os: os.into(),
            arch: arch.into(),
            target: target.into(),
            ..Default::default()
        }
    }

    fn other_os() -> &'static str {
        if os_tag() == "windows" {
            "linux"
        } else {
            "windows"
        }
    }

    #[test]
    fn prefers_current_os_then_any_then_first() {
        assert_eq!(
            pick_target(&[t(other_os(), "", "a"), t(os_tag(), "", "b")])
                .unwrap()
                .target,
            "b"
        );
        assert_eq!(
            pick_target(&[t(other_os(), "", "a"), t("any", "", "c")])
                .unwrap()
                .target,
            "c"
        );
        assert_eq!(pick_target(&[t(other_os(), "", "a")]).unwrap().target, "a");
    }

    #[test]
    fn prefers_current_arch_and_skips_empty_targets() {
        let other_arch = if arch_tag() == "arm64" {
            "x64"
        } else {
            "arm64"
        };
        let targets = [
            t(os_tag(), arch_tag(), ""),
            t(os_tag(), other_arch, "a"),
            t(os_tag(), arch_tag(), "b"),
        ];
        assert_eq!(pick_target(&targets).unwrap().target, "b");
    }

    #[test]
    fn resolve_target_defaults_start_in_to_exe_folder() {
        let resolved = resolve_target(Path::new("/g/Game"), &t("linux", "x64", "bin/./run.sh"));
        assert_eq!(resolved.target, PathBuf::from("/g/Game/bin/run.sh"));
        assert_eq!(resolved.start_in, PathBuf::from("/g/Game/bin"));
    }

    #[test]
    fn windows_paths_translate_into_the_proton_prefix() {
        assert_eq!(
            translate_windows_to_proton("%appdata%\\Studio\\Game", "/pfx"),
            "/pfx/users/steamuser/AppData/Roaming/Studio/Game"
        );
        assert_eq!(
            translate_windows_to_proton("C:/Games/Saves", "/pfx"),
            "/pfx/Games/Saves"
        );
    }

    #[test]
    fn save_paths_substitute_vars_and_anchor_relative_paths() {
        let vars = HashMap::from([("steamappid".to_string(), "42".to_string())]);
        assert_eq!(
            resolve_save_path("saves/${steamappid}", Path::new("/g/Game"), &vars),
            Some(PathBuf::from("/g/Game/saves/42"))
        );
        assert_eq!(
            resolve_save_path("/abs/./x", Path::new("/g/Game"), &vars),
            Some(PathBuf::from("/abs/x"))
        );
        assert_eq!(resolve_save_path("", Path::new("/g"), &vars), None);
    }

    #[test]
    fn save_entries_for_other_os_are_skipped() {
        let entry = SavePath {
            os: "macos".into(),
            path: "/x".into(),
        };
        let valid = [os_tag(), ""];
        let expected = (os_tag() == "macos").then(|| PathBuf::from("/x"));
        assert_eq!(
            resolve_save_entry(&entry, &valid, Path::new("/g"), &HashMap::new()),
            expected
        );
    }

    #[test]
    fn scan_loads_metadata_and_discovers_skeletons() {
        let dir = TempDir::new();
        dir.mkdir("Bare Game");
        dir.mkdir(".hidden");
        dir.write(
            "Known/.cartridge/game.json",
            br#"{"title": "Known Game", "targets": [{"os": "any", "target": "run.sh"}], "savePaths": [{"os": "", "path": "saves"}]}"#,
        );
        let games = scan(dir.path());
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].title, "Bare Game");
        assert!(!games[0].has_cartridge);
        let known = &games[1];
        assert_eq!(known.title, "Known Game");
        assert_eq!(
            known.resolved.as_ref().unwrap().target,
            dir.path().join("Known/run.sh")
        );
        assert_eq!(
            known.resolved_save_paths,
            vec![dir.path().join("Known/saves")]
        );
    }

    #[test]
    fn broken_game_json_is_skipped_not_replaced() {
        let dir = TempDir::new();
        let path = dir.write("Broken/.cartridge/game.json", b"{ not json");
        assert!(scan(dir.path()).is_empty());
        assert_eq!(std::fs::read(path).unwrap(), b"{ not json");
    }

    fn pack_into(dir: &TempDir, name: &str) -> PathBuf {
        let game_dir = dir.mkdir(&format!("src/{name}"));
        dir.write(&format!("src/{name}/run.sh"), b"#!/bin/sh");
        let game = Game::skeleton(&game_dir);
        let package = package::package_path(&dir.path().join("games"), name);
        std::fs::create_dir_all(package.parent().unwrap()).unwrap();
        package::pack(&game, &package, &package::Progress::default()).unwrap();
        package
    }

    #[test]
    fn library_lists_packages_once_and_ignores_staging() {
        let dir = TempDir::new();
        let only_package = pack_into(&dir, "Alpha");
        pack_into(&dir, "Beta");
        dir.mkdir("games/Beta");
        dir.mkdir("games/.Gamma.part");
        let games = load_library(&dir.path().join("games"));
        let names: Vec<_> = games.iter().map(|g| g.folder_name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Beta"]);
        assert_eq!(
            games[0].install,
            Install::Packaged {
                package: only_package
            }
        );
        assert!(games[0].resolved.is_none() && games[0].size == Some(9));
        assert!(matches!(
            games[1].install,
            Install::InstalledWithPackage {
                verified: false,
                ..
            }
        ));
    }

    #[test]
    fn unpacked_folder_is_verified_against_its_package() {
        let dir = TempDir::new();
        let package = pack_into(&dir, "Alpha");
        let games_dir = dir.path().join("games");
        package::unpack(&package, &games_dir, &package::Progress::default()).unwrap();
        let games = load_library(&games_dir);
        assert_eq!(games.len(), 1);
        assert!(matches!(
            games[0].install,
            Install::InstalledWithPackage { verified: true, .. }
        ));
    }

    #[test]
    fn compatibility_prefers_native_then_proton() {
        assert_eq!(compatibility(&[]), Compatibility::Unknown);
        assert_eq!(
            compatibility(&[t(os_tag(), arch_tag(), "a")]),
            Compatibility::Native
        );
        assert_eq!(compatibility(&[t("any", "", "a")]), Compatibility::Native);
        let windows_only = compatibility(&[t("windows", arch_tag(), "a.exe")]);
        let expected = match os_tag() {
            "windows" => Compatibility::Native,
            "linux" => Compatibility::Proton,
            _ => Compatibility::Unsupported,
        };
        assert_eq!(windows_only, expected);
        assert_eq!(
            compatibility(&[t(os_tag(), "other-arch", "a")]),
            Compatibility::Unsupported
        );
    }

    #[test]
    fn platforms_are_distinct_labels() {
        let targets = [
            t("linux", "x64", "a"),
            t("linux", "x64", "b"),
            t("windows", "", "c.exe"),
            t("macos", "arm64", ""),
        ];
        assert_eq!(platforms(&targets), ["Linux x64", "Windows"]);
    }
}
