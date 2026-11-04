//! First run: create the `.cartouche/` workspace dir, the feature folders and a
//! seeded `config.txt`. The UI has its own setup screen; this module holds the
//! shared logic and the terminal wizard.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use log::{error, info};

use super::app::{
    app_name, is_translocated, is_workspace, launched_file, WORKSPACE_DIR, WORKSPACE_README,
};
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

/// Written to `system/README.txt`; its first word marks the folder as ours.
const WORKSPACE_README_TEXT: &str = "\
Cartouche workspace: settings (config.txt), caches and thumbnails.

Cartouche is portable: this folder, the app next to it and your games folder
travel together. Move, copy or back up the whole parent folder as one piece.
Edit config.txt by hand or in the app's SETTINGS tab.
";

/// Create `parent/system/`, the chosen folders (relative to `parent` or
/// absolute) and `config.txt` (kept if it exists). Returns the config path.
/// Refuses a `system/` folder that isn't a Cartouche workspace.
pub fn initialize(parent: &Path, feature_paths: &[(String, String)]) -> std::io::Result<PathBuf> {
    let app_dir = parent.join(WORKSPACE_DIR);
    if app_dir.exists() && !is_workspace(&app_dir) {
        return Err(std::io::Error::other(format!(
            "{} already exists and isn't a Cartouche workspace; pick a dedicated folder",
            app_dir.display()
        )));
    }
    std::fs::create_dir_all(&app_dir)?;
    let readme = app_dir.join(WORKSPACE_README);
    if !readme.exists() {
        std::fs::write(&readme, WORKSPACE_README_TEXT)?;
    }
    for (_, value) in feature_paths {
        std::fs::create_dir_all(parent.join(value))?;
    }
    let config = app_dir.join("config.txt");
    if !config.exists() {
        std::fs::write(&config, config_text(feature_paths))?;
    }
    Ok(config)
}

/// The setup wizard's defaults: every feature on, in its standard folder.
pub fn default_paths() -> Vec<(String, String)> {
    FEATURES
        .iter()
        .map(|f| (f.key.to_string(), f.default.to_string()))
        .collect()
}

/// `config.txt` inside `app_dir`. When missing (e.g. a `system/` fresh from a
/// release archive), it is created with the standard folders, ready to use.
pub fn config_in(app_dir: &Path) -> PathBuf {
    let config = app_dir.join("config.txt");
    if config.exists() {
        info!("Loaded config from {}", config.display());
        return config;
    }
    let parent = app_dir.parent().unwrap_or(app_dir);
    match initialize(parent, &default_paths()) {
        Ok(created) => info!(
            "config.txt missing: created {} with the standard folders",
            created.display()
        ),
        Err(e) => error!("Failed to create {}: {e}", config.display()),
    }
    config
}

/// Folder names a cartridge holds besides the app itself.
const CARTRIDGE_NAMES: [&str; 7] = [
    WORKSPACE_DIR,
    "games",
    "mods",
    "saves-copies",
    "saves-links",
    "README.txt",
    ".DS_Store",
];

/// How many entries of `dir` are neither the app nor part of a cartridge.
fn unrelated_entries(names: &[String], app_file: Option<&str>) -> usize {
    names
        .iter()
        .filter(|n| Some(n.as_str()) != app_file && !CARTRIDGE_NAMES.contains(&n.as_str()))
        .count()
}

