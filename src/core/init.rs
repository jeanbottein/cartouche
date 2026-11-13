//! First run: create the `system/` workspace, the cartridge folders and a
//! seeded `config.txt`. The UI has its own setup screen; this module holds the
//! shared logic and the terminal wizard.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use log::{error, info};

use super::app::{
    is_translocated, is_workspace, launched_file, DISPLAY_NAME, FOLDERS, WORKSPACE_DIR,
    WORKSPACE_README,
};
use super::config::DEFAULT_CONFIG;
use super::paths;

/// Written to `system/README.txt`; its first word marks the folder as ours.
/// Shared with the release archives' template, so both always match.
const WORKSPACE_README_TEXT: &str = include_str!("../../packaging/cartridge/system/README.txt");

/// Make `parent` a cartridge: `system/` (README, `config.txt` kept if it
/// exists) and the fixed folders. Returns the config path. Refuses a
/// `system/` folder that isn't a •cartridge workspace.
pub fn initialize(parent: &Path) -> std::io::Result<PathBuf> {
    let system_dir = parent.join(WORKSPACE_DIR);
    if system_dir.exists() && !is_workspace(&system_dir) {
        return Err(std::io::Error::other(format!(
            "{} already exists and isn't a •cartridge workspace; pick a dedicated folder",
            system_dir.display()
        )));
    }
    std::fs::create_dir_all(&system_dir)?;
    let readme = system_dir.join(WORKSPACE_README);
    if !readme.exists() {
        paths::write_atomic(&readme, WORKSPACE_README_TEXT)?;
    }
    for folder in FOLDERS {
        std::fs::create_dir_all(parent.join(folder))?;
    }
    let config = system_dir.join("config.txt");
    if !config.exists() {
        paths::write_atomic(&config, DEFAULT_CONFIG)?;
    }
    Ok(config)
}

/// `config.txt` inside `system_dir`. When missing (e.g. a `system/` fresh from a
/// release archive), it is created with the standard folders, ready to use.
pub fn config_in(system_dir: &Path) -> PathBuf {
    let config = system_dir.join("config.txt");
    if config.exists() {
        info!("Loaded config from {}", config.display());
        return config;
    }
    let parent = system_dir.parent().unwrap_or(system_dir);
    match initialize(parent) {
        Ok(created) => info!(
            "config.txt missing: created {} with the standard folders",
            created.display()
        ),
        Err(e) => error!("Failed to create {}: {e}", config.display()),
    }
    config
}

/// Entries a cartridge holds besides the app and its `FOLDERS`.
const CARTRIDGE_FILES: [&str; 3] = [WORKSPACE_DIR, "README.txt", ".DS_Store"];

fn is_cartridge_entry(name: &str) -> bool {
    FOLDERS.contains(&name) || CARTRIDGE_FILES.contains(&name)
}

/// How many entries of `dir` are neither the app nor part of a cartridge.
fn unrelated_entries(names: &[String], app_file: Option<&str>) -> usize {
    names
        .iter()
        .filter(|n| Some(n.as_str()) != app_file && !is_cartridge_entry(n))
        .count()
}

/// Folders where AppImage managers (Gear Lever, AppImageLauncher) move files.
fn is_appimage_manager_dir(dir: &Path) -> bool {
    let name = dir.file_name().map(|n| n.to_string_lossy().to_lowercase());
    matches!(name.as_deref(), Some("appimages" | "applications"))
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".cartridge-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// What to tell the user before creating a cartridge in `dir`: the portable
/// idea, plus warnings when `dir` is a poor home for a cartridge.
pub fn folder_advice(dir: &Path) -> Vec<String> {
    let mut advice = vec![format!(
        "{} is portable: {WORKSPACE_DIR}/, games and saves live next to the app, \
         so the whole folder moves and backs up as one piece.",
        DISPLAY_NAME
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
            "macOS runs {DISPLAY_NAME} from a temporary read-only copy, away from its \
             folder. Quit, move the app into its folder with Finder (or run \
             `xattr -dr com.apple.quarantine` on that folder), then open it again."
        ));
    } else if dir.exists() && !is_writable(dir) {
        advice.push("This folder can't be written: move the app to a folder of its own.".into());
    }
    if is_appimage && app_here.is_some() && is_appimage_manager_dir(dir) {
        advice.push(
            "This AppImage looks moved by an AppImage manager, away from its folder: \
             launch the copy inside your •cartridge folder instead."
                .into(),
        );
    }
    let names = super::paths::sorted_entries(dir);
    if unrelated_entries(&names, app_here.as_deref()) > 2 {
        advice.push(format!(
            "This folder already holds other files: better give {DISPLAY_NAME} a folder \
             of its own, e.g. a new Cartridge/ folder here, and move the app into it."
        ));
    }
    advice
}

