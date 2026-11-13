//! Pipeline step: measure the size on disk of installed games whose size
//! isn't known yet, and store it in `game.json`.

use std::path::Path;

use log::{info, warn};

use super::app::GAME_DATA_DIR;
use super::models::Game;
use super::package;

/// Bytes of the game's own files (`.cartridge/` excluded); symlinks not followed.
pub fn measure_game(dir: &Path) -> std::io::Result<u64> {
    let data = Path::new(GAME_DATA_DIR);
    Ok(package::walk(dir)?
        .iter()
        .filter(|item| !item.rel.starts_with(data))
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

/// Measure installed games of unknown size; flags them for persistence.
/// A known size is kept: walking every game's files on every run is slow on
/// a big library (SD card), and unpacking records the size anyway.
pub fn measure(games: &mut [Game]) {
    let mut measured = 0;
    for game in games
        .iter_mut()
        .filter(|g| g.is_installed() && g.size.is_none())
    {
        match measure_game(&game.dir) {
            Ok(size) => {
                game.size = Some(size);
                game.needs_persist = true;
                measured += 1;
            }
            Err(e) => warn!("  Cannot measure {}: {e}", game.folder_name),
        }
    }
    if measured > 0 {
        info!("Measured {measured} game(s)");
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
        dir.write(&format!("G/{GAME_DATA_DIR}/cover.png"), &[0; 100]);
        assert_eq!(measure_game(&dir.path().join("G")).unwrap(), 15);
    }

    #[test]
    fn measures_only_installed_games_of_unknown_size() {
        let dir = TempDir::new();
        dir.write("Known/a", &[0; 4]);
        dir.write("New/a", &[0; 4]);
        // Walking every game on every run is slow on a big SD card: a known
        // size stays until the game is installed again.
        let mut known = Game::skeleton(&dir.path().join("Known"));
        known.size = Some(1);
        let new = Game::skeleton(&dir.path().join("New"));
        let mut packaged = Game::skeleton(&dir.path().join("Missing"));
        packaged.install = Install::Packaged {
            package: dir.path().join("Missing.cartridge.tar"),
        };
        let mut games = vec![known, new, packaged];
        measure(&mut games);
        assert!(!games[0].needs_persist && games[0].size == Some(1));
        assert!(games[1].needs_persist && games[1].size == Some(4));
        assert!(!games[2].needs_persist && games[2].size.is_none());
    }
}
