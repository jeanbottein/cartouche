//! Steam integration: non-Steam shortcuts in `userdata/<id>/config/shortcuts.vdf`
//! (pipeline steps 5 and 6), their artwork in the `grid/` folder, and Proton
//! compatibility mappings in `config/config.vdf`.
//!
//! Shortcuts we own carry this cartridge's ownership tag (`cartridge:<id>`),
//! so other non-Steam games, and other cartridges' games, are never touched.
//! The cartridge's label (its folder name) is a second, visible tag.
//!
//! The user picks which games are in Steam, per game (Library, Y);
//! `system/steam.json` remembers the choice, and the pipeline keeps exactly
//! those shortcuts up to date. Steam reads shortcuts at startup: a change
//! shows after Steam restarts (see `is_running`).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use log::{debug, error, info, warn};

use super::app::APP_NAME;
use super::config::Config;
use super::models::{to_pretty_json, Game, ImageSlot};
use super::vdf::{self, Map, Value};
use super::{paths, proton};

pub const PROTON_PREFIX: &str = "~/.local/share/Steam/steamapps/compatdata/{appid}/pfx/drive_c";
const DEFAULT_COMPAT_PRIORITY: &str = "250";

pub fn proton_prefix(appid: u32) -> String {
    PROTON_PREFIX.replace("{appid}", &appid.to_string())
}

