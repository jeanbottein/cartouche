//! Background music: every audio file under the workspace's `.cartouche/`
//! folder, shuffled, looping. Fades out and pauses while a game runs, fades
//! back in when it exits. Off with `MUSIC_ENABLED=False`, or when built
//! without the `music` feature.

// Without the feature only the stub player is used.
#![cfg_attr(not(feature = "music"), allow(dead_code))]

use std::path::{Path, PathBuf};

use crate::core::package;

/// Background level: music sits under the UI, not over it.
const VOLUME: f32 = 0.5;
const FADE_SECS: f32 = 1.5;
const EXTENSIONS: [&str; 7] = ["mp3", "flac", "ogg", "oga", "wav", "m4a", "aac"];

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Audio files anywhere under `dir`.
pub fn tracks(dir: &Path) -> Vec<PathBuf> {
    package::walk(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| matches!(item.kind, package::Kind::File(_)) && is_audio(&item.rel))
        .map(|item| dir.join(item.rel))
        .collect()
}

/// `items` in a random order (Fisher-Yates over xorshift: no `rand` needed
/// for a playlist).
fn shuffled<T>(mut items: Vec<T>, seed: u64) -> Vec<T> {
    let mut state = seed | 1;
    for i in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        items.swap(i, (state % (i as u64 + 1)) as usize);
    }
    items
}

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// Move `volume` toward `target` by one frame of a `FADE_SECS` fade.
fn fade_step(volume: f32, target: f32, dt: f32) -> f32 {
    let step = VOLUME * dt / FADE_SECS;
    if volume < target {
        (volume + step).min(target)
    } else {
        (volume - step).max(target)
    }
}

#[cfg(feature = "music")]
mod player {
    use std::fs::File;
    use std::io::BufReader;
    use std::path::{Path, PathBuf};

    use log::{info, warn};
    use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink};

    use super::{fade_step, seed, shuffled, tracks, VOLUME};

    pub struct Music {
        dir: PathBuf,
        /// Dropping the stream stops the sound: keep it alive with the sink.
        _stream: OutputStream,
        sink: Sink,
        /// Tracks left in this round; reshuffled from disk when empty.
        queue: Vec<PathBuf>,
        volume: f32,
    }

    impl Music {
        /// `None` when `dir` holds no audio or there is no audio device.
        pub fn start(dir: &Path) -> Option<Music> {
            if tracks(dir).is_empty() {
                return None;
            }
            let mut stream = OutputStreamBuilder::open_default_stream()
                .map_err(|e| warn!("No audio output, music off: {e}"))
                .ok()?;
            stream.log_on_drop(false);
            let sink = Sink::connect_new(stream.mixer());
            sink.set_volume(0.0);
            Some(Music {
                dir: dir.to_path_buf(),
                _stream: stream,
                sink,
                queue: Vec::new(),
                volume: 0.0,
            })
        }

        pub fn dir(&self) -> &Path {
            &self.dir
        }

        /// Per frame: fade toward silence while `quiet` (a game runs), else
        /// toward the background level, and keep a track playing.
        pub fn update(&mut self, dt: f32, quiet: bool) {
            let target = if quiet { 0.0 } else { VOLUME };
            self.volume = fade_step(self.volume, target, dt);
            self.sink.set_volume(self.volume);
            if quiet && self.volume == 0.0 {
                self.sink.pause();
                return;
            }
            self.sink.play();
            if self.sink.empty() {
                self.queue_next();
            }
        }

        /// Append the next playable track; skips files that don't decode.
        fn queue_next(&mut self) {
            // Bounded: each attempt consumes a track, a whole round at most.
            for _ in 0..=self.queue.len().max(tracks(&self.dir).len()) {
                if self.queue.is_empty() {
                    // Picks up files added or removed since the last round.
                    self.queue = shuffled(tracks(&self.dir), seed());
                }
                let Some(path) = self.queue.pop() else { return };
                let decoded = File::open(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|f| Decoder::new(BufReader::new(f)).map_err(|e| e.to_string()));
                match decoded {
                    Ok(source) => {
                        info!("Music: {}", path.display());
                        self.sink.append(source);
                        return;
                    }
                    Err(e) => warn!("Music: skipping {}: {e}", path.display()),
                }
            }
        }
    }
}

#[cfg(not(feature = "music"))]
mod player {
    use std::path::Path;

    /// Built without the `music` feature: never starts.
    pub struct Music;

    impl Music {
        pub fn start(_dir: &Path) -> Option<Music> {
            None
        }

        pub fn dir(&self) -> &Path {
            Path::new("")
        }

        pub fn update(&mut self, _dt: f32, _quiet: bool) {}
    }
}

pub use player::Music;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn finds_audio_files_in_subfolders_only() {
        let dir = TempDir::new();
        dir.write("a.MP3", b"");
        dir.write("music/b.flac", b"");
        dir.write("ui-thumbs/c.png", b"");
        dir.write("config.txt", b"");
        let mut names: Vec<_> = tracks(dir.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["a.MP3", "b.flac"]);
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let items: Vec<u32> = (0..50).collect();
        let mut out = shuffled(items.clone(), 42);
        assert_ne!(out, items);
        out.sort();
        assert_eq!(out, items);
    }

    #[test]
    fn fades_reach_their_target_without_overshoot() {
        let up = (0..200).fold(0.0, |v, _| fade_step(v, VOLUME, 0.016));
        assert_eq!(up, VOLUME);
        let half_way = fade_step(VOLUME, 0.0, FADE_SECS / 2.0);
        assert!((half_way - VOLUME / 2.0).abs() < 1e-6);
        assert_eq!(fade_step(0.01, 0.0, 1.0), 0.0);
    }
}
