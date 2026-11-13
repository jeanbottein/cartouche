//! Pipeline step 2: find executables for games that have no targets yet,
//! restore execute bits that copies lost, plus Proton save-folder detection
//! for the game editor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use log::{info, warn};

use super::models::{Game, SavePath, Target};
use super::paths;
use super::platform::{
    arch_tag, detect_binary_arch, has_exec_permission, is_executable, is_native_binary,
    make_executable, os_tag,
};
use super::scanner;
use super::steam;

const SKIP_DIRS: [&str; 4] = ["java", "jre", "lib", "__pycache__"];
const MAX_DEPTH: usize = 3;
/// Native programs found even without their execute bit (copies over FAT
/// drives, zips or web downloads drop it). No extension counts too; libraries
/// (`.so`, `.so.6`, `.dylib`) never match.
const PROGRAM_EXTENSIONS: [&str; 6] = ["appimage", "x86_64", "x86", "x64", "bin", "run"];

// ── Similarity (difflib.SequenceMatcher.ratio) ───────────────────────────

/// Longest common block in `a[alo..ahi]` × `b[blo..bhi]`, earliest on ties.
fn longest_match(
    a: &[char],
    b2j: &HashMap<char, Vec<usize>>,
    lo_hi: (usize, usize, usize, usize),
) -> (usize, usize, usize) {
    let (alo, ahi, blo, bhi) = lo_hi;
    let (mut best_i, mut best_j, mut best_k) = (alo, blo, 0);
    let mut lengths: HashMap<usize, usize> = HashMap::new();
    for (i, ch) in a.iter().enumerate().take(ahi).skip(alo) {
        let mut next = HashMap::new();
        for &j in b2j.get(ch).into_iter().flatten() {
            if j < blo {
                continue;
            }
            if j >= bhi {
                break;
            }
            let k = j.checked_sub(1).and_then(|p| lengths.get(&p)).unwrap_or(&0) + 1;
            next.insert(j, k);
            if k > best_k {
                (best_i, best_j, best_k) = (i + 1 - k, j + 1 - k, k);
            }
        }
        lengths = next;
    }
    (best_i, best_j, best_k)
}

fn matched_chars(
    a: &[char],
    b2j: &HashMap<char, Vec<usize>>,
    lo_hi: (usize, usize, usize, usize),
) -> usize {
    let (alo, ahi, blo, bhi) = lo_hi;
    let (i, j, k) = longest_match(a, b2j, lo_hi);
    if k == 0 {
        return 0;
    }
    let left = if alo < i && blo < j {
        matched_chars(a, b2j, (alo, i, blo, j))
    } else {
        0
    };
    let right = if i + k < ahi && j + k < bhi {
        matched_chars(a, b2j, (i + k, ahi, j + k, bhi))
    } else {
        0
    };
    k + left + right
}

/// Ratcliff/Obershelp similarity, equal to Python's `SequenceMatcher(None, a, b).ratio()`
/// for names under 200 chars (where difflib's autojunk heuristic never applies).
pub fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
    b.iter()
        .enumerate()
        .for_each(|(j, &c)| b2j.entry(c).or_default().push(j));
    let matches = matched_chars(&a, &b2j, (0, a.len(), 0, b.len()));
    2.0 * matches as f64 / (a.len() + b.len()) as f64
}

// ── Executable discovery ─────────────────────────────────────────────────

fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The candidate whose name best matches the folder name (first on ties).
fn find_best(game_dir: &Path, files: &[PathBuf]) -> Option<PathBuf> {
    let folder = base_name(game_dir);
    let score = |f: &PathBuf| {
        similarity(
            &folder,
            &f.file_stem().unwrap_or_default().to_string_lossy(),
        )
    };
    files
        .iter()
        .fold(None::<(&PathBuf, f64)>, |best, f| {
            let s = score(f);
            match best {
                Some((_, bs)) if bs >= s => best,
                _ => Some((f, s)),
            }
        })
        .map(|(f, _)| f.clone())
}

