//! First run: create the `.cartouche/` workspace dir, the feature folders and a
//! seeded `config.txt`. The UI has its own setup screen; this module holds the
//! shared logic and the terminal wizard.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use log::{error, info};

use super::app::data_dir_name;
use super::config::DEFAULT_CONFIG;

pub struct Feature {
    pub key: &'static str,
    pub label: &'static str,
    pub default: &'static str,
    pub description: &'static str,
}

pub const FEATURES: [Feature; 4] = [
    Feature {
        key: "FREEGAMES_PATH",
        label: "Games",
        default: "games",
        description: "DRM-free game library.",
    },
    Feature {
        key: "PATCHES_PATH",
        label: "Mods & Patches",
        default: "mods",
        description: "Patches and file replacements (BPS format).",
    },
    Feature {
        key: "SAVESCOPY_PATH",
        label: "Save Copies",
        default: "saves-copies",
        description: "Backup copies of save files.",
    },
    Feature {
        key: "SAVESLINK_PATH",
        label: "Save Links",
        default: "saves-links",
        description: "Symlinks to save folders, for Syncthing sync.",
    },
];

fn config_text(feature_paths: &[(String, String)]) -> String {
    if feature_paths.is_empty() {
        return DEFAULT_CONFIG.to_string();
    }
    let lines: String = feature_paths
        .iter()
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect();
    format!("# Paths configured by the setup wizard\n{lines}\n{DEFAULT_CONFIG}")
}

/// Create `parent/.cartouche/`, the chosen folders (relative to `parent` or
/// absolute) and `config.txt` (kept if it exists). Returns the config path.
pub fn initialize(parent: &Path, feature_paths: &[(String, String)]) -> std::io::Result<PathBuf> {
    let app_dir = parent.join(data_dir_name());
    std::fs::create_dir_all(&app_dir)?;
    for (_, value) in feature_paths {
        std::fs::create_dir_all(parent.join(value))?;
    }
    let config = app_dir.join("config.txt");
    if !config.exists() {
        std::fs::write(&config, config_text(feature_paths))?;
    }
    Ok(config)
}

/// `config.txt` inside `app_dir`, seeded from the defaults when missing.
pub fn config_in(app_dir: &Path) -> PathBuf {
    let config = app_dir.join("config.txt");
    if config.exists() {
        info!("Loaded config from {}", config.display());
        return config;
    }
    match std::fs::write(&config, DEFAULT_CONFIG) {
        Ok(()) => info!(
            "config.txt missing. Copied defaults to {}",
            config.display()
        ),
        Err(e) => error!("Failed to write {}: {e}", config.display()),
    }
    config
}

/// Where the workspace can go: next to the binary and/or the current directory.
pub fn locations(exe_dir: &Path, cwd: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut out = Vec::new();
    if std::fs::canonicalize(exe_dir).ok() != std::fs::canonicalize(cwd).ok() {
        out.push(("Next to binary", exe_dir.to_path_buf()));
    }
    out.push(("Current directory", cwd.to_path_buf()));
    out
}

fn prompt(input: &mut impl BufRead, question: &str) -> Option<String> {
    eprint!("{question}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_string()),
    }
}

/// Terminal wizard. Returns the new config path, or `None` if the user quit.
pub fn run_terminal_wizard(exe_dir: &Path, cwd: &Path) -> Option<PathBuf> {
    let name = data_dir_name();
    let locations = locations(exe_dir, cwd);
    let mut input = std::io::stdin().lock();

    eprintln!("\nInitialize {}\n{}", &name[1..], "=".repeat(40));
    eprintln!("\nWorkspace location (a {name}/ directory will be created here for settings and app files):\n");
    for (i, (label, path)) in locations.iter().enumerate() {
        eprintln!("  {}. {label}  ({})", i + 1, path.display());
    }
    eprintln!("  q. Exit\n");
    let parent = loop {
        let answer = prompt(&mut input, &format!("Choice [{}]: ", locations.len()))?;
        let answer = if answer.is_empty() {
            locations.len().to_string()
        } else {
            answer
        };
        if answer.eq_ignore_ascii_case("q") {
            return None;
        }
        match answer.parse::<usize>() {
            Ok(n) if (1..=locations.len()).contains(&n) => break locations[n - 1].1.clone(),
            _ => eprintln!("  Please enter 1-{} or q.", locations.len()),
        }
    };

    eprintln!(
        "\nFolders (relative to the workspace or absolute; Enter keeps the default, n skips):\n"
    );
    let mut chosen = Vec::new();
    for feature in &FEATURES {
        let answer = prompt(
            &mut input,
            &format!("  {} [{}]: ", feature.label, feature.default),
        )?;
        match answer.to_lowercase().as_str() {
            "" => chosen.push((feature.key.to_string(), feature.default.to_string())),
            "n" | "no" | "skip" => {}
            _ => chosen.push((feature.key.to_string(), answer)),
        }
    }
    initialize(&parent, &chosen)
        .map_err(|e| error!("Setup failed: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn initialize_creates_folders_and_prefixed_config() {
        let dir = TempDir::new();
        let paths = vec![("FREEGAMES_PATH".to_string(), "games".to_string())];
        let config = initialize(dir.path(), &paths).unwrap();
        assert!(dir.path().join("games").is_dir());
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.starts_with("# Paths configured by the setup wizard\nFREEGAMES_PATH=games\n"));
        assert!(text.contains("STEAMGRIDDB_API_KEY"));

        std::fs::write(&config, "MINE=1\n").unwrap();
        initialize(dir.path(), &paths).unwrap();
        assert_eq!(std::fs::read_to_string(config).unwrap(), "MINE=1\n");
    }
}
