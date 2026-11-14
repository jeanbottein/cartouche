//! Command-line arguments.

use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: cartridge [OPTIONS] [DIR]

Manage DRM-free games: detect executables, fetch artwork, sync to Steam and
apply patches, by itself whenever the games folder changes. Opens the UI
unless told otherwise.

Arguments:
  [DIR]                Folder holding the system/ workspace
                       (default: next to the binary, then the current folder)

Options:
      --serve          Headless library, without the UI: keep the games up
                       to date and share them on the LAN, logging what happens
      --pack <GAME>    Pack the game folder GAME into GAME.cartridge.tar
      --unpack <GAME>  Unpack GAME.cartridge.tar into the folder GAME
      --peers          List •cartridge servers found on the LAN, with their games
      --download <PEER> <GAME>
                       Copy GAME from PEER as a package (GAME.cartridge.tar)
      --install <PEER> <GAME>
                       Copy GAME from PEER into the folder GAME
                       PEER: a name shown by --peers, or host[:port]
  -h, --help           Print this help
  -V, --version        Print the version

A cartridge is a portable folder (DIR = the app's folder by default). Its
layout is fixed; replace a folder with a symbolic link to put it elsewhere.

Files:
  DIR/system/config.txt          Settings, KEY=value. Created on first run (UI or
                                 a terminal wizard) with every key documented in
                                 comments; also editable in the UI's SETTINGS tab.
  DIR/system/cartridge-id        This cartridge's identity (Steam ownership)
  DIR/system/*.png               Optional Steam art for the cartridge itself
  DIR/games/<Game>/              One folder per game
  DIR/games/<Game>/.cartridge/game.json
                                 Title, targets (os/arch/exe), savePaths, year,
                                 players, description, size. Filled by itself.
  DIR/games/<Game>.cartridge.tar A packaged game: shared/copied, not playable
                                 until unpacked (--unpack or the UI).
  DIR/mods/                      Patches (folders with a patch.json)
  DIR/saves-copies/              Save backups
  DIR/saves-links/               Links to live save folders (for Syncthing)

Main settings:
  STEAM_ADD_SELF=True            Add the cartridge itself (named after DIR)
  STEAMGRIDDB_API_KEY=<key>      Artwork, names and years from SteamGridDB
  SHARE_ENABLED=True             Share games on the LAN while the UI runs
  SHARE_PORT=47800               TCP + UDP port used for sharing
  SHARE_PEERS=host[:port],...    Peers the LAN broadcast can't reach

Share games between machines:
  1. Sharing machine: SHARE_ENABLED=True and open the UI, or run --serve
     (headless library). Open the port in the firewall, e.g. on Bazzite/Fedora:
       sudo firewall-cmd --permanent --add-port=47800/tcp --add-port=47800/udp
       sudo firewall-cmd --reload
  2. Other machine: `cartridge --peers` lists servers and their games, then
     `cartridge --install <PEER> <GAME>` (or --download to keep a package).

Examples:
  cartridge                      Open the UI (first run: setup)
  cartridge --serve ~/Games      Run that cartridge without the UI (e.g. over SSH)

Logs go to stderr, or to the systemd journal when stderr is not a terminal
(journalctl --user -t cartridge -f).

Exit codes: 0 success, 1 failure (see the log), 2 bad arguments.";

#[derive(Debug, PartialEq)]
pub enum Mode {
    Ui,
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
    let (mut command, mut dir) = (None, None);
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
    Ok(Args {
        mode: command.unwrap_or(Mode::Ui),
        dir,
    })
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
        assert!(parse_str("-t").is_err());
        assert!(parse_str("a b").is_err());
    }
}
