//! Windows games through Steam's own Proton, started by cartridge.
//!
//! They run the way Steam runs the game's non-Steam shortcut: the same Proton
//! build (`steamapps/common`, `compatibilitytools.d`), inside the Steam Linux
//! Runtime that build asks for, in the shortcut's prefix
//! (`steamapps/compatdata/<appid>`). So saves and prefix settings are the same
//! whether the game starts from Steam or from cartridge.
//!
//! The build is the one the game's `proton` settings (game.json) name, else
//! Steam's default, else the newest Proton. Those settings also add
//! environment variables and name winetricks verbs, installed into the prefix
//! before the first launch (winetricks, or the one inside the Protontricks
//! Flatpak). What winetricks downloads is taken from the cartridge
//! (`system/winetricks`) when it is there, and kept there with
//! `PROTON_KEEP_DOWNLOADS`, so a cartridge can bring it to other machines.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use log::{info, warn};

use super::config::Config;
use super::models::{folder_name_of, is_env_name, Game, Resolved};
use super::{paths, steam, vdf};

/// Named for a Steam shortcut when no Proton is installed yet: Steam installs
/// it on the first launch.
pub const FALLBACK_TOOL: &str = "proton_experimental";
const PROTONTRICKS_FLATPAK: &str = "com.github.Matoking.protontricks";

/// An installed Proton build.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    /// Steam's internal name (`proton_9`, `GE-Proton9-20`).
    pub name: String,
    pub dir: PathBuf,
}

impl Tool {
    fn folder(&self) -> String {
        folder_name_of(&self.dir)
    }

    fn matches(&self, wanted: &str) -> bool {
        self.name.eq_ignore_ascii_case(wanted) || self.folder().eq_ignore_ascii_case(wanted)
    }

    /// Proton's own Wine programs (`files/bin`, `dist/bin` before Proton 5.13).
    fn bin(&self, program: &str) -> PathBuf {
        let in_dist = self.dir.join("dist/bin").join(program);
        if in_dist.is_file() {
            return in_dist;
        }
        self.dir.join("files/bin").join(program)
    }
}

/// Steam's data folder (where `steamapps/` is).
pub fn steam_root() -> Option<PathBuf> {
    let home = paths::home_dir();
    [".local/share/Steam", ".steam/steam"]
        .iter()
        .map(|rel| home.join(rel))
        .find(|root| root.join("steamapps").is_dir())
}

/// (major, minor) of a Valve Proton folder: `Proton 9.0 (Beta)` -> (9, 0).
fn valve_version(folder: &str) -> Option<(u32, u32)> {
    let version = folder.strip_prefix("Proton ")?.split_whitespace().next()?;
    let (major, minor) = version.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Steam's internal name of a Valve Proton folder: `Proton 9.0` ->
/// `proton_9`, `Proton 6.3` -> `proton_63`.
fn valve_tool_name(folder: &str) -> Option<String> {
    match folder {
        "Proton - Experimental" => return Some("proton_experimental".into()),
        "Proton Hotfix" => return Some("proton_hotfix".into()),
        _ => {}
    }
    let (major, minor) = valve_version(folder)?;
    Some(match minor {
        0 => format!("proton_{major}"),
        _ => format!("proton_{major}{minor}"),
    })
}

/// The name a custom build registers in its `compatibilitytool.vdf`.
fn custom_tool_name(manifest: &str) -> Option<String> {
    let data = vdf::parse_text(manifest).ok()?;
    let name = data
        .get_map("compatibilitytools")?
        .get_map("compat_tools")?
        .keys()
        .next()
        .map(str::to_string);
    name
}

fn tools_in(dir: &Path, name_of: impl Fn(&str, &Path) -> Option<String>) -> Vec<Tool> {
    paths::sorted_entries(dir)
        .into_iter()
        .filter_map(|folder| {
            let tool_dir = dir.join(&folder);
            let name = name_of(&folder, &tool_dir)?;
            tool_dir.join("proton").is_file().then_some(Tool {
                name,
                dir: tool_dir,
            })
        })
        .collect()
}

/// Valve's Proton builds, then custom ones (GE-Proton...).
pub fn installed_tools(root: &Path) -> Vec<Tool> {
    let valve = tools_in(&root.join("steamapps/common"), |folder, _| {
        valve_tool_name(folder)
    });
    let custom = tools_in(&root.join("compatibilitytools.d"), |_, dir| {
        custom_tool_name(&std::fs::read_to_string(dir.join("compatibilitytool.vdf")).ok()?)
    });
    valve.into_iter().chain(custom).collect()
}

/// The first of `wanted` that is installed (by Steam name or folder name, any
/// case), else the newest numbered Valve Proton, else any build.
pub fn pick_tool<'a>(tools: &'a [Tool], wanted: &[&str]) -> Option<&'a Tool> {
    let named = wanted
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .find_map(|w| tools.iter().find(|t| t.matches(w)));
    let newest = || {
        tools
            .iter()
            .filter_map(|t| Some((valve_version(&t.folder())?, t)))
            .max_by_key(|(version, _)| *version)
            .map(|(_, t)| t)
    };
    named.or_else(newest).or_else(|| tools.first())
}