/// Stable non-Steam shortcut AppID (what Steam itself computes).
pub fn generate_appid(name: &str, exe: &str) -> u32 {
    crc32fast::hash(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

/// The AppID of `game`'s shortcut and Proton prefix when it runs `exe`: the
/// one game.json keeps, else the one its title gives now.
pub fn appid_for(game: &Game, exe: &Path) -> u32 {
    game.steam_appid
        .unwrap_or_else(|| generate_appid(&game.title, &paths::lossy(exe)))
}

/// `appid_for` the game's resolved target.
pub fn appid_of(game: &Game) -> Option<u32> {
    game.resolved.as_ref().map(|r| appid_for(game, &r.target))
}

/// `shortcuts.vdf` stores AppIDs as signed 32-bit ints.
pub fn signed32(appid: u32) -> i32 {
    appid as i32
}

// ── Steam user directories ───────────────────────────────────────────────

/// `userdata/0` is Steam's placeholder for "nobody logged in", not an account.
fn is_account(userdata_entry: &str) -> bool {
    userdata_entry != "0"
}

/// Every `userdata/<id>/config` directory (deduplicated across symlinked roots).
pub fn userdata_config_dirs() -> Vec<PathBuf> {
    let home = paths::home_dir();
    let bases = [
        home.join(".steam/steam/userdata"),
        home.join(".local/share/Steam/userdata"),
    ];
    let mut seen = HashSet::new();
    bases
        .iter()
        .flat_map(|base| {
            paths::sorted_entries(base)
                .into_iter()
                .filter(|uid| is_account(uid))
                .map(move |uid| base.join(uid).join("config"))
        })
        .filter(|dir| dir.is_dir() && seen.insert(paths::resolve(dir)))
        .collect()
}

/// Steam64 ids are the account id (the `userdata/<id>` folder name) plus this.
const STEAM64_BASE: u64 = 76_561_197_960_265_728;

/// Display names by account id, from the text of `config/loginusers.vdf`.
fn persona_names(loginusers: &str) -> BTreeMap<String, String> {
    let Ok(parsed) = vdf::parse_text(loginusers) else {
        return BTreeMap::new();
    };
    let Some(users) = parsed.get_map("users") else {
        return BTreeMap::new();
    };
    users
        .0
        .iter()
        .filter_map(|(steam64, user)| {
            let account = steam64.parse::<u64>().ok()?.checked_sub(STEAM64_BASE)?;
            let Value::Map(user) = user else { return None };
            Some((
                account.to_string(),
                user.get_str("PersonaName")?.to_string(),
            ))
        })
        .collect()
}

/// Steam accounts on this machine as (account id, label), the label being
/// "Name (id)" when Steam knows the name.
pub fn users() -> Vec<(String, String)> {
    userdata_config_dirs()
        .iter()
        .map(|config_dir| {
            let id = user_id(config_dir);
            let names = steam_root(config_dir)
                .and_then(|root| std::fs::read_to_string(root.join("config/loginusers.vdf")).ok())
                .map(|text| persona_names(&text))
                .unwrap_or_default();
            let label = names
                .get(&id)
                .map_or_else(|| id.clone(), |name| format!("{name} ({id})"));
            (id, label)
        })
        .collect()
}

/// User config dirs, narrowed to `STEAM_USERID` when set.
pub fn select_config_dirs(cfg: &Config) -> Vec<PathBuf> {
    let dirs = userdata_config_dirs();
    if dirs.is_empty() {
        debug!("No Steam userdata directories found");
        return dirs;
    }
    let Some(uid) = cfg
        .get("STEAM_USERID")
        .map(str::trim)
        .filter(|u| !u.is_empty())
    else {
        return dirs;
    };
    let needle = format!("/{uid}/");
    let filtered: Vec<PathBuf> = dirs
        .into_iter()
        .filter(|d| paths::lossy(d).contains(&needle))
        .collect();
    if filtered.is_empty() {
        warn!("STEAM_USERID={uid} not found");
    }
    filtered
}

const SELECTION_FILE: &str = "steam.json";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Selection {
    /// Game folder names.
    games: BTreeSet<String>,
}

fn selection_path(cfg: &Config) -> PathBuf {
    cfg.app_dir().join(SELECTION_FILE)
}

fn load_selection(cfg: &Config) -> Option<BTreeSet<String>> {
    let text = std::fs::read_to_string(selection_path(cfg)).ok()?;
    serde_json::from_str::<Selection>(&text)
        .ok()
        .map(|s| s.games)
}

fn save_selection(cfg: &Config, games: BTreeSet<String>) -> Result<(), String> {
    let path = selection_path(cfg);
    paths::write_atomic(&path, to_pretty_json(&Selection { games }))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The games that already have one of our shortcuts (first run, before
/// `steam.json` exists).
fn games_with_shortcuts(cfg: &Config, games: &[Game]) -> BTreeSet<String> {
    let owner = Owner::of(cfg);
    let exes: HashSet<String> = userdata_config_dirs()
        .iter()
        .filter_map(|dir| load_shortcuts(&shortcuts_path(dir)).ok())
        .flat_map(|shortcuts| {
            shortcuts
                .values()
                .filter_map(as_shortcut)
                .filter(|s| has_ownership_tag(s, &owner.tag))
                .map(|s| exe_of(s).to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    games
        .iter()
        .filter(|g| resolved_target(g).is_some_and(|t| exes.contains(&t)))
        .map(|g| g.folder_name.clone())
        .collect()
}

/// This cartridge's games the user put in Steam.
pub fn games_in_steam(cfg: &Config, games: &[Game]) -> BTreeSet<String> {
    load_selection(cfg).unwrap_or_else(|| games_with_shortcuts(cfg, games))
}

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

pub fn user_id(config_dir: &Path) -> String {
    config_dir
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn shortcuts_path(config_dir: &Path) -> PathBuf {
    config_dir.join("shortcuts.vdf")
}

pub fn grid_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("grid")
}

// ── shortcuts.vdf ────────────────────────────────────────────────────────

/// The shortcut entries of a `shortcuts.vdf` (empty when the file is missing).
pub fn load_shortcuts(path: &Path) -> Result<Map, String> {
    if !path.exists() {
        return Ok(Map::default());
    }
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root = vdf::parse_binary(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(root.get_map("shortcuts").cloned().unwrap_or_default())
}

pub fn save_shortcuts(path: &Path, shortcuts: Map) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let root = Map(vec![("shortcuts".to_string(), Value::Map(shortcuts))]);
    paths::write_atomic(path, vdf::to_binary(&root))
}

pub fn as_shortcut(value: &Value) -> Option<&Map> {
    match value {
        Value::Map(m) => Some(m),
        _ => None,
    }
}

pub fn tag_values(shortcut: &Map) -> Vec<&str> {
    shortcut
        .get_map("tags")
        .map(|tags| {
            tags.values()
                .filter_map(|v| {
                    if let Value::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Which shortcuts are this cartridge's: its id tag, plus its label shown in
/// Steam as a tag (a collection). The id never changes with renames or moves.
pub struct Owner {
    pub tag: String,
    pub label: String,
}

impl Owner {
    pub fn of(cfg: &Config) -> Owner {
        Owner {
            tag: format!("{APP_NAME}:{}", cfg.cartridge_id()),
            label: cfg.label(),
        }
    }
}

pub fn has_ownership_tag(shortcut: &Map, owner_tag: &str) -> bool {
    tag_values(shortcut).contains(&owner_tag)
}

pub fn appname(shortcut: &Map) -> &str {
    [shortcut.get_str("AppName"), shortcut.get_str("appname")]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .unwrap_or("")
}

/// The raw `Exe` value (quoted, as Steam stores it).
pub fn raw_exe(shortcut: &Map) -> &str {
    shortcut
        .get_str("Exe")
        .or_else(|| shortcut.get_str("exe"))
        .unwrap_or("")
}

pub fn exe_of(shortcut: &Map) -> &str {
    raw_exe(shortcut).trim_matches('"')
}

pub fn next_index(shortcuts: &Map) -> String {
    shortcuts
        .keys()
        .filter_map(|k| k.parse::<u64>().ok())
        .max()
        .map_or(0, |m| m + 1)
        .to_string()
}

/// Renumber entries "0", "1", ... keeping their order.
pub fn reindex(shortcuts: Map) -> Map {
    let mut entries = shortcuts.0;
    entries.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(0));
    Map(entries
        .into_iter()
        .enumerate()
        .map(|(i, (_, v))| (i.to_string(), v))
        .collect())
}

/// A shortcut entry in the layout Steam writes.
pub fn make_shortcut(
    name: &str,
    exe: &str,
    start_dir: &str,
    launch_options: &str,
    icon: &str,
    tags: &[&str],
) -> Map {
    let int = Value::Int32;
    let text = |s: &str| Value::Str(s.to_string());
    let entries = vec![
        ("appid", int(signed32(generate_appid(name, exe)))),
        ("AppName", text(name)),
        ("Exe", text(&format!("\"{exe}\""))),
        ("StartDir", text(&format!("\"{start_dir}\""))),
        ("icon", text(icon)),
        ("ShortcutPath", text("")),
        ("LaunchOptions", text(launch_options)),
        ("IsHidden", int(0)),
        ("AllowDesktopConfig", int(1)),
        ("AllowOverlay", int(1)),
        ("OpenVR", int(0)),
        ("Devkit", int(0)),
        ("DevkitGameID", text("")),
        ("DevkitOverrideAppID", int(0)),
        ("LastPlayTime", int(0)),
        (
            "tags",
            Value::Map(Map(tags
                .iter()
                .enumerate()
                .map(|(i, tag)| (i.to_string(), text(tag)))
                .collect())),
        ),
    ];
    Map(entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect())
}

/// Names of the shortcuts this cartridge owns for one Steam user.
pub fn owned_shortcut_names(config_dir: &Path, owner_tag: &str) -> Result<Vec<String>, String> {
    let shortcuts = load_shortcuts(&shortcuts_path(config_dir))?;
    Ok(shortcuts
        .values()
        .filter_map(as_shortcut)
        .filter(|s| has_ownership_tag(s, owner_tag))
        .map(|s| appname(s).to_string())
        .collect())
}

fn resolved_target(game: &Game) -> Option<String> {
    game.resolved.as_ref().map(|r| paths::lossy(&r.target))
}

// ── Step 5: remove stale shortcuts ───────────────────────────────────────

/// Remove our shortcuts whose executable matches `doomed`. Returns how many.
fn remove_owned(
    config_dir: &Path,
    owner_tag: &str,
    doomed: impl Fn(&str) -> bool,
) -> Result<usize, String> {
    let path = shortcuts_path(config_dir);
    let shortcuts = load_shortcuts(&path)?;
    let (removed, kept): (Vec<_>, Vec<_>) = shortcuts.0.into_iter().partition(|(_, v)| {
        as_shortcut(v).is_some_and(|s| has_ownership_tag(s, owner_tag) && doomed(exe_of(s)))
    });
    if removed.is_empty() {
        return Ok(0);
    }
    for (_, shortcut) in &removed {
        info!(
            "  Removing shortcut: {}",
            as_shortcut(shortcut).map(appname).unwrap_or("")
        );
    }
    save_shortcuts(&path, reindex(Map(kept))).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(removed.len())
}

/// The shortcuts cleanup keeps: those of the games in Steam, and any in a
/// game folder the scan could not load (a game.json mid-write or with a
/// typo says nothing about whether the game is still wanted).
struct Kept {
    targets: HashSet<String>,
    unloaded_dirs: Vec<PathBuf>,
}

impl Kept {
    fn of(games: &[Game], selected: &BTreeSet<String>, games_dir: Option<&Path>) -> Kept {
        let loaded: HashSet<&Path> = games.iter().map(|g| g.dir.as_path()).collect();
        let unloaded_dirs = games_dir
            .map(paths::visible_subdirs)
            .unwrap_or_default()
            .into_iter()
            .filter(|dir| !loaded.contains(dir.as_path()))
            .collect();
        Kept {
            targets: games
                .iter()
                .filter(|g| selected.contains(&g.folder_name))
                .filter_map(resolved_target)
                .collect(),
            unloaded_dirs,
        }
    }

    fn is_stale(&self, exe: &str) -> bool {
        !self.targets.contains(exe)
            && !self
                .unloaded_dirs
                .iter()
                .any(|d| Path::new(exe).starts_with(d))
    }
}

fn remove_stale(config_dir: &Path, kept: &Kept, owner_tag: &str) -> Result<(), String> {
    let removed = remove_owned(config_dir, owner_tag, |exe| kept.is_stale(exe))?;
    if removed > 0 {
        info!(
            "  Steam user {}: removed {removed} stale shortcut(s)",
            user_id(config_dir)
        );
    }
    Ok(())
}

/// Remove our shortcuts whose executable is no longer in the library.
pub fn clean(games: &[Game], cfg: &Config) {
    let kept = Kept::of(
        games,
        &games_in_steam(cfg, games),
        cfg.games_dir().as_deref(),
    );
    let owner = Owner::of(cfg);
    for config_dir in select_config_dirs(cfg) {
        if let Err(e) = remove_stale(&config_dir, &kept, &owner.tag) {
            error!("  Skipping Steam user {}: {e}", user_id(&config_dir));
        }
    }
}

// ── Step 6: create / update shortcuts ────────────────────────────────────

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

fn copy_if_missing(src: &Path, dest: &Path) {
    if dest.is_file() || !src.is_file() {
        return;
    }
    if let Err(e) = paths::copy_with_mtime(src, dest) {
        warn!(
            "    Failed to copy artwork {} -> {}: {e}",
            src.display(),
            dest.display()
        );
    }
}

/// Copy `.cartridge/` artwork into Steam's grid folder under the AppID names.
fn copy_artwork(game: &Game, grid: &Path, appid: u32) {
    if let Err(e) = std::fs::create_dir_all(grid) {
        warn!("    Cannot create {}: {e}", grid.display());
        return;
    }
    let slots = [
        (ImageSlot::Cover, format!("{appid}p")),
        (ImageSlot::Hero, format!("{appid}_hero")),
        (ImageSlot::Logo, format!("{appid}_logo")),
        (ImageSlot::Icon, format!("{appid}_icon")),
    ];
    for (slot, prefix) in slots {
        let Some(name) = game.images.get(slot) else {
            continue;
        };
        copy_if_missing(
            &game.data_dir().join(name),
            &grid.join(format!("{prefix}{}", extension(name))),
        );
    }
    if let Some(name) = game.images.get(ImageSlot::Header) {
        install_wide_capsule(&game.data_dir().join(name), grid, appid);
    }
}

/// The wide capsule must be the wide header, never the vertical cover that
/// older versions copied under the same name: replace whatever sits there
/// unless it already is this file (same modification time).
fn install_wide_capsule(src: &Path, grid: &Path, appid: u32) {
    let dest = grid.join(format!("{appid}{}", extension(&src.to_string_lossy())));
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if !src.is_file() || (dest.is_file() && modified(&dest) == modified(src)) {
        return;
    }
    for other in ["png", "jpg", "jpeg", "webp"] {
        // Ignore failures: the file may not exist.
        let _ = std::fs::remove_file(grid.join(format!("{appid}.{other}")));
    }
    if let Err(e) = paths::copy_with_mtime(src, &dest) {
        warn!(
            "    Failed to copy artwork {} -> {}: {e}",
            src.display(),
            dest.display()
        );
    }
}

fn icon_path(game: &Game, grid: &Path, appid: u32) -> String {
    game.images
        .icon
        .as_deref()
        .map(|icon| paths::lossy(&grid.join(format!("{appid}_icon{}", extension(icon)))))
        .unwrap_or_default()
}

/// Add or refresh shortcuts for one Steam user. Returns (added, updated).
fn export_to_user(
    config_dir: &Path,
    games: &[(&Game, u32)],
    owner: &Owner,
) -> Result<(usize, usize), String> {
    let path = shortcuts_path(config_dir);
    let grid = grid_dir(config_dir);
    let mut shortcuts = load_shortcuts(&path)?;
    let (mut added, mut updated) = (0, 0);

    for &(game, appid) in games {
        let Some(resolved) = &game.resolved else {
            continue;
        };
        let target = paths::lossy(&resolved.target);
        let start_in = paths::lossy(&resolved.start_in);
        let icon = icon_path(game, &grid, appid);
        let managed = managed_launch_options(game);
        let entry = |options: &str| {
            let mut entry = make_shortcut(
                &game.title,
                &target,
                &start_in,
                options,
                &icon,
                &[&owner.tag, &owner.label],
            );
            entry.insert("appid", Value::Int32(signed32(appid)));
            entry
        };

        // (key, launch options to keep, changed?) of the shortcut we already
        // own for this exe. Options set in Steam stay unless the game's own
        // Proton environment manages them.
        let owned = shortcuts.0.iter().find_map(|(k, v)| {
            let s = as_shortcut(v)
                .filter(|s| has_ownership_tag(s, &owner.tag) && exe_of(s) == target)?;
            let current = s.get_str("LaunchOptions").unwrap_or("");
            let options = managed.clone().unwrap_or_else(|| current.to_string());
            let wanted = entry(&options);
            let changed = appname(s) != game.title
                || options != current
                || ["appid", "icon", "StartDir", "tags"]
                    .iter()
                    .any(|key| s.get(key) != wanted.get(key));
            Some((k.clone(), options, changed))
        });
        match owned {
            Some((_, _, false)) => {}
            Some((key, options, true)) => {
                shortcuts.insert(&key, Value::Map(entry(&options)));
                updated += 1;
            }
            None => {
                let key = next_index(&shortcuts);
                let options = managed.unwrap_or_else(|| resolved.launch_options.clone());
                shortcuts.insert(&key, Value::Map(entry(&options)));
                added += 1;
            }
        }
        copy_artwork(game, &grid, appid);
    }

    if added + updated > 0 {
        save_shortcuts(&path, shortcuts).map_err(|e| format!("{}: {e}", path.display()))?;
        info!(
            "  Steam user {}: +{added} added, ~{updated} updated",
            user_id(config_dir)
        );
    }
    Ok((added, updated))
}

/// Launch options cartridge owns on a shortcut: those of a Windows game with
/// a Proton environment (`KEY=value %command% args`). Otherwise the
/// shortcut's own options are left as they are.
fn managed_launch_options(game: &Game) -> Option<String> {
    let resolved = game.resolved.as_ref().filter(|r| r.os == "windows")?;
    (!game.proton.env.is_empty())
        .then(|| proton::shortcut_launch_options(&game.proton.env, &resolved.launch_options))
}

/// The Proton build for a Windows game's shortcut: the one its game.json
/// names when it is installed here, else the default (only set when the
/// shortcut has none). A build missing here is never forced on Steam.
fn compat_tool(game: &Game, installed: Option<&proton::Installed>) -> CompatTool {
    let default = installed
        .and_then(proton::Installed::default_tool)
        .map_or_else(|| proton::FALLBACK_TOOL.to_string(), |t| t.name.clone());
    let pinned = game.proton.version.trim();
    if pinned.is_empty() {
        return CompatTool::Default(default);
    }
    match installed.and_then(|i| i.find(pinned)) {
        Some(tool) => CompatTool::Pinned(tool.name.clone()),
        None => {
            warn!(
                "{}: {pinned} is not installed, Steam keeps its build",
                game.title
            );
            CompatTool::Default(default)
        }
    }
}

/// Write shortcuts (and Proton for Windows games) for `with_targets` into
/// each Steam user's config. Returns (added, updated).
fn export_games(
    cfg: &Config,
    config_dirs: &[PathBuf],
    with_targets: &[(&Game, u32)],
) -> (usize, usize) {
    let owner = Owner::of(cfg);
    let counts = config_dirs
        .iter()
        .filter_map(|dir| {
            export_to_user(dir, with_targets, &owner)
                .map_err(|e| error!("  Skipping Steam user {}: {e}", user_id(dir)))
                .ok()
        })
        .fold((0, 0), |(a, u), (da, du)| (a + da, u + du));
    let windows: Vec<(&Game, u32)> = with_targets
        .iter()
        .filter(|(g, _)| g.resolved.as_ref().is_some_and(|r| r.os == "windows"))
        .copied()
        .collect();
    if windows.is_empty() {
        return counts;
    }
    let installed = proton::Installed::read();
    let wanted: Vec<(u32, CompatTool)> = windows
        .iter()
        .map(|&(game, appid)| (appid, compat_tool(game, installed.as_ref())))
        .collect();
    config_dirs.iter().for_each(|dir| {
        set_compat_tools(&wanted, dir);
    });
    counts
}

fn with_appid(game: &Game) -> Option<(&Game, u32)> {
    appid_of(game).map(|appid| (game, appid))
}

/// Pipeline step 6: create/update the shortcuts of the games the user put in
/// Steam, then set Proton for the Windows ones.
pub fn export(games: &[Game], cfg: &Config) {
    let selected = games_in_steam(cfg, games);
    let with_targets: Vec<(&Game, u32)> = games
        .iter()
        .filter(|g| selected.contains(&g.folder_name))
        .filter_map(with_appid)
        .collect();
    if with_targets.is_empty() {
        info!("No games to keep in Steam");
        return;
    }
    let (added, updated) = export_games(cfg, &select_config_dirs(cfg), &with_targets);
    if added + updated > 0 {
        info!("Steam export complete: {added} added, {updated} updated");
    } else {
        info!("Steam shortcuts already up to date");
    }
}

/// Put one game in Steam now, and remember it.
pub fn add_game(cfg: &Config, library: &[Game], game: &Game) -> Result<(), String> {
    let entry =
        with_appid(game).ok_or("No executable found yet: run the pipeline from STATUS first")?;
    let config_dirs = select_config_dirs(cfg);
    if config_dirs.is_empty() {
        return Err("Steam not found on this machine".into());
    }
    let mut selected = games_in_steam(cfg, library);
    export_games(cfg, &config_dirs, &[entry]);
    selected.insert(game.folder_name.clone());
    save_selection(cfg, selected)
}

/// Take one game out of Steam now (only this cartridge's shortcut), and remember it.
pub fn remove_game(cfg: &Config, library: &[Game], game: &Game) -> Result<(), String> {
    let mut selected = games_in_steam(cfg, library);
    if let Some(target) = resolved_target(game) {
        let owner = Owner::of(cfg);
        for dir in select_config_dirs(cfg) {
            remove_owned(&dir, &owner.tag, |exe| exe == target)?;
        }
    }
    selected.remove(&game.folder_name);
    save_selection(cfg, selected)
}

// ── Step 6b: Proton for Windows games ────────────────────────────────────

/// Steam root from a `userdata/<id>/config` path.
fn steam_root(config_dir: &Path) -> Option<PathBuf> {
    let root = config_dir.parent()?.parent()?.parent()?;
    root.join("config").is_dir().then(|| root.to_path_buf())
}

/// The Proton build wanted for a Windows game's shortcut.
#[derive(Clone, Debug, PartialEq)]
pub enum CompatTool {
    /// The build used when the game names none: set only when the shortcut
    /// has none, so a build picked in Steam stays.
    Default(String),
    /// Named in the game's game.json: always set.
    Pinned(String),
}

/// Map AppIDs to their Proton build in `config.vdf`. Returns how many changed.
pub fn set_compat_tools(wanted: &[(u32, CompatTool)], config_dir: &Path) -> usize {
    let Some(root) = steam_root(config_dir) else {
        warn!(
            "Could not determine Steam root from {}",
            config_dir.display()
        );
        return 0;
    };
    let path = root.join("config").join("config.vdf");
    if !path.is_file() {
        warn!("config.vdf not found at {}/config/", root.display());
        return 0;
    }
    let mut data = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|t| vdf::parse_text(&t))
    {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to read {}: {e}", path.display());
            return 0;
        }
    };

    let mapping = [
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    ]
    .iter()
    .fold(&mut data, |map, key| map.map_entry_ignore_case(key));
    let changes: Vec<(String, &str)> = wanted
        .iter()
        .filter_map(|(appid, tool)| {
            let id = appid.to_string();
            let current = mapping.get_map(&id).and_then(|m| m.get_str("name"));
            match tool {
                CompatTool::Default(_) if mapping.contains(&id) => None,
                CompatTool::Pinned(name) if current == Some(name.as_str()) => None,
                CompatTool::Default(name) | CompatTool::Pinned(name) => Some((id, name.as_str())),
            }
        })
        .collect();
    for (appid, tool) in &changes {
        let entry = [
            ("name", *tool),
            ("config", ""),
            ("priority", DEFAULT_COMPAT_PRIORITY),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), Value::Str(v.to_string())))
        .collect();
        mapping.insert(appid, Value::Map(Map(entry)));
    }
    if changes.is_empty() {
        return 0;
    }
    if let Err(e) = paths::write_atomic(&path, vdf::to_text(&data)) {
        error!("Failed to write {}: {e}", path.display());
        return 0;
    }
    info!(
        "Set Proton for {} Windows game(s) in config.vdf",
        changes.len()
    );
    changes.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Resolved;
    use crate::core::testutil::TempDir;

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

    fn shortcut(name: &str, exe: &str, tag: &str) -> Value {
        Value::Map(make_shortcut(name, exe, "/", "", "", &[tag]))
    }

    #[test]
    fn appid_matches_python_generate_appid() {
        assert_eq!(
            generate_appid("Hollow Knight", "/g/Hollow Knight/hollow_knight.exe"),
            3770780903
        );
        assert_eq!(
            generate_appid("Hollow Knight", "/g/HK/hk.x86_64"),
            2664893048
        );
        assert_eq!(signed32(2664893048), -1630074248);
    }

    #[test]
    fn the_placeholder_user_zero_is_not_an_account() {
        assert!(!is_account("0"));
        assert!(is_account("66213690"));
        assert!(is_account("10"));
    }

    #[test]
    fn persona_names_are_keyed_by_account_id() {
        let names = persona_names(
            "\"users\"\n{\n\t\"76561197960265730\"\n\t{\n\t\t\"AccountName\"\t\"x\"\n\t\t\"PersonaName\"\t\"Jean\"\n\t}\n}\n",
        );
        assert_eq!(names.get("2").map(String::as_str), Some("Jean"));
        assert!(persona_names("not vdf {").is_empty());
    }

    #[test]
    fn wide_capsule_replaces_a_cover_copy_left_by_older_versions() {
        let dir = TempDir::new();
        let grid = dir.path().join("grid");
        std::fs::create_dir_all(&grid).unwrap();
        let header = dir.path().join("header.jpg");
        std::fs::write(&header, "wide").unwrap();
        std::fs::write(grid.join("7.png"), "vertical cover").unwrap();

        install_wide_capsule(&header, &grid, 7);

        assert!(!grid.join("7.png").exists());
        assert_eq!(std::fs::read_to_string(grid.join("7.jpg")).unwrap(), "wide");
        install_wide_capsule(&header, &grid, 7);
        assert_eq!(std::fs::read_to_string(grid.join("7.jpg")).unwrap(), "wide");
    }

    #[test]
    fn new_shortcut_matches_python_layout() {
        let golden = vdf::parse_binary(include_bytes!("../../testdata/shortcuts.vdf")).unwrap();
        let expected = golden.get_map("shortcuts").unwrap().get_map("0").unwrap();
        let built = make_shortcut(
            "Hollow Knight",
            "/g/HK/hk.x86_64",
            "/g/HK",
            "-v",
            "/grid/1_icon.png",
            &["golden"],
        );
        assert_eq!(&built, expected);
    }

    #[test]
    fn ownership_is_by_tag() {
        let Value::Map(ours) = shortcut("A", "/a", "cartridge:1") else {
            unreachable!()
        };
        let Value::Map(other_cartridge) = shortcut("B", "/b", "cartridge:2") else {
            unreachable!()
        };
        assert!(has_ownership_tag(&ours, "cartridge:1"));
        assert!(!has_ownership_tag(&other_cartridge, "cartridge:1"));
        assert_eq!(exe_of(&ours), "/a");
    }

    #[test]
    fn stale_owned_shortcuts_are_removed_and_reindexed() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let shortcuts = Map(vec![
            ("0".into(), shortcut("Gone", "/g/gone", "cartridge:1")),
            ("1".into(), shortcut("Mine", "/g/kept", "favorite")),
            ("2".into(), shortcut("Kept", "/g/kept2", "cartridge:1")),
            ("3".into(), shortcut("Other", "/g/other", "cartridge:2")),
        ]);
        save_shortcuts(&shortcuts_path(&config_dir), shortcuts).unwrap();

        remove_stale(
            &config_dir,
            &Kept {
                targets: HashSet::from(["/g/kept2".to_string()]),
                unloaded_dirs: Vec::new(),
            },
            "cartridge:1",
        )
        .unwrap();
        let after = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let names: Vec<(&str, &str)> = after
            .0
            .iter()
            .map(|(k, v)| (k.as_str(), appname(as_shortcut(v).unwrap())))
            .collect();
        assert_eq!(names, vec![("0", "Mine"), ("1", "Kept"), ("2", "Other")]);
    }

    #[test]
    fn export_adds_once_then_updates_renamed_games() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Celeste"));
        game.title = "Celeste".into();
        game.resolved = Some(Resolved {
            target: "/g/celeste".into(),
            start_in: "/g".into(),
            ..Default::default()
        });
        let appid = generate_appid("Celeste", "/g/celeste");
        let mut owner = Owner {
            tag: "cartridge:1".into(),
            label: "Potato-games".into(),
        };
        let export = |owner: &Owner, game: &Game| {
            export_to_user(&config_dir, &[(game, appid)], owner).unwrap()
        };

        assert_eq!(export(&owner, &game), (1, 0));
        assert_eq!(export(&owner, &game), (0, 0));
        game.title = "Celeste DX".into();
        assert_eq!(export(&owner, &game), (0, 1));
        // Renaming the cartridge folder updates the visible tag, same shortcut.
        owner.label = "Potatoes".into();
        assert_eq!(export(&owner, &game), (0, 1));
        let saved = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let entry = as_shortcut(saved.values().next().unwrap()).unwrap();
        assert_eq!(tag_values(entry), vec!["cartridge:1", "Potatoes"]);
        assert_eq!(
            load_shortcuts(&shortcuts_path(&config_dir))
                .unwrap()
                .0
                .len(),
            1
        );
    }

    #[test]
    fn a_renamed_game_keeps_its_stored_appid() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Celeste"));
        game.resolved = Some(Resolved {
            target: "/g/celeste".into(),
            ..Default::default()
        });
        game.steam_appid = appid_of(&game);
        let before = game.steam_appid.unwrap();
        game.title = "Celeste DX".into();
        assert_eq!(appid_of(&game), Some(before));

        let owner = Owner {
            tag: "cartridge:1".into(),
            label: "games".into(),
        };
        export_to_user(&config_dir, &[with_appid(&game).unwrap()], &owner).unwrap();
        let saved = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let entry = as_shortcut(saved.values().next().unwrap()).unwrap();
        assert_eq!(entry.get_int("appid"), Some(i64::from(signed32(before))));
        assert_eq!(appname(entry), "Celeste DX");
    }

    #[test]
    fn without_a_stored_appid_it_comes_from_title_and_exe() {
        let mut game = Game::skeleton(Path::new("/g/Hollow Knight"));
        game.title = "Hollow Knight".into();
        game.resolved = Some(Resolved {
            target: "/g/Hollow Knight/hollow_knight.exe".into(),
            ..Default::default()
        });
        assert_eq!(appid_of(&game), Some(3770780903));
    }

    #[test]
    fn cleanup_keeps_shortcuts_of_game_folders_that_did_not_load() {
        let dir = TempDir::new();
        let games_dir = dir.mkdir("games");
        let loaded = dir.mkdir("games/Loaded");
        let broken = dir.mkdir("games/Broken");
        let mut game = Game::skeleton(&loaded);
        game.resolved = Some(Resolved {
            target: loaded.join("run"),
            ..Default::default()
        });
        let kept = Kept::of(
            &[game],
            &BTreeSet::from(["Loaded".into()]),
            Some(&games_dir),
        );
        let exe = |p: PathBuf| paths::lossy(&p);

        assert!(!kept.is_stale(&exe(loaded.join("run"))));
        assert!(!kept.is_stale(&exe(broken.join("run"))));
        assert!(kept.is_stale(&exe(loaded.join("old-target"))));
        assert!(kept.is_stale(&exe(games_dir.join("Deleted/run"))));
    }

    #[test]
    fn cleanup_removes_games_taken_out_of_steam() {
        let dir = TempDir::new();
        let games_dir = dir.mkdir("games");
        let folder = dir.mkdir("games/Out");
        let mut game = Game::skeleton(&folder);
        game.resolved = Some(Resolved {
            target: folder.join("run"),
            ..Default::default()
        });
        let kept = Kept::of(&[game], &BTreeSet::new(), Some(&games_dir));
        assert!(kept.is_stale(&paths::lossy(&folder.join("run"))));
    }

    #[test]
    fn export_updates_a_changed_start_folder() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Celeste"));
        game.resolved = Some(Resolved {
            target: "/g/celeste".into(),
            start_in: "/g".into(),
            ..Default::default()
        });
        let owner = Owner {
            tag: "cartridge:1".into(),
            label: "games".into(),
        };
        let appid = generate_appid(&game.title, "/g/celeste");
        export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap();

        game.resolved.as_mut().unwrap().start_in = "/g/bin".into();
        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap(),
            (0, 1)
        );
        let saved = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let entry = as_shortcut(saved.values().next().unwrap()).unwrap();
        assert_eq!(entry.get_str("StartDir"), Some("\"/g/bin\""));
    }

    #[test]
    fn compat_tools_are_added_once_case_insensitively() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("Steam/userdata/1/config");
        let vdf_path = dir.write(
            "Steam/config/config.vdf",
            b"\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"Other\"\t\t\"1\"\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let default = |appid| (appid, CompatTool::Default("proton_9".into()));
        assert_eq!(
            set_compat_tools(&[default(123), default(456)], &config_dir),
            2
        );
        assert_eq!(set_compat_tools(&[default(123)], &config_dir), 0);
        let data = vdf::parse_text(&std::fs::read_to_string(vdf_path).unwrap()).unwrap();
        let steam = data
            .get_map("InstallConfigStore")
            .unwrap()
            .get_map("Software")
            .unwrap()
            .get_map("valve")
            .unwrap()
            .get_map("Steam")
            .unwrap();
        assert_eq!(steam.get_str("Other"), Some("1"));
        let mapping = steam.get_map("CompatToolMapping").unwrap();
        assert_eq!(
            mapping.get_map("456").unwrap().get_str("name"),
            Some("proton_9")
        );
    }

    #[test]
    fn a_game_pinned_build_replaces_the_shortcut_build() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("Steam/userdata/1/config");
        dir.write("Steam/config/config.vdf", b"\"InstallConfigStore\" { }");
        let default = [(123, CompatTool::Default("proton_9".into()))];
        let pinned = [(123, CompatTool::Pinned("proton_10".into()))];
        assert_eq!(set_compat_tools(&default, &config_dir), 1);
        assert_eq!(set_compat_tools(&pinned, &config_dir), 1);
        assert_eq!(set_compat_tools(&pinned, &config_dir), 0);
        assert_eq!(set_compat_tools(&default, &config_dir), 0);
    }

    #[test]
    fn windows_shortcuts_carry_the_game_environment() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Rayman"));
        game.title = "Rayman".into();
        game.resolved = Some(Resolved {
            target: "/g/rayman.exe".into(),
            start_in: "/g".into(),
            launch_options: "-w".into(),
            os: "windows".into(),
        });
        let appid = generate_appid("Rayman", "/g/rayman.exe");
        let owner = Owner {
            tag: "cartridge:1".into(),
            label: "games".into(),
        };
        let options = || {
            let saved = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
            let entry = as_shortcut(saved.values().next().unwrap()).unwrap().clone();
            entry.get_str("LaunchOptions").unwrap().to_string()
        };

        export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap();
        assert_eq!(options(), "-w");
        game.proton
            .env
            .insert("PROTON_USE_WINED3D".into(), "1".into());
        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap(),
            (0, 1)
        );
        assert_eq!(options(), "PROTON_USE_WINED3D=1 %command% -w");
    }

    #[test]
    fn launch_options_set_in_steam_survive_updates() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let mut game = Game::skeleton(&dir.mkdir("games/Celeste"));
        game.title = "Celeste".into();
        game.resolved = Some(Resolved {
            target: "/g/celeste".into(),
            start_in: "/g".into(),
            ..Default::default()
        });
        let appid = generate_appid("Celeste", "/g/celeste");
        let owner = Owner {
            tag: "cartridge:1".into(),
            label: "games".into(),
        };
        let path = shortcuts_path(&config_dir);
        export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap();
        let mut saved = load_shortcuts(&path).unwrap();
        if let Some((_, Value::Map(entry))) = saved.0.first_mut() {
            entry.insert("LaunchOptions", Value::Str("gamemoderun %command%".into()));
        }
        save_shortcuts(&path, saved).unwrap();

        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap(),
            (0, 0)
        );
        game.title = "Celeste DX".into();
        assert_eq!(
            export_to_user(&config_dir, &[(&game, appid)], &owner).unwrap(),
            (0, 1)
        );
        let saved = load_shortcuts(&path).unwrap();
        let entry = as_shortcut(saved.values().next().unwrap()).unwrap();
        assert_eq!(
            entry.get_str("LaunchOptions"),
            Some("gamemoderun %command%")
        );
    }

    fn windows_game_pinned(version: &str) -> Game {
        let mut game = Game::skeleton(Path::new("/g/Rayman"));
        game.proton.version = version.into();
        game
    }

    #[test]
    fn only_an_installed_pinned_build_is_forced_on_steam() {
        let steam = TempDir::new();
        steam.write("steamapps/common/Proton 10.0/proton", b"");
        let installed = proton::Installed::read_from(steam.path());
        let tool = |version| compat_tool(&windows_game_pinned(version), Some(&installed));
        assert_eq!(tool("Proton 10.0"), CompatTool::Pinned("proton_10".into()));
        assert_eq!(
            tool("GE-Proton9-20"),
            CompatTool::Default("proton_10".into())
        );
        assert_eq!(tool(""), CompatTool::Default("proton_10".into()));
        assert_eq!(
            compat_tool(&windows_game_pinned("proton_9"), None),
            CompatTool::Default(proton::FALLBACK_TOOL.into())
        );
    }

    #[test]
    fn the_selection_is_remembered_in_system() {
        let dir = TempDir::new();
        dir.write("system/config.txt", b"");
        let cfg = Config::load(&dir.path().join("system/config.txt"));
        assert!(load_selection(&cfg).is_none());
        save_selection(&cfg, BTreeSet::from(["Celeste".to_string()])).unwrap();
        assert_eq!(
            games_in_steam(&cfg, &[]),
            BTreeSet::from(["Celeste".to_string()])
        );
    }

    #[test]
    fn removing_one_game_keeps_the_others() {
        let dir = TempDir::new();
        let config_dir = dir.mkdir("userdata/1/config");
        let shortcuts = Map(vec![
            ("0".into(), shortcut("A", "/g/a", "cartridge:1")),
            ("1".into(), shortcut("B", "/g/b", "cartridge:1")),
            ("2".into(), shortcut("Mine", "/g/a", "favorite")),
        ]);
        save_shortcuts(&shortcuts_path(&config_dir), shortcuts).unwrap();
        assert_eq!(
            remove_owned(&config_dir, "cartridge:1", |exe| exe == "/g/a").unwrap(),
            1
        );
        let after = load_shortcuts(&shortcuts_path(&config_dir)).unwrap();
        let names: Vec<&str> = after
            .values()
            .filter_map(as_shortcut)
            .map(appname)
            .collect();
        assert_eq!(names, ["B", "Mine"]);
    }
}
