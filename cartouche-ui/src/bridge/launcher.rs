//! Launch a game for the current platform.
//!
//! Target selection mirrors `_pick_target_entry` in the Python `lib/detector.py`:
//! narrow by OS, then by arch — each step prefers an exact match, then a blank /
//! `any` value, then keeps the whole pool — and take the first remaining entry.
//!
//! Native targets are spawned directly (detached, `startIn` as the working dir).
//! A Windows target on Linux/macOS is started through its Steam non-Steam
//! shortcut, so it runs under the Proton prefix the backend configured (and that
//! the save paths point to). The shortcut appid is derived exactly like
//! `generate_appid` in `lib/steam_exporter.py`.
//!
//! Every launch returns a [`Session`] that reports whether the game is still
//! running, so the UI can stop reacting to the controller while it plays.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::models::Target;

/// Canonical OS tag, matching Python's `platform_info.os_tag()`.
pub fn os_tag() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "macos",
        _ => "other",
    }
}

/// Canonical arch tag, matching Python's `platform_info.arch_tag()`.
pub fn arch_tag() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" | "arm" => "arm64",
        "x86_64" | "x86" => "x64",
        _ => "other",
    }
}

fn filter<'a>(pool: Vec<&'a Target>, key: fn(&Target) -> &str, value: &str) -> Vec<&'a Target> {
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

/// Best target for the current OS/arch, or `None` if the game has no targets.
pub fn pick_target(targets: &[Target]) -> Option<&Target> {
    let pool: Vec<&Target> = targets
        .iter()
        .filter(|t| !t.target.trim().is_empty())
        .collect();
    let pool = filter(pool, |t| &t.os, os_tag());
    let pool = filter(pool, |t| &t.arch, arch_tag());
    pool.first().copied()
}

/// A launched game. `running()` flips to false once the game has exited.
pub struct Session {
    pub title: String,
    running: Arc<AtomicBool>,
}

impl Session {
    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// Launch `game_dir`'s best target.
pub fn launch(title: &str, game_dir: &Path, targets: &[Target]) -> Result<Session, String> {
    let target = pick_target(targets).ok_or("No executable target in game.json")?;
    let exe = normalize(&game_dir.join(&target.target));
    let start_in = if target.start_in.trim().is_empty() {
        exe.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| game_dir.to_path_buf())
    } else {
        normalize(&game_dir.join(&target.start_in))
    };

    let is_windows_target = target.os.eq_ignore_ascii_case("windows");
    if is_windows_target && os_tag() != "windows" {
        return launch_via_steam(title, &exe);
    }

    if !exe.exists() {
        return Err(format!("Not found: {}", exe.display()));
    }

    let mut cmd = if os_tag() == "macos" && exe.extension().is_some_and(|e| e == "app") {
        let mut c = Command::new("open");
        c.arg("-a").arg(&exe);
        if !target.launch_options.trim().is_empty() {
            c.arg("--args");
        }
        c
    } else {
        Command::new(&exe)
    };
    cmd.args(target.launch_options.split_whitespace())
        .current_dir(&start_in);
    let running = spawn_detached(cmd).map_err(|e| format!("Launch failed: {e}"))?;
    Ok(Session {
        title: title.to_string(),
        running,
    })
}

/// Start a Windows game via its Steam shortcut (`steam://rungameid/<gameid>`).
fn launch_via_steam(title: &str, exe: &Path) -> Result<Session, String> {
    let appid = shortcut_appid(title, &exe.to_string_lossy());
    let game_id = ((appid as u64) << 32) | 0x0200_0000;
    let url = format!("steam://rungameid/{game_id}");

    let opener = if os_tag() == "macos" {
        "open"
    } else {
        "xdg-open"
    };
    let mut cmd = Command::new(opener);
    cmd.arg(&url);
    spawn_detached(cmd)
        .map_err(|e| format!("Windows game: could not open Steam ({e}). Run Steam Sync first."))?;
    Ok(Session {
        title: title.to_string(),
        running: watch_steam_app(appid),
    })
}