/// Steam's `CompatToolMapping` (text of `config/config.vdf`): the build set
/// for each AppID, `0` being Steam's default for Windows titles.
fn compat_mapping(config_vdf: &str) -> BTreeMap<String, String> {
    let path = [
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    ];
    let Ok(data) = vdf::parse_text(config_vdf) else {
        return BTreeMap::new();
    };
    let Some(mapping) = path
        .iter()
        .try_fold(&data, |map, key| map.get_map_ignore_case(key))
    else {
        return BTreeMap::new();
    };
    mapping
        .0
        .iter()
        .filter_map(|(appid, entry)| match entry {
            vdf::Value::Map(entry) => Some((appid, entry.get_str("name")?)),
            _ => None,
        })
        .filter(|(_, name)| !name.is_empty())
        .map(|(appid, name)| (appid.clone(), name.to_string()))
        .collect()
}

/// Steam's Proton builds and which one each title uses, read once.
pub struct Installed {
    root: PathBuf,
    pub tools: Vec<Tool>,
    /// AppID -> build, `0` being Steam's default.
    mapping: BTreeMap<String, String>,
}

impl Installed {
    pub fn read() -> Option<Installed> {
        steam_root().map(|root| Installed::read_from(&root))
    }

    pub fn read_from(root: &Path) -> Installed {
        let config = std::fs::read_to_string(root.join("config/config.vdf"));
        Installed {
            root: root.to_path_buf(),
            tools: installed_tools(root),
            mapping: compat_mapping(&config.unwrap_or_default()),
        }
    }

    /// The installed build `wanted` names (Steam name or folder, any case).
    pub fn find(&self, wanted: &str) -> Option<&Tool> {
        let wanted = wanted.trim();
        self.tools
            .iter()
            .find(|t| !wanted.is_empty() && t.matches(wanted))
    }

    fn mapped(&self, appid: &str) -> &str {
        self.mapping.get(appid).map_or("", String::as_str)
    }

    /// For a game that names no build and has none set in Steam: Steam's
    /// default, else the newest Proton.
    pub fn default_tool(&self) -> Option<&Tool> {
        pick_tool(&self.tools, &[self.mapped("0")])
    }

    /// The build for the game whose shortcut is `appid`: the one its
    /// game.json names (`version`), else the one Steam has for the shortcut,
    /// else the default. The same choice as Steam's, so both share the prefix.
    pub fn tool_for(&self, version: &str, appid: u32) -> Option<&Tool> {
        let shortcut = self.mapped(&appid.to_string());
        pick_tool(&self.tools, &[version, shortcut, self.mapped("0")])
    }
}

fn read_vdf(path: &Path) -> Option<vdf::Map> {
    vdf::parse_text(&std::fs::read_to_string(path).ok()?).ok()
}

/// Entry point of the Steam Linux Runtime `tool` asks for, if installed.
fn runtime_for(root: &Path, tool: &Tool) -> Option<PathBuf> {
    let manifest = read_vdf(&tool.dir.join("toolmanifest.vdf"))?;
    let appid = manifest
        .get_map("manifest")?
        .get_str("require_tool_appid")?
        .to_string();
    let app = read_vdf(&root.join(format!("steamapps/appmanifest_{appid}.acf")))?;
    let folder = app.get_map("AppState")?.get_str("installdir")?;
    let entry = root
        .join("steamapps/common")
        .join(folder)
        .join("_v2-entry-point");
    entry.is_file().then_some(entry)
}

