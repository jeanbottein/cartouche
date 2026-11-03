//! LAN sharing: a read-only HTTP server that lists and streams games as
//! packages, plus UDP broadcast discovery. No auth: meant for a home LAN.

pub mod client;
pub mod discovery;
pub mod server;

use serde::{Deserialize, Serialize};

use super::config::Config;
use super::models::{Game, ImageSlot, Target};

/// Bumped on incompatible API changes; peers with another version are ignored.
pub const PROTOCOL: u32 = 1;
pub const DEFAULT_PORT: u16 = 47800;

/// `SHARE_PORT`, else the default.
pub fn port(cfg: &Config) -> u16 {
    cfg.get("SHARE_PORT")
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Name shown to other machines: `MACHINE_NAME`, else the host name.
pub fn machine_name(cfg: &Config) -> String {
    cfg.get("MACHINE_NAME")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(host_name)
}

fn host_name() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| {
            let out = std::process::Command::new("hostname").output().ok()?;
            String::from_utf8(out.stdout).ok()
        })
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "cartouche".into())
}

/// One game as listed by a server (`GET /api/v1/games`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteGame {
    /// Folder name; also the id in URLs.
    pub id: String,
    pub title: String,
    pub year: Option<u32>,
    pub players: String,
    pub description: String,
    /// Bytes of the game's own files.
    pub size: Option<u64>,
    pub targets: Vec<Target>,
    pub installed: bool,
    pub has_cover: bool,
}

impl From<&Game> for RemoteGame {
    fn from(game: &Game) -> RemoteGame {
        RemoteGame {
            id: game.folder_name.clone(),
            title: game.title.clone(),
            year: game.year,
            players: game.players.clone(),
            description: game.description.clone(),
            size: game.size,
            targets: game.targets.clone(),
            installed: game.is_installed(),
            has_cover: game.images.get(ImageSlot::Cover).is_some(),
        }
    }
}

/// `GET /api/v1/info` and discovery replies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub port: u16,
    pub version: String,
    pub protocol: u32,
}

impl ServerInfo {
    pub fn new(name: String, port: u16) -> ServerInfo {
        ServerInfo {
            name,
            port,
            version: env!("CARGO_PKG_VERSION").into(),
            protocol: PROTOCOL,
        }
    }
}

/// Percent-encode a URL path segment.
pub fn encode_segment(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Decode a percent-encoded URL path segment; `None` when malformed.
pub fn decode_segment(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    // A loop: each step consumes one or three bytes.
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_round_trip() {
        let name = "Hollow Knight: Silk/Song é";
        let encoded = encode_segment(name);
        assert!(!encoded.contains(' ') && !encoded.contains('/'));
        assert_eq!(decode_segment(&encoded).as_deref(), Some(name));
        assert_eq!(decode_segment("bad%2"), None);
        assert_eq!(decode_segment("bad%zz"), None);
    }

    #[test]
    fn port_falls_back_to_default() {
        let cfg = Config::from_raw("/ws/.cartouche/config.txt".into(), Default::default());
        assert_eq!(port(&cfg), DEFAULT_PORT);
    }
}
