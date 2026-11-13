//! Steam user folders (`userdata/<id>/config`), account names, the Steam root.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use log::{debug, warn};

use crate::core::config::Config;
use crate::core::paths;
use crate::core::vdf::{self, Value};

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
    let filtered = of_user(dirs, uid);
    if filtered.is_empty() {
        warn!("STEAM_USERID={uid} not found");
    }
    filtered
}

fn of_user(config_dirs: Vec<PathBuf>, uid: &str) -> Vec<PathBuf> {
    config_dirs
        .into_iter()
        .filter(|dir| user_id(dir) == uid)
        .collect()
}

/// Steam root from a `userdata/<id>/config` path.
pub(super) fn steam_root(config_dir: &Path) -> Option<PathBuf> {
    let root = config_dir.parent()?.parent()?.parent()?;
    root.join("config").is_dir().then(|| root.to_path_buf())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_placeholder_user_zero_is_not_an_account() {
        assert!(!is_account("0"));
        assert!(is_account("66213690"));
        assert!(is_account("10"));
    }

    #[test]
    fn steam_userid_picks_the_userdata_folder_not_any_path_with_that_number() {
        let dirs = vec![
            PathBuf::from("/home/1234/.steam/steam/userdata/99/config"),
            PathBuf::from("/home/1234/.steam/steam/userdata/1234/config"),
        ];
        assert_eq!(of_user(dirs.clone(), "1234"), [dirs[1].clone()]);
        assert!(of_user(dirs, "12").is_empty());
    }

    #[test]
    fn persona_names_are_keyed_by_account_id() {
        let names = persona_names(
            "\"users\"\n{\n\t\"76561197960265730\"\n\t{\n\t\t\"AccountName\"\t\"x\"\n\t\t\"PersonaName\"\t\"Jean\"\n\t}\n}\n",
        );
        assert_eq!(names.get("2").map(String::as_str), Some("Jean"));
        assert!(persona_names("not vdf {").is_empty());
    }
}
