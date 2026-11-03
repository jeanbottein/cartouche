//! Long file transfers (unpack, pack, download, install) on worker threads.
//! The UI reads their byte progress every frame and collects results when
//! they finish; at most one job per game.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

use macroquad::prelude::get_time;

use crate::core::package::Progress;
use crate::core::sizer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Unpack,
    Pack,
    Download,
    Install,
}

impl Kind {
    pub fn verb(self) -> &'static str {
        match self {
            Kind::Unpack => "Unpacking",
            Kind::Pack => "Packing",
            Kind::Download => "Downloading",
            Kind::Install => "Installing",
        }
    }

    fn done(self) -> &'static str {
        match self {
            Kind::Unpack => "Unpacked",
            Kind::Pack => "Packed",
            Kind::Download => "Downloaded",
            Kind::Install => "Installed",
        }
    }
}

type Work = Box<dyn FnOnce(&Progress) -> Result<PathBuf, String> + Send>;

pub struct Job {
    pub kind: Kind,
    /// Game folder name (also the remote id).
    pub game: String,
    pub title: String,
    progress: Arc<Progress>,
    started: f64,
    worker: JoinHandle<Result<PathBuf, String>>,
}

impl Job {
    pub fn fraction(&self) -> f32 {
        let total = self.progress.total();
        if total == 0 {
            return 0.0;
        }
        (self.progress.done() as f64 / total as f64).min(1.0) as f32
    }

    /// `45% · 3.2 GB / 9.1 GB · 45.0 MB/s · ~2 min`
    pub fn summary(&self) -> String {
        let (done, total) = (self.progress.done(), self.progress.total());
        let elapsed = (get_time() - self.started).max(0.001);
        let rate = done as f64 / elapsed;
        let mut parts = vec![
            format!("{:.0}%", self.fraction() * 100.0),
            format!("{} / {}", sizer::human(done), sizer::human(total)),
        ];
        if elapsed > 1.0 && rate > 0.0 {
            parts.push(format!("{}/s", sizer::human(rate as u64)));
            parts.push(eta(total.saturating_sub(done) as f64 / rate));
        }
        parts.join(" · ")
    }

    pub fn cancelling(&self) -> bool {
        self.progress.is_cancelled()
    }
}

fn eta(seconds: f64) -> String {
    match seconds as u64 {
        s if s < 60 => format!("~{s} s"),
        s if s < 3600 => format!("~{} min", s.div_ceil(60)),
        s => format!("~{} h {:02} min", s / 3600, (s % 3600) / 60),
    }
}

/// A finished job, for a toast and a library reload.
pub struct Finished {
    pub message: String,
    pub ok: bool,
}

#[derive(Default)]
pub struct Jobs {
    running: Vec<Job>,
}

impl Jobs {
    /// Run `work` for `game` unless that game already has a job.
    pub fn start(
        &mut self,
        kind: Kind,
        game: &str,
        title: &str,
        work: impl FnOnce(&Progress) -> Result<PathBuf, String> + Send + 'static,
    ) {
        if self.for_game(game).is_some() {
            return;
        }
        let progress = Arc::new(Progress::default());
        let shared = Arc::clone(&progress);
        let work: Work = Box::new(work);
        self.running.push(Job {
            kind,
            game: game.to_string(),
            title: title.to_string(),
            progress,
            started: get_time(),
            worker: std::thread::spawn(move || work(&shared)),
        });
    }

    pub fn for_game(&self, game: &str) -> Option<&Job> {
        self.running.iter().find(|j| j.game == game)
    }

    pub fn cancel(&self, game: &str) {
        if let Some(job) = self.for_game(game) {
            job.progress.cancel();
        }
    }

    pub fn running(&self) -> &[Job] {
        &self.running
    }

    /// Overall progress of every running job (0..1).
    pub fn fraction(&self) -> f32 {
        let (done, total) = self.running.iter().fold((0, 0), |(d, t), j| {
            (d + j.progress.done(), t + j.progress.total())
        });
        if total == 0 {
            0.0
        } else {
            (done as f64 / total as f64) as f32
        }
    }

    /// Remove and report jobs that finished since the last call.
    pub fn poll(&mut self) -> Vec<Finished> {
        let (finished, running): (Vec<Job>, Vec<Job>) = std::mem::take(&mut self.running)
            .into_iter()
            .partition(|j| j.worker.is_finished());
        self.running = running;
        finished.into_iter().map(report).collect()
    }
}

fn report(job: Job) -> Finished {
    let cancelled = job.progress.is_cancelled();
    match job.worker.join() {
        Ok(Ok(_)) => Finished {
            message: format!("{} {}", job.kind.done(), job.title),
            ok: true,
        },
        Ok(Err(_)) if cancelled => Finished {
            message: format!("Cancelled: {}", job.title),
            ok: false,
        },
        Ok(Err(e)) => {
            log::error!("{} {} failed: {e}", job.kind.verb(), job.title);
            Finished {
                message: format!("{} failed: {e}", job.title),
                ok: false,
            }
        }
        Err(_) => Finished {
            message: format!("{} crashed", job.title),
            ok: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eta_reads_naturally() {
        assert_eq!(eta(42.0), "~42 s");
        assert_eq!(eta(61.0), "~2 min");
        assert_eq!(eta(3_720.0), "~1 h 02 min");
    }
}
