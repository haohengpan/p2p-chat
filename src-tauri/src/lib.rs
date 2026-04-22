pub mod api;
pub mod app;
pub mod net;
pub mod network;
pub mod node;

pub mod bridge;
pub mod commands;

pub const DEFAULT_PORTS: &[u16] = &[9000, 9001, 9002, 9003, 9004];

/// Try to find an available port. If `port` is non-zero, use it directly.
/// Otherwise probe DEFAULT_PORTS and return the first available one.
pub fn resolve_listen_addr(port: u16) -> anyhow::Result<std::net::SocketAddr> {
    use std::net::SocketAddr;

    if port != 0 {
        return Ok(SocketAddr::from(([0, 0, 0, 0], port)));
    }
    for &p in DEFAULT_PORTS {
        let addr = SocketAddr::from(([0, 0, 0, 0], p));
        if std::net::TcpListener::bind(addr).is_ok() {
            return Ok(addr);
        }
    }
    anyhow::bail!("No available port in {:?}", DEFAULT_PORTS)
}

/// Try to get the primary outgoing LAN IP via a non-sending UDP trick.
pub fn local_ip() -> String {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    if let Some(sock) = s {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(a) = sock.local_addr() {
                return a.ip().to_string();
            }
        }
    }
    "unknown".to_string()
}
