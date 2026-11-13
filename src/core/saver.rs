//! Pipeline step 9: back up (or restore) save folders into `saves-copies/`,
//! plus `BACKUP_<name>` folders, the config file, and an optional tree of
//! symlinks to the live save folders in `saves-links/` (for Syncthing & co).
//!
//! One save path  → `saves-copies/<Title>/`
//! 2+ save paths  → `saves-copies/<Title>/<folder name>/`

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
            info!("  {title}: 'sync' strategy behaves like 'backup' (use saves-links/ + Syncthing for bidirectional sync)");
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

/// Remove a symlink (Windows removes folder links with `remove_dir`).
fn remove_link(link: &Path) -> std::io::Result<()> {
    std::fs::remove_file(link).or_else(|_| std::fs::remove_dir(link))
}

/// Remove a folder of our links (left when a game had several save paths).
/// A folder holding anything but symlinks is the user's: it stays.
fn remove_link_folder(dir: &Path) -> bool {
    let entries: Vec<PathBuf> = paths::sorted_entries(dir)
        .into_iter()
        .map(|n| dir.join(n))
        .collect();
    if !entries.iter().all(|p| paths::is_symlink(p)) {
        return false;
    }
    entries.iter().for_each(|link| {
        let _ = remove_link(link);
    });
    std::fs::remove_dir(dir).is_ok()
}

