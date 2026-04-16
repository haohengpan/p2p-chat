//! Peer registry – persists known node info across sessions.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

// ---------------------------------------------------------------------------
// Port constants & address normalisation
// ---------------------------------------------------------------------------

/// Default listening ports tried in order at startup (when `--port` is 0).
/// Peers on one of these ports are stored by IP only in the registry so that
/// `connect <node_id>` re-uses IP discovery automatically on the next run.
pub const DEFAULT_PORTS: [u16; 5] = [9000, 9001, 9002, 9003, 9004];

/// Normalise a peer's self-reported `listen_addr` for registry storage.
///
/// * Port ∈ `DEFAULT_PORTS` → store just the IP string (e.g. `"192.168.1.5"`)
/// * Any other port → store `"ip:port"` verbatim
///
/// This way a peer on a default port can always be re-discovered with
/// `connect <ip>`, while a peer on a custom port keeps the exact address.
pub fn normalize_peer_addr(addr_str: &str) -> String {
    if let Ok(sock) = addr_str.parse::<SocketAddr>() {
        if DEFAULT_PORTS.contains(&sock.port()) {
            return sock.ip().to_string();
        }
    }
    addr_str.to_string()
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A node that we have connected to at least once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownPeer {
    pub node_id: String,
    pub username: String,
    /// Stored listening address: `"ip:port"` for non-default ports, or just
    /// `"ip"` when the peer is on one of the `DEFAULT_PORTS` (re-discovered
    /// automatically via IP scan on next connect).
    pub address: String,
}

// ---------------------------------------------------------------------------
// PeerRegistry
// ---------------------------------------------------------------------------

/// In-memory registry backed by a JSON file on disk.
///
/// Every `upsert` flushes immediately so no data is lost on crash.
pub struct PeerRegistry {
    path: PathBuf,
    peers: HashMap<String, KnownPeer>,
}

impl PeerRegistry {
    /// Load from `path`.  Returns an empty registry if the file is absent or
    /// cannot be parsed.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let peers = Self::read_from_disk(&path).unwrap_or_default();
        Self { path, peers }
    }

    fn read_from_disk(path: &Path) -> Option<HashMap<String, KnownPeer>> {
        let content = std::fs::read_to_string(path).ok()?;
        let list: Vec<KnownPeer> = serde_json::from_str(&content).ok()?;
        Some(list.into_iter().map(|p| (p.node_id.clone(), p)).collect())
    }

    /// Insert or update a peer then flush to disk.
    /// TODO： node_id is the unique key , so we should not allow two peers with the same node_id.
    pub fn upsert(&mut self, peer: KnownPeer) {
        self.peers.insert(peer.node_id.clone(), peer);
        self.flush();
    }

    pub fn get(&self, node_id: &str) -> Option<&KnownPeer> {
        self.peers.get(node_id)
    }

    /// Return all peers sorted by node_id.
    pub fn all(&self) -> Vec<&KnownPeer> {
        let mut v: Vec<&KnownPeer> = self.peers.values().collect();
        v.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        v
    }

    /// Write current state to disk.
    pub fn flush(&self) {
        let list: Vec<&KnownPeer> = self.peers.values().collect();
        match serde_json::to_string_pretty(&list) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.path, json) {
                    warn!("Failed to write registry to {:?}: {}", self.path, e);
                }
            }
            Err(e) => warn!("Failed to serialize registry: {}", e),
        }
    }
}

pub type RegistryRef = Arc<Mutex<PeerRegistry>>;

// ---------------------------------------------------------------------------
// Peer arg parser
// ---------------------------------------------------------------------------

/// Parse `"ip:port:node_id"` or `"[ipv6]:port:node_id"` into
/// `(SocketAddr, node_id)`.  The `node_id` is always the segment after
/// the **last** colon.
pub fn parse_peer_arg(arg: &str) -> Result<(SocketAddr, String)> {
    let last = arg.rfind(':').ok_or_else(|| {
        anyhow::anyhow!("Invalid format. Expected: ip:port:node_id or [ipv6]:port:node_id")
    })?;
    let (addr_part, rest) = arg.split_at(last);
    let node_id = &rest[1..]; // skip ':'
    if node_id.is_empty() {
        return Err(anyhow::anyhow!("node_id cannot be empty in '{}'", arg));
    }
    let addr = addr_part
        .parse::<SocketAddr>()
        .map_err(|e| anyhow::anyhow!("Invalid address '{}': {}", addr_part, e))?;
    Ok((addr, node_id.to_string()))
}
