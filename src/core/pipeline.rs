//! The pipeline: named phases run in order over an in-memory game list. Each
//! phase only does what is missing (no targets, unknown size, no artwork, a
//! stale shortcut), so a run over an unchanged library writes nothing. The
//! autopilot runs it at launch and whenever the games folder changes.

use std::path::PathBuf;

use log::info;

use super::config::Config;
use super::models::Game;
use super::{
    detector, manifest, migrations, mods, persister, scanner, sgdb, sizer, steam, steam_self,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Migrate,
    Scan,
    Detect,
    Measure,
    Enrich,
    Persist,
    SteamClean,
    SteamExport,
    Manifest,
    Patch,
}

impl Phase {
    pub const ALL: [Phase; 10] = [
        Phase::Migrate,
        Phase::Scan,
        Phase::Detect,
        Phase::Measure,
        Phase::Enrich,
        Phase::Persist,
        Phase::SteamClean,
        Phase::SteamExport,
        Phase::Manifest,
        Phase::Patch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Phase::Migrate => "Migration",
            Phase::Scan => "Scanning",
            Phase::Detect => "Detection",
            Phase::Measure => "Size",
            Phase::Enrich => "Enrichment",
            Phase::Persist => "Persistence",
            Phase::SteamClean => "Steam cleanup",
            Phase::SteamExport => "Steam export",
            Phase::Manifest => "Manifest",
            Phase::Patch => "Patcher",
        }
    }
}

/// How a run went.
#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    /// Every phase ran (not stopped early).
    pub completed: bool,
    /// Steam shortcuts were added, updated or removed: Steam shows them after a restart.
    pub steam_changed: bool,
    /// game.json files were written: the library on screen is out of date.
    pub games_changed: bool,
}

pub struct Pipeline<'a> {
    cfg: &'a Config,
    games_dir: PathBuf,
    games: Option<Vec<Game>>,
}

impl<'a> Pipeline<'a> {
    pub fn new(cfg: &'a Config) -> Pipeline<'a> {
        let games_dir = cfg.cartridge_dir().join(super::app::GAMES_DIR);
        Pipeline {
            cfg,
            games_dir,
            games: None,
        }
    }

    /// Run one phase, noting in `summary` what it changed.
    fn run_phase(&mut self, phase: Phase, summary: &mut Summary) {
        info!("\n--- {} ---", phase.label().to_uppercase());
        let cfg = self.cfg;
        if phase == Phase::Scan {
            self.games = Some(scanner::scan(&self.games_dir));
            return;
        }
        // An empty scan (e.g. games on an unmounted SD card) must not reach the
        // game phases: Steam cleanup would remove every owned shortcut.
        let games = self.games.as_mut().filter(|g| !g.is_empty());
        match (phase, games) {
            (Phase::Migrate, _) => {
                let migrated = migrations::run_all(&self.games_dir);
                summary.games_changed |= migrated.iter().any(|(_, count)| *count > 0);
            }
            (Phase::Detect, Some(games)) => detector::detect(games),
            (Phase::Measure, Some(games)) => sizer::measure(games),
            (Phase::Enrich, Some(games)) => sgdb::enrich(games, cfg),
            (Phase::Persist, Some(games)) => {
                summary.games_changed |= games.iter().any(|g| g.needs_persist && g.is_installed());
                persister::persist(games);
            }
            (Phase::SteamClean, Some(games)) => {
                summary.steam_changed |= steam::clean(games, cfg) > 0
            }
            (Phase::SteamExport, games) => {
                let exported = games.is_some_and(|games| steam::export(games, cfg) > 0);
                summary.steam_changed |= steam_self::register(cfg) || exported;
            }
            (Phase::Manifest, Some(games)) if cfg.is_not_false("MANIFEST_EXPORT") => {
                manifest::write(games, &self.games_dir.join("manifests.json"));
            }
            (Phase::Patch, _) => mods::run(cfg),
            _ => {}
        }
    }

    /// Run every phase, stopping before the next one once `keep_going` says no.
    pub fn run(&mut self, keep_going: impl Fn() -> bool) -> Summary {
        let mut summary = Summary::default();
        for phase in Phase::ALL {
            if !keep_going() {
                return summary;
            }
            self.run_phase(phase, &mut summary);
        }
        summary.completed = true;
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_run_starts_no_phase() {
        let cfg = Config::from_raw("/nowhere/system/config.txt".into(), Default::default());
        assert_eq!(Pipeline::new(&cfg).run(|| false), Summary::default());
    }
}
