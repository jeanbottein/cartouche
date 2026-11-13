//! Detecting, checking and restarting Steam on this machine.

use super::userdata_config_dirs;
use crate::core::config::Config;

const FLATPAK_STEAM: &str = "com.valvesoftware.Steam";

/// How Steam gets restarted on this machine.
#[derive(Debug, PartialEq)]
enum RestartPlan {
    /// Bazzite/SteamOS-style Game Mode: restart the session unit, so Steam
    /// comes back in Game Mode. `steam -shutdown` would end that session and
    /// may land on the desktop.
    GameModeSession(String),
    NativeLinux,
    FlatpakLinux,
    MacOs,
}

/// What the machine looks like, as far as restarting Steam is concerned.
struct Host {
    os: &'static str,
    game_mode_unit: Option<String>,
    /// Running under Steam's gamepad UI, whatever the session manager is.
    gamepad_ui: bool,
    native_steam: bool,
    flatpak_steam: bool,
}

/// `None` when we can't restart Steam safely: better to leave it alone than
/// to close it for good or drop the user out of Game Mode.
fn restart_plan(host: &Host) -> Option<RestartPlan> {
    match host.os {
        "macos" => Some(RestartPlan::MacOs),
        "linux" => match &host.game_mode_unit {
            Some(unit) => Some(RestartPlan::GameModeSession(unit.clone())),
            None if host.gamepad_ui => None,
            None if host.native_steam => Some(RestartPlan::NativeLinux),
            None if host.flatpak_steam => Some(RestartPlan::FlatpakLinux),
            None => None,
        },
        _ => None,
    }
}

/// Shell script run detached: it must outlive Steam, which ends the
/// processes of the games it launched, this app included.
fn restart_script(plan: &RestartPlan) -> String {
    const WAIT_FOR_EXIT: &str =
        "i=0; while pgrep -x steam >/dev/null && [ $i -lt 60 ]; do sleep 1; i=$((i+1)); done; sleep 3";
    match plan {
        RestartPlan::GameModeSession(unit) => format!("systemctl --user restart '{unit}'"),
        RestartPlan::NativeLinux => format!(
            "steam -shutdown; {WAIT_FOR_EXIT}; pgrep -x steam >/dev/null || steam >/dev/null 2>&1"
        ),
        RestartPlan::FlatpakLinux => format!(
            "flatpak kill {FLATPAK_STEAM}; {WAIT_FOR_EXIT}; \
             pgrep -x steam >/dev/null || flatpak run {FLATPAK_STEAM} >/dev/null 2>&1"
        ),
        RestartPlan::MacOs => "osascript -e 'quit app \"Steam\"'; i=0; \
             while pgrep -x steam_osx >/dev/null && [ $i -lt 60 ]; do sleep 1; i=$((i+1)); done; \
             open -a Steam"
            .to_string(),
    }
}

/// The active Game Mode session unit (Bazzite names it per launcher, e.g.
/// `gamescope-session-plus@ogui-steam.service`), if Steam runs in one.
fn game_mode_unit() -> Option<String> {
    let out = std::process::Command::new("systemctl")
        .args([
            "--user",
            "list-units",
            "gamescope-session-plus@*",
            "--state=active",
            "--plain",
            "--no-legend",
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
}

pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn flatpak_steam_installed() -> bool {
    std::process::Command::new("flatpak")
        .args(["info", FLATPAK_STEAM])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn detect_host() -> Host {
    Host {
        os: std::env::consts::OS,
        game_mode_unit: game_mode_unit(),
        gamepad_ui: std::env::var_os("SteamGamepadUI").is_some(),
        native_steam: on_path("steam"),
        flatpak_steam: flatpak_steam_installed(),
    }
}

/// Whether a Steam install with a user profile is found: nothing to put
/// shortcuts into otherwise. `DEBUG_NO_STEAM` pretends it isn't, to see the
/// interface without Steam.
pub fn detected(cfg: &Config) -> bool {
    !cfg.is_true("DEBUG_NO_STEAM") && !userdata_config_dirs().is_empty()
}

/// Whether `restart` knows how to restart Steam on this machine.
pub fn can_restart() -> bool {
    restart_plan(&detect_host()).is_some()
}

/// Shut Steam down and start it again, so it reads the new shortcuts.
pub fn restart() -> Result<(), String> {
    use std::process::{Command, Stdio};
    let plan = restart_plan(&detect_host())
        .ok_or("Don't know how to restart Steam here: restart it yourself")?;
    let script = restart_script(&plan);
    let detached = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .args(["sh", "-c", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    };
    detached("systemd-run", &["--user", "--quiet", "--collect"])
        .or_else(|_| detached("setsid", &[]))
        .or_else(|_| detached("nohup", &[]))
        .map_err(|e| format!("Cannot restart Steam: {e}"))
}

/// Steam reads its shortcuts at startup and rewrites them from memory:
/// changes made while it runs show (and stick) only after a restart.
pub fn is_running() -> bool {
    let name = match std::env::consts::OS {
        "linux" => "steam",
        "macos" => "steam_osx",
        _ => return false,
    };
    std::process::Command::new("pgrep")
        .args(["-x", name])
        .output()
        .is_ok_and(|out| out.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(os: &'static str) -> Host {
        Host {
            os,
            game_mode_unit: None,
            gamepad_ui: false,
            native_steam: false,
            flatpak_steam: false,
        }
    }

    #[test]
    fn game_mode_session_wins_over_installed_steam() {
        let unit = "gamescope-session-plus@ogui-steam.service".to_string();
        let h = Host {
            game_mode_unit: Some(unit.clone()),
            native_steam: true,
            ..host("linux")
        };
        assert_eq!(restart_plan(&h), Some(RestartPlan::GameModeSession(unit)));
    }

    #[test]
    fn unknown_game_mode_session_is_left_alone() {
        let h = Host {
            gamepad_ui: true,
            native_steam: true,
            ..host("linux")
        };
        assert_eq!(restart_plan(&h), None);
    }

    #[test]
    fn linux_desktop_prefers_native_then_flatpak() {
        let both = Host {
            native_steam: true,
            flatpak_steam: true,
            ..host("linux")
        };
        assert_eq!(restart_plan(&both), Some(RestartPlan::NativeLinux));
        let flatpak = Host {
            flatpak_steam: true,
            ..host("linux")
        };
        assert_eq!(restart_plan(&flatpak), Some(RestartPlan::FlatpakLinux));
        assert_eq!(restart_plan(&host("linux")), None);
    }

    #[test]
    fn macos_restarts_and_windows_does_not() {
        assert_eq!(restart_plan(&host("macos")), Some(RestartPlan::MacOs));
        assert_eq!(restart_plan(&host("windows")), None);
    }

    #[test]
    fn scripts_restart_the_right_thing() {
        let unit = RestartPlan::GameModeSession("gamescope-session-plus@x.service".into());
        assert_eq!(
            restart_script(&unit),
            "systemctl --user restart 'gamescope-session-plus@x.service'"
        );
        assert!(restart_script(&RestartPlan::NativeLinux).starts_with("steam -shutdown;"));
        assert!(restart_script(&RestartPlan::FlatpakLinux)
            .contains("flatpak run com.valvesoftware.Steam"));
        assert!(restart_script(&RestartPlan::MacOs).contains("open -a Steam"));
    }
}
