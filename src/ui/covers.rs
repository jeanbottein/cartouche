//! Background image decoding. Artwork is decoded on a thread (macroquad's own
//! loader is PNG-only and panics on JPEG), downscaled (Lanczos3) and uploaded to
//! GPU textures on the main thread as it arrives, so screens never block.
//! Library covers are cached as small PNG thumbnails, revalidated by mtime.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::SystemTime;

use macroquad::prelude::*;

use crate::core::models::{Game, ImageSlot};
use crate::core::package;
use crate::core::share::client;

/// Max thumbnail dimension: covers full-screen display sizes while keeping
/// decode fast and the cache small.
const THUMB_CAP: u32 = 512;

/// Where an image's bytes live.
pub enum Source {
    File(PathBuf),
    /// Artwork inside a package's `.cartouche/` (game not unpacked yet).
    Package {
        package: PathBuf,
        name: String,
    },
    /// A game's cover on another machine (`GET /api/v1/games/{id}/cover`).
    Remote {
        addr: SocketAddr,
        id: String,
    },
}

impl Source {
    /// Modification time of the local file holding the image (cache freshness).
    fn mtime(&self) -> Option<SystemTime> {
        let path = match self {
            Source::File(path) | Source::Package { package: path, .. } => path,
            Source::Remote { .. } => return None,
        };
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    fn read(&self) -> Option<Vec<u8>> {
        match self {
            Source::File(path) => std::fs::read(path).ok(),
            Source::Package { package, name } => package::read_data_file(package, name),
            Source::Remote { addr, id } => client::cover(*addr, id),
        }
    }
}

pub struct Job {
    pub key: usize,
    pub source: Source,
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
            let source = match g.package().filter(|_| !g.is_installed()) {
                Some(package) => Source::Package {
                    package: package.to_path_buf(),
                    name: g.images.cover.clone()?,
                },
                None => Source::File(g.image_path(ImageSlot::Cover)?),
            };
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
                source: Source::File(game.image_path(slot)?),
                cache: None,
            })
        })
        .collect()
}

fn decode(src: &Source, cache: Option<&Path>) -> Option<(Vec<u8>, u16, u16)> {
    // Remote images have no local mtime: decoded every time, never cached.
    let src_mtime = src.mtime();
    let cache = cache.filter(|_| src_mtime.is_some());
    let rgba = match (cache, src_mtime) {
        (Some(thumb), Some(mtime)) if is_fresh(thumb, mtime) => image::open(thumb).ok()?.to_rgba8(),
        _ => {
            let bytes = src.read()?;
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
