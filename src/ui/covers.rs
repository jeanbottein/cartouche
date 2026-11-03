//! Background image decoding. Artwork is decoded on a thread (macroquad's own
//! loader is PNG-only and panics on JPEG), downscaled (Lanczos3) and uploaded to
//! GPU textures on the main thread as it arrives, so screens never block.
//! Library covers are cached as small PNG thumbnails, revalidated by mtime.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::SystemTime;

use macroquad::prelude::*;

use crate::core::models::{Game, ImageSlot};

/// Max thumbnail dimension: covers full-screen display sizes while keeping
/// decode fast and the cache small.
const THUMB_CAP: u32 = 512;

pub struct Job {
    pub key: usize,
    pub source: PathBuf,
    /// Thumbnail cache file, if this image should be cached.
    pub cache: Option<PathBuf>,
}

struct Decoded {
    key: usize,
    rgba: Vec<u8>,
    w: u16,
    h: u16,
}

pub struct ImageLoader {
    rx: Receiver<Decoded>,
    total: usize,
    done: usize,
}

impl ImageLoader {
    pub fn spawn(jobs: Vec<Job>) -> ImageLoader {
        let total = jobs.len();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for job in jobs {
                let decoded = decode(&job.source, job.cache.as_deref());
                // Count failures too, so progress completes.
                let msg = decoded.map_or(
                    Decoded {
                        key: job.key,
                        rgba: Vec::new(),
                        w: 0,
                        h: 0,
                    },
                    |(rgba, w, h)| Decoded {
                        key: job.key,
                        rgba,
                        w,
                        h,
                    },
                );
                if tx.send(msg).is_err() {
                    break; // receiver gone
                }
            }
        });
        ImageLoader { rx, total, done: 0 }
    }

    /// Textures decoded since the last call (main thread only).
    pub fn poll(&mut self) -> Vec<(usize, Texture2D)> {
        let mut ready = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            self.done += 1;
            if msg.rgba.is_empty() {
                continue;
            }
            let tex = Texture2D::from_rgba8(msg.w, msg.h, &msg.rgba);
            tex.set_filter(FilterMode::Linear);
            ready.push((msg.key, tex));
        }
        ready
    }

    pub fn loading(&self) -> bool {
        self.done < self.total
    }

    /// (done, total) for a progress indicator.
    pub fn progress(&self) -> (usize, usize) {
        (self.done, self.total)
    }
}

/// Cover jobs keyed by game index, cached under `cache_dir`.
pub fn cover_jobs(games: &[Game], cache_dir: &Path) -> Vec<Job> {
    let _ = std::fs::create_dir_all(cache_dir);
    games
        .iter()
        .enumerate()
        .filter_map(|(key, g)| {
            let source = g.image_path(ImageSlot::Cover)?;
            Some(Job {
                key,
                source,
                cache: Some(cache_dir.join(format!("{}.png", g.folder_name))),
            })
        })
        .collect()
}

/// Uncached jobs for every artwork slot of one game, keyed by slot index.
pub fn artwork_jobs(game: &Game) -> Vec<Job> {
    ImageSlot::ALL
        .iter()
        .enumerate()
        .filter_map(|(key, &slot)| {
            Some(Job {
                key,
                source: game.image_path(slot)?,
                cache: None,
            })
        })
        .collect()
}

fn decode(src: &Path, cache: Option<&Path>) -> Option<(Vec<u8>, u16, u16)> {
    let src_mtime = std::fs::metadata(src).and_then(|m| m.modified()).ok()?;
    let rgba = match cache {
        Some(thumb) if is_fresh(thumb, src_mtime) => image::open(thumb).ok()?.to_rgba8(),
        _ => {
            let bytes = std::fs::read(src).ok()?;
            let resized = image::load_from_memory(&bytes).ok()?.resize(
                THUMB_CAP,
                THUMB_CAP,
                image::imageops::FilterType::Lanczos3,
            );
            if let Some(thumb) = cache {
                let _ = resized.save(thumb); // best-effort; regenerated next time if it fails
            }
            resized.to_rgba8()
        }
    };
    let (w, h) = rgba.dimensions();
    Some((rgba.into_raw(), w as u16, h as u16))
}

/// A cached thumbnail is usable if it is at least as new as its source.
fn is_fresh(thumb: &Path, src_mtime: SystemTime) -> bool {
    std::fs::metadata(thumb)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t >= src_mtime)
}
