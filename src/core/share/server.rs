//! Read-only HTTP/1.1 server (GET only, one thread per connection):
//!
//!   GET /api/v1/info               ServerInfo
//!   GET /api/v1/games              [RemoteGame]
//!   GET /api/v1/games/{id}/cover   cover image
//!   GET /api/v1/games/{id}/package package tar, streamed
//!
//! Games are looked up by folder name only; a client path never reaches the
//! filesystem. Installed games are packed on the fly (no temporary file);
//! packaged-only games send their tar as is.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use serde::Serialize;

use super::{decode_segment, discovery, RemoteGame, ServerInfo};
use crate::core::models::{Game, ImageSlot};
use crate::core::package::{self, Progress};
use crate::core::scanner;

const MAX_HEAD: u64 = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a library scan answers requests: a peer browsing asks for the
/// list then every cover, and a scan reads every game.json and package.
const LIBRARY_TTL: Duration = Duration::from_secs(5);

/// What a request asks for.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    Info,
    Games,
    Cover(String),
    Package(String),
}

/// Parse a request line (`GET /path HTTP/1.1`) into a route.
fn route(request_line: &str) -> Result<Route, (u16, &'static str)> {
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Err((400, "Bad request"));
    };
    if method != "GET" {
        return Err((405, "Only GET is supported"));
    }
    let path = target.split('?').next().unwrap_or_default();
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    match segments.as_slice() {
        ["api", "v1", "info"] => Ok(Route::Info),
        ["api", "v1", "games"] => Ok(Route::Games),
        ["api", "v1", "games", id, "cover"] => decode_segment(id)
            .map(Route::Cover)
            .ok_or((400, "Bad game id")),
        ["api", "v1", "games", id, "package"] => decode_segment(id)
            .map(Route::Package)
            .ok_or((400, "Bad game id")),
        _ => Err((404, "Not found")),
    }
}

/// Shared state of a running server.
pub struct Shared {
    games_dir: PathBuf,
    info: ServerInfo,
    stop: AtomicBool,
    /// Packages being sent right now.
    pub sending: AtomicUsize,
    /// The last scan and when it was made.
    library: Mutex<Option<(Instant, Arc<Vec<Game>>)>>,
}

/// A running server; stops when dropped (and frees its port).
pub struct Server {
    shared: Arc<Shared>,
    addr: SocketAddr,
    threads: Vec<JoinHandle<()>>,
}

impl Server {
    /// Listen on `port` (TCP for HTTP, UDP for discovery) on every interface.
    pub fn start(games_dir: &Path, name: String, port: u16) -> io::Result<Server> {
        Server::start_on(games_dir, name, IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)
    }