/// Verbs of `wanted` not listed in the prefix's `winetricks.log`.
fn missing_verbs(wanted: &[String], installed_log: &str) -> Vec<String> {
    let installed: HashSet<&str> = installed_log.lines().map(str::trim).collect();
    wanted
        .iter()
        .map(|verb| verb.trim())
        .filter(|verb| !verb.is_empty() && !installed.contains(verb))
        .map(str::to_string)
        .collect()
}

/// `value` as one shell word.
fn shell_word(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.,:/=+@%".contains(c));
    if plain {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Launch options of the game's Steam shortcut: its environment before
/// `%command%`, then its own arguments. Steam runs them through a shell, so
/// only plain variable names get in.
pub fn shortcut_launch_options(env: &BTreeMap<String, String>, args: &str) -> String {
    let vars: Vec<String> = env
        .iter()
        .filter(|(key, _)| is_env_name(key))
        .map(|(key, value)| format!("{key}={}", shell_word(value)))
        .collect();
    if vars.is_empty() {
        return args.to_string();
    }
    format!("{} %command% {args}", vars.join(" "))
        .trim_end()
        .to_string()
}

/// The error of a step the user cancelled.
pub const CANCELLED: &str = "Launch cancelled";

/// Run `cmd` to its end, or kill it once `cancel` is set.
fn run(mut cmd: Command, what: &str, cancel: &AtomicBool) -> Result<(), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{what}: {e}"))?;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("{what}: {e}"))? {
            if status.success() {
                return Ok(());
            }
            return Err(format!("{what} failed ({status})"));
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CANCELLED.into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Where winetricks keeps its downloads (redistributables, runtimes...).
#[derive(Clone, Debug, Default)]
pub struct Downloads {
    /// The cartridge's own store, `system/winetricks`: used when it has a file.
    pub kept: PathBuf,
    /// Download into `kept` (`PROTON_KEEP_DOWNLOADS`), so the cartridge has them.
    pub keep: bool,
}

impl Downloads {
    pub fn of(cfg: &Config) -> Downloads {
        Downloads {
            kept: cfg.system_dir().join("winetricks"),
            keep: cfg.is_true("PROTON_KEEP_DOWNLOADS"),
        }
    }

    /// winetricks' download folder (`W_CACHE`).
    fn cache(&self) -> PathBuf {
        if self.keep {
            return self.kept.clone();
        }
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| paths::home_dir().join(".cache"))
            .join("winetricks")
    }
}

/// Link every file under `from` missing from `to` (same relative path), so
/// winetricks finds what the cartridge keeps instead of downloading it.
/// Links left dangling (a cartridge that moved) are replaced; files never are.
fn link_missing(from: &Path, to: &Path) {
    for name in paths::sorted_entries(from) {
        let (source, target) = (from.join(&name), to.join(&name));
        if source.is_dir() {
            link_missing(&source, &target);
            continue;
        }
        if paths::is_symlink(&target) && !target.exists() {
            let _ = std::fs::remove_file(&target);
        }
        if target.exists() || paths::is_symlink(&target) {
            continue;
        }
        let linked = std::fs::create_dir_all(to).and_then(|()| symlink(&source, &target));
        if let Err(e) = linked {
            warn!("Could not link {}: {e}", target.display());
        }
    }
}

#[cfg(unix)]
fn symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(not(unix))]
fn symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::copy(source, target).map(drop)
}

