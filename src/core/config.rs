//! `.cartouche/config.txt`: `KEY=value` lines, `#` comments (whole-line and
//! inline). Path-like values may be relative to the workspace root (the
//! `.cartouche/` parent) and are resolved on load; the raw text values are kept
//! for the Settings editor, which rewrites the file preserving comments.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::paths;

pub const DEFAULT_CONFIG: &str = include_str!("../../assets/config-default.txt");

/// Keys whose values may be relative to the workspace root (plus `BACKUP_*`).
const RELATIVE_PATH_KEYS: [&str; 5] = [
    "FREEGAMES_PATH",
    "PATCHES_PATH",
    "SAVESCOPY_PATH",
    "SAVESLINK_PATH",
    "MANIFEST_PATH",
];

pub fn is_path_key(key: &str) -> bool {
    RELATIVE_PATH_KEYS.contains(&key) || key.starts_with("BACKUP_")
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
        let workspace = workspace_of(&path);
        let values = raw
            .iter()
            .map(|(k, v)| (k.clone(), resolve_value(&workspace, k, v)))
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

    pub fn path_of(&self, key: &str) -> Option<PathBuf> {
        self.get(key).map(PathBuf::from)
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

    /// The `.cartouche/` directory.
    pub fn app_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    /// The workspace root: the `.cartouche/` parent.
    pub fn workspace(&self) -> PathBuf {
        workspace_of(&self.path)
    }

    /// `FREEGAMES_PATH`, when it points at an existing directory.
    pub fn games_dir(&self) -> Option<PathBuf> {
        self.path_of("FREEGAMES_PATH").filter(|p| p.is_dir())
    }
}

fn workspace_of(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn resolve_value(workspace: &Path, key: &str, value: &str) -> String {
    if !is_path_key(key) || Path::new(value).is_absolute() {
        return value.to_string();
    }
    paths::lossy(&paths::resolve(&workspace.join(value)))
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
    std::fs::write(path, apply_updates(&text, updates))
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
    fn relative_paths_resolve_against_workspace() {
        let raw = parse("FREEGAMES_PATH=games\nBACKUP_x=/abs\nMACHINE_NAME=deck\n");
        let cfg = Config::from_raw(PathBuf::from("/ws/.cartouche/config.txt"), raw);
        // Resolved like the code does, so `/ws` reads `D:\ws` on Windows.
        let resolved = |p: &str| paths::lossy(&paths::resolve(Path::new(p)));
        assert_eq!(
            cfg.get("FREEGAMES_PATH"),
            Some(resolved("/ws/games").as_str())
        );
        assert_eq!(cfg.get("BACKUP_x"), Some(resolved("/abs").as_str()));
        assert_eq!(cfg.get("MACHINE_NAME"), Some("deck"));
        assert_eq!(cfg.raw()["FREEGAMES_PATH"], "games");
    }

    #[test]
    fn flags_have_opt_in_and_opt_out_defaults() {
        let raw = parse("STEAM_EXPOSE=TRUE\nPERSIST_DATA=false\n");
        let cfg = Config::from_raw(PathBuf::from("/ws/.cartouche/config.txt"), raw);
        assert!(cfg.is_true("STEAM_EXPOSE"));
        assert!(!cfg.is_true("STEAM_ADD_SELF"));
        assert!(!cfg.is_not_false("PERSIST_DATA"));
        assert!(cfg.is_not_false("MANIFEST_EXPORT"));
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
