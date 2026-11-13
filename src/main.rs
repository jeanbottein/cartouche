//! •cartridge: manage DRM-free games on Linux / Steam Deck. Opens the retro UI by
//! default; `-t` runs the pipeline in the terminal instead.

mod cli;
mod core;
mod terminal;
mod ui;

use std::process::ExitCode;

use cli::Mode;
use core::pipeline::Group;
use terminal::Copy;

fn main() -> ExitCode {
    core::logging::init();
    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("error: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    if let Some(dir) = args.dir.as_ref().filter(|d| !d.is_dir()) {
        eprintln!("error: {} is not a directory", dir.display());
        return ExitCode::from(2);
    }
    // Absolute, so the self shortcut's StartDir and config symlinks work from anywhere.
    let dir = args.dir.as_deref().map(core::paths::resolve);
    let dir = dir.as_deref();
    match args.mode {
        Mode::Help => {
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Mode::Version => {
            println!("{} {}", core::app::APP_NAME, env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Mode::Ui if core::app::has_display() => {
            ui::run(dir);
            ExitCode::SUCCESS
        }
        Mode::Ui => {
            log::info!("No display detected. Running in terminal mode.");
            terminal::with_cartridge(dir, |cfg, _| terminal::run(cfg, Group::All))
        }
        Mode::Terminal(group) => terminal::with_cartridge(dir, |cfg, _| terminal::run(cfg, group)),
        Mode::DryRun => terminal::with_cartridge(dir, terminal::preview),
        Mode::Pack(name) => terminal::with_cartridge(dir, |_, games| terminal::pack(games, &name)),
        Mode::Unpack(name) => {
            terminal::with_cartridge(dir, |_, games| terminal::unpack(games, &name))
        }
        Mode::Serve => terminal::with_cartridge(dir, terminal::serve),
        Mode::Peers => terminal::with_cartridge(dir, |cfg, _| terminal::peers(cfg)),
        Mode::Download { peer, game } => terminal::with_cartridge(dir, |cfg, games| {
            terminal::copy(cfg, games, &peer, &game, Copy::Download)
        }),
        Mode::Install { peer, game } => terminal::with_cartridge(dir, |cfg, games| {
            terminal::copy(cfg, games, &peer, &game, Copy::Install)
        }),
    }
}
