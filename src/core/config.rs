//! `system/config.txt`: `KEY=value` lines, `#` comments (whole-line and
//! inline). The cartridge's folders are fixed (`games/`, `mods/`, ... next to
//! `system/`); only `BACKUP_*` sources are paths, and may be relative to the
//! cartridge folder. Raw text values are kept for the Settings editor, which
//! rewrites the file preserving comments.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::app::{GAMES_DIR, MODS_DIR, SAVES_COPIES_DIR, SAVES_LINKS_DIR};
use super::paths;

pub const DEFAULT_CONFIG: &str = include_str!("../../assets/config-default.txt");

/// `BACKUP_<name>=<path>`: an extra folder (outside the cartridge) to back up.
pub fn is_path_key(key: &str) -> bool {
    key.starts_with("BACKUP_")
}

#[derive(Clone, Debug)]
pub struct Config {
    /// The `config.txt` file.
    pub path: PathBuf,
    raw: BTreeMap<String, String>,
    values: BTreeMap<String, String>,
}

impl Config {
    pub fn load(path: &Path) -> Config {
        let raw = match std::fs::read_to_string(path) {
            Ok(text) => parse(&text),
            Err(_) => {
                log::warn!(
                    "Config file {} not found; continuing with empty configuration",
                    path.display()
                );
                BTreeMap::new()
            }
        };
        Config::from_raw(path.to_path_buf(), raw)
    }

    pub fn from_raw(path: PathBuf, raw: BTreeMap<String, String>) -> Config {
        let cartridge = cartridge_dir_of(&path);
        let values = raw
            .iter()
            .map(|(k, v)| (k.clone(), resolve_value(&cartridge, k, v)))
            .collect();
        Config { path, raw, values }
    }

    /// Resolved value, if set and non-empty.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Value as typed in `config.txt` (unresolved).
    pub fn raw(&self) -> &BTreeMap<String, String> {
        &self.raw
    }

    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.values
    }

    /// Opt-in flag: true only when set to `true` (any case).
    pub fn is_true(&self, key: &str) -> bool {
        self.get(key)
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    /// Opt-out flag: true unless set to `false` (any case).
    pub fn is_not_false(&self, key: &str) -> bool {
        !self
            .get(key)
            .is_some_and(|v| v.eq_ignore_ascii_case("false"))
    }

    /// The workspace, `system/`: config, caches, remembered choices.
    pub fn system_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    /// The cartridge folder holding `system/`, `games/`, `mods/`...
    pub fn cartridge_dir(&self) -> PathBuf {
        cartridge_dir_of(&self.path)
    }

    /// The cartridge's label: its folder's name.
    pub fn label(&self) -> String {
        super::app::label(&self.cartridge_dir())
    }

    /// The cartridge's stable id (`system/cartridge-id`, created on first use).
    pub fn cartridge_id(&self) -> String {
        super::app::cartridge_id(&self.system_dir())
    }

    /// `games/`, when it exists.
    pub fn games_dir(&self) -> Option<PathBuf> {
        Some(self.cartridge_dir().join(GAMES_DIR)).filter(|p| p.is_dir())
    }

    pub fn mods_dir(&self) -> PathBuf {
        self.cartridge_dir().join(MODS_DIR)
    }

    pub fn saves_copies_dir(&self) -> PathBuf {
        self.cartridge_dir().join(SAVES_COPIES_DIR)
    }

    pub fn saves_links_dir(&self) -> PathBuf {
        self.cartridge_dir().join(SAVES_LINKS_DIR)
    }
}

fn cartridge_dir_of(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn resolve_value(cartridge: &Path, key: &str, value: &str) -> String {
    // `~` and `$VAR` are expanded by the saver: only a path that stays
    // relative once expanded is relative to the cartridge.
    if !is_path_key(key) || paths::expand(value).is_absolute() {
        return value.to_string();
    }
    paths::lossy(&paths::resolve(&cartridge.join(value)))
}

/// Parse `KEY=value` lines, ignoring comments and empty values.
pub fn parse(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| {
            (
                k.trim().to_string(),
                v.split('#').next().unwrap_or("").trim().to_string(),
            )
        })
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
        .collect()
}

