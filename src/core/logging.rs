//! Log sink: every record goes to stderr (`%(message)s`, like the old Python
//! `basicConfig`), and — while the UI is listening — to a channel the Status
//! screen drains into its log panel.

use std::sync::mpsc::Sender;
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

pub struct Line {
    pub level: Level,
    pub text: String,
}

static UI_SINK: Mutex<Option<Sender<Line>>> = Mutex::new(None);

struct Logger;

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) || !record.target().starts_with(env!("CARGO_PKG_NAME"))
        {
            return;
        }
        let text = record.args().to_string();
        eprintln!("{text}");
        if let Some(tx) = UI_SINK.lock().unwrap().as_ref() {
            let _ = tx.send(Line {
                level: record.level(),
                text,
            });
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let level = if std::env::var_os("CARTOUCHE_DEBUG").is_some() {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    if log::set_logger(&Logger).is_ok() {
        log::set_max_level(level);
    }
}

/// Mirror log records into `tx` (replaces any previous listener).
pub fn attach(tx: Sender<Line>) {
    *UI_SINK.lock().unwrap() = Some(tx);
}
