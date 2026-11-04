//! The pipeline: named phases run in order over an in-memory game list, in
//! groups (`all`, `parse`, `steam`, `backup`, `patch`). Used by the terminal
//! mode and the UI's Status screen (which runs it on a thread and can cancel
//! between phases).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use log::info;

use super::config::Config;
use super::models::Game;
use super::{
    commands, detector, manifest, migrations, patcher, persister, saver, scanner, sgdb, sizer,
    steam, steam_self,
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
    Save,
    PostCommands,
}

impl Phase {
    pub const ALL: [Phase; 12] = [
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
        Phase::Save,
        Phase::PostCommands,
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
            Phase::Save => "Saver",
            Phase::PostCommands => "Post commands",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    All,
    Parse,
    Steam,
    Backup,
    Patch,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::All,
        Group::Parse,
        Group::Steam,
        Group::Backup,
        Group::Patch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::All => "all",
            Group::Parse => "parse",
            Group::Steam => "steam",
            Group::Backup => "backup",
            Group::Patch => "patch",
        }
    }

    pub fn from_name(name: &str) -> Option<Group> {
        Group::ALL
            .into_iter()
            .find(|g| g.name().eq_ignore_ascii_case(name))
    }

    pub fn phases(self) -> &'static [Phase] {
        use Phase::*;
        match self {
            Group::All => &Phase::ALL,
            Group::Parse => &[Migrate, Scan, Detect, Measure, Enrich, Persist],
            Group::Steam => &[Scan, SteamClean, SteamExport],
            Group::Backup => &[Scan, Save],
            Group::Patch => &[Patch],
        }
    }
}

pub enum Event {
    Started(Phase),
    Finished(Phase),
}

pub struct Pipeline<'a> {
    cfg: &'a Config,
    games_dir: PathBuf,
    games: Option<Vec<Game>>,
}

impl<'a> Pipeline<'a> {
    pub fn new(cfg: &'a Config) -> Pipeline<'a> {
        let games_dir = cfg.workspace().join(super::app::GAMES_DIR);
        Pipeline {
            cfg,
            games_dir,
            games: None,
        }
    }

    pub fn run_phase(&mut self, phase: Phase) {
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
                migrations::run_all(&self.games_dir);
            }
            (Phase::Detect, Some(games)) => detector::detect(games),
            (Phase::Measure, Some(games)) => sizer::measure(games),
            (Phase::Enrich, Some(games)) => sgdb::enrich(games, cfg),
            (Phase::Persist, Some(games)) if cfg.is_not_false("PERSIST_DATA") => {
                persister::persist(games)
            }
            (Phase::SteamClean, Some(games)) => steam::clean(games, cfg),
            (Phase::SteamExport, games) => {
                if let Some(games) = games {
                    steam::export(games, cfg);
                }
                steam_self::register(cfg);
            }
            (Phase::Manifest, Some(games)) if cfg.is_not_false("MANIFEST_EXPORT") => {
                manifest::write(games, &self.games_dir.join("manifests.json"));
            }
            (Phase::Patch, _) => patcher::run(cfg),
            (Phase::Save, Some(games)) => saver::run(games, cfg),
            (Phase::PostCommands, _) => commands::run(cfg),
            _ => {}
        }
    }

    /// Run `group`, stopping early if `cancel` is raised. Returns false when cancelled.
    pub fn run_group(
        &mut self,
        group: Group,
        cancel: &AtomicBool,
        mut on_event: impl FnMut(Event),
    ) -> bool {
        for &phase in group.phases() {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            on_event(Event::Started(phase));
            self.run_phase(phase);
            on_event(Event::Finished(phase));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_parse_by_name() {
        assert_eq!(Group::from_name("Steam"), Some(Group::Steam));
        assert_eq!(Group::from_name("nope"), None);
        assert_eq!(Group::All.phases().len(), 12);
    }

    #[test]
    fn cancel_stops_before_the_next_phase() {
        let cfg = Config::from_raw("/nowhere/.cartridge/config.txt".into(), Default::default());
        let cancel = AtomicBool::new(true);
        let mut started = 0;
        assert!(!Pipeline::new(&cfg).run_group(Group::Patch, &cancel, |_| started += 1));
        assert_eq!(started, 0);
    }
}
