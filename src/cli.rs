//! Command-line arguments.

use std::path::PathBuf;

use crate::core::pipeline::Group;

pub const USAGE: &str = "\
Usage: cartouche [OPTIONS] [DIR]

Manage DRM-free games: detect executables, fetch artwork, sync to Steam,
apply patches and back up saves. Opens the UI unless told otherwise.

Arguments:
  [DIR]                Folder holding the .cartouche/ workspace
                       (default: next to the binary, then the current folder)

Options:
  -t, --terminal       Run the pipeline in the terminal, without the UI
  -g, --group <NAME>   Phases to run in the terminal (implies -t):
                       all (default), parse, steam, backup, patch
  -n, --dry-run        Preview the Steam sync without changing Steam
      --pack <GAME>    Pack the game folder GAME into GAME.cartouche.tar
      --unpack <GAME>  Unpack GAME.cartouche.tar into the folder GAME
      --serve          Share the games folder on the LAN, without the UI
      --peers          List Cartouche servers found on the LAN, with their games
      --download <PEER> <GAME>
                       Copy GAME from PEER as a package (GAME.cartouche.tar)
      --install <PEER> <GAME>
                       Copy GAME from PEER into the folder GAME
                       PEER: a name shown by --peers, or host[:port]
  -h, --help           Print this help
  -V, --version        Print the version

Files:
  DIR/.cartouche/config.txt      Settings, KEY=value. Created on first run (UI or
                                 terminal wizard) with every key documented in
                                 comments; also editable in the UI's SETTINGS tab.
  GAMES/<Game>/                  One folder per game (GAMES = FREEGAMES_PATH)
  GAMES/<Game>/.cartouche/game.json
                                 Title, targets (os/arch/exe), savePaths, year,
                                 players, description, size. Filled by `-g parse`.
  GAMES/<Game>.cartouche.tar     A packaged game: shared/copied, not playable
                                 until unpacked (--unpack or the UI).

Main settings:
  FREEGAMES_PATH=<dir>           Games folder (required; relative to DIR allowed)
  STEAM_EXPOSE=True              Add games to Steam as non-Steam shortcuts
  STEAMGRIDDB_API_KEY=<key>      Artwork, names and years from SteamGridDB
  SAVESCOPY_PATH=<dir>           Back up saves there (`-g backup`)
  SHARE_ENABLED=True             Share games on the LAN while the UI runs
  SHARE_PORT=47800               TCP + UDP port used for sharing
  SHARE_PEERS=host[:port],...    Peers the LAN broadcast can't reach

Share games between machines:
  1. Sharing machine: SHARE_ENABLED=True and open the UI, or run --serve
     (headless library). Open the port in the firewall, e.g. on Bazzite/Fedora:
       sudo firewall-cmd --permanent --add-port=47800/tcp --add-port=47800/udp
       sudo firewall-cmd --reload
  2. Other machine: `cartouche --peers` lists servers and their games, then
     `cartouche --install <PEER> <GAME>` (or --download to keep a package).

Examples:
  cartouche                      Open the UI (first run: setup)
  cartouche -t                   Run every phase in the terminal
  cartouche -g parse ~/games-ws  Detect, measure and enrich games only
  cartouche -n                   Show what a Steam sync would change

Exit codes: 0 success, 1 failure (see the log on stderr), 2 bad arguments.";

#[derive(Debug, PartialEq)]
pub enum Mode {
    Ui,
    Terminal(Group),
    DryRun,
    Pack(String),
    Unpack(String),
    Serve,
    Peers,
    Download { peer: String, game: String },
    Install { peer: String, game: String },
    Help,
    Version,
}

