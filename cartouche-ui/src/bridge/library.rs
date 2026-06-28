//! Game discovery (BRIDGE-2) + asynchronous cover loading.
//!
//! `scan_games` reads the (small, fast) `game.json` metadata so the grid can show
//! immediately. Cover art is then decoded on a background thread by [`CoverLoader`]
//! and uploaded to GPU textures on the main thread as it arrives, so startup never
//! blocks. Covers are decoded ourselves (macroquad's loader is PNG-only and
//! *panics* on JPEG), high-quality downscaled (Lanczos3), and cached as small PNG
//! thumbnails. The cache is revalidated against source mtime each launch.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::SystemTime;

use macroquad::prelude::*;

use super::models::GameJson;

/// Max thumbnail dimension — covers full-screen display sizes while keeping decode
/// fast and the cache small.
const THUMB_CAP: u32 = 512;

pub struct GameEntry {
    pub title: String,
    pub folder_name: String,
    pub dir: PathBuf,
    /// Cover filename from `game.json` (the game expects art), if any.
    pub cover_file: Option<String>,
    /// GPU texture, filled in asynchronously once decoded.
    pub cover: Option<Texture2D>,
    pub steamgriddb_id: Option<i64>,
    pub target_count: usize,
    pub save_count: usize,
}

/// Phase A: parse metadata for every game (cover textures not yet loaded).
pub fn scan_games(games_dir: &Path) -> Vec<GameEntry> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(games_dir) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    for dir in dirs {
        let json_path = dir.join(".cartouche").join("game.json");
        let Ok(text) = std::fs::read_to_string(&json_path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<GameJson>(&text) else {
            continue;
        };

        let folder_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let title = if meta.title.trim().is_empty() {
            folder_name.clone()
        } else {
            meta.title.clone()
        };

        out.push(GameEntry {
            title,
            folder_name,
            dir,
            cover_file: meta.images.cover.clone(),
            cover: None,
            steamgriddb_id: meta.steamgriddb_id,
            target_count: meta.targets.len(),
            save_count: meta.save_paths.len(),
        });
    }
    out
}

struct CoverMsg {
    index: usize,
    rgba: Vec<u8>,
    w: u16,
    h: u16,
}

/// Phase B: decodes/caches covers off-thread and feeds them to the main thread.
pub struct CoverLoader {
    rx: Receiver<CoverMsg>,
    total: usize,
    done: usize,
}

impl CoverLoader {
    /// Spawn the worker. `cache_dir` holds the PNG thumbnail cache (created here).
    pub fn spawn(games: &[GameEntry], cache_dir: PathBuf) -> CoverLoader {
        let _ = std::fs::create_dir_all(&cache_dir);

        let mut jobs: Vec<(usize, PathBuf, PathBuf)> = Vec::new();
        for (i, g) in games.iter().enumerate() {
            if let Some(cover_file) = &g.cover_file {
                let src = g.dir.join(".cartouche").join(cover_file);
                let thumb = cache_dir.join(format!("{}.png", g.folder_name));
                jobs.push((i, src, thumb));
            }
        }
        let total = jobs.len();

        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for (index, src, thumb) in jobs {
                if let Some((rgba, w, h)) = decode_cover(&src, &thumb) {
                    if tx.send(CoverMsg { index, rgba, w, h }).is_err() {
                        break; // receiver gone (app closing)
                    }
                }
            }
        });

        CoverLoader { rx, total, done: 0 }
    }

    /// Upload any decoded covers that have arrived (call once per frame, main thread).
    pub fn poll(&mut self, games: &mut [GameEntry]) {
        while let Ok(msg) = self.rx.try_recv() {
            let tex = Texture2D::from_rgba8(msg.w, msg.h, &msg.rgba);
            tex.set_filter(FilterMode::Linear);
            if let Some(g) = games.get_mut(msg.index) {
                g.cover = Some(tex);
            }
            self.done += 1;
        }
    }

    pub fn loading(&self) -> bool {
        self.done < self.total
    }

    /// (done, total) for a progress indicator.
    pub fn progress(&self) -> (usize, usize) {
        (self.done, self.total)
    }
}

fn decode_cover(src: &Path, thumb: &Path) -> Option<(Vec<u8>, u16, u16)> {
    let src_mtime = std::fs::metadata(src).and_then(|m| m.modified()).ok()?;

    let rgba = if cache_is_fresh(thumb, src_mtime) {
        image::open(thumb).ok()?.to_rgba8()
    } else {
        let bytes = std::fs::read(src).ok()?;
        let resized = image::load_from_memory(&bytes)
            .ok()?
            .resize(THUMB_CAP, THUMB_CAP, image::imageops::FilterType::Lanczos3);
        let _ = resized.save(thumb); // best-effort; regenerated next launch if it fails
        resized.to_rgba8()
    };

    let (w, h) = rgba.dimensions();
    Some((rgba.into_raw(), w as u16, h as u16))
}

/// A cached thumbnail is usable if it exists and is at least as new as its source.
fn cache_is_fresh(thumb: &Path, src_mtime: SystemTime) -> bool {
    std::fs::metadata(thumb)
        .and_then(|m| m.modified())
        .map(|tm| tm >= src_mtime)
        .unwrap_or(false)
}
