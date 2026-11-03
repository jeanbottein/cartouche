//! Cartouche: manage DRM-free games on Linux / Steam Deck. Opens the retro UI by
//! default; `-t` runs the pipeline in the terminal instead.

mod cli;
mod core;
mod terminal;
mod ui;

use std::process::ExitCode;

use cli::Mode;
use core::pipeline::Group;

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
    let dir = args.dir.as_deref();
    match args.mode {
        Mode::Help => println!("{}", cli::USAGE),
        Mode::Version => println!("{} {}", core::app::app_name(), env!("CARGO_PKG_VERSION")),
        Mode::Terminal(group) => return terminal::run(dir, group),
        Mode::DryRun => return terminal::preview(dir),
        Mode::Ui if core::app::has_display() => ui::run(dir),
        Mode::Ui => {
            log::info!("No display detected. Running in terminal mode.");
            return terminal::run(dir, Group::All);
        }
    }
    ExitCode::SUCCESS
}
