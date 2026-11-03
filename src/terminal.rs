//! Terminal mode: run pipeline phases with plain log output, or preview what a
//! Steam sync would do.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use log::{error, info};

use crate::core::app::{app_name, exe_dir, find_app_dir};
use crate::core::config::Config;
use crate::core::pipeline::{Group, Pipeline};
use crate::core::{detector, init, migrations, paths, scanner, sgdb, steam};

/// The workspace config, running the terminal setup wizard on first use.
pub fn locate_config(cli_dir: Option<&Path>) -> Option<PathBuf> {
    match find_app_dir(cli_dir) {
        Some(app_dir) => Some(init::config_in(&app_dir)),
        None => init::run_terminal_wizard(&exe_dir(), &std::env::current_dir().unwrap_or_default()),
    }
}

fn load(cli_dir: Option<&Path>) -> Result<Config, ExitCode> {
    let config = Config::load(&locate_config(cli_dir).ok_or(ExitCode::SUCCESS)?);
    if config.games_dir().is_none() {
        error!("FREEGAMES_PATH not configured or invalid. Stopping the app.");
        return Err(ExitCode::FAILURE);
    }
    Ok(config)
}

pub fn run(cli_dir: Option<&Path>, group: Group) -> ExitCode {
    let config = match load(cli_dir) {
        Ok(config) => config,
        Err(code) => return code,
    };
    Pipeline::new(&config).run_group(group, &AtomicBool::new(false), |_| {});
    ExitCode::SUCCESS
}

/// Dry run: what a Steam sync would create, without touching Steam.
pub fn preview(cli_dir: Option<&Path>) -> ExitCode {
    let config = match load(cli_dir) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let games_dir = config.games_dir().unwrap_or_default();
    migrations::run_all(&games_dir);
    let mut games = scanner::scan(&games_dir);
    detector::detect(&mut games);
    sgdb::enrich(&mut games, &config);

    let with_targets: Vec<_> = games
        .iter()
        .filter_map(|g| g.resolved.as_ref().map(|r| (g, r)))
        .collect();
    info!("Steam preview: {} game(s) with targets", with_targets.len());
    info!(
        "SteamGridDB API key: {}",
        if sgdb::api_key(&config).is_some() {
            "configured"
        } else {
            "NOT SET"
        }
    );
    info!(
        "Steam sync (STEAM_EXPOSE): {}\n",
        if config.is_true("STEAM_EXPOSE") {
            "on"
        } else {
            "off"
        }
    );

    for (game, resolved) in with_targets {
        let appid = steam::generate_appid(&game.title, &paths::lossy(&resolved.target));
        info!("  {}", game.title);
        info!(
            "     AppID:    {appid} (signed: {})",
            steam::signed32(appid)
        );
        info!("     Exe:      {}", resolved.target.display());
        info!("     StartDir: {}", resolved.start_in.display());
        if !resolved.launch_options.is_empty() {
            info!("     Options:  {}", resolved.launch_options);
        }
        info!(
            "     SGDB ID:  {}",
            game.steamgriddb_id
                .map_or("none".to_string(), |id| id.to_string())
        );
        for slot in crate::core::models::ImageSlot::ALL.iter().take(4) {
            info!(
                "     {}: {}",
                slot.name(),
                game.images.get(*slot).unwrap_or("none")
            );
        }
        info!("");
    }

    for config_dir in steam::userdata_config_dirs() {
        let uid = steam::user_id(&config_dir);
        match steam::owned_shortcut_names(&config_dir) {
            Ok(names) if names.is_empty() => {
                info!("  Steam user {uid}: no {} shortcuts yet", app_name())
            }
            Ok(names) => {
                info!(
                    "  Steam user {uid}: {} existing {} shortcuts",
                    names.len(),
                    app_name()
                );
                names.iter().for_each(|n| info!("     - {n}"));
            }
            Err(e) => error!("  Steam user {uid}: {e}"),
        }
    }
    ExitCode::SUCCESS
}