    fn start_on(games_dir: &Path, name: String, ip: IpAddr, port: u16) -> io::Result<Server> {
        let listener = TcpListener::bind((ip, port))?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared::new(games_dir, ServerInfo::new(name, addr.port())));
        let accept_shared = Arc::clone(&shared);
        let mut threads = vec![std::thread::spawn(move || {
            accept_loop(listener, accept_shared)
        })];
        match UdpSocket::bind((ip, addr.port())) {
            Ok(socket) => {
                let udp_shared = Arc::clone(&shared);
                threads.push(std::thread::spawn(move || {
                    discovery::answer_loop(socket, udp_shared)
                }));
            }
            Err(e) => warn!("Discovery disabled (UDP port {}): {e}", addr.port()),
        }
        info!(
            "Sharing {} as \"{}\" on port {} (TCP + UDP)",
            games_dir.display(),
            shared.info.name,
            addr.port()
        );
        Ok(Server {
            shared,
            addr,
            threads,
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn games_dir(&self) -> &Path {
        &self.shared.games_dir
    }

    pub fn sending(&self) -> usize {
        self.shared.sending.load(Ordering::Relaxed)
    }

    /// Block until the process is killed (`--serve`).
    pub fn wait(self) {
        // Parks forever: the server only stops with the process.
        loop {
            std::thread::park();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        // Wake the blocking accept so its loop sees the flag; the UDP loop
        // wakes on its read timeout. Joining frees the ports for a restart.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.addr.port()));
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Shared {
    fn new(games_dir: &Path, info: ServerInfo) -> Shared {
        Shared {
            games_dir: games_dir.to_path_buf(),
            info,
            stop: AtomicBool::new(false),
            sending: AtomicUsize::new(0),
            library: Mutex::new(None),
        }
    }

    pub fn info(&self) -> &ServerInfo {
        &self.info
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn library(&self) -> Arc<Vec<Game>> {
        self.library_at(Instant::now())
    }

    /// The cached scan while fresh at `now`, else a new one. The lock is held
    /// while scanning, so simultaneous requests share one scan.
    fn library_at(&self, now: Instant) -> Arc<Vec<Game>> {
        let mut cache = self.library.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, games)) = cache.as_ref().filter(|(at, _)| now < *at + LIBRARY_TTL) {
            return Arc::clone(games);
        }
        let games = Arc::new(scanner::load_library(&self.games_dir));
        *cache = Some((now, Arc::clone(&games)));
        games
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stopped() {
            break;
        }
        let Ok(stream) = stream else { continue };
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            let peer = stream.peer_addr().ok();
            if let Err(e) = handle(stream, &shared) {
                debug!("Share connection {peer:?}: {e}");
            }
        });
    }
}

/// Read the request line, skip the headers (bounded), answer.
fn handle(stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?).take(MAX_HEAD);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut header = String::new();
    // Headers carry nothing we use; read them so the client isn't reset.
    while reader.read_line(&mut header)? > 2 {
        header.clear();
    }
    let mut out = BufWriter::new(stream);
    match route(&request_line) {
        Ok(route) => answer(route, shared, &mut out),
        Err((status, message)) => send_error(&mut out, status, message),
    }?;
    out.flush()
}

fn find(shared: &Shared, id: &str) -> Option<Game> {
    shared
        .library()
        .iter()
        .find(|g| g.folder_name == id)
        .cloned()
}

fn answer(route: Route, shared: &Shared, out: &mut impl Write) -> io::Result<()> {
    match route {
        Route::Info => send_json(out, shared.info()),
        Route::Games => {
            let games: Vec<RemoteGame> = shared.library().iter().map(RemoteGame::from).collect();
            send_json(out, &games)
        }
        Route::Cover(id) => match find(shared, &id).and_then(|g| cover_of(&g)) {
            Some((bytes, mime)) => send(out, 200, mime, &bytes),
            None => send_error(out, 404, "No such cover"),
        },
        Route::Package(id) => match find(shared, &id) {
            Some(game) => {
                shared.sending.fetch_add(1, Ordering::Relaxed);
                let result = send_package(out, &game);
                shared.sending.fetch_sub(1, Ordering::Relaxed);
                info!(
                    "Sent {}{}",
                    game.title,
                    result
                        .as_ref()
                        .err()
                        .map_or(String::new(), |e| format!(" (stopped: {e})"))
                );
                result
            }
            None => send_error(out, 404, "No such game"),
        },
    }
}

fn mime_of(name: &str) -> &'static str {
    match Path::new(name).extension().and_then(|e| e.to_str()) {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

fn cover_of(game: &Game) -> Option<(Vec<u8>, &'static str)> {
    let name = game.images.get(ImageSlot::Cover)?;
    let bytes = match game.package().filter(|_| !game.is_installed()) {
        Some(package) => package::read_data_file(package, name)?,
        None => std::fs::read(game.image_path(ImageSlot::Cover)?).ok()?,
    };
    Some((bytes, mime_of(name)))
}

/// The folder when installed (it is what plays), else the package file.
fn send_package(out: &mut impl Write, game: &Game) -> io::Result<()> {
    if let Some(package) = game.package().filter(|_| !game.is_installed()) {
        let mut file = File::open(package)?;
        let length = file.metadata()?.len();
        write_head(out, 200, "application/x-tar", Some(length))?;
        io::copy(&mut file, out)?;
        return Ok(());
    }
    write_head(out, 200, "application/x-tar", None)?;
    package::pack_to(game, &mut *out, &Progress::default())
        .map(|_| ())
        .map_err(io::Error::other)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    }
}

/// Status line and headers. Without a length the body ends when the
/// connection closes; the package's trailing checksums catch a cut stream.
fn write_head(
    out: &mut impl Write,
    status: u16,
    mime: &str,
    length: Option<u64>,
) -> io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {status} {}\r\nContent-Type: {mime}\r\n",
        reason(status)
    )?;
    if let Some(length) = length {
        write!(out, "Content-Length: {length}\r\n")?;
    }
    write!(out, "Connection: close\r\n\r\n")
}