#[derive(Debug, PartialEq)]
pub struct Args {
    pub mode: Mode,
    pub dir: Option<PathBuf>,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut args = args.into_iter();
    let (mut terminal, mut dry_run, mut group, mut dir) = (false, false, None, None);
    let mut command = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Ok(Args {
                    mode: Mode::Help,
                    dir: None,
                })
            }
            "-V" | "--version" => {
                return Ok(Args {
                    mode: Mode::Version,
                    dir: None,
                })
            }
            "-t" | "--terminal" => terminal = true,
            "-n" | "--dry-run" => dry_run = true,
            "-g" | "--group" => {
                let name = args.next().ok_or("--group needs a value")?;
                group = Some(Group::from_name(&name).ok_or(format!("unknown group '{name}'"))?);
            }
            flag if flag.starts_with("--group=") => {
                let name = &flag["--group=".len()..];
                group = Some(Group::from_name(name).ok_or(format!("unknown group '{name}'"))?);
            }
            "--serve" => command = Some(Mode::Serve),
            "--peers" => command = Some(Mode::Peers),
            flag @ ("--download" | "--install") => {
                let missing = || format!("{flag} needs a peer and a game");
                let peer = args.next().ok_or_else(missing)?;
                let game = args.next().ok_or_else(missing)?;
                command = Some(if flag == "--download" {
                    Mode::Download { peer, game }
                } else {
                    Mode::Install { peer, game }
                });
            }
            "--pack" => {
                let name = args.next().ok_or("--pack needs a game folder name")?;
                command = Some(Mode::Pack(name));
            }
            "--unpack" => {
                let name = args.next().ok_or("--unpack needs a game folder name")?;
                command = Some(Mode::Unpack(name));
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(format!("unknown option '{flag}'"))
            }
            path if dir.is_none() => dir = Some(PathBuf::from(path)),
            extra => return Err(format!("unexpected argument '{extra}'")),
        }
    }
    let mode = match (command, dry_run, terminal || group.is_some()) {
        (Some(mode), _, _) => mode,
        (None, true, _) => Mode::DryRun,
        (None, false, true) => Mode::Terminal(group.unwrap_or(Group::All)),
        (None, false, false) => Mode::Ui,
    };
    Ok(Args { mode, dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Args, String> {
        parse(s.split_whitespace().map(str::to_string))
    }

    #[test]
    fn defaults_to_the_ui() {
        assert_eq!(
            parse_str("").unwrap(),
            Args {
                mode: Mode::Ui,
                dir: None
            }
        );
        assert_eq!(parse_str("/ws").unwrap().dir, Some(PathBuf::from("/ws")));
    }

    #[test]
    fn terminal_flags() {
        assert_eq!(parse_str("-t").unwrap().mode, Mode::Terminal(Group::All));
        assert_eq!(
            parse_str("--group steam /ws").unwrap().mode,
            Mode::Terminal(Group::Steam)
        );
        assert_eq!(
            parse_str("--group=backup").unwrap().mode,
            Mode::Terminal(Group::Backup)
        );
        assert_eq!(parse_str("-t --dry-run").unwrap().mode, Mode::DryRun);
    }

    #[test]
    fn package_flags() {
        let args = parse_str("--pack Celeste /ws").unwrap();
        assert_eq!(args.mode, Mode::Pack("Celeste".into()));
        assert_eq!(args.dir, Some(PathBuf::from("/ws")));
        assert_eq!(
            parse_str("--unpack Celeste").unwrap().mode,
            Mode::Unpack("Celeste".into())
        );
        assert_eq!(
            parse_str("/ws --pack Celeste").unwrap().dir,
            Some(PathBuf::from("/ws"))
        );
        assert!(parse_str("--pack").is_err());
        assert_eq!(parse_str("--serve /ws").unwrap().mode, Mode::Serve);
        assert_eq!(
            parse_str("--install deck Celeste").unwrap().mode,
            Mode::Install {
                peer: "deck".into(),
                game: "Celeste".into()
            }
        );
        assert!(parse_str("--download deck").is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_str("-x").is_err());
        assert!(parse_str("-g nope").is_err());
        assert!(parse_str("a b").is_err());
    }
}
