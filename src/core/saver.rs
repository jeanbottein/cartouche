//! Pipeline step 9: back up (or restore) save folders into `SAVESCOPY_PATH`,
//! plus `BACKUP_<name>` folders, the config file, and an optional tree of
//! symlinks to the live save folders in `SAVESLINK_PATH` (for Syncthing & co).
//!
//! One save path  → `SAVESCOPY_PATH/<Title>/`
//! 2+ save paths  → `SAVESCOPY_PATH/<Title>/<folder name>/`

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use log::{error, info, warn};

use super::config::Config;
use super::models::Game;
use super::paths;

const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Strategy {
    Backup,
    Sync,
    Restore,
}

fn strategy(raw: Option<&str>) -> Strategy {
    let value = raw.unwrap_or("backup").trim().to_lowercase();
    match value.as_str() {
        "backup" => Strategy::Backup,
        "sync" => Strategy::Sync,
        "restore" => Strategy::Restore,
        _ => {
            warn!("Invalid SAVESCOPY_STRATEGY '{value}', falling back to 'backup'");
            Strategy::Backup
        }
    }
}

fn base_path(raw: &str) -> PathBuf {
    paths::abspath(&paths::expand(raw))
}

/// A folder name safe on every OS.
pub fn sanitize_title(title: &str) -> String {
    let replaced: String = title
        .trim()
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 {
                '_'
            } else {
                c
            }
        })
        .collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join("_");
    let trimmed = collapsed.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return "game".into();
    }
    let name = if WINDOWS_RESERVED.contains(&trimmed.to_uppercase().as_str()) {
        format!("{trimmed}_game")
    } else {
        trimmed.to_string()
    };
    let short: String = name.chars().take(100).collect();
    let short = short.trim_end_matches(['.', ' ']);
    if short.is_empty() {
        "game".into()
    } else {
        short.to_string()
    }
}

/// Every file under `root` by relative path (folder symlinks not followed).
fn file_map(root: &Path) -> BTreeMap<PathBuf, PathBuf> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, PathBuf>) {
        for path in paths::sorted_entries(dir).into_iter().map(|n| dir.join(n)) {
            if path.is_dir() {
                if !paths::is_symlink(&path) {
                    walk(&path, root, out);
                }
            } else {
                out.insert(path.strip_prefix(root).unwrap_or(&path).to_path_buf(), path);
            }
        }
    }
    let mut out = BTreeMap::new();
    if root.is_dir() {
        walk(root, root, &mut out);
    }
    out
}

/// Same size and not older than the source.
fn is_current(src: &Path, dst: &Path) -> bool {
    let (Ok(s), Ok(d)) = (std::fs::metadata(src), std::fs::metadata(dst)) else {
        return false;
    };
    let (Ok(sm), Ok(dm)) = (s.modified(), d.modified()) else {
        return false;
    };
    s.len() == d.len() && sm <= dm
}

fn copy_file(src: &Path, dst: &Path) {
    if let Some(dir) = dst.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if is_current(src, dst) {
        return;
    }
    if let Err(e) = paths::copy_with_mtime(src, dst) {
        error!("Error copying {} -> {}: {e}", src.display(), dst.display());
    }
}

/// Mirror `src` into `dst`, deleting backup files whose source is gone.
fn backup_dir(title: &str, src: &Path, dst: &Path) {
    if !src.is_dir() {
        info!("  {title}: source {} not found, skipping", src.display());
        return;
    }
    let src_files = file_map(src);
    if src_files.is_empty() {
        info!("  {title}: source {} is empty, skipping", src.display());
        return;
    }
    let dst_files = file_map(dst);
    let _ = std::fs::create_dir_all(dst);
    info!("  Backing up: {title}");
    src_files
        .iter()
        .for_each(|(rel, path)| copy_file(path, &dst.join(rel)));
    for (_, obsolete) in dst_files
        .iter()
        .filter(|(rel, _)| !src_files.contains_key(*rel))
    {
        if let Err(e) = std::fs::remove_file(obsolete) {
            error!(
                "  Error removing obsolete backup file {}: {e}",
                obsolete.display()
            );
        }
    }
}

