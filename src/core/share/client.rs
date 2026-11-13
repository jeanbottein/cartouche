//! Client side: find peers, list their games, and copy a game either as a
//! package (Download: kept as `Foo.cartridge.tar`, e.g. on a library machine)
//! or straight into a folder (Install: unpacked while it streams, no tar on disk).

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::discovery::{self, Peer};
use super::{encode_segment, port, RemoteGame, ServerInfo, PROTOCOL};
use crate::core::config::Config;
use crate::core::package::{self, Progress};

const LIST_TIMEOUT: Duration = Duration::from_secs(10);
/// Per read: a stalled stream fails instead of hanging; a long one is fine.
const STREAM_TIMEOUT: Duration = Duration::from_secs(60);

fn get(url: &str, read_timeout: Duration) -> Result<ureq::Response, String> {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(read_timeout)
        .build()
        .get(url)
        .call()
        .map_err(|e| e.to_string())
}

/// Covers are a few hundred KB, game lists well under a MB: a broken or
/// hostile peer can't fill memory with either.
const MAX_BODY: u64 = 16 * 1024 * 1024;

/// The whole body, unless it is longer than `limit`.
fn read_body(response: ureq::Response, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("answer larger than {limit} bytes"));
    }
    Ok(bytes)
}

fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let body = read_body(get(url, LIST_TIMEOUT)?, MAX_BODY)?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

fn api(addr: SocketAddr, path: &str) -> String {
    format!("http://{addr}/api/v1/{path}")
}

fn game_url(addr: SocketAddr, id: &str, what: &str) -> String {
    api(addr, &format!("games/{}/{what}", encode_segment(id)))
}

/// The server at `host[:port]`, if one answers with our protocol.
pub fn probe(host: &str, default_port: u16) -> Option<Peer> {
    let with_port = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:{default_port}")
    };
    let addr = with_port.to_socket_addrs().ok()?.next()?;
    let info: ServerInfo = get_json(&api(addr, "info")).ok()?;
    (info.protocol == PROTOCOL).then_some(Peer { info, addr })
}

/// `SHARE_PEERS` (`host[:port], ...`) for networks where broadcast doesn't reach.
fn configured_peers(cfg: &Config) -> Vec<Peer> {
    cfg.get("SHARE_PEERS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| probe(entry, port(cfg)))
        .collect()
}

/// Servers answering the LAN broadcast, plus the configured ones.
pub fn find_peers(cfg: &Config) -> Vec<Peer> {
    let found = discovery::discover(port(cfg), Duration::from_secs(1));
    let configured = configured_peers(cfg);
    found
        .into_iter()
        .chain(configured)
        .fold(Vec::new(), |mut acc, peer| {
            if !acc.iter().any(|p: &Peer| p.addr == peer.addr) {
                acc.push(peer);
            }
            acc
        })
}

pub fn list(addr: SocketAddr) -> Result<Vec<RemoteGame>, String> {
    get_json(&api(addr, "games"))
}

pub fn cover(addr: SocketAddr, id: &str) -> Option<Vec<u8>> {
    let response = get(&game_url(addr, id, "cover"), LIST_TIMEOUT).ok()?;
    read_body(response, MAX_BODY).ok()
}

/// Why `id` can't be copied into `games_dir`, if it can't: the game is
/// already here, installed or packaged.
pub fn conflict(games_dir: &Path, id: &str) -> Option<String> {
    if games_dir.join(id).exists() {
        return Some(format!("{id} is already installed"));
    }
    package::package_path(games_dir, id)
        .exists()
        .then(|| format!("{id} is already here as a package"))
}

fn check_free(games_dir: &Path, id: &str) -> Result<(), String> {
    conflict(games_dir, id).map_or(Ok(()), Err)
}

fn stream(addr: SocketAddr, id: &str) -> Result<impl Read, String> {
    Ok(get(&game_url(addr, id, "package"), STREAM_TIMEOUT)?.into_reader())
}

/// Copy a remote game into `games_dir/<id>/`, unpacked as it arrives.
pub fn install(
    addr: SocketAddr,
    id: &str,
    games_dir: &Path,
    progress: &Progress,
) -> Result<PathBuf, String> {
    check_free(games_dir, id)?;
    package::unpack_from(stream(addr, id)?, games_dir, Some(id), progress)
}

/// Writes everything read through it to `file` as well.
struct Tee<R, W> {
    inner: R,
    copy: W,
}

impl<R: Read, W: Write> Read for Tee<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.copy.write_all(&buf[..n])?;
        Ok(n)
    }
}

