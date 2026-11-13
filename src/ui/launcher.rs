//! Launch a game's resolved target (see `core::scanner`).
//!
//! Native targets are spawned directly (detached, `startIn` as the working dir).
//! A Windows target on Linux runs with Steam's Proton, in the prefix of its
//! Steam shortcut (`core::proton`); the prefix and its winetricks verbs are
//! prepared first, on the session's thread.
//!
//! Every launch returns a [`Session`] that reports whether the game is still
//! running, so the UI can stop reacting to the controller while it plays.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use log::{error, info};

use crate::core::detector;
use crate::core::models::Game;
use crate::core::platform::os_tag;
use crate::core::proton::{Downloads, Runner, CANCELLED};

/// A launched game. `running()` flips to false once the game has exited.
pub struct Session {
    pub title: String,
    running: Arc<AtomicBool>,
    /// What is being prepared before the game starts.
    step: Arc<Mutex<Option<String>>>,
    /// Why the game could not start.
    error: Arc<Mutex<Option<String>>>,
    /// Set to abandon the preparation.
    cancel: Arc<AtomicBool>,
}

impl Session {
    fn new(title: &str, running: Arc<AtomicBool>) -> Session {
        Session {
            title: title.to_string(),
            running,
            step: Arc::default(),
            error: Arc::default(),
            cancel: Arc::default(),
        }
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn step(&self) -> Option<String> {
        self.step.lock().ok()?.clone()
    }

    /// Why the game could not start (`None` too when the user cancelled).
    pub fn error(&self) -> Option<String> {
        self.outcome().filter(|message| message != CANCELLED)
    }

    /// The user cancelled the preparation.
    pub fn cancelled(&self) -> bool {
        self.outcome().as_deref() == Some(CANCELLED)
    }

    fn outcome(&self) -> Option<String> {
        self.error.lock().ok()?.clone()
    }

    /// Still getting ready (Proton prefix, winetricks): the game hasn't started.
    pub fn preparing(&self) -> bool {
        self.step().is_some()
    }

    /// Stop preparing; the game won't start.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn set(slot: &Mutex<Option<String>>, value: Option<String>) {
    if let Ok(mut slot) = slot.lock() {
        *slot = value;
    }
}

/// `downloads`: where winetricks keeps its downloads, for Windows games.
pub fn launch(game: &Game, downloads: &Downloads) -> Result<Session, String> {
    let resolved = game
        .resolved
        .as_ref()
        .ok_or("No executable target in game.json")?;
    if resolved.os == "windows" && os_tag() != "windows" {
        return launch_with_proton(game, downloads);
    }
    if !resolved.target.exists() {
        return Err(format!("Not found: {}", resolved.target.display()));
    }

    let exe = &resolved.target;
    detector::restore_exec_bit(exe);
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
    Ok(Session::new(&game.title, running))
}

/// Prepare the prefix, then run the game and wait for it, on a thread.
fn launch_with_proton(game: &Game, downloads: &Downloads) -> Result<Session, String> {
    if os_tag() != "linux" {
        return Err("Windows games run with Steam's Proton, on Linux".into());
    }
    let resolved = game.resolved.clone().ok_or("No executable target")?;
    if !resolved.target.exists() {
        return Err(format!("Not found: {}", resolved.target.display()));
    }
    let runner = Runner::for_game(game, downloads)?;
    let verbs = game.proton.winetricks.clone();
    let session = Session::new(&game.title, Arc::new(AtomicBool::new(true)));
    let (running, step, failure, cancel) = (
        session.running.clone(),
        session.step.clone(),
        session.error.clone(),
        session.cancel.clone(),
    );
    let title = game.title.clone();
    std::thread::spawn(move || {
        let played = runner
            .prepare(&verbs, |text| set(&step, Some(text.to_string())), &cancel)
            .and_then(|()| {
                set(&step, None);
                wait_quietly(runner.game_command(&resolved))
            });
        set(&step, None);
        if let Err(message) = played {
            if message == CANCELLED {
                info!("{title}: {message}");
            } else {
                error!("{title}: {message}");
            }
            set(&failure, Some(message));
        }
        running.store(false, Ordering::Relaxed);
    });
    Ok(session)
}

fn wait_quietly(mut cmd: Command) -> Result<(), String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(drop)
        .map_err(|e| format!("Launch failed: {e}"))
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
    use std::time::{Duration, Instant};

    #[test]
    fn a_cancelled_launch_is_not_an_error() {
        let session = Session::new("Rayman", Arc::default());
        set(&session.error, Some(CANCELLED.into()));
        assert!(session.cancelled());
        assert_eq!(session.error(), None);
        set(&session.error, Some("Proton crashed".into()));
        assert!(!session.cancelled());
        assert_eq!(session.error().as_deref(), Some("Proton crashed"));
    }

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