/// Files of `dir` and its subfolders up to `max_depth` levels (1 = `dir` only),
/// skipping hidden and runtime folders, never following folder symlinks.
fn walk_files(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    if max_depth == 0 {
        return Vec::new();
    }
    let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) = paths::sorted_entries(dir)
        .into_iter()
        .map(|n| dir.join(n))
        .partition(|p| p.is_dir());
    let nested = dirs
        .into_iter()
        .filter(|d| {
            let name = base_name(d);
            !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) && !paths::is_symlink(d)
        })
        .flat_map(|d| walk_files(&d, max_depth - 1));
    files.into_iter().chain(nested).collect()
}

fn is_exe_name(path: &Path) -> bool {
    base_name(path).to_lowercase().ends_with(".exe")
}

fn has_program_name(path: &Path) -> bool {
    path.extension().is_none_or(|ext| {
        PROGRAM_EXTENSIONS.contains(&ext.to_string_lossy().to_lowercase().as_str())
    })
}

/// A file worth offering as a target: a Windows `.exe`, or a native program
/// (with its execute bit, or named like one).
fn is_candidate(path: &Path) -> bool {
    (is_exe_name(path) || has_exec_permission(path) || has_program_name(path))
        && is_executable(path)
}

/// Descend through folders that only contain a single subfolder.
fn real_game_dir(game_dir: &Path) -> PathBuf {
    let mut dir = paths::normpath(game_dir);
    for _ in 0..=10 {
        let entries: Vec<PathBuf> = paths::sorted_entries(&dir)
            .into_iter()
            .filter(|n| !n.starts_with('.'))
            .map(|n| dir.join(n))
            .collect();
        let only_one_dir = entries.len() == 1 && entries[0].is_dir();
        if !only_one_dir {
            break;
        }
        dir = entries[0].clone();
    }
    dir
}

/// Display title for a freshly detected game: the innermost meaningful folder.
pub fn title_for(game_dir: &Path) -> String {
    base_name(&real_game_dir(game_dir))
}

/// Arch from the binary header, else from name hints, else this machine's.
fn arch_of(exe: &Path) -> String {
    if let Some(arch) = detect_binary_arch(exe) {
        return arch.to_string();
    }
    let name = base_name(exe).to_lowercase();
    let arch = if name.contains("arm64") || name.contains("aarch64") {
        "arm64"
    } else if name.contains("x86_64") || name.contains("amd64") || name.contains("64") {
        "x64"
    } else {
        arch_tag()
    };
    arch.to_string()
}

fn make_target(os: &str, arch: &str, exe: &Path, game_dir: &Path) -> Target {
    Target {
        os: os.to_string(),
        arch: arch.to_string(),
        target: paths::lossy(&paths::relpath(exe, game_dir)),
        start_in: paths::lossy(&paths::relpath(exe.parent().unwrap_or(game_dir), game_dir)),
        launch_options: String::new(),
    }
}

/// Windows host: the best-named `.exe` among those built for this machine's
/// arch, else for the other one. By header, not name: a "64" in a helper
/// such as `UnityCrashHandler64.exe` says nothing about the game.
fn best_windows_exe(dir: &Path, depth: usize) -> Option<PathBuf> {
    let candidates: Vec<(PathBuf, String)> = walk_files(dir, depth)
        .into_iter()
        .filter(|p| is_exe_name(p))
        .map(|p| {
            let arch = arch_of(&p);
            (p, arch)
        })
        .collect();
    let arch_order = match arch_tag() {
        "arm64" => ["arm64", "x64"],
        _ => ["x64", "arm64"],
    };
    let built_for = |arch: &str| -> Vec<PathBuf> {
        candidates
            .iter()
            .filter(|(_, a)| a == arch)
            .map(|(p, _)| p.clone())
            .collect()
    };
    let any: Vec<PathBuf> = candidates.iter().map(|(p, _)| p.clone()).collect();
    arch_order
        .iter()
        .find_map(|arch| find_best(dir, &built_for(arch)))
        .or_else(|| find_best(dir, &any))
}

