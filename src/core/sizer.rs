//! Pipeline step: measure each installed game's size on disk and store it in
//! `game.json` when it changed.

use std::path::Path;

use log::{info, warn};

use super::app::data_dir_name;
use super::models::Game;
use super::package;

/// Bytes of the game's own files (`.cartouche/` excluded); symlinks not followed.
pub fn measure_game(dir: &Path) -> std::io::Result<u64> {
    let data = Path::new(&data_dir_name()).to_path_buf();
    Ok(package::walk(dir)?
        .iter()
        .filter(|item| !item.rel.starts_with(&data))
        .map(package::Item::file_size)
        .sum())
}

/// `1.2 GB`-style size (powers of 1024, like file managers on Linux).
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let exponent = (0..UNITS.len() - 1)
        .take_while(|&i| bytes >= 1024u64.pow(i as u32 + 1))
        .count();
    if exponent == 0 {
        return format!("{bytes} B");
    }
    let value = bytes as f64 / 1024f64.powi(exponent as i32);
    format!("{value:.1} {}", UNITS[exponent])
}

/// Update `size` on installed games; flags changed ones for persistence.
pub fn measure(games: &mut [Game]) {
    let mut changed = 0;
    for game in games.iter_mut().filter(|g| g.is_installed()) {
        match measure_game(&game.dir) {
            Ok(size) if game.size != Some(size) => {
                game.size = Some(size);
                game.needs_persist = true;
                changed += 1;
            }
            Ok(_) => {}
            Err(e) => warn!("  Cannot measure {}: {e}", game.folder_name),
        }
    }
    if changed > 0 {
        info!("Measured {changed} game(s) with a new size");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Install;
    use crate::core::testutil::TempDir;

    #[test]
    fn human_sizes() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1536), "1.5 KB");
        assert_eq!(human(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn counts_game_files_but_not_metadata() {
        let dir = TempDir::new();
        dir.write("G/a.bin", &[0; 10]);
        dir.write("G/sub/b.bin", &[0; 5]);
        dir.write(&format!("G/{}/cover.png", data_dir_name()), &[0; 100]);
        assert_eq!(measure_game(&dir.path().join("G")).unwrap(), 15);
    }

    #[test]
    fn flags_only_changed_installed_games() {
        let dir = TempDir::new();
        dir.write("Same/a", &[0; 4]);
        dir.write("New/a", &[0; 4]);
        let mut same = Game::skeleton(&dir.path().join("Same"));
        same.size = Some(4);
        let new = Game::skeleton(&dir.path().join("New"));
        let mut packaged = Game::skeleton(&dir.path().join("Missing"));
        packaged.install = Install::Packaged {
            package: dir.path().join("Missing.cartouche.tar"),
        };
        let mut games = vec![same, new, packaged];
        measure(&mut games);
        assert!(!games[0].needs_persist);
        assert!(games[1].needs_persist && games[1].size == Some(4));
        assert!(!games[2].needs_persist && games[2].size.is_none());
    }
}