fn restore_dir(title: &str, src: &Path, dst: &Path) {
    if !dst.is_dir() {
        info!(
            "  {title}: backup {} not found, skipping restore",
            dst.display()
        );
        return;
    }
    let _ = std::fs::create_dir_all(src);
    warn!("  Restoring: {title} (overwrites existing files)");
    file_map(dst)
        .iter()
        .for_each(|(rel, path)| copy_file(path, &src.join(rel)));
}

fn sync_directory(title: &str, src: &Path, dst: &Path, strategy: Strategy) {
    match strategy {
        Strategy::Backup => backup_dir(title, src, dst),
        Strategy::Sync => {
            info!("  {title}: 'sync' strategy behaves like 'backup' (use SAVESLINK_PATH + Syncthing for bidirectional sync)");
            backup_dir(title, src, dst);
        }
        Strategy::Restore => restore_dir(title, src, dst),
    }
}

// ── Symlink tree ─────────────────────────────────────────────────────────

fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    return if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
}

fn create_symlink(link: &Path, target: &Path) {
    if paths::is_symlink(link) {
        if std::fs::read_link(link).is_ok_and(|t| t == target) {
            return;
        }
        let _ = std::fs::remove_file(link);
    } else if link.exists() {
        warn!("  {} exists and is not a symlink, skipping", link.display());
        return;
    }
    if let Some(dir) = link.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = symlink(target, link) {
        error!(
            "  Error creating symlink {} -> {}: {e}",
            link.display(),
            target.display()
        );
    }
}

fn remove_stale_symlinks(root: &Path, active: &HashSet<String>) {
    for name in paths::sorted_entries(root)
        .into_iter()
        .filter(|n| !active.contains(n))
    {
        let path = root.join(&name);
        if !paths::is_symlink(&path) {
            continue;
        }
        info!("  Removing stale symlink: {name}");
        if let Err(e) = std::fs::remove_file(&path) {
            error!("  Error removing stale symlink {}: {e}", path.display());
        }
    }
}

/// A symlink to create: `root/<top>` or `root/<top>/<sub>` → `source`.
struct LinkEntry {
    top: String,
    sub: Option<String>,
    source: PathBuf,
}

fn build_symlink_tree(entries: &[LinkEntry], root: &Path) {
    let _ = std::fs::create_dir_all(root);
    let mut created = HashSet::new();
    for entry in entries.iter().filter(|e| e.source.is_dir()) {
        let link = match &entry.sub {
            None => root.join(&entry.top),
            Some(sub) => root.join(&entry.top).join(sub),
        };
        create_symlink(&link, &entry.source);
        created.insert(entry.top.clone());
    }
    remove_stale_symlinks(root, &created);
    if !created.is_empty() {
        info!(
            "  Symlink tree updated at {} ({} entries)",
            root.display(),
            created.len()
        );
    }
}

// ── Run ──────────────────────────────────────────────────────────────────

/// Copy `config.txt` into the backup root as `<machine>_config.txt`.
fn backup_config(cfg: &Config, saves_root: &Path) -> Option<String> {
    let machine = cfg
        .get("MACHINE_NAME")
        .map(str::trim)
        .filter(|m| !m.is_empty())?;
    if !cfg.path.is_file() {
        return None;
    }
    let name = format!("{}_config.txt", sanitize_title(machine));
    copy_file(&cfg.path, &saves_root.join(&name));
    info!("  Config backed up as {name}");
    Some(name)
}

fn process_game_saves(games: &[Game], saves_root: &Path, strategy: Strategy) -> Vec<LinkEntry> {
    let mut links = Vec::new();
    let mut processed = 0;
    for game in games.iter().filter(|g| !g.resolved_save_paths.is_empty()) {
        let title = sanitize_title(&game.title);
        let multi = game.resolved_save_paths.len() > 1;
        for save in &game.resolved_save_paths {
            let sub = sanitize_title(&save.file_name().unwrap_or_default().to_string_lossy());
            let (dst, label) = if multi {
                (
                    saves_root.join(&title).join(&sub),
                    format!("{}/{sub}", game.title),
                )
            } else {
                (saves_root.join(&title), game.title.clone())
            };
            sync_directory(&label, save, &dst, strategy);
            links.push(LinkEntry {
                top: title.clone(),
                sub: multi.then_some(sub),
                source: save.clone(),
            });
        }
        processed += 1;
    }
    if processed > 0 {
        info!("Processed saves for {processed} game(s)");
    }
    links
}

