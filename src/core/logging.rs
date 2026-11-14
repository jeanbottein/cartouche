//! Log sink: every record goes to stderr (`%(message)s`, like the old Python
//! `basicConfig`), or to the systemd journal when stderr is no terminal (Steam
//! Gaming Mode, a menu launch: `journalctl --user -t cartridge -f`); and —
//! while the UI is listening — to a channel (it toasts warnings and errors).

use std::io::IsTerminal;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

pub struct Line {
    pub level: Level,
    pub text: String,
}

static UI_SINK: Mutex<Option<Sender<Line>>> = Mutex::new(None);
static OUTPUT: OnceLock<Output> = OnceLock::new();

enum Output {
    Stderr,
    #[cfg(unix)]
    Journal(std::os::unix::net::UnixDatagram),
}

impl Output {
    fn pick() -> Output {
        if std::io::stderr().is_terminal() {
            return Output::Stderr;
        }
        #[cfg(unix)]
        if let Some(socket) = journal::connect() {
            return Output::Journal(socket);
        }
        Output::Stderr
    }

    fn write(&self, level: Level, text: &str) {
        #[cfg(unix)]
        if let Output::Journal(socket) = self {
            if socket.send(&journal::entry(level, text)).is_ok() {
                return;
            }
        }
        eprintln!("{text}");
    }
}

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
        OUTPUT
            .get_or_init(Output::pick)
            .write(record.level(), &text);
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
    let level = if std::env::var_os("CARTRIDGE_DEBUG").is_some() {
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

/// systemd journal, through its native protocol (no library needed). Only
/// Linux has the socket; elsewhere `connect` finds nothing and stderr stays.
#[cfg(unix)]
mod journal {
    use std::os::unix::net::UnixDatagram;

    use log::Level;

    use crate::core::app::APP_NAME;

    const SOCKET: &str = "/run/systemd/journal/socket";

    pub fn connect() -> Option<UnixDatagram> {
        let socket = UnixDatagram::unbound().ok()?;
        socket.connect(SOCKET).ok()?;
        Some(socket)
    }

    /// syslog priorities, so `journalctl -p warning` shows only problems.
    fn priority(level: Level) -> u8 {
        match level {
            Level::Error => 3,
            Level::Warn => 4,
            Level::Info => 6,
            Level::Debug | Level::Trace => 7,
        }
    }

    /// One journal entry. MESSAGE uses the length-prefixed form, which may
    /// hold newlines (phase headers start with one).
    pub fn entry(level: Level, text: &str) -> Vec<u8> {
        let fields = format!(
            "PRIORITY={}\nSYSLOG_IDENTIFIER={APP_NAME}\nMESSAGE\n",
            priority(level)
        );
        [
            fields.as_bytes(),
            &(text.len() as u64).to_le_bytes(),
            text.as_bytes(),
            b"\n",
        ]
        .concat()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn an_entry_carries_level_name_and_a_multiline_message() {
            let mut expected = b"PRIORITY=4\nSYSLOG_IDENTIFIER=cartridge\nMESSAGE\n".to_vec();
            expected.extend(7u64.to_le_bytes());
            expected.extend(b"\nA\nB ok\n");
            assert_eq!(entry(Level::Warn, "\nA\nB ok"), expected);
        }

        #[test]
        fn entries_reach_a_listening_socket() {
            let dir = crate::core::testutil::TempDir::new();
            let path = dir.path().join("journal.socket");
            let listener = UnixDatagram::bind(&path).unwrap();
            let socket = UnixDatagram::unbound().unwrap();
            socket.connect(&path).unwrap();
            socket.send(&entry(Level::Error, "boom")).unwrap();
            let mut buf = [0u8; 256];
            let n = listener.recv(&mut buf).unwrap();
            assert!(buf[..n].starts_with(b"PRIORITY=3\n"));
        }
    }
}
