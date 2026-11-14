//! The pipeline: named phases run in order over an in-memory game list. Each
//! phase only does what is missing (no targets, unknown size, no artwork, a
//! stale shortcut), so a run over an unchanged library writes nothing. The
//! autopilot runs it at launch and whenever the games folder changes.
//!
//! The game phases (detect, measure, enrich, persist) go through the games in
//! batches, each batch written before the next: a long first run over a big
//! new library can stop early (a game starts, the editor opens) and pick up
//! where it was, and new games appear in the library as they are done.

use std::ops::Range;
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
    /// Over the whole library, before the games.
    const LEAD: [Phase; 2] = [Phase::Migrate, Phase::Scan];
    /// Per batch of games.
    const GAMES: [Phase; 4] = [Phase::Detect, Phase::Measure, Phase::Enrich, Phase::Persist];
    /// Over the whole library, after the games.
    const TAIL: [Phase; 4] = [
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

/// Games per batch of the game phases.
const BATCH: usize = 10;

/// The batches of a library of `total` games.
fn batches(total: usize) -> Vec<Range<usize>> {
    (0..total)
        .step_by(BATCH)
        .map(|start| start..(start + BATCH).min(total))
        .collect()
}

/// Where a run is, for a progress display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub phase: Phase,
    /// Game phases: games done before this batch, out of the library's.
    pub games: Option<(usize, usize)>,
    /// game.json files written so far in this run: the library on screen can
    /// show them before the run ends.
    pub written: usize,
}

impl Step {
    /// How far along the run is (0..1). The game phases count as one stage,
    /// filled batch by batch.
    pub fn fraction(self) -> f32 {
        let stages = (Phase::LEAD.len() + 1 + Phase::TAIL.len()) as f32;
        let position = |phases: &[Phase]| phases.iter().position(|p| *p == self.phase);
        let stage = match self.games {
            Some((done, total)) => Phase::LEAD.len() as f32 + done as f32 / total.max(1) as f32,
            None => position(&Phase::LEAD)
                .or_else(|| position(&Phase::TAIL).map(|i| Phase::LEAD.len() + 1 + i))
                .unwrap_or(0) as f32,
        };
        stage / stages
    }

    /// `Detection 10/200` for a game phase, else the phase's name.
    pub fn label(self) -> String {
        match self.games {
            Some((done, total)) => format!("{} {done}/{total}", self.phase.label()),
            None => self.phase.label().to_string(),
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
    /// How many game.json files the game phases wrote.
    pub games_written: usize,
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

    /// Run a whole-library phase, noting in `summary` what it changed.
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

    /// Run a game phase over the games of `batch`. No header: the phases
    /// log only what they do, and batches repeat them.
    fn run_game_phase(&mut self, phase: Phase, batch: Range<usize>, summary: &mut Summary) {
        let Some(games) = self.games.as_mut().map(|games| &mut games[batch]) else {
            return;
        };
        match phase {
            Phase::Detect => detector::detect(games),
            Phase::Measure => sizer::measure(games),
            Phase::Enrich => sgdb::enrich(games, self.cfg),
            Phase::Persist => {
                let dirty = games
                    .iter()
                    .filter(|g| g.needs_persist && g.is_installed())
                    .count();
                summary.games_written += dirty;
                summary.games_changed |= dirty > 0;
                persister::persist(games);
            }
            _ => {}
        }
    }

    /// Run every phase, stopping before the next one once `keep_going` says
    /// no; `on_step` hears of each step as it starts.
    pub fn run(&mut self, keep_going: impl Fn() -> bool, mut on_step: impl FnMut(Step)) -> Summary {
        let mut summary = Summary::default();
        let mut start = |phase, games, written| {
            let going = keep_going();
            if going {
                on_step(Step {
                    phase,
                    games,
                    written,
                });
            }
            going
        };
        for phase in Phase::LEAD {
            if !start(phase, None, 0) {
                return summary;
            }
            self.run_phase(phase, &mut summary);
        }
        info!("\n--- GAMES ---");
        let total = self.games.as_ref().map_or(0, Vec::len);
        for batch in batches(total) {
            for phase in Phase::GAMES {
                if !start(phase, Some((batch.start, total)), summary.games_written) {
                    return summary;
                }
                self.run_game_phase(phase, batch.clone(), &mut summary);
            }
        }
        for phase in Phase::TAIL {
            if !start(phase, None, summary.games_written) {
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
        let mut started = 0;
        assert_eq!(
            Pipeline::new(&cfg).run(|| false, |_| started += 1),
            Summary::default()
        );
        assert_eq!(started, 0);
    }

    #[test]
    fn games_go_in_batches_of_ten() {
        assert_eq!(batches(23), [0..10, 10..20, 20..23]);
        assert!(batches(0).is_empty());
    }

    #[test]
    fn progress_fills_through_the_game_batches() {
        let at = |phase, games| {
            Step {
                phase,
                games,
                written: 0,
            }
            .fraction()
        };
        assert_eq!(at(Phase::Migrate, None), 0.0);
        let half_the_games = at(Phase::Persist, Some((100, 200)));
        assert!(at(Phase::Scan, None) < half_the_games);
        assert!(half_the_games < at(Phase::SteamClean, None));
        assert!(at(Phase::Patch, None) < 1.0);
    }

    #[test]
    fn a_game_step_names_its_count() {
        let step = Step {
            phase: Phase::Detect,
            games: Some((10, 200)),
            written: 0,
        };
        assert_eq!(step.label(), "Detection 10/200");
    }
}