fn collect_windows_host_targets(game_dir: &Path) -> Vec<Target> {
    let real_dir = real_game_dir(game_dir);
    (1..=MAX_DEPTH)
        .find_map(|depth| best_windows_exe(&real_dir, depth))
        .map(|exe| vec![make_target("windows", &arch_of(&exe), &exe, game_dir)])
        .unwrap_or_default()
}

/// One target per (os, arch): native executables and Windows `.exe` files.
fn collect_posix_host_targets(game_dir: &Path) -> Vec<Target> {
    let real_dir = real_game_dir(game_dir);
    let exes = walk_files(&real_dir, MAX_DEPTH)
        .into_iter()
        .filter(|p| is_candidate(p));

    let mut groups: Vec<((String, String), Vec<PathBuf>)> = Vec::new();
    for exe in exes {
        let os = if is_exe_name(&exe) {
            "windows"
        } else {
            os_tag()
        };
        let key = (os.to_string(), arch_of(&exe));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(exe),
            None => groups.push((key, vec![exe])),
        }
    }
    groups
        .iter()
        .filter_map(|((os, arch), exes)| {
            find_best(&real_dir, exes).map(|best| make_target(os, arch, &best, game_dir))
        })
        .collect()
}

/// Every executable target detected in `game_dir`.
pub fn collect_targets(game_dir: &Path) -> Vec<Target> {
    if os_tag() == "windows" {
        collect_windows_host_targets(game_dir)
    } else {
        collect_posix_host_targets(game_dir)
    }
}

/// Fill targets + title for every game that has no targets yet, and give
/// native targets back the execute bit a copy lost.
pub fn detect(games: &mut [Game]) {
    detect_missing_targets(games);
    remember_appids(games);
    games
        .iter()
        .filter(|g| g.is_installed())
        .for_each(restore_exec_bits);
}

/// Store the AppID each game has now (from its current title), so a later
/// rename keeps its Steam shortcut, Proton prefix and saves.
fn remember_appids(games: &mut [Game]) {
    for game in games
        .iter_mut()
        .filter(|g| g.is_installed() && g.steam_appid.is_none())
    {
        if let Some(appid) = steam::appid_of(game) {
            game.steam_appid = Some(appid);
            game.needs_persist = true;
        }
    }
}

fn restore_exec_bits(game: &Game) {
    game.targets
        .iter()
        .for_each(|t| restore_exec_bit(&scanner::resolve_target(&game.dir, t).target));
}

/// Make a native program executable again if it lost the bit (logged).
pub fn restore_exec_bit(exe: &Path) {
    if has_exec_permission(exe) || !is_native_binary(exe) {
        return;
    }
    match make_executable(exe) {
        Ok(()) => info!("  Made executable: {}", exe.display()),
        Err(e) => warn!("  Cannot make {} executable: {e}", exe.display()),
    }
}

fn detect_missing_targets(games: &mut [Game]) {
    let incomplete: Vec<&mut Game> = games
        .iter_mut()
        .filter(|g| g.is_installed() && g.targets.is_empty())
        .collect();
    if incomplete.is_empty() {
        return;
    }
    info!("Detecting executables for {} game(s)", incomplete.len());
    for game in incomplete {
        let targets = collect_targets(&game.dir);
        if targets.is_empty() {
            info!("  No executable found: {}", game.folder_name);
            continue;
        }
        // Only a placeholder title (no game.json yet) is replaced.
        if game.title == game.folder_name {
            game.title = title_for(&game.dir);
        }
        game.resolved =
            scanner::pick_target(&targets).map(|t| scanner::resolve_target(&game.dir, t));
        game.targets = targets;
        game.needs_persist = true;
        info!(
            "  Detected: {} ({} target(s))",
            game.title,
            game.targets.len()
        );
    }
}

// ── Proton save detection ────────────────────────────────────────────────

