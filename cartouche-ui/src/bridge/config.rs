//! Locate and parse `.cartouche/config.txt` (BRIDGE-4, read side).
//!
//! Parsing mirrors `load_config_map` in `cartouche.py`: `KEY=value` lines, `#`
//! comments (whole-line and inline) stripped, blanks ignored. We find the
//! `.cartouche/` dir by walking up from the working dir, then from the executable
//! location, so it works whether launched from the repo or by path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Config {
    pub map: HashMap<String, String>,
    /// The `.cartouche/` directory the config was loaded from.
    pub cartouche_dir: PathBuf,
}

impl Config {
    /// Search the working dir and executable dir (and their ancestors) for a
    /// `.cartouche/config.txt`.
    pub fn load() -> Option<Config> {
        let mut starts: Vec<PathBuf> = Vec::new();
        if let Some(arg) = std::env::args().nth(1) {
            let p = PathBuf::from(arg);
            if p.is_dir() {
                starts.push(p);
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            starts.push(cwd);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                starts.push(dir.to_path_buf());
            }
        }

        for start in starts {
            if let Some(cfg) = find_upwards(&start) {
                let map = parse(&cfg);
                let cartouche_dir = cfg.parent()?.to_path_buf();
                return Some(Config { map, cartouche_dir });
            }
        }
        None
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|s| s.as_str())
    }

    /// Resolve `FREEGAMES_PATH`, relative values being relative to the workspace
    /// root (the `.cartouche/` parent), matching the Python path resolution.
    pub fn games_dir(&self) -> Option<PathBuf> {
        let raw = self.get("FREEGAMES_PATH")?;
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            Some(p)
        } else {
            self.cartouche_dir.parent().map(|base| base.join(p))
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.cartouche_dir.join("config.txt")
    }
}

fn find_upwards(start: &Path) -> Option<PathBuf> {
    let mut dir = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    loop {
        let cfg = dir.join(".cartouche").join("config.txt");
        if cfg.is_file() {
            return Some(cfg);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn parse(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return map;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap();
        let key = key.trim();
        // Strip inline comments.
        let value = value.split('#').next().unwrap_or("").trim();
        if !key.is_empty() && !value.is_empty() {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}
