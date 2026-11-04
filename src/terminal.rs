//! Terminal mode: run pipeline phases with plain log output, or preview what a
//! Steam sync would do.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use log::{error, info};

use crate::core::app::{exe_dir, find_app_dir};
use crate::core::config::Config;
use crate::core::package::{self, Progress};
use crate::core::pipeline::{Group, Pipeline};
use crate::core::share::{self, client, discovery::Peer, server::Server};
use crate::core::{detector, init, migrations, paths, scanner, sgdb, sizer, steam};

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

    let owner = steam::Owner::of(&config);
    for config_dir in steam::userdata_config_dirs() {
        let uid = steam::user_id(&config_dir);
        match steam::owned_shortcut_names(&config_dir, &owner.tag) {
            Ok(names) if names.is_empty() => {
                info!("  Steam user {uid}: no {} shortcuts yet", owner.label)
            }
            Ok(names) => {
                info!(
                    "  Steam user {uid}: {} existing {} shortcuts",
                    names.len(),
                    owner.label
                );
                names.iter().for_each(|n| info!("     - {n}"));
            }
            Err(e) => error!("  Steam user {uid}: {e}"),
        }
    }
    ExitCode::SUCCESS
}

/// Run `job` on a thread, printing its progress on stderr.
fn with_progress<T: Send>(job: impl FnOnce(&Progress) -> T + Send) -> T {
    let progress = Progress::default();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| job(&progress));
        loop {
            // Read before printing, so the last line printed is the final state.
            let finished = worker.is_finished();
            let (done, total) = (progress.done(), progress.total().max(1));
            eprint!(
                "\r  {:>3}%  {} / {}   ",
                done * 100 / total,
                sizer::human(done),
                sizer::human(total)
            );
            if finished {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        eprintln!();
        worker.join().expect("package worker panicked")
    })
}

fn report<T>(result: Result<T, String>, done: impl FnOnce(T)) -> ExitCode {
    match result {
        Ok(value) => {
            done(value);
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// `--pack GAME`: `GAME/` -> `GAME.cartridge.tar`, next to it.
pub fn pack(cli_dir: Option<&Path>, name: &str) -> ExitCode {
    let config = match load(cli_dir) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let games_dir = config.games_dir().unwrap_or_default();
    let Some(game) = scanner::load_library(&games_dir)
        .into_iter()
        .find(|g| g.folder_name == name && g.is_installed())
    else {
        error!(
            "No installed game folder named {name} in {}",
            games_dir.display()
        );
        return ExitCode::FAILURE;
    };
    let target = package::package_path(&games_dir, name);
    info!("Packing {} -> {}", game.title, target.display());
    report(with_progress(|p| package::pack(&game, &target, p)), |m| {
        info!("Packed {} files ({})", m.files, sizer::human(m.size))
    })
}

/// `--unpack GAME`: `GAME.cartridge.tar` -> `GAME/`, keeping the package.
pub fn unpack(cli_dir: Option<&Path>, name: &str) -> ExitCode {
    let config = match load(cli_dir) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let games_dir = config.games_dir().unwrap_or_default();
    let source = package::package_path(&games_dir, name);
    if !source.is_file() {
        error!("No package {}", source.display());
        return ExitCode::FAILURE;
    }
    info!("Unpacking {}", source.display());
    report(
        with_progress(|p| package::unpack(&source, &games_dir, p)),
        |dest| info!("Unpacked and verified into {}", dest.display()),
    )
}

/// `--serve`: share the games folder until killed (headless library machine).
pub fn serve(cli_dir: Option<&Path>) -> ExitCode {
    let config = match load(cli_dir) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let games_dir = config.games_dir().unwrap_or_default();
    let port = share::port(&config);
    match Server::start(&games_dir, share::machine_name(&config), port) {
        Ok(server) => {
            info!(
                "Other machines can't see this one? Open the port, e.g.:\n  \
                 sudo firewall-cmd --permanent --add-port={port}/tcp --add-port={port}/udp \
                 && sudo firewall-cmd --reload"
            );
            server.wait();
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("Cannot share on port {port}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `--peers`: •cartridge servers answering on the LAN, with their games.
pub fn peers(cli_dir: Option<&Path>) -> ExitCode {
    let config = match load(cli_dir) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let peers = client::find_peers(&config);
    if peers.is_empty() {
        info!("No •cartridge server found on the LAN.");
    }
    for peer in &peers {
        info!(
            "{} at {} (v{})",
            peer.info.name, peer.addr, peer.info.version
        );
        match client::list(peer.addr) {
            Ok(games) => games.iter().for_each(|g| {
                let size = g.size.map(sizer::human).unwrap_or_default();
                let state = if g.installed { "" } else { " [package]" };
                let compat = scanner::compatibility(&g.targets).label();
                info!("  {:<32} {size:>9}  {compat}{state}", g.id);
            }),
            Err(e) => error!("  Cannot list games: {e}"),
        }
    }
    ExitCode::SUCCESS
}

/// How `--download` / `--install` copy a game.
pub enum Copy {
    Download,
    Install,
}

/// A peer by the name `--peers` shows, else `host[:port]`.
fn resolve_peer(config: &Config, wanted: &str) -> Option<Peer> {
    client::find_peers(config)
        .into_iter()
        .find(|p| p.info.name.eq_ignore_ascii_case(wanted))
        .or_else(|| client::probe(wanted, share::port(config)))
}

/// `--download` / `--install PEER GAME`.
pub fn copy(cli_dir: Option<&Path>, peer: &str, game: &str, how: Copy) -> ExitCode {
    let config = match load(cli_dir) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let games_dir = config.games_dir().unwrap_or_default();
    let Some(peer) = resolve_peer(&config, peer) else {
        error!("No •cartridge server named or reachable at {peer}");
        return ExitCode::FAILURE;
    };
    info!("Copying {game} from {} ({})", peer.info.name, peer.addr);
    let result = with_progress(|p| match how {
        Copy::Download => client::download(peer.addr, game, &games_dir, p),
        Copy::Install => client::install(peer.addr, game, &games_dir, p),
    });
    report(result, |path| info!("Done, verified: {}", path.display()))
}