fn create_symlink(link: &Path, target: &Path) {
    if paths::is_symlink(link) {
        if std::fs::read_link(link).is_ok_and(|t| t == target) {
            return;
        }
        let _ = remove_link(link);
    } else if link.exists() && !(link.is_dir() && remove_link_folder(link)) {
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
            if path.is_dir() && remove_link_folder(&path) {
                info!("  Removed stale symlinks: {name}/");
            }
            continue;
        }
        info!("  Removing stale symlink: {name}");
        if let Err(e) = remove_link(&path) {
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
        let top = root.join(&entry.top);
        let link = match &entry.sub {
            None => top,
            Some(sub) => {
                // A link left from when the game had one save path: links
                // created through it would land in the live save folder.
                if paths::is_symlink(&top) {
                    let _ = remove_link(&top);
                }
                top.join(sub)
            }
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

/// Hands out folder names, adding `_2`, `_3`… to a name already taken, so
/// two backups never share (and prune) one folder. Case-insensitive:
/// Windows and macOS folders are.
#[derive(Default)]
struct FolderNames(HashSet<String>);

impl FolderNames {
    fn claim(&mut self, wanted: &str) -> String {
        let name = std::iter::once(wanted.to_string())
            .chain((2..).map(|n| format!("{wanted}_{n}")))
            .find(|name| !self.0.contains(&name.to_lowercase()))
            .unwrap_or_default();
        self.0.insert(name.to_lowercase());
        name
    }
}

fn process_game_saves(
    games: &[Game],
    saves_root: &Path,
    strategy: Strategy,
    tops: &mut FolderNames,
) -> Vec<LinkEntry> {
    let mut links = Vec::new();
    let mut processed = 0;
    for game in games.iter().filter(|g| !g.resolved_save_paths.is_empty()) {
        let title = tops.claim(&sanitize_title(&game.title));
        let multi = game.resolved_save_paths.len() > 1;
        let mut subs = FolderNames::default();
        for save in &game.resolved_save_paths {
            let sub = subs.claim(&sanitize_title(
                &save.file_name().unwrap_or_default().to_string_lossy(),
            ));
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

fn process_custom_backups(
    cfg: &Config,
    saves_root: &Path,
    strategy: Strategy,
    tops: &mut FolderNames,
) -> Vec<LinkEntry> {
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
            let top = tops.claim(&sanitize_title(name));
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
    let saves_root = cfg.saves_copies_dir();
    if let Err(e) = std::fs::create_dir_all(&saves_root) {
        error!("Cannot create {}: {e}", saves_root.display());
        return;
    }
    let strategy = strategy(cfg.get("SAVESCOPY_STRATEGY"));
    info!(
        "Running saver with strategy='{}'",
        format!("{strategy:?}").to_lowercase()
    );

    let mut tops = FolderNames::default();
    let mut links = process_game_saves(games, &saves_root, strategy, &mut tops);
    links.extend(process_custom_backups(
        cfg,
        &saves_root,
        strategy,
        &mut tops,
    ));

    let link_root = cfg.saves_links_dir();
    if links.is_empty() {
        return;
    }
    info!("Building symlink tree at {}", link_root.display());
    build_symlink_tree(&links, &link_root);
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

    fn game_with_saves(title: &str, saves: Vec<PathBuf>) -> Game {
        Game {
            title: title.into(),
            resolved_save_paths: saves,
            ..Default::default()
        }
    }

    #[test]
    fn folder_names_add_a_number_to_a_taken_name_ignoring_case() {
        let mut names = FolderNames::default();
        assert_eq!(names.claim("Saves"), "Saves");
        assert_eq!(names.claim("saves"), "saves_2");
        assert_eq!(names.claim("Saves"), "Saves_3");
    }

    #[test]
    fn save_paths_with_the_same_folder_name_back_up_to_separate_folders() {
        let dir = TempDir::new();
        dir.write("local/Studio/Saves/a.sav", b"a");
        dir.write("documents/Studio/Saves/b.sav", b"b");
        let game = game_with_saves(
            "Game",
            vec![
                dir.path().join("local/Studio/Saves"),
                dir.path().join("documents/Studio/Saves"),
            ],
        );
        let root = dir.path().join("saves-copies");

        process_game_saves(
            &[game],
            &root,
            Strategy::Backup,
            &mut FolderNames::default(),
        );
        assert!(root.join("Game/Saves/a.sav").exists());
        assert!(root.join("Game/Saves_2/b.sav").exists());
    }

    #[test]
    fn titles_that_sanitize_alike_back_up_to_separate_folders() {
        let dir = TempDir::new();
        dir.write("one/a.sav", b"a");
        dir.write("two/b.sav", b"b");
        let games = [
            game_with_saves("Game: X", vec![dir.path().join("one")]),
            game_with_saves("Game? X", vec![dir.path().join("two")]),
        ];
        let root = dir.path().join("saves-copies");

        process_game_saves(&games, &root, Strategy::Backup, &mut FolderNames::default());
        assert!(root.join("Game__X/a.sav").exists());
        assert!(root.join("Game__X_2/b.sav").exists());
    }

    #[test]
    fn custom_backup_named_like_a_game_gets_its_own_folder() {
        let dir = TempDir::new();
        dir.write("game/a.sav", b"a");
        dir.write("custom/b.cfg", b"b");
        let game = game_with_saves("shaders", vec![dir.path().join("game")]);
        let values = [(
            "BACKUP_shaders".to_string(),
            dir.path().join("custom").to_string_lossy().into_owned(),
        )];
        let cfg = Config::from_raw(
            dir.path().join("system/config.txt"),
            values.into_iter().collect(),
        );

        run(&[game], &cfg);
        assert!(dir.path().join("saves-copies/shaders/a.sav").exists());
        assert!(dir.path().join("saves-copies/shaders_2/b.cfg").exists());
    }

    #[cfg(unix)]
    #[test]
    fn back_to_one_save_path_replaces_the_folder_of_links_with_a_link() {
        let dir = TempDir::new();
        let saves = dir.mkdir("live/saves");
        let root = dir.mkdir("saves-links");
        dir.mkdir("saves-links/Game");
        std::os::unix::fs::symlink(&saves, root.join("Game/saves")).unwrap();
        std::os::unix::fs::symlink(dir.mkdir("live/old"), root.join("Game/old")).unwrap();
        let entry = LinkEntry {
            top: "Game".into(),
            sub: None,
            source: saves.clone(),
        };

        build_symlink_tree(&[entry], &root);
        assert_eq!(std::fs::read_link(root.join("Game")).unwrap(), saves);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_holding_real_files_is_never_replaced_or_removed() {
        let dir = TempDir::new();
        let saves = dir.mkdir("live/saves");
        let root = dir.mkdir("saves-links");
        dir.write("saves-links/Game/mine.txt", b"keep");
        dir.write("saves-links/Gone/mine.txt", b"keep");
        let entry = LinkEntry {
            top: "Game".into(),
            sub: None,
            source: saves,
        };

        build_symlink_tree(&[entry], &root);
        assert!(root.join("Game/mine.txt").is_file());
        assert!(root.join("Gone/mine.txt").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_removed_game_with_several_save_paths_loses_its_folder_of_links() {
        let dir = TempDir::new();
        let root = dir.mkdir("saves-links");
        dir.mkdir("saves-links/Gone");
        std::os::unix::fs::symlink(dir.mkdir("live/a"), root.join("Gone/a")).unwrap();

        build_symlink_tree(&[], &root);
        assert!(!root.join("Gone").exists());
    }

    #[cfg(unix)]
    #[test]
    fn second_save_path_replaces_the_game_link_with_a_folder() {
        let dir = TempDir::new();
        let first = dir.mkdir("live/first");
        let second = dir.mkdir("live/second");
        let root = dir.mkdir("saves-links");
        std::os::unix::fs::symlink(&first, root.join("Game")).unwrap();
        let entries = [&first, &second].map(|source| LinkEntry {
            top: "Game".into(),
            sub: source.file_name().map(|n| n.to_string_lossy().into_owned()),
            source: source.clone(),
        });

        build_symlink_tree(&entries, &root);
        assert!(!paths::is_symlink(&root.join("Game")));
        assert_eq!(
            std::fs::read_link(root.join("Game/second")).unwrap(),
            second
        );
        assert_eq!(paths::sorted_entries(&first), Vec::<String>::new());
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
        dir.mkdir("saves-links");
        std::os::unix::fs::symlink(&saves, dir.path().join("saves-links/Stale")).unwrap();
        let cfg = Config::from_raw(dir.path().join("system/config.txt"), Default::default());

        run(&[game], &cfg);
        assert_eq!(
            std::fs::read(dir.path().join("saves-copies/My_Game/a.sav")).unwrap(),
            b"a"
        );
        assert_eq!(
            std::fs::read_link(dir.path().join("saves-links/My_Game")).unwrap(),
            saves
        );
        assert!(!dir.path().join("saves-links/Stale").exists());
    }
}