fn process_custom_backups(cfg: &Config, saves_root: &Path, strategy: Strategy) -> Vec<LinkEntry> {
    let custom: Vec<(&str, &str)> = cfg
        .values()
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("BACKUP_")
                .filter(|n| !n.is_empty())
                .map(|n| (n, v.as_str()))
        })
        .collect();
    if custom.is_empty() {
        return Vec::new();
    }
    info!("Processing {} custom directory backup(s)", custom.len());
    custom
        .into_iter()
        .map(|(name, source)| {
            let src = base_path(source);
            let top = sanitize_title(name);
            sync_directory(name, &src, &saves_root.join(&top), strategy);
            LinkEntry {
                top,
                sub: None,
                source: src,
            }
        })
        .collect()
}

pub fn run(games: &[Game], cfg: &Config) {
    let Some(raw_root) = cfg.get("SAVESCOPY_PATH") else {
        info!("SAVESCOPY_PATH not configured, skipping save backup");
        return;
    };
    let saves_root = base_path(raw_root);
    if let Err(e) = std::fs::create_dir_all(&saves_root) {
        error!("Cannot create {}: {e}", saves_root.display());
        return;
    }
    let strategy = strategy(cfg.get("SAVESCOPY_STRATEGY"));
    info!(
        "Running saver with strategy='{}'",
        format!("{strategy:?}").to_lowercase()
    );

    let config_backup = backup_config(cfg, &saves_root);
    let mut links = process_game_saves(games, &saves_root, strategy);
    links.extend(process_custom_backups(cfg, &saves_root, strategy));

    let Some(link_root) = cfg.get("SAVESLINK_PATH").map(base_path) else {
        return;
    };
    if links.is_empty() {
        return;
    }
    info!("Building symlink tree at {}", link_root.display());
    build_symlink_tree(&links, &link_root);
    if let Some(name) = config_backup {
        create_symlink(&link_root.join(name), &cfg.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn sanitize_title_is_filesystem_safe() {
        assert_eq!(
            sanitize_title("Hollow Knight: Silksong?"),
            "Hollow_Knight__Silksong_"
        );
        assert_eq!(sanitize_title("  CON "), "CON_game");
        assert_eq!(sanitize_title("..."), "game");
        assert_eq!(sanitize_title("a.  "), "a");
    }

    #[test]
    fn backup_mirrors_and_prunes_then_restore_copies_back() {
        let dir = TempDir::new();
        let src = dir.mkdir("live");
        dir.write("live/slot1.sav", b"one");
        dir.write("live/sub/slot2.sav", b"two");
        let dst = dir.path().join("backup");
        dir.write("backup/obsolete.sav", b"old");

        backup_dir("Game", &src, &dst);
        assert_eq!(std::fs::read(dst.join("sub/slot2.sav")).unwrap(), b"two");
        assert!(!dst.join("obsolete.sav").exists());

        std::fs::remove_file(src.join("slot1.sav")).unwrap();
        restore_dir("Game", &src, &dst);
        assert_eq!(std::fs::read(src.join("slot1.sav")).unwrap(), b"one");
    }

    #[cfg(unix)]
    #[test]
    fn run_backs_up_games_and_builds_symlinks() {
        let dir = TempDir::new();
        let saves = dir.mkdir("live/saves");
        dir.write("live/saves/a.sav", b"a");
        let mut game = Game {
            title: "My Game".into(),
            ..Default::default()
        };
        game.resolved_save_paths = vec![saves.clone()];
        let raw = crate::core::config::parse(&format!(
            "SAVESCOPY_PATH={}\nSAVESLINK_PATH={}\n",
            dir.path().join("copies").display(),
            dir.path().join("links").display()
        ));
        dir.mkdir("links");
        std::os::unix::fs::symlink(&saves, dir.path().join("links/Stale")).unwrap();
        let cfg = Config::from_raw(dir.path().join(".cartridge/config.txt"), raw);

        run(&[game], &cfg);
        assert_eq!(
            std::fs::read(dir.path().join("copies/My_Game/a.sav")).unwrap(),
            b"a"
        );
        assert_eq!(
            std::fs::read_link(dir.path().join("links/My_Game")).unwrap(),
            saves
        );
        assert!(!dir.path().join("links/Stale").exists());
    }
}
