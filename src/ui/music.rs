//! Background music: every audio file under the workspace's `system/`
//! folder, shuffled, looping, at `MUSIC_VOLUME`. Fades out and pauses while a
//! game runs or while muted (`MUSIC_ENABLED=False`, toggled by Select), fades
//! back in to the same track. Silent when built without the `music` feature.

// Without the feature only the stub player is used.
#![cfg_attr(not(feature = "music"), allow(dead_code))]

use std::path::{Path, PathBuf};

use crate::core::package;

/// `MUSIC_VOLUME` when unset: music sits under the UI, not over it.
const DEFAULT_PERCENT: f32 = 50.0;
/// Seconds a fade takes from full volume to silence.
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

/// The volume `MUSIC_VOLUME` asks for (a percentage), from 0 to 1.
pub fn level(setting: Option<&str>) -> f32 {
    let percent = setting
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(DEFAULT_PERCENT);
    percent.clamp(0.0, 100.0) / 100.0
}

/// Move `volume` toward `target` by one frame of a fade.
fn fade_step(volume: f32, target: f32, dt: f32) -> f32 {
    let step = dt / FADE_SECS;
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

    use super::{fade_step, seed, shuffled, tracks};

    pub struct Music {
        dir: PathBuf,
        /// Dropping the stream stops the sound: keep it alive with the sink.
        _stream: OutputStream,
        sink: Sink,
        /// Tracks left in this round; reshuffled from disk when empty.
        queue: Vec<PathBuf>,
        /// Where the volume is now, and where it settles when playing.
        volume: f32,
        level: f32,
        muted: bool,
        /// No file could be decoded: stop trying (until the app restarts).
        silent: bool,
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
                level: super::level(None),
                muted: false,
                silent: false,
            })
        }

        pub fn dir(&self) -> &Path {
            &self.dir
        }

        pub fn set_level(&mut self, level: f32) {
            self.level = level;
        }

        pub fn set_muted(&mut self, muted: bool) {
            self.muted = muted;
        }

        /// Per frame: fade toward silence while `quiet` (a game runs) or
        /// muted, else toward the level, and keep a track playing.
        pub fn update(&mut self, dt: f32, quiet: bool) {
            let target = if quiet || self.muted { 0.0 } else { self.level };
            self.volume = fade_step(self.volume, target, dt);
            self.sink.set_volume(self.volume);
            if target == 0.0 && self.volume == 0.0 {
                self.sink.pause();
                return;
            }
            self.sink.play();
            if self.sink.empty() && !self.silent {
                self.silent = !self.queue_next();
            }
        }

        /// Append the next playable track, skipping files that don't decode;
        /// false when none in a whole round could be played.
        fn queue_next(&mut self) -> bool {
            // Bounded: each attempt consumes a track, a whole round at most.
            for _ in 0..=self.queue.len().max(tracks(&self.dir).len()) {
                if self.queue.is_empty() {
                    // Picks up files added or removed since the last round.
                    self.queue = shuffled(tracks(&self.dir), seed());
                }
                let Some(path) = self.queue.pop() else {
                    return false;
                };
                let decoded = File::open(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|f| Decoder::new(BufReader::new(f)).map_err(|e| e.to_string()));
                match decoded {
                    Ok(source) => {
                        info!("Music: {}", path.display());
                        self.sink.append(source);
                        return true;
                    }
                    Err(e) => warn!("Music: skipping {}: {e}", path.display()),
                }
            }
            warn!("Music off: no playable file in {}", self.dir.display());
            false
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

        pub fn set_level(&mut self, _level: f32) {}

        pub fn set_muted(&mut self, _muted: bool) {}

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
        let up = (0..200).fold(0.0, |v, _| fade_step(v, 0.8, 0.016));
        assert_eq!(up, 0.8);
        let half_way = fade_step(1.0, 0.0, FADE_SECS / 2.0);
        assert!((half_way - 0.5).abs() < 1e-6);
        assert_eq!(fade_step(0.01, 0.0, 1.0), 0.0);
    }

    #[test]
    fn a_fade_to_silence_ends_whatever_the_volume() {
        let down = (0..200).fold(0.5, |v, _| fade_step(v, 0.0, 0.016));
        assert_eq!(down, 0.0);
    }

    #[test]
    fn the_volume_setting_is_a_percentage_defaulting_to_half() {
        assert_eq!(level(Some("80")), 0.8);
        assert_eq!(level(Some(" 0 ")), 0.0);
        assert_eq!(level(Some("150")), 1.0);
        assert_eq!(level(Some("loud")), 0.5);
        assert_eq!(level(None), 0.5);
    }
}
