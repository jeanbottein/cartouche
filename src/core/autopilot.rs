//! Autopilot: keeps the library up to date without a button. A worker thread
//! runs the pipeline at launch, then whenever the games folder changes.
//!
//! It lists `games/` every [`TICK`]: the kernel answers from its cache, so an
//! unchanged folder costs no disk read. A new entry is only processed once it
//! stopped changing (a copy still running would get a half-copied game's
//! executable detected and its size recorded for good). Our own unpacks and
//! downloads arrive under hidden staging names, which the listing ignores.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::app::GAMES_DIR;
use super::config::Config;
use super::pipeline::{Pipeline, Summary};
use super::{package, paths};

pub const TICK: Duration = Duration::from_secs(2);

/// What `games/` holds: game folder names, and each package's size and mtime.
/// Only names for folders: our own runs write inside them.
#[derive(Clone, Debug, Default, PartialEq)]
struct Listing(BTreeMap<String, Option<(u64, SystemTime)>>);

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn listing(games_dir: &Path) -> Listing {
    let folders = paths::visible_subdirs(games_dir)
        .into_iter()
        .map(|dir| (name_of(&dir), None));
    let packages = package::list(games_dir).into_iter().map(|file| {
        let meta = std::fs::metadata(&file).ok();
        let version = meta.and_then(|m| Some((m.len(), m.modified().ok()?)));
        (name_of(&file), version)
    });
    Listing(folders.chain(packages).collect())
}

/// Files, bytes and newest change under a path: the same twice in a row
/// once a copy is done.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Stamp {
    files: u64,
    bytes: u64,
    newest: Option<SystemTime>,
}

impl Stamp {
    fn add(self, other: Stamp) -> Stamp {
        Stamp {
            files: self.files + other.files,
            bytes: self.bytes + other.bytes,
            newest: self.newest.max(other.newest),
        }
    }
}

/// Symbolic links inside a game are not followed.
fn stamp(path: &Path) -> Stamp {
    let Ok(entries) = std::fs::read_dir(path) else {
        let meta = std::fs::metadata(path).ok();
        return Stamp {
            files: 1,
            bytes: meta.as_ref().map_or(0, |m| m.len()),
            newest: meta.and_then(|m| m.modified().ok()),
        };
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => stamp(&entry.path()),
            _ => {
                let meta = entry.metadata().ok();
                Stamp {
                    files: 1,
                    bytes: meta.as_ref().map_or(0, |m| m.len()),
                    newest: meta.and_then(|m| m.modified().ok()),
                }
            }
        })
        .fold(Stamp::default(), Stamp::add)
}

/// Watches `games/` for added, changed and removed entries.
pub struct Watcher {
    games_dir: PathBuf,
    known: Listing,
    /// A change seen at the last check, with the stamps of its new entries.
    pending: Option<(Listing, BTreeMap<String, Stamp>)>,
}

impl Watcher {
    pub fn new(games_dir: &Path) -> Watcher {
        Watcher {
            games_dir: games_dir.to_path_buf(),
            known: listing(games_dir),
            pending: None,
        }
    }

    /// One check; true once the folder changed and the change settled (the
    /// same at two checks in a row). A removal settles at the next check too,
    /// so a rename is seen as one change.
    pub fn tick(&mut self) -> bool {
        let now = listing(&self.games_dir);
        if now == self.known {
            self.pending = None;
            return false;
        }
        let stamps: BTreeMap<String, Stamp> = now
            .0
            .iter()
            .filter(|(name, version)| self.known.0.get(*name) != Some(version))
            .map(|(name, _)| (name.clone(), stamp(&self.games_dir.join(name))))
            .collect();
        let settled = self
            .pending
            .as_ref()
            .is_some_and(|(seen, seen_stamps)| *seen == now && *seen_stamps == stamps);
        if settled {
            self.known = now;
            self.pending = None;
        } else {
            self.pending = Some((now, stamps));
        }
        settled
    }
}

enum Command {
    Config(Config),
    Run,
}

/// The worker thread and how to steer it. Dropping it stops the thread
/// after its current phase.
pub struct Autopilot {
    commands: Sender<Command>,
    reports: Receiver<Summary>,
    paused: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl Autopilot {
    /// Start watching `cfg`'s games folder, with a first run right away.
    pub fn start(cfg: Config) -> Autopilot {
        let (commands, inbox) = channel();
        let (outbox, reports) = channel();
        let paused = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            watcher: Watcher::new(&cfg.cartridge_dir().join(GAMES_DIR)),
            cfg,
            inbox,
            outbox,
            paused: Arc::clone(&paused),
            running: Arc::clone(&running),
        };
        std::thread::spawn(move || worker.work());
        Autopilot {
            commands,
            reports,
            paused,
            running,
        }
    }

