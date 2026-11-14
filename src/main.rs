//! •cartridge: manage DRM-free games on Linux / Steam Deck. Opens the retro UI by
//! default; `--serve` runs the library without it.

mod cli;
mod core;
mod headless;
mod ui;

use std::process::ExitCode;

use cli::Mode;
use headless::Copy;

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
            eprintln!("error: no display detected; run `cartridge --serve` for a headless library");
            ExitCode::FAILURE
        }
        Mode::Pack(name) => headless::with_cartridge(dir, |_, games| headless::pack(games, &name)),
        Mode::Unpack(name) => {
            headless::with_cartridge(dir, |_, games| headless::unpack(games, &name))
        }
        Mode::Serve => headless::with_cartridge(dir, headless::serve),
        Mode::Peers => headless::with_cartridge(dir, |cfg, _| headless::peers(cfg)),
        Mode::Download { peer, game } => headless::with_cartridge(dir, |cfg, games| {
            headless::copy(cfg, games, &peer, &game, Copy::Download)
        }),
        Mode::Install { peer, game } => headless::with_cartridge(dir, |cfg, games| {
            headless::copy(cfg, games, &peer, &game, Copy::Install)
        }),
    }
}
