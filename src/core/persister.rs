//! Pipeline step 4: write `.cartridge/game.json` and download artwork for every
//! game changed in memory.

use std::io::Read;
use std::path::Path;

use log::{error, info, warn};

use super::app::GAME_DATA_DIR;
use super::models::{Game, ImageSlot, GAME_JSON};
use super::{paths, sgdb};

fn download(url: &str, dest: &Path) -> Result<(), String> {
    let response = sgdb::http_get(url, 15)?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    paths::write_atomic(dest, bytes).map_err(|e| e.to_string())
}

/// Download the artwork collected during enrichment whose destination passes
/// `wanted`. Clears the pending URLs.
fn download_images(game: &mut Game, wanted: impl Fn(&Path) -> bool) {
    for slot in ImageSlot::ALL {
        let url = game.artwork_urls.get(slot.art_type()).cloned().flatten();
        let (Some(url), Some(dest)) = (url, game.image_path(slot)) else {
            continue;
        };
        if !wanted(&dest) {
            continue;
        }
        match download(&url, &dest) {
            Ok(()) => info!(
                "  Downloaded {}: {}",
                slot.name(),
                game.images.get(slot).unwrap_or_default()
            ),
            Err(e) => warn!("Failed to download {url}: {e}"),
        }
    }
    game.artwork_urls.clear();
}

/// Write `game.json` for one game.
pub fn write_game_json(game: &mut Game) -> std::io::Result<()> {
    std::fs::create_dir_all(game.data_dir())?;
    paths::write_atomic(game.game_json_path(), game.to_json())?;
    game.has_cartridge = true;
    game.needs_persist = false;
    Ok(())
}

/// Download every image with a pending URL, replacing existing files (editor
/// "Fetch images").
pub fn download_all_images(game: &mut Game) {
    if let Err(e) = std::fs::create_dir_all(game.data_dir()) {
        warn!("Cannot create {}: {e}", game.data_dir().display());
    }
    download_images(game, |_| true);
}

/// Persist every game flagged `needs_persist`.
pub fn persist(games: &mut [Game]) {
    // Packaged games are read-only: writing their metadata would create a
    // `Foo/` folder that looks like a second, installed copy.
    let dirty: Vec<&mut Game> = games
        .iter_mut()
        .filter(|g| g.needs_persist && g.is_installed())
        .collect();
    if dirty.is_empty() {
        return;
    }
    info!("Persisting {} game(s)", dirty.len());
    for game in dirty {
        match write_game_json(game) {
            Ok(()) => info!("  Saved: {}/{GAME_DATA_DIR}/{GAME_JSON}", game.folder_name),
            Err(e) => {
                error!("  Failed to write {}: {e}", game.game_json_path().display());
                continue;
            }
        }
        download_images(game, |dest| !dest.is_file());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn persists_only_dirty_games() {
        let dir = TempDir::new();
        let mut clean = Game::skeleton(&dir.mkdir("Clean"));
        let mut dirty = Game::skeleton(&dir.mkdir("Dirty"));
        dirty.needs_persist = true;
        clean.title = "unchanged".into();
        let mut games = vec![clean, dirty];
        persist(&mut games);
        assert!(!games[0].game_json_path().exists());
        assert!(games[1].game_json_path().is_file());
        assert!(!games[1].needs_persist && games[1].has_cartridge);
    }
}