fn flatpak_installed(app: &str) -> bool {
    Command::new("flatpak")
        .args(["info", app])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Runs programs in one game's prefix, with its Proton build.
#[derive(Debug)]
pub struct Runner {
    root: PathBuf,
    tool: Tool,
    /// Steam Linux Runtime entry point, when the build asks for one.
    runtime: Option<PathBuf>,
    /// `steamapps/compatdata/<appid>`: Proton keeps the prefix in `pfx/`.
    compat_data: PathBuf,
    install_dir: PathBuf,
    env: BTreeMap<String, String>,
    downloads: Downloads,
}

impl Runner {
    /// For `game`'s Windows target.
    pub fn for_game(game: &Game, downloads: &Downloads) -> Result<Runner, String> {
        let installed =
            Installed::read().ok_or("Steam not found: Windows games run with Steam's Proton")?;
        Runner::new(&installed, game, downloads)
    }

    fn new(installed: &Installed, game: &Game, downloads: &Downloads) -> Result<Runner, String> {
        let resolved = game
            .resolved
            .as_ref()
            .ok_or("No executable target in game.json")?;
        let appid = steam::appid_for(game, &resolved.target);
        let tool = installed
            .tool_for(&game.proton.version, appid)
            .ok_or("No Proton installed: install one in Steam (Library > Tools)")?;
        let asked = game.proton.version.trim();
        if !asked.is_empty() && !tool.matches(asked) {
            warn!(
                "{}: {asked} is not installed, using {}",
                game.title, tool.name
            );
        }
        let root = &installed.root;
        Ok(Runner {
            root: root.clone(),
            tool: tool.clone(),
            runtime: runtime_for(root, tool),
            compat_data: root.join("steamapps/compatdata").join(appid.to_string()),
            install_dir: game.dir.clone(),
            env: game.proton.env.clone(),
            downloads: downloads.clone(),
        })
    }

    fn prefix(&self) -> PathBuf {
        self.compat_data.join("pfx")
    }

    /// Proton running `waitforexitandrun`, inside the runtime, as Steam does.
    fn proton(&self) -> Command {
        let proton = self.tool.dir.join("proton");
        let mut cmd = match &self.runtime {
            Some(entry) => {
                let mut cmd = Command::new(entry);
                cmd.arg("--verb=waitforexitandrun").arg("--").arg(proton);
                cmd
            }
            None => Command::new(proton),
        };
        cmd.arg("waitforexitandrun")
            .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", &self.root)
            .env("STEAM_COMPAT_DATA_PATH", &self.compat_data)
            .env("STEAM_COMPAT_INSTALL_PATH", &self.install_dir);
        cmd
    }

    /// The game itself, with its environment and arguments.
    pub fn game_command(&self, resolved: &Resolved) -> Command {
        let mut cmd = self.proton();
        cmd.arg(&resolved.target)
            .args(resolved.launch_options.split_whitespace())
            .envs(&self.env)
            .current_dir(&resolved.start_in);
        cmd
    }

    fn winetricks(&self) -> Result<Command, String> {
        let cache = self.downloads.cache();
        let env = [
            ("WINEPREFIX", self.prefix()),
            ("WINE", self.tool.bin("wine")),
            ("WINESERVER", self.tool.bin("wineserver")),
            ("W_CACHE", cache.clone()),
        ];
        if steam::on_path("winetricks") {
            let mut cmd = Command::new("winetricks");
            cmd.envs(env);
            return Ok(cmd);
        }
        if !flatpak_installed(PROTONTRICKS_FLATPAK) {
            return Err("winetricks not found: install winetricks or Protontricks".into());
        }
        // The sandbox sees the cache, and the cartridge store its links point to.
        let shared = [cache, self.downloads.kept.clone()];
        let mut cmd = Command::new("flatpak");
        cmd.arg("run")
            .args(
                env.iter()
                    .map(|(k, v)| format!("--env={k}={}", v.display())),
            )
            .args(
                shared
                    .iter()
                    .map(|dir| format!("--filesystem={}", dir.display())),
            )
            .args(["--command=winetricks", PROTONTRICKS_FLATPAK]);
        Ok(cmd)
    }

    /// Create the prefix if needed, then install the winetricks `verbs` it
    /// lacks, telling `step` what happens. Blocks (verbs download
    /// installers): call it off the UI thread. Setting `cancel` stops it,
    /// and whatever it left running in the prefix.
    pub fn prepare(
        &self,
        verbs: &[String],
        step: impl Fn(&str),
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        let prepared = self.prepare_prefix(verbs, step, cancel);
        if prepared.is_err() {
            self.stop_wine();
        }
        prepared
    }

    fn prepare_prefix(
        &self,
        verbs: &[String],
        step: impl Fn(&str),
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        std::fs::create_dir_all(&self.compat_data)
            .map_err(|e| format!("{}: {e}", self.compat_data.display()))?;
        if !self.prefix().join("system.reg").is_file() {
            step("Creating the Proton prefix...");
            info!(
                "Creating the Proton prefix in {}",
                self.compat_data.display()
            );
            let mut wineboot = self.proton();
            wineboot.arg("wineboot");
            run(wineboot, "Creating the Proton prefix", cancel)?;
        }
        let installed = std::fs::read_to_string(self.prefix().join("winetricks.log"));
        let missing = missing_verbs(verbs, &installed.unwrap_or_default());
        if missing.is_empty() {
            return Ok(());
        }
        let list = missing.join(", ");
        step(&format!("Installing {list}..."));
        info!("winetricks: installing {list}");
        self.share_downloads()?;
        let mut winetricks = self.winetricks()?;
        winetricks.arg("-q").args(&missing);
        run(winetricks, &format!("winetricks {list}"), cancel)
    }

    /// Make what the cartridge keeps visible in winetricks' download folder.
    fn share_downloads(&self) -> Result<(), String> {
        let (kept, cache) = (&self.downloads.kept, self.downloads.cache());
        std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
        if &cache != kept {
            link_missing(kept, &cache);
        }
        Ok(())
    }

    /// Stop what a killed step (wineboot, winetricks) left running in the prefix.
    fn stop_wine(&self) {
        let stopped = Command::new(self.tool.bin("wineserver"))
            .arg("-k")
            .env("WINEPREFIX", self.prefix())
            .status();
        if let Err(e) = stopped {
            warn!("Could not stop Wine in {}: {e}", self.prefix().display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;
    use std::ffi::OsStr;

    fn tool(name: &str, folder: &str) -> Tool {
        Tool {
            name: name.into(),
            dir: PathBuf::from("/steam/steamapps/common").join(folder),
        }
    }

    #[test]
    fn valve_folders_map_to_steam_names() {
        let names: Vec<Option<String>> = [
            "Proton - Experimental",
            "Proton Hotfix",
            "Proton 9.0 (Beta)",
            "Proton 10.0",
            "Proton 6.3",
            "SteamLinuxRuntime_sniper",
        ]
        .iter()
        .map(|f| valve_tool_name(f))
        .collect();
        assert_eq!(
            names,
            [
                Some("proton_experimental".into()),
                Some("proton_hotfix".into()),
                Some("proton_9".into()),
                Some("proton_10".into()),
                Some("proton_63".into()),
                None
            ]
        );
    }

    #[test]
    fn custom_builds_are_named_by_their_manifest() {
        let manifest = r#""compatibilitytools" { "compat_tools" {
            "GE-Proton11-5" // Internal name
            { "install_path" "." "display_name" "GE-Proton11-5" } } }"#;
        assert_eq!(custom_tool_name(manifest).as_deref(), Some("GE-Proton11-5"));
    }

    #[test]
    fn picks_the_first_wanted_build_by_name_or_folder() {
        let tools = [
            tool("proton_9", "Proton 9.0"),
            tool("proton_10", "Proton 10.0"),
        ];
        assert_eq!(
            pick_tool(&tools, &["", "proton_9"]).unwrap().name,
            "proton_9"
        );
        assert_eq!(
            pick_tool(&tools, &["proton 10.0"]).unwrap().name,
            "proton_10"
        );
    }

    const MAPPING: &str = r#""InstallConfigStore" { "Software" { "valve" { "Steam" {
        "CompatToolMapping" { "0" { "name" "proton_10" } "123" { "name" "proton_9" } "7" { "name" "" } }
    } } } }"#;

    #[test]
    fn compat_mapping_reads_steam_builds_by_appid() {
        let mapping = compat_mapping(MAPPING);
        assert_eq!(
            mapping.into_iter().collect::<Vec<_>>(),
            [
                ("0".into(), "proton_10".into()),
                ("123".into(), "proton_9".into())
            ]
        );
        assert!(compat_mapping(r#""InstallConfigStore" { }"#).is_empty());
    }

    fn steam_with(builds: &[&str], config: &str) -> TempDir {
        let steam = TempDir::new();
        builds.iter().for_each(|folder| {
            steam.write(&format!("steamapps/common/{folder}/proton"), b"");
        });
        steam.write("config/config.vdf", config.as_bytes());
        steam
    }

    #[test]
    fn the_shortcut_build_comes_after_the_game_one_and_before_steam_default() {
        let steam = steam_with(&["Proton 8.0", "Proton 9.0", "Proton 10.0"], MAPPING);
        let installed = Installed::read_from(steam.path());
        let name = |version: &str, appid| installed.tool_for(version, appid).unwrap().name.clone();
        assert_eq!(name("proton_8", 123), "proton_8");
        assert_eq!(name("", 123), "proton_9");
        assert_eq!(name("", 456), "proton_10");
        assert_eq!(installed.default_tool().unwrap().name, "proton_10");
    }

    #[test]
    fn runs_in_the_build_steam_has_for_the_shortcut() {
        let game = windows_game();
        let appid = steam::generate_appid("Rayman", "/g/Rayman/Rayman.exe");
        let config = format!(
            r#""InstallConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{
            "CompatToolMapping" {{ "{appid}" {{ "name" "proton_9" }} }} }} }} }} }}"#
        );
        let steam = steam_with(&["Proton 9.0", "Proton 10.0"], &config);
        let installed = Installed::read_from(steam.path());
        let runner = Runner::new(&installed, &game, &Downloads::default()).unwrap();
        let cmd = runner.game_command(game.resolved.as_ref().unwrap());
        let proton = steam.path().join("steamapps/common/Proton 9.0/proton");
        assert_eq!(Path::new(cmd.get_program()), proton);
    }

    #[test]
    fn without_a_wanted_build_the_newest_valve_one_is_picked() {
        let tools = [
            tool("proton_experimental", "Proton - Experimental"),
            tool("proton_10", "Proton 10.0"),
        ];
        assert_eq!(pick_tool(&tools, &[""]).unwrap().name, "proton_10");
    }

    #[test]
    fn falls_back_to_the_newest_valve_build() {
        let tools = [
            tool("GE-Proton11-5", "GE-Proton11-5"),
            tool("proton_10", "Proton 10.0"),
            tool("proton_63", "Proton 6.3"),
        ];
        assert_eq!(
            pick_tool(&tools, &["proton_experimental"]).unwrap().name,
            "proton_10"
        );
        assert_eq!(
            pick_tool(&tools[..1], &["nope"]).unwrap().name,
            "GE-Proton11-5"
        );
        assert_eq!(pick_tool(&[], &["nope"]), None);
    }

    #[test]
    fn only_verbs_missing_from_the_log_are_installed() {
        let wanted = ["d3dx9".to_string(), " vcrun2019 ".into(), String::new()];
        assert_eq!(missing_verbs(&wanted, "d3dx9\n"), ["vcrun2019"]);
        assert!(missing_verbs(&wanted, "vcrun2019\nd3dx9\n").is_empty());
    }

    #[test]
    fn shortcut_options_put_the_environment_before_the_command() {
        let env = BTreeMap::from([
            ("PROTON_USE_WINED3D".to_string(), "1".to_string()),
            ("WINEDLLOVERRIDES".to_string(), "d3d9=n;dxgi=n".to_string()),
        ]);
        assert_eq!(
            shortcut_launch_options(&env, "-windowed"),
            "PROTON_USE_WINED3D=1 WINEDLLOVERRIDES='d3d9=n;dxgi=n' %command% -windowed"
        );
        assert_eq!(shortcut_launch_options(&BTreeMap::new(), "-w"), "-w");
    }

    #[test]
    fn shortcut_options_never_carry_a_name_that_is_not_a_plain_word() {
        let env = BTreeMap::from([
            ("X=1; reboot; Y".to_string(), "1".to_string()),
            ("OK".to_string(), "1".to_string()),
        ]);
        assert_eq!(shortcut_launch_options(&env, ""), "OK=1 %command%");
    }

    fn fake_steam(steam: &TempDir) {
        steam.write("steamapps/common/Proton 10.0/proton", b"");
        steam.write(
            "steamapps/common/Proton 10.0/toolmanifest.vdf",
            br#""manifest" { "require_tool_appid" "1628350" }"#,
        );
        steam.write(
            "steamapps/appmanifest_1628350.acf",
            br#""AppState" { "installdir" "SteamLinuxRuntime_sniper" }"#,
        );
        steam.write(
            "steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point",
            b"",
        );
    }

    fn windows_game() -> Game {
        let mut game = Game::skeleton(Path::new("/g/Rayman"));
        game.title = "Rayman".into();
        game.resolved = Some(Resolved {
            target: "/g/Rayman/Rayman.exe".into(),
            start_in: "/g/Rayman".into(),
            launch_options: "-w".into(),
            os: "windows".into(),
        });
        game.proton.env.insert("PROTON_LOG".into(), "1".into());
        game
    }

    #[test]
    fn games_run_through_the_runtime_in_their_shortcut_prefix() {
        let steam = TempDir::new();
        fake_steam(&steam);
        let game = windows_game();
        let installed = Installed::read_from(steam.path());
        let runner = Runner::new(&installed, &game, &Downloads::default()).unwrap();
        let cmd = runner.game_command(game.resolved.as_ref().unwrap());

        let runtime = steam
            .path()
            .join("steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point");
        assert_eq!(Path::new(cmd.get_program()), runtime);
        let args: Vec<&Path> = cmd.get_args().map(Path::new).collect();
        let proton = steam.path().join("steamapps/common/Proton 10.0/proton");
        assert_eq!(
            args,
            [
                Path::new("--verb=waitforexitandrun"),
                Path::new("--"),
                &proton,
                Path::new("waitforexitandrun"),
                Path::new("/g/Rayman/Rayman.exe"),
                Path::new("-w"),
            ]
        );
        let env: BTreeMap<&OsStr, Option<&OsStr>> = cmd.get_envs().collect();
        let appid = steam::generate_appid("Rayman", "/g/Rayman/Rayman.exe");
        let compat = steam.path().join(format!("steamapps/compatdata/{appid}"));
        assert_eq!(
            env[OsStr::new("STEAM_COMPAT_DATA_PATH")].map(Path::new),
            Some(compat.as_path())
        );
        assert_eq!(env[OsStr::new("PROTON_LOG")], Some(OsStr::new("1")));
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/g/Rayman")));
    }

    #[test]
    fn builds_without_a_runtime_run_proton_directly() {
        let steam = TempDir::new();
        steam.write("compatibilitytools.d/GE/proton", b"");
        steam.write(
            "compatibilitytools.d/GE/compatibilitytool.vdf",
            br#""compatibilitytools" { "compat_tools" { "GE-Proton" { } } }"#,
        );
        let game = windows_game();
        let installed = Installed::read_from(steam.path());
        let runner = Runner::new(&installed, &game, &Downloads::default()).unwrap();
        let cmd = runner.game_command(game.resolved.as_ref().unwrap());
        let proton = steam.path().join("compatibilitytools.d/GE/proton");
        assert_eq!(Path::new(cmd.get_program()), proton);
    }

    #[cfg(unix)]
    #[test]
    fn a_cancelled_step_stops_its_program() {
        let started = std::time::Instant::now();
        let mut sleep = Command::new("sleep");
        sleep.arg("30");
        let err = run(sleep, "sleep", &AtomicBool::new(true)).unwrap_err();
        assert_eq!(err, CANCELLED);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_step_is_an_error() {
        let live = AtomicBool::new(false);
        assert!(run(Command::new("true"), "true", &live).is_ok());
        assert!(run(Command::new("false"), "false", &live)
            .unwrap_err()
            .contains("false failed"));
    }

    #[test]
    fn downloads_go_to_the_cartridge_only_when_kept() {
        let kept = PathBuf::from("/c/system/winetricks");
        let off = Downloads {
            kept: kept.clone(),
            keep: false,
        };
        let on = Downloads {
            kept: kept.clone(),
            keep: true,
        };
        assert_eq!(on.cache(), kept);
        assert!(off.cache().ends_with("winetricks"));
        assert_ne!(off.cache(), kept);
    }

    #[cfg(unix)]
    #[test]
    fn kept_downloads_are_linked_into_the_cache_without_replacing_files() {
        let dir = TempDir::new();
        let kept = dir.write("kept/directx9/redist.exe", b"kept");
        dir.write("kept/vcrun/vc.exe", b"kept");
        dir.write("cache/vcrun/vc.exe", b"own");
        std::os::unix::fs::symlink(
            dir.path().join("gone.exe"),
            dir.mkdir("cache/old").join("x.exe"),
        )
        .unwrap();
        dir.write("kept/old/x.exe", b"kept");

        link_missing(&dir.path().join("kept"), &dir.path().join("cache"));

        let cache = dir.path().join("cache");
        assert_eq!(
            std::fs::read_link(cache.join("directx9/redist.exe")).unwrap(),
            kept
        );
        assert_eq!(std::fs::read(cache.join("vcrun/vc.exe")).unwrap(), b"own");
        assert_eq!(std::fs::read(cache.join("old/x.exe")).unwrap(), b"kept");
    }

    #[test]
    fn no_proton_is_an_error() {
        let steam = TempDir::new();
        let installed = Installed::read_from(steam.path());
        let err = Runner::new(&installed, &windows_game(), &Downloads::default()).unwrap_err();
        assert!(err.contains("No Proton"));
    }
}
