//! Pipeline step 10: `RUN_AFTER_<label>=<command>` shell commands, run in key
//! order, with `${KEY}` replaced by config values, or by the cartridge's
//! folders: `${FREEGAMES_PATH}`, `${PATCHES_PATH}`, `${SAVESCOPY_PATH}` and
//! `${SAVESLINK_PATH}` (the names of the former path settings).

use std::process::Command;

use log::{error, info, warn};

use super::config::Config;
use super::paths;

fn expand_template(command: &str, cfg: &Config) -> String {
    let workspace = cfg.workspace();
    let builtins = [
        ("_CONFIG_PATH", paths::lossy(&cfg.path)),
        ("_SCRIPT_DIR", paths::lossy(&super::app::exe_dir())),
        (
            "FREEGAMES_PATH",
            paths::lossy(&workspace.join(super::app::GAMES_DIR)),
        ),
        ("PATCHES_PATH", paths::lossy(&cfg.mods_dir())),
        ("SAVESCOPY_PATH", paths::lossy(&cfg.saves_copies_dir())),
        ("SAVESLINK_PATH", paths::lossy(&cfg.saves_links_dir())),
    ];
    // Built-ins first: a leftover `SAVESLINK_PATH=` line in an old config
    // must not override the cartridge's fixed folders.
    let lookup = |name: &str| {
        builtins
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
            .or_else(|| cfg.get(name).map(str::to_string))
    };
    // Only the braced form: `$1`-style shell syntax must reach the shell intact.
    command
        .split("${")
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                return part.to_string();
            }
            match part
                .split_once('}')
                .and_then(|(name, rest)| lookup(name).map(|v| v + rest))
            {
                Some(expanded) => expanded,
                None => format!("${{{part}"),
            }
        })
        .collect()
}

fn shell(command: &str) -> Command {
    let mut cmd = if cfg!(windows) {
        Command::new("cmd")
    } else {
        Command::new("sh")
    };
    cmd.arg(if cfg!(windows) { "/C" } else { "-c" })
        .arg(command);
    cmd
}

fn run_one(label: &str, command: &str) {
    info!("  {label}: {command}");
    let output = match shell(command).output() {
        Ok(output) => output,
        Err(e) => {
            error!("  {label}: failed to execute: {e}");
            return;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        info!("{}", stdout.trim());
    }
    if output.status.success() {
        info!("  {label}: done");
        return;
    }
    warn!(
        "  {label}: exited with code {}",
        output.status.code().unwrap_or(-1)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        warn!("   {}", stderr.trim());
    }
}

pub fn run(cfg: &Config) {
    let commands: Vec<(&str, &str)> = cfg
        .values()
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("RUN_AFTER_")
                .map(|label| (label, v.as_str()))
        })
        .collect();
    if commands.is_empty() {
        return;
    }
    info!("Running {} post-command(s)", commands.len());
    for (label, command) in commands {
        run_one(label, &expand_template(command, cfg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config;

    #[test]
    fn expands_config_keys_and_leaves_the_rest() {
        // A leftover path setting is ignored: the folder is fixed.
        let raw = config::parse("SAVESLINK_PATH=/elsewhere\nMACHINE_NAME=deck\n");
        let cfg = Config::from_raw("/ws/system/config.txt".into(), raw);
        assert_eq!(
            expand_template(
                "rclone sync -L \"${SAVESLINK_PATH}\" r:${MACHINE_NAME} ${UNKNOWN} $1",
                &cfg
            ),
            format!(
                "rclone sync -L \"{}\" r:deck ${{UNKNOWN}} $1",
                paths::lossy(&std::path::Path::new("/ws").join("saves-links"))
            )
        );
        assert_eq!(
            expand_template("${_CONFIG_PATH}", &cfg),
            "/ws/system/config.txt"
        );
    }
}