/// Where the cartridge can go: next to the binary and/or the current directory.
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

    eprintln!("\nInitialize {DISPLAY_NAME}\n{}", "=".repeat(40));
    for line in folder_advice(exe_dir) {
        eprintln!("\n{line}");
    }
    eprintln!("\nWhere to create the cartridge ({WORKSPACE_DIR}/, games/, mods/, saves-copies/, saves-links/):\n");
    for (i, (label, path)) in locations.iter().enumerate() {
        eprintln!("  {}. {label}  ({})", i + 1, path.display());
    }
    eprintln!("  q. Exit\n");
    let parent = loop {
        // Portable first: the default is next to the app.
        let answer = prompt(&mut input, "Choice [1]: ")?;
        let answer = if answer.is_empty() {
            "1".to_string()
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

    initialize(&parent)
        .map_err(|e| error!("Setup failed: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn initialize_creates_the_cartridge_and_keeps_a_config() {
        let dir = TempDir::new();
        let config = initialize(dir.path()).unwrap();
        for folder in FOLDERS {
            assert!(dir.path().join(folder).is_dir(), "{folder}");
        }
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("STEAMGRIDDB_API_KEY"));

        std::fs::write(&config, "MINE=1\n").unwrap();
        initialize(dir.path()).unwrap();
        assert_eq!(std::fs::read_to_string(config).unwrap(), "MINE=1\n");
    }

    #[test]
    fn initialize_writes_system_with_our_readme() {
        let dir = TempDir::new();
        let config = initialize(dir.path()).unwrap();
        assert_eq!(config, dir.path().join("system/config.txt"));
        assert!(is_workspace(&dir.path().join("system")));
    }

    #[test]
    fn initialize_refuses_an_unrelated_system_folder() {
        let dir = TempDir::new();
        dir.write("system/notes.txt", b"not ours");
        assert!(initialize(dir.path()).is_err());
        assert!(!dir.path().join("system/config.txt").exists());
    }

    #[test]
    fn a_missing_config_gets_the_standard_folders() {
        let dir = TempDir::new();
        dir.write("system/README.txt", "•cartridge workspace".as_bytes());
        let config = config_in(&dir.path().join("system"));
        assert!(config.is_file());
        assert!(dir.path().join("games").is_dir());
        assert!(dir.path().join("saves-links").is_dir());
    }

    #[test]
    fn only_unrelated_entries_count_as_clutter() {
        let names: Vec<String> = ["cartridge", "system", "games", "README.txt", "notes.pdf"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(unrelated_entries(&names, Some("cartridge")), 1);
        assert_eq!(unrelated_entries(&names, None), 2);
    }

    #[test]
    fn appimage_manager_folders_are_recognised() {
        assert!(is_appimage_manager_dir(Path::new("/home/deck/AppImages")));
        assert!(is_appimage_manager_dir(Path::new(
            "/home/deck/Applications"
        )));
        assert!(!is_appimage_manager_dir(Path::new("/home/deck/•cartridge")));
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

    #[test]
    fn the_archive_template_marks_system_as_a_workspace() {
        let dir = TempDir::new();
        dir.write("system/README.txt", WORKSPACE_README_TEXT.as_bytes());
        assert!(is_workspace(&dir.path().join("system")));
    }
}
