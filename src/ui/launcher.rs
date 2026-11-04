//! Launch a game's resolved target (see `core::scanner`).
//!
//! Native targets are spawned directly (detached, `startIn` as the working dir).
//! A Windows target on Linux/macOS is started through its Steam non-Steam
//! shortcut, so it runs under the Proton prefix the pipeline configured (and
//! that its save paths point to).
//!
//! Every launch returns a [`Session`] that reports whether the game is still
//! running, so the UI can stop reacting to the controller while it plays.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::models::Game;
use crate::core::platform::os_tag;
use crate::core::{paths, steam};

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

pub fn launch(game: &Game) -> Result<Session, String> {
    let resolved = game
        .resolved
        .as_ref()
        .ok_or("No executable target in game.json")?;
    if resolved.os == "windows" && os_tag() != "windows" {
        return launch_via_steam(&game.title, &resolved.target);
    }
    if !resolved.target.exists() {
        return Err(format!("Not found: {}", resolved.target.display()));
    }

    let exe = &resolved.target;
    let mut cmd = if os_tag() == "macos" && exe.extension().is_some_and(|e| e == "app") {
        let mut c = Command::new("open");
        c.arg("-a").arg(exe);
        if !resolved.launch_options.trim().is_empty() {
            c.arg("--args");
        }
        c
    } else {
        Command::new(exe)
    };
    cmd.args(resolved.launch_options.split_whitespace())
        .current_dir(&resolved.start_in);
    let running = spawn_detached(cmd).map_err(|e| format!("Launch failed: {e}"))?;
    Ok(Session {
        title: game.title.clone(),
        running,
    })
}

/// Start a Windows game via its Steam shortcut (`steam://rungameid/<gameid>`).
fn launch_via_steam(title: &str, exe: &Path) -> Result<Session, String> {
    let appid = steam::generate_appid(title, &paths::lossy(exe));
    let game_id = ((appid as u64) << 32) | 0x0200_0000;
    let opener = if os_tag() == "macos" {
        "open"
    } else {
        "xdg-open"
    };
    let mut cmd = Command::new(opener);
    cmd.arg(format!("steam://rungameid/{game_id}"));
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
            .is_ok_and(|c| c.split(|&b| b == 0).any(|a| a == arg.as_bytes()))
    })
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn session_ends_when_process_exits() {
        let running = spawn_detached(Command::new("true")).unwrap();
        let start = Instant::now();
        while running.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!running.load(Ordering::Relaxed));
    }
}