/// Track a Steam-launched game through its `reaper SteamLaunch AppId=<appid>`
/// process (Linux `/proc`). Waits for it to appear, then for it to exit. If it
/// never shows up (Steam missing, no `/proc`), the session ends after a timeout
/// so the UI never stays locked.
fn watch_steam_app(appid: u32) -> Arc<AtomicBool> {
    const APPEAR_TIMEOUT: Duration = Duration::from_secs(45);
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    let needle = format!("AppId={appid}");
    std::thread::spawn(move || {
        let start = Instant::now();
        while !process_with_arg(&needle) {
            if start.elapsed() > APPEAR_TIMEOUT {
                flag.store(false, Ordering::Relaxed);
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        while process_with_arg(&needle) {
            std::thread::sleep(Duration::from_secs(1));
        }
        flag.store(false, Ordering::Relaxed);
    });
    running
}

/// True if any process has `arg` as one of its command-line arguments.
fn process_with_arg(arg: &str) -> bool {
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return false;
    };
    procs.filter_map(|e| e.ok()).any(|e| {
        std::fs::read(e.path().join("cmdline"))
            .map(|c| c.split(|&b| b == 0).any(|a| a == arg.as_bytes()))
            .unwrap_or(false)
    })
}

/// Unsigned non-Steam shortcut appid, identical to the Python `generate_appid`.
fn shortcut_appid(title: &str, exe: &str) -> u32 {
    crc32(format!("{exe}{title}").as_bytes()) | 0x8000_0000
}

/// Standard CRC-32 (IEEE), same as `zlib.crc32`.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Lexical path cleanup like Python's `os.path.normpath` (drops `.`, folds `..`),
/// so the exe path — and thus the Steam appid — matches the backend's exactly.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Spawn without inheriting our stdio, and reap the child on a thread so it
/// never lingers as a zombie. The game keeps running if the UI exits. The
/// returned flag goes false when the child exits.
fn spawn_detached(mut cmd: Command) -> std::io::Result<Arc<AtomicBool>> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        flag.store(false, Ordering::Relaxed);
    });
    Ok(running)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(os: &str, arch: &str, target: &str) -> Target {
        Target {
            os: os.into(),
            arch: arch.into(),
            target: target.into(),
            ..Default::default()
        }
    }

    #[test]
    fn crc32_matches_zlib() {
        // zlib.crc32(b"123456789") == 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn appid_matches_python_generate_appid() {
        // lib.steam_exporter.generate_appid('Hollow Knight', '/g/Hollow Knight/hollow_knight.exe')
        assert_eq!(
            shortcut_appid("Hollow Knight", "/g/Hollow Knight/hollow_knight.exe"),
            3770780903
        );
    }

    #[test]
    fn prefers_current_os_then_any_then_first() {
        let other = if os_tag() == "windows" {
            "linux"
        } else {
            "windows"
        };
        let targets = vec![t(other, "", "a"), t(os_tag(), "", "b")];
        assert_eq!(pick_target(&targets).unwrap().target, "b");

        let targets = vec![t(other, "", "a"), t("any", "", "c")];
        assert_eq!(pick_target(&targets).unwrap().target, "c");

        let targets = vec![t(other, "", "a")];
        assert_eq!(pick_target(&targets).unwrap().target, "a");
    }

    #[test]
    fn prefers_current_arch() {
        let other = if arch_tag() == "arm64" {
            "x64"
        } else {
            "arm64"
        };
        let targets = vec![t(os_tag(), other, "a"), t(os_tag(), arch_tag(), "b")];
        assert_eq!(pick_target(&targets).unwrap().target, "b");
    }

    #[cfg(unix)]
    #[test]
    fn session_ends_when_process_exits() {
        let running = spawn_detached(Command::new("true")).unwrap();
        let start = Instant::now();
        while running.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!running.load(Ordering::Relaxed));
    }

    #[test]
    fn normalize_folds_dots() {
        assert_eq!(
            normalize(Path::new("/g/Game/./bin/../run.sh")),
            PathBuf::from("/g/Game/run.sh")
        );
    }
}