    /// Use this config from the next run on (after Settings changed it).
    pub fn set_config(&self, cfg: Config) {
        let _ = self.commands.send(Command::Config(cfg));
    }

    /// Run again soon, even if the folder didn't change (a setting or a game changed).
    pub fn request_run(&self) {
        let _ = self.commands.send(Command::Run);
    }

    /// While paused (a game plays, the editor is open), nothing runs or is
    /// checked; a run in progress stops after its current phase and starts
    /// over once resumed.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Runs that ended since the last call.
    pub fn finished(&self) -> Vec<Summary> {
        self.reports.try_iter().collect()
    }

    /// Wait for the next run to end (`None`: the worker stopped).
    pub fn next_run(&self) -> Option<Summary> {
        self.reports.recv().ok()
    }
}

struct Worker {
    cfg: Config,
    watcher: Watcher,
    inbox: Receiver<Command>,
    outbox: Sender<Summary>,
    paused: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl Worker {
    fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    fn work(mut self) {
        let (mut wanted, mut folder_changed) = (true, false);
        loop {
            if wanted && !self.paused() {
                let mut summary = self.run();
                summary.games_changed |= folder_changed;
                // A run stopped early starts over, still owing the folder change.
                wanted = !summary.completed;
                folder_changed &= wanted;
                if self.outbox.send(summary).is_err() {
                    return;
                }
            }
            match self.inbox.recv_timeout(TICK) {
                Ok(Command::Config(cfg)) => self.cfg = cfg,
                Ok(Command::Run) => wanted = true,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if !self.paused() && self.watcher.tick() {
                (wanted, folder_changed) = (true, true);
            }
        }
    }

    fn run(&self) -> Summary {
        self.running.store(true, Ordering::Relaxed);
        let summary = Pipeline::new(&self.cfg).run(|| !self.paused());
        self.running.store(false, Ordering::Relaxed);
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    fn watching(dir: &TempDir) -> Watcher {
        Watcher::new(&dir.mkdir("games"))
    }

    #[test]
    fn an_unchanged_folder_is_no_change() {
        let dir = TempDir::new();
        dir.mkdir("games/Celeste");
        let mut watcher = watching(&dir);
        assert!(!watcher.tick());
        assert!(!watcher.tick());
    }

    #[test]
    fn a_new_folder_counts_once_it_stops_changing() {
        let dir = TempDir::new();
        let mut watcher = watching(&dir);
        dir.write("games/Celeste/part1", b"1");
        assert!(!watcher.tick());
        dir.write("games/Celeste/data/part2", b"22"); // still copying
        assert!(!watcher.tick());
        assert!(watcher.tick());
        assert!(!watcher.tick());
    }

    #[test]
    fn a_growing_package_counts_once_it_stops_changing() {
        let dir = TempDir::new();
        let mut watcher = watching(&dir);
        dir.write("games/Hades.cartridge.tar", b"1");
        assert!(!watcher.tick());
        dir.write("games/Hades.cartridge.tar", b"12");
        assert!(!watcher.tick());
        assert!(watcher.tick());
    }

    #[test]
    fn a_removed_folder_counts_at_the_next_check() {
        let dir = TempDir::new();
        let game = dir.mkdir("games/Celeste");
        let mut watcher = watching(&dir);
        std::fs::remove_dir(game).unwrap();
        assert!(!watcher.tick());
        assert!(watcher.tick());
    }

    #[test]
    fn a_folder_gone_before_settling_is_no_change() {
        let dir = TempDir::new();
        let mut watcher = watching(&dir);
        let game = dir.mkdir("games/Celeste");
        assert!(!watcher.tick());
        std::fs::remove_dir(game).unwrap();
        assert!(!watcher.tick());
        assert!(!watcher.tick());
    }

    #[test]
    fn staging_and_other_files_are_ignored() {
        let dir = TempDir::new();
        let mut watcher = watching(&dir);
        dir.write("games/.Celeste.part/data", b"1");
        dir.write("games/manifests.json", b"[]");
        assert!(!watcher.tick());
    }

    #[test]
    fn writing_inside_a_known_game_is_no_change() {
        let dir = TempDir::new();
        dir.mkdir("games/Celeste");
        let mut watcher = watching(&dir);
        dir.write("games/Celeste/.cartridge/game.json", b"{}");
        assert!(!watcher.tick());
    }
}