fn send(out: &mut impl Write, status: u16, mime: &str, body: &[u8]) -> io::Result<()> {
    write_head(out, status, mime, Some(body.len() as u64))?;
    out.write_all(body)
}

fn send_json<T: Serialize>(out: &mut impl Write, value: &T) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    send(out, 200, "application/json", &body)
}

fn send_error(out: &mut impl Write, status: u16, message: &str) -> io::Result<()> {
    let body = serde_json::json!({ "error": message }).to_string();
    send(out, status, "application/json", body.as_bytes())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::core::app::data_dir_name;
    use crate::core::models::GAME_JSON;
    use crate::core::share::encode_segment;
    use crate::core::testutil::TempDir;

    #[test]
    fn the_library_is_scanned_once_per_burst_of_requests() {
        let dir = TempDir::new();
        dir.mkdir("games/One");
        let shared = Shared::new(&dir.path().join("games"), ServerInfo::new("t".into(), 1));
        let start = Instant::now();
        assert_eq!(shared.library_at(start).len(), 1);

        dir.mkdir("games/Two");
        assert_eq!(shared.library_at(start + Duration::from_secs(1)).len(), 1);
        assert_eq!(shared.library_at(start + LIBRARY_TTL).len(), 2);
    }

    #[test]
    fn routes_only_known_get_paths() {
        assert_eq!(route("GET /api/v1/games HTTP/1.1"), Ok(Route::Games));
        assert_eq!(
            route("GET /api/v1/games/My%20Game/package?x=1 HTTP/1.1"),
            Ok(Route::Package("My Game".into()))
        );
        assert_eq!(route("POST /api/v1/games HTTP/1.1").unwrap_err().0, 405);
        assert_eq!(route("GET /etc/passwd HTTP/1.1").unwrap_err().0, 404);
        assert_eq!(route("").unwrap_err().0, 400);
    }

    /// Loopback only, so tests never open a port on the network.
    pub fn local_server(games_dir: &Path) -> Server {
        Server::start_on(games_dir, "test".into(), IpAddr::V4(Ipv4Addr::LOCALHOST), 0).unwrap()
    }

    fn get(port: u16, path: &str) -> ureq::Response {
        ureq::get(&format!("http://127.0.0.1:{port}{path}"))
            .call()
            .unwrap()
    }

    #[test]
    fn lists_and_streams_installed_games() {
        let dir = TempDir::new();
        dir.write("games/My Game/data.bin", &[3u8; 70_000]);
        let mut game = Game::skeleton(&dir.path().join("games/My Game"));
        game.year = Some(1999);
        dir.write(
            &format!("games/My Game/{}/{GAME_JSON}", data_dir_name()),
            game.to_json().as_bytes(),
        );
        let server = local_server(&dir.path().join("games"));

        let games: Vec<RemoteGame> =
            serde_json::from_reader(get(server.port(), "/api/v1/games").into_reader()).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(
            (games[0].id.as_str(), games[0].year),
            ("My Game", Some(1999))
        );

        let path = format!("/api/v1/games/{}/package", encode_segment("My Game"));
        let target = dir.mkdir("client");
        let dest = package::unpack_from(
            get(server.port(), &path).into_reader(),
            &target,
            &Progress::default(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(dest.join("data.bin")).unwrap(),
            vec![3u8; 70_000]
        );
        assert_eq!(server.sending(), 0);
    }

    #[test]
    fn unknown_games_are_404() {
        let dir = TempDir::new();
        let server = local_server(dir.path());
        let err = ureq::get(&format!(
            "http://127.0.0.1:{}/api/v1/games/x/package",
            server.port()
        ))
        .call()
        .unwrap_err();
        assert!(matches!(err, ureq::Error::Status(404, _)));
    }

    #[test]
    fn restarts_on_the_same_port() {
        let dir = TempDir::new();
        let port = local_server(dir.path()).port();
        let again = Server::start_on(
            dir.path(),
            "x".into(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
        );
        assert_eq!(again.unwrap().port(), port);
    }
}