/// Prefix subfolders searched, with the Windows location they stand for.
const PROTON_SEARCH_DIRS: [(&str, &str); 5] = [
    (
        "users/steamuser/AppData/LocalLow",
        "%USERPROFILE%/AppData/LocalLow",
    ),
    ("users/steamuser/AppData/Local", "%LOCALAPPDATA%"),
    ("users/steamuser/AppData/Roaming", "%APPDATA%"),
    ("users/steamuser/Documents", "%USERPROFILE%/Documents"),
    ("users/steamuser/Saved Games", "%USERPROFILE%/Saved Games"),
];

const PROTON_IGNORE_DIRS: [&str; 18] = [
    "Microsoft",
    "Temp",
    "dxvk",
    "CrashDumps",
    ".cef",
    "CEF",
    "desktop.ini",
    "Public",
    "Windows",
    "Contacts",
    "Favorites",
    "Links",
    "Pictures",
    "Music",
    "Videos",
    "Searches",
    "Steam",
    "SteamVR",
];

const PROTON_SEARCH_DEPTH: usize = 2;

fn alphanumeric_key(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Folders (relative to `base`, `base` itself included) up to the search depth.
fn proton_candidate_dirs(base: &Path, rel: PathBuf, depth: usize) -> Vec<PathBuf> {
    let children = if depth < PROTON_SEARCH_DEPTH {
        paths::visible_subdirs(&base.join(&rel))
            .into_iter()
            .map(|d| base_name(&d))
            .filter(|name| !PROTON_IGNORE_DIRS.contains(&name.as_str()))
            .flat_map(|name| proton_candidate_dirs(base, rel.join(name), depth + 1))
            .collect()
    } else {
        Vec::new()
    };
    std::iter::once(rel).chain(children).collect()
}

fn search_proton_dir(
    drive_c: &Path,
    title_key: &str,
    sub: &str,
    windows_root: &str,
) -> Vec<SavePath> {
    let base = drive_c.join(sub);
    if !base.is_dir() {
        return Vec::new();
    }
    proton_candidate_dirs(&base, PathBuf::new(), 0)
        .into_iter()
        .filter(|rel| {
            let name = if rel.as_os_str().is_empty() {
                base_name(&base)
            } else {
                base_name(rel)
            };
            let key = alphanumeric_key(&name);
            !key.is_empty() && key.contains(title_key)
        })
        .map(|rel| {
            let rel = paths::lossy(&rel).replace('\\', "/");
            let path = if rel.is_empty() {
                windows_root.to_string()
            } else {
                format!("{windows_root}/{rel}")
            };
            SavePath {
                os: "windows".into(),
                path,
            }
        })
        .collect()
}

/// Save folders inside a game's Proton prefix whose name contains the title,
/// as Windows-style paths (`%APPDATA%/...`).
pub fn detect_proton_save_paths(title: &str, appid: u32) -> Vec<SavePath> {
    let title_key = alphanumeric_key(title);
    if title_key.is_empty() {
        return Vec::new();
    }
    let drive_c = paths::expand(&steam::proton_prefix(appid));
    if !drive_c.is_dir() {
        return Vec::new();
    }
    PROTON_SEARCH_DIRS
        .iter()
        .flat_map(|(sub, root)| search_proton_dir(&drive_c, &title_key, sub, root))
        .fold(Vec::new(), |mut acc, sp| {
            if !acc.contains(&sp) {
                acc.push(sp);
            }
            acc
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn similarity_matches_python_difflib() {
        // Golden values from difflib.SequenceMatcher(None, a, b).ratio()
        let cases = [
            ("Hollow Knight", "hollow_knight", 0.7692307692307693),
            ("Celeste", "Celeste.bin.x86_64", 0.56),
            ("abcabc", "cbacba", 0.5),
            ("Game", "UnityCrashHandler64", 0.17391304347826086),
            ("Stardew Valley", "StardewValley", 0.9629629629629629),
        ];
        for (a, b, expected) in cases {
            assert!((similarity(a, b) - expected).abs() < 1e-12, "{a} vs {b}");
        }
    }

    #[cfg(unix)]
    fn elf() -> Vec<u8> {
        let mut bytes = b"\x7fELF".to_vec();
        bytes.resize(64, 0);
        bytes[18] = 0x3E;
        bytes
    }

    #[cfg(unix)]
    fn make_exec(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn detects_one_target_per_os_arch_with_best_name() {
        let dir = TempDir::new();
        let game = dir.mkdir("Celeste");
        for name in [
            "Celeste/wrapper/Celeste.bin.x86_64",
            "Celeste/wrapper/crashpad",
        ] {
            make_exec(&dir.write(name, &elf()));
        }
        let mut pe = b"MZ".to_vec();
        pe.resize(64, 0);
        dir.write("Celeste/wrapper/Celeste.exe", &pe);
        dir.write("Celeste/readme.txt", b"hi");

        let targets = collect_targets(&game);
        assert_eq!(title_for(&game), "Celeste");
        let native = targets.iter().find(|t| t.os == os_tag()).unwrap();
        assert_eq!(native.arch, "x64");
        assert_eq!(native.target, "wrapper/Celeste.bin.x86_64");
        assert_eq!(native.start_in, "wrapper");
        let windows = targets.iter().find(|t| t.os == "windows").unwrap();
        assert_eq!(windows.target, "wrapper/Celeste.exe");
    }

    /// A Windows program for `machine` (0x8664 = x64, 0xAA64 = arm64).
    fn pe(machine: u16) -> Vec<u8> {
        let mut bytes = b"MZ".to_vec();
        bytes.resize(64, 0);
        bytes[60] = 64; // PE header right after the DOS header
        bytes.extend(b"PE\0\0");
        bytes.extend(machine.to_le_bytes());
        bytes
    }

    #[test]
    fn a_64_in_a_helper_name_does_not_beat_the_game_exe() {
        let dir = TempDir::new();
        let game = dir.mkdir("Hollow Knight");
        dir.write("Hollow Knight/hollow_knight.exe", &pe(0x8664));
        dir.write("Hollow Knight/UnityCrashHandler64.exe", &pe(0x8664));

        let best = best_windows_exe(&game, 1).unwrap();
        assert_eq!(best.file_name().unwrap(), "hollow_knight.exe");
    }

    #[test]
    fn windows_host_prefers_an_exe_built_for_its_arch() {
        let dir = TempDir::new();
        let game = dir.mkdir("Game");
        dir.write("Game/Game.exe", &pe(0xAA64));
        dir.write("Game/Game-x64.exe", &pe(0x8664));
        let wanted = if arch_tag() == "arm64" {
            "Game.exe"
        } else {
            "Game-x64.exe"
        };

        let best = best_windows_exe(&game, 1).unwrap();
        assert_eq!(best.file_name().unwrap(), wanted);
    }

    #[test]
    fn a_windows_host_target_has_the_arch_its_exe_is_built_for() {
        let dir = TempDir::new();
        let game = dir.mkdir("Game");
        let (other_arch, machine) = if arch_tag() == "arm64" {
            ("x64", 0x8664)
        } else {
            ("arm64", 0xAA64)
        };
        dir.write("Game/Game.exe", &pe(machine));

        let targets = collect_windows_host_targets(&game);
        assert_eq!(targets[0].arch, other_arch);
    }

    #[test]
    fn detection_stores_the_appid_the_game_has_now_and_keeps_a_stored_one() {
        let dir = TempDir::new();
        let game_dir = dir.mkdir("Game");
        dir.write("Game/Game.exe", &pe(0x8664));
        let target = Target {
            target: "Game.exe".into(),
            os: "windows".into(),
            ..Default::default()
        };
        let mut games = [
            Game {
                targets: vec![target.clone()],
                resolved: Some(scanner::resolve_target(&game_dir, &target)),
                ..Game::skeleton(&game_dir)
            },
            Game {
                targets: vec![target.clone()],
                resolved: Some(scanner::resolve_target(&game_dir, &target)),
                steam_appid: Some(42),
                ..Game::skeleton(&game_dir)
            },
        ];

        detect(&mut games);
        let exe = scanner::resolve_target(&game_dir, &target).target;
        let expected = steam::generate_appid("Game", &paths::lossy(&exe));
        assert_eq!(games[0].steam_appid, Some(expected));
        assert!(games[0].needs_persist);
        assert_eq!(games[1].steam_appid, Some(42));
        assert!(!games[1].needs_persist);
    }

    #[test]
    fn detection_keeps_a_title_the_game_json_already_has() {
        let dir = TempDir::new();
        let game_dir = dir.mkdir("HK_v1.5");
        dir.write("HK_v1.5/hollow_knight.exe", &pe(0x8664));
        let mut games = [Game {
            title: "Hollow Knight".into(),
            ..Game::skeleton(&game_dir)
        }];

        detect(&mut games);
        assert_eq!(games[0].targets.len(), 1);
        assert_eq!(games[0].title, "Hollow Knight");
    }

    /// Write a file without execute bits (as a copy that lost them leaves it).
    #[cfg(unix)]
    fn write_644(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.write(name, content);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        path
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn programs_that_lost_their_execute_bit_are_candidates_libraries_are_not() {
        let dir = TempDir::new();
        let candidate = |name: &str| is_candidate(&write_644(&dir, name, &elf()));
        assert!(candidate("Strange Flesh.AppImage"));
        assert!(candidate("BanjoRecompiled"));
        assert!(candidate("Celeste.bin.x86_64"));
        assert!(!candidate("libgame.so"));
        assert!(!candidate("libgame.so.6"));
        assert!(!is_candidate(&write_644(&dir, "notes", b"not a program")));
    }

    #[cfg(unix)]
    #[test]
    fn detect_finds_and_repairs_a_program_without_execute_bit() {
        let dir = TempDir::new();
        let game_dir = dir.mkdir("Strange Flesh");
        let exe = write_644(&dir, "Strange Flesh/Strange Flesh.AppImage", &elf());
        let mut games = [Game {
            dir: game_dir,
            ..Default::default()
        }];
        detect(&mut games);
        assert_eq!(games[0].targets[0].target, "Strange Flesh.AppImage");
        assert_eq!(mode(&exe), 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn known_targets_get_their_execute_bit_back_text_files_do_not() {
        let dir = TempDir::new();
        let game_dir = dir.mkdir("Game");
        let exe = write_644(&dir, "Game/game", &elf());
        let script = write_644(&dir, "Game/start.txt", b"echo hi");
        let target = |name: &str| Target {
            os: os_tag().into(),
            target: name.into(),
            ..Default::default()
        };
        let mut games = [Game {
            dir: game_dir,
            targets: vec![target("game"), target("start.txt")],
            ..Default::default()
        }];
        detect(&mut games);
        assert_eq!(mode(&exe), 0o755);
        assert_eq!(mode(&script), 0o644);
    }

    #[test]
    fn title_descends_single_child_folders() {
        let dir = TempDir::new();
        dir.write("Pack/Real Game/run.sh", b"");
        dir.mkdir("Pack/.cartridge");
        assert_eq!(title_for(&dir.path().join("Pack")), "Real Game");
    }

    #[test]
    fn walk_respects_depth_and_skips_runtime_dirs() {
        let dir = TempDir::new();
        dir.write("a.txt", b"");
        dir.write("lib/skip.so", b"");
        dir.write("one/two/three/deep.txt", b"");
        dir.write("one/b.txt", b"");
        let names: Vec<String> = walk_files(dir.path(), 2)
            .iter()
            .map(|p| base_name(p))
            .collect();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn proton_candidates_include_base_and_two_levels() {
        let dir = TempDir::new();
        dir.mkdir("Studio/Game Saves/too/deep");
        dir.mkdir("Microsoft/Game");
        let found: Vec<String> = proton_candidate_dirs(dir.path(), PathBuf::new(), 0)
            .iter()
            .map(|p| paths::lossy(p))
            .collect();
        let nested = paths::lossy(&Path::new("Studio").join("Game Saves"));
        assert_eq!(found, vec!["", "Studio", nested.as_str()]);
    }
}