/// Copy a remote game as `games_dir/<id>.cartridge.tar`, verified while it
/// downloads (one pass) and renamed into place only once complete.
pub fn download(
    addr: SocketAddr,
    id: &str,
    games_dir: &Path,
    progress: &Progress,
) -> Result<PathBuf, String> {
    check_free(games_dir, id)?;
    let target = package::package_path(games_dir, id);
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let staging = games_dir.join(package::staging_name(&name));
    let result = (|| {
        let file = File::create(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
        let mut tee = Tee {
            inner: stream(addr, id)?,
            copy: BufWriter::new(file),
        };
        let manifest = package::verify_from(&mut tee, progress)?;
        if manifest.folder_name != id {
            return Err(format!(
                "Server sent {} instead of {id}",
                manifest.folder_name
            ));
        }
        // The tar reader stops at the end marker; keep the padding after it.
        io::copy(&mut tee, &mut io::sink()).map_err(|e| e.to_string())?;
        tee.copy.flush().map_err(|e| e.to_string())?;
        fs::rename(&staging, &target).map_err(|e| format!("{}: {e}", target.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result.map(|()| target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::app::GAME_DATA_DIR;
    use crate::core::models::{Game, GAME_JSON};
    use crate::core::share::server::tests::local_server;
    use crate::core::testutil::TempDir;

    #[test]
    fn a_body_past_the_limit_is_refused_not_read_whole() {
        let response = |body: &str| ureq::Response::new(200, "OK", body).unwrap();
        assert_eq!(read_body(response("abcd"), 4), Ok(b"abcd".to_vec()));
        assert!(read_body(response("abcde"), 4).is_err());
    }

    /// A server sharing `server/games` with one installed game, "My Game".
    fn sharing(dir: &TempDir) -> crate::core::share::server::Server {
        dir.write("server/games/My Game/data.bin", &[5u8; 40_000]);
        let game = Game::skeleton(&dir.path().join("server/games/My Game"));
        dir.write(
            &format!("server/games/My Game/{GAME_DATA_DIR}/{GAME_JSON}"),
            game.to_json().as_bytes(),
        );
        local_server(&dir.path().join("server/games"))
    }

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    #[test]
    fn probes_a_host() {
        let dir = TempDir::new();
        let server = sharing(&dir);
        let peer = probe(&format!("127.0.0.1:{}", server.port()), 1).unwrap();
        assert_eq!(peer.info.name, "test");
        assert!(probe("127.0.0.1:1", 1).is_none());
    }

    #[test]
    fn lists_remote_games() {
        let dir = TempDir::new();
        let server = sharing(&dir);
        let games = list(addr(server.port())).unwrap();
        assert_eq!(games[0].id, "My Game");
        assert!(games[0].installed);
    }

    #[test]
    fn install_unpacks_without_keeping_a_package() {
        let dir = TempDir::new();
        let server = sharing(&dir);
        let games_dir = dir.mkdir("client");
        let progress = Progress::default();
        let dest = install(addr(server.port()), "My Game", &games_dir, &progress).unwrap();
        assert_eq!(fs::read(dest.join("data.bin")).unwrap(), vec![5u8; 40_000]);
        assert!(progress.done() >= 40_000 && progress.done() == progress.total());
        assert_eq!(fs::read_dir(&games_dir).unwrap().count(), 1);
    }

    #[test]
    fn download_keeps_a_valid_package_that_can_be_served_again() {
        let dir = TempDir::new();
        let server = sharing(&dir);
        let games_dir = dir.mkdir("library");
        let package = download(
            addr(server.port()),
            "My Game",
            &games_dir,
            &Progress::default(),
        )
        .unwrap();
        assert_eq!(fs::read_dir(&games_dir).unwrap().count(), 1);
        let unpacked = dir.mkdir("unpacked");
        package::unpack(&package, &unpacked, &Progress::default()).unwrap();

        // The library machine serves the package as is.
        let relay = local_server(&games_dir);
        let games = list(addr(relay.port())).unwrap();
        assert!(!games[0].installed);
        let again = dir.mkdir("again");
        install(addr(relay.port()), "My Game", &again, &Progress::default()).unwrap();
    }

    #[test]
    fn refuses_games_already_here() {
        let dir = TempDir::new();
        let server = sharing(&dir);
        let games_dir = dir.mkdir("client");
        dir.mkdir("client/My Game");
        let err = download(
            addr(server.port()),
            "My Game",
            &games_dir,
            &Progress::default(),
        );
        assert!(err.unwrap_err().contains("already installed"));
        assert_eq!(fs::read_dir(&games_dir).unwrap().count(), 1);
    }

    #[test]
    fn missing_game_leaves_no_partial_file() {
        let dir = TempDir::new();
        let server = sharing(&dir);
        let games_dir = dir.mkdir("client");
        assert!(download(
            addr(server.port()),
            "Nope",
            &games_dir,
            &Progress::default()
        )
        .is_err());
        assert_eq!(fs::read_dir(&games_dir).unwrap().count(), 0);
    }
}