/// Folders where AppImage managers (Gear Lever, AppImageLauncher) move files.
fn is_appimage_manager_dir(dir: &Path) -> bool {
    let name = dir.file_name().map(|n| n.to_string_lossy().to_lowercase());
    matches!(name.as_deref(), Some("appimages" | "applications"))
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".cartouche-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The app name for sentences: `cartouche` -> `Cartouche`.
fn display_name() -> String {
    let name = app_name();
    let mut chars = name.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// What to tell the user before creating a workspace in `dir`: the portable
/// idea, plus warnings when `dir` is a poor home for a cartridge.
pub fn folder_advice(dir: &Path) -> Vec<String> {
    let mut advice = vec![format!(
        "{} is portable: {WORKSPACE_DIR}/, games and saves live next to the app, \
         so the whole folder moves and backs up as one piece.",
        display_name()
    )];
    let launched = launched_file();
    let app_here = launched
        .as_deref()
        .filter(|f| f.parent() == Some(dir))
        .and_then(|f| f.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    let is_appimage = std::env::var_os("APPIMAGE").is_some_and(|a| !a.is_empty());
    if launched.as_deref().is_some_and(is_translocated) {
        advice.push(format!(
            "macOS runs {name} from a temporary read-only copy, away from its folder. \
             Quit, move {name}.app into its folder with Finder (or run \
             `xattr -dr com.apple.quarantine` on that folder), then open it again.",
            name = display_name()
        ));
    } else if dir.exists() && !is_writable(dir) {
        advice.push("This folder can't be written: move the app to a folder of its own.".into());
    }
    if is_appimage && app_here.is_some() && is_appimage_manager_dir(dir) {
        advice.push(
            "This AppImage looks moved by an AppImage manager, away from its folder: \
             launch the copy inside your Cartouche folder instead."
                .into(),
        );
    }
    let names = super::paths::sorted_entries(dir);
    if unrelated_entries(&names, app_here.as_deref()) > 2 {
        advice.push(format!(
            "This folder already holds other files: better give {name} a folder of its \
             own, e.g. a new {name}/ folder here, and move the app into it.",
            name = display_name()
        ));
    }
    advice
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
    let locations = locations(exe_dir, cwd);
    let mut input = std::io::stdin().lock();

    eprintln!("\nInitialize {}\n{}", app_name(), "=".repeat(40));
    for line in folder_advice(exe_dir) {
        eprintln!("\n{line}");
    }
    eprintln!("\nWorkspace location (a {WORKSPACE_DIR}/ folder will be created here for settings and app files):\n");
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

    #[test]
    fn initialize_writes_system_with_our_readme() {
        let dir = TempDir::new();
        let config = initialize(dir.path(), &[]).unwrap();
        assert_eq!(config, dir.path().join("system/config.txt"));
        assert!(is_workspace(&dir.path().join("system")));
    }

    #[test]
    fn initialize_refuses_an_unrelated_system_folder() {
        let dir = TempDir::new();
        dir.write("system/notes.txt", b"not ours");
        assert!(initialize(dir.path(), &[]).is_err());
        assert!(!dir.path().join("system/config.txt").exists());
    }

    #[test]
    fn a_missing_config_gets_the_standard_folders() {
        let dir = TempDir::new();
        dir.write("system/README.txt", b"Cartouche workspace");
        let config = config_in(&dir.path().join("system"));
        let text = std::fs::read_to_string(config).unwrap();
        assert!(text.contains("FREEGAMES_PATH=games\n"));
        assert!(dir.path().join("games").is_dir());
        assert!(dir.path().join("saves-links").is_dir());
    }

    #[test]
    fn only_unrelated_entries_count_as_clutter() {
        let names: Vec<String> = ["cartouche", "system", "games", "README.txt", "notes.pdf"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(unrelated_entries(&names, Some("cartouche")), 1);
        assert_eq!(unrelated_entries(&names, None), 2);
    }

    #[test]
    fn appimage_manager_folders_are_recognised() {
        assert!(is_appimage_manager_dir(Path::new("/home/deck/AppImages")));
        assert!(is_appimage_manager_dir(Path::new(
            "/home/deck/Applications"
        )));
        assert!(!is_appimage_manager_dir(Path::new("/home/deck/Cartouche")));
    }

    #[test]
    fn advice_warns_about_a_cluttered_folder() {
        let dir = TempDir::new();
        ["a.pdf", "b.zip", "c.txt"].iter().for_each(|n| {
            dir.write(n, b"");
        });
        let advice = folder_advice(dir.path());
        assert!(advice[0].contains("portable"));
        assert!(advice.iter().any(|a| a.contains("other files")));
        assert_eq!(folder_advice(&dir.path().join("empty-new")).len(), 1);
    }
}
