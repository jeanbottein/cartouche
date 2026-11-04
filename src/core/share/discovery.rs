//! UDP broadcast discovery: a client broadcasts a query on the share port;
//! every server answers with its `ServerInfo` (name, HTTP port, version).

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::warn;

use super::server::Shared;
use super::{ServerInfo, PROTOCOL};

const QUERY: &[u8] = b"CARTRIDGE?";

/// Server side: answer queries until the server stops.
pub fn answer_loop(socket: UdpSocket, shared: Arc<Shared>) {
    let Ok(reply) = serde_json::to_vec(shared.info()) else {
        return;
    };
    // The timeout lets the loop notice a stop.
    if let Err(e) = socket.set_read_timeout(Some(Duration::from_secs(1))) {
        warn!("Discovery disabled: {e}");
        return;
    }
    let mut buf = [0u8; 64];
    while !shared.stopped() {
        if let Ok((n, from)) = socket.recv_from(&mut buf) {
            if &buf[..n] == QUERY {
                let _ = socket.send_to(&reply, from);
            }
        }
    }
}

/// A server that answered: its info and the address to reach its HTTP API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub info: ServerInfo,
    pub addr: SocketAddr,
}

/// Broadcast a query on the LAN and collect answers for `wait`.
pub fn discover(port: u16, wait: Duration) -> Vec<Peer> {
    discover_at(SocketAddr::from((Ipv4Addr::BROADCAST, port)), wait)
}

fn parse_reply(bytes: &[u8], from: SocketAddr) -> Option<Peer> {
    let info: ServerInfo = serde_json::from_slice(bytes).ok()?;
    (info.protocol == PROTOCOL).then(|| Peer {
        addr: SocketAddr::new(from.ip(), info.port),
        info,
    })
}

/// Send one query to `target` (a broadcast or a single host) and collect answers.
pub fn discover_at(target: SocketAddr, wait: Duration) -> Vec<Peer> {
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(socket) => socket,
        Err(e) => {
            warn!("Discovery failed: {e}");
            return Vec::new();
        }
    };
    if let Err(e) = socket
        .set_broadcast(true)
        .and_then(|()| socket.send_to(QUERY, target).map(|_| ()))
    {
        warn!("Discovery failed: {e}");
        return Vec::new();
    }
    let deadline = Instant::now() + wait;
    let mut peers: Vec<Peer> = Vec::new();
    let mut buf = [0u8; 2048];
    // A loop: answers arrive one by one until the deadline.
    while let Some(left) = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
    {
        let _ = socket.set_read_timeout(Some(left));
        let Ok((n, from)) = socket.recv_from(&mut buf) else {
            break;
        };
        if let Some(peer) = parse_reply(&buf[..n], from) {
            if !peers.iter().any(|p| p.addr == peer.addr) {
                peers.push(peer);
            }
        }
    }
    peers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::share::server::tests::local_server;
    use crate::core::testutil::TempDir;

    #[test]
    fn finds_a_server_and_its_http_port() {
        let dir = TempDir::new();
        let server = local_server(dir.path());
        let target = SocketAddr::from((Ipv4Addr::LOCALHOST, server.port()));
        let peers = discover_at(target, Duration::from_millis(500));
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].info.name, "test");
        assert_eq!(peers[0].addr.port(), server.port());
    }

    #[test]
    fn ignores_other_protocols_and_junk() {
        let from = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
        let mut info = ServerInfo::new("x".into(), 9);
        assert!(parse_reply(&serde_json::to_vec(&info).unwrap(), from).is_some());
        info.protocol = PROTOCOL + 1;
        assert!(parse_reply(&serde_json::to_vec(&info).unwrap(), from).is_none());
        assert!(parse_reply(b"junk", from).is_none());
    }
}
