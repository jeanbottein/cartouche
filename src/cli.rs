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
  -h, --help           Print this help
  -V, --version        Print the version";

#[derive(Debug, PartialEq)]
pub enum Mode {
    Ui,
    Terminal(Group),
    DryRun,
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
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(format!("unknown option '{flag}'"))
            }
            path if dir.is_none() => dir = Some(PathBuf::from(path)),
            extra => return Err(format!("unexpected argument '{extra}'")),
        }
    }
    let mode = match (dry_run, terminal || group.is_some()) {
        (true, _) => Mode::DryRun,
        (false, true) => Mode::Terminal(group.unwrap_or(Group::All)),
        (false, false) => Mode::Ui,
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
    fn rejects_bad_input() {
        assert!(parse_str("-x").is_err());
        assert!(parse_str("-g nope").is_err());
        assert!(parse_str("a b").is_err());
    }
}