/// Key of an active `KEY=value` line.
fn active_key(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    line.split_once('=').map(|(k, _)| k.trim())
}

/// Key of a commented-out default such as `#KEY=value`.
fn commented_key(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix('#')?.trim_start();
    let (key, _) = rest.split_once('=')?;
    let key = key.trim();
    let is_key = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    is_key.then_some(key)
}

/// Rewrite `text` with `updates` applied: active lines are replaced in place,
/// otherwise the first commented-out default (`#KEY=...`) is uncommented,
/// otherwise the key is appended. Everything else is left untouched.
pub fn apply_updates(text: &str, updates: &BTreeMap<String, String>) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for (key, value) in updates {
        let new_line = format!("{key}={value}");
        let slot = lines
            .iter()
            .position(|l| active_key(l) == Some(key))
            .or_else(|| lines.iter().position(|l| commented_key(l) == Some(key)));
        match slot {
            Some(i) => lines[i] = new_line,
            None => lines.push(new_line),
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub fn update_file(path: &Path, updates: &BTreeMap<String, String>) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        // Unreadable (or not UTF-8): rewriting it would drop every other line.
        Err(e) => return Err(e),
    };
    paths::write_atomic(path, apply_updates(&text, updates))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strips_comments_and_empty_values() {
        let map = parse("# header\nA=1\n B = two # note\nEMPTY=\n#C=3\nnoequals\n");
        assert_eq!(map.len(), 2);
        assert_eq!(map["A"], "1");
        assert_eq!(map["B"], "two");
    }

    #[test]
    fn backup_sources_resolve_against_the_cartridge() {
        let raw = parse("BACKUP_x=emu/saves\nBACKUP_y=/abs\n");
        let cfg = Config::from_raw(PathBuf::from("/ws/system/config.txt"), raw);
        // Resolved like the code does, so `/ws` reads `D:\ws` on Windows.
        let resolved = |p: &str| paths::lossy(&paths::resolve(Path::new(p)));
        assert_eq!(
            cfg.get("BACKUP_x"),
            Some(resolved("/ws/emu/saves").as_str())
        );
        assert_eq!(cfg.get("BACKUP_y"), Some(resolved("/abs").as_str()));
        assert_eq!(cfg.raw()["BACKUP_x"], "emu/saves");
        assert_eq!(cfg.mods_dir(), Path::new("/ws/mods"));
    }

    #[test]
    fn backup_sources_under_home_are_left_for_the_saver_to_expand() {
        let raw = parse("BACKUP_x=~/Emulation/saves\nBACKUP_y=$HOME/saves\n");
        let cfg = Config::from_raw(PathBuf::from("/ws/system/config.txt"), raw);
        assert_eq!(cfg.get("BACKUP_x"), Some("~/Emulation/saves"));
        // Windows has no HOME (USERPROFILE instead).
        if cfg!(unix) {
            assert_eq!(cfg.get("BACKUP_y"), Some("$HOME/saves"));
        }
    }

    #[test]
    fn flags_have_opt_in_and_opt_out_defaults() {
        let raw = parse("SHARE_ENABLED=TRUE\nMANIFEST_EXPORT=false\n");
        let cfg = Config::from_raw(PathBuf::from("/ws/system/config.txt"), raw);
        assert!(cfg.is_true("SHARE_ENABLED"));
        assert!(!cfg.is_true("STEAM_ADD_SELF"));
        assert!(!cfg.is_not_false("MANIFEST_EXPORT"));
        assert!(cfg.is_not_false("MUSIC_ENABLED"));
    }

    #[test]
    fn updates_replace_uncomment_or_append() {
        let text = "# Games\nFREEGAMES_PATH=games\n# Proton\n#PROTON_VERSION=proton_experimental\n";
        let updates = BTreeMap::from([
            ("FREEGAMES_PATH".to_string(), "/g".to_string()),
            ("PROTON_VERSION".to_string(), "proton_9".to_string()),
            ("NEW_KEY".to_string(), "v".to_string()),
        ]);
        assert_eq!(
            apply_updates(text, &updates),
            "# Games\nFREEGAMES_PATH=/g\n# Proton\nPROTON_VERSION=proton_9\nNEW_KEY=v\n"
        );
    }
}
