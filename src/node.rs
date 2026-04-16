//! P2PNode – the core state machine.
//!
//! Owns the connection map and peer registry, drives the TCP accept loop,
//! and processes `NodeCommand`s sent from the UI layer.  All user-visible
//! output is emitted through `event_tx` rather than written to stdout.

use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use crate::connection::{read_msg, write_msg, Connection, ConnectionMap};
use crate::event::{AppEvent, NodeCommand};
use crate::message::P2PMessage;
use crate::network::{
    handle_incoming_connection, handle_read_loop, handle_write_loop, now_secs,
};
use crate::registry::{
    normalize_peer_addr, KnownPeer, PeerRegistry, RegistryRef, parse_peer_arg, DEFAULT_PORTS,
};

// ---------------------------------------------------------------------------
// Port binding  (lives here because it is P2P-node policy, not app wiring)
// ---------------------------------------------------------------------------

/// Bind a TCP listener using the node's port strategy:
///
/// 1. `requested_port != 0` → bind exactly that port (error if unavailable).
/// 2. `requested_port == 0` → try `DEFAULT_PORTS` [9000-9004] in order;
///    first free one wins.  All taken → error, user must pass `--port <N>`.
pub async fn bind_listener(requested_port: u16) -> Result<tokio::net::TcpListener> {
    if requested_port != 0 {
        let l = tokio::net::TcpListener::bind(("0.0.0.0", requested_port))
            .await
            .map_err(|e| anyhow::anyhow!("Cannot bind to port {}: {}", requested_port, e))?;
        info!("Bound to requested port {}", requested_port);
        return Ok(l);
    }

    for &port in DEFAULT_PORTS.iter() {
        match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => {
                info!("Bound to default port {}", port);
                return Ok(l);
            }
            Err(e) => warn!("Default port {} unavailable: {}", port, e),
        }
    }

    Err(anyhow::anyhow!(
        "All default ports {:?} are in use. Start with --port <N> to specify a free port.",
        DEFAULT_PORTS
    ))
}

// ---------------------------------------------------------------------------
// Struct
// ---------------------------------------------------------------------------

pub struct P2PNode {
    pub node_id: String,
    pub username: String,
    pub listening_addr: SocketAddr,
    /// Peer addresses supplied via `--peers` at startup.
    pub initial_peers: Vec<String>,
    /// Live TCP connections keyed by peer `node_id`.
    pub connections: ConnectionMap,
    /// Persistent store of known peer info.
    pub registry: RegistryRef,
    /// Channel for sending events to the UI.
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

// ---------------------------------------------------------------------------
// Construction & startup
// ---------------------------------------------------------------------------

impl P2PNode {
    pub fn new(
        node_id: String,
        username: String,
        addr: SocketAddr,
        initial_peers: Vec<String>,
        peers_file: &str,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        info!("Creating node '{}' at {}", node_id, addr);
        Ok(Self {
            node_id,
            username,
            listening_addr: addr,
            initial_peers,
            connections: Arc::new(Mutex::new(HashMap::new())),
            registry: Arc::new(Mutex::new(PeerRegistry::load(peers_file))),
            event_tx,
        })
    }

    /// Convenience: send an event without unwrapping the send result.
    fn emit(&self, ev: AppEvent) {
        let _ = self.event_tx.send(ev);
    }

    /// Start the node using an already-bound `listener`.
    ///
    /// Runs `on_start` (accept loop + initial peers), then processes
    /// `NodeCommand`s until `Quit` is received or the channel closes.
    pub async fn start(
        self,
        mut cmd_rx: mpsc::UnboundedReceiver<NodeCommand>,
        listener: tokio::net::TcpListener,
    ) -> Result<()> {
        self.on_start(listener).await;

        while let Some(cmd) = cmd_rx.recv().await {
            if self.handle_command(cmd).await {
                break;
            }
        }

        info!("Node shut down");
        Ok(())
    }

    /// Initialise: show registry count, start accept loop, connect initial peers.
    async fn on_start(&self, listener: tokio::net::TcpListener) {
        info!("Listening on {}", self.listening_addr);

        {
            let n = self.registry.lock().await.all().len();
            if n > 0 {
                self.emit(AppEvent::CommandOutput(format!(
                    "  Loaded {} known peer(s) from registry. Use 'peers' to list.",
                    n
                )));
            }
        }

        self.spawn_accept_loop(listener);

        for peer_info in self.initial_peers.clone() {
            match parse_peer_arg(&peer_info) {
                Ok((addr, _)) => {
                    // peer_id is learned from the handshake; ignore the arg
                    if let Err(e) = self.connect_to_addr(&addr.to_string()).await {
                        error!("Failed to connect to '{}': {}", peer_info, e);
                        self.emit(AppEvent::SystemNotice(format!(
                            "Failed to connect to '{}': {}",
                            peer_info, e
                        )));
                    }
                }
                Err(e) => {
                    error!("Invalid peer arg '{}': {}", peer_info, e);
                    self.emit(AppEvent::SystemNotice(format!(
                        "Invalid peer argument '{}': {}",
                        peer_info, e
                    )));
                }
            }
        }
    }

    /// Process one command.  Returns `true` if the node should shut down.
    async fn handle_command(&self, cmd: NodeCommand) -> bool {
        match cmd {
            NodeCommand::Quit => {
                info!("Quit received — shutting down");
                self.shutdown().await;
                self.emit(AppEvent::NodeShutdown);
                true
            }

            NodeCommand::SendMessage { to, content } => {
                if let Err(e) = self.send_message(&to, content).await {
                    error!("send_message error: {}", e);
                }
                false
            }

            NodeCommand::BroadcastMessage { content } => {
                if let Err(e) = self.broadcast_message(content).await {
                    error!("broadcast_message error: {}", e);
                }
                false
            }

            NodeCommand::Connect { addr } => {
                if let Err(e) = self.connect_to_addr(&addr).await {
                    self.emit(AppEvent::SystemNotice(format!(
                        "Connection to '{}' failed: {}",
                        addr, e
                    )));
                }
                false
            }

            NodeCommand::ConnectById { node_id } => {
                let addr = self
                    .registry
                    .lock()
                    .await
                    .get(&node_id)
                    .map(|p| p.address.clone());
                match addr {
                    Some(addr_str) => {
                        if let Err(e) = self.connect_by_addr_or_ip(&addr_str).await {
                            self.emit(AppEvent::SystemNotice(format!(
                                "Connection to '{}' failed: {}",
                                node_id, e
                            )));
                        }
                    }
                    None => self.emit(AppEvent::SystemNotice(format!(
                        "Unknown peer '{}'. Use 'connect <ip>' or 'connect <ip:port>' first.",
                        node_id
                    ))),
                }
                false
            }

            NodeCommand::ConnectByIp { ip } => {
                if let Err(e) = self.connect_by_ip(&ip).await {
                    self.emit(AppEvent::SystemNotice(format!("IP connect failed: {}", e)));
                }
                false
            }

            NodeCommand::Chat(Some(node_id)) => {
                let online = self.connections.lock().await.contains_key(&node_id);
                if !online {
                    let addr = self
                        .registry
                        .lock()
                        .await
                        .get(&node_id)
                        .map(|p| p.address.clone());
                    if let Some(addr_str) = addr {
                        if let Err(e) = self.connect_by_addr_or_ip(&addr_str).await {
                            self.emit(AppEvent::SystemNotice(format!(
                                "Auto-connect to '{}' failed: {}",
                                node_id, e
                            )));
                        }
                    }
                }
                false
            }

            NodeCommand::Chat(None) => false,

            NodeCommand::ListConnected => {
                let conns = self.connections.lock().await;
                if conns.is_empty() {
                    self.emit(AppEvent::CommandOutput("  No connected peers".to_string()));
                } else {
                    self.emit(AppEvent::CommandOutput("Connected peers:".to_string()));
                    for id in conns.keys() {
                        self.emit(AppEvent::CommandOutput(format!("  - {}", id)));
                    }
                }
                false
            }

            NodeCommand::ListPeers => {
                let reg = self.registry.lock().await;
                let known = reg.all();
                if known.is_empty() {
                    self.emit(AppEvent::CommandOutput(
                        "  No known peers in registry".to_string(),
                    ));
                } else {
                    self.emit(AppEvent::CommandOutput("Known peers:".to_string()));
                    let conns = self.connections.lock().await;
                    for p in known {
                        let status = if conns.contains_key(&p.node_id) {
                            "online"
                        } else {
                            "offline"
                        };
                        self.emit(AppEvent::CommandOutput(format!(
                            "  - {} ({}) @ {}  [{}]",
                            p.node_id, p.username, p.address, status
                        )));
                    }
                }
                false
            }
        }
    }

    /// Notify all peers of shutdown, wait for flush, then flush the registry.
    async fn shutdown(&self) {
        {
            let conns = self.connections.lock().await;
            for (peer_id, conn) in conns.iter() {
                let _ = conn
                    .tx
                    .send(P2PMessage::new_disconnect("Node shutting down".into()))
                    .await;
                info!("Sent disconnect to {}", peer_id);
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        self.registry.lock().await.flush();
    }

    /// Spawn the background TCP accept loop.
    fn spawn_accept_loop(&self, listener: tokio::net::TcpListener) {
        let connections = self.connections.clone();
        let registry = self.registry.clone();
        let node_id = self.node_id.clone();
        let username = self.username.clone();
        let listen_addr = self.listening_addr.to_string();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        info!("Incoming connection from {}", addr);
                        let (c, r, n, u, la, et) = (
                            connections.clone(),
                            registry.clone(),
                            node_id.clone(),
                            username.clone(),
                            listen_addr.clone(),
                            event_tx.clone(),
                        );
                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_incoming_connection(socket, n, u, la, c, r, et).await
                            {
                                error!("Incoming connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {}, retrying...", e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Connection management
// ---------------------------------------------------------------------------

impl P2PNode {
    /// Probe all DEFAULT_PORTS on `ip` concurrently (200 ms timeout each).
    /// The first port that accepts a TCP connection is used; the peer identity
    /// is obtained from the Handshake reply.
    async fn connect_by_ip(&self, ip: &str) -> Result<()> {
        let (found_tx, mut found_rx) =
            tokio::sync::mpsc::channel::<(TcpStream, SocketAddr)>(DEFAULT_PORTS.len());

        for &port in DEFAULT_PORTS.iter() {
            let tx = found_tx.clone();
            let ip_str = ip.to_string();
            tokio::spawn(async move {
                let addr_str = format!("{}:{}", ip_str, port);
                let addr: SocketAddr = match addr_str.parse() {
                    Ok(a) => a,
                    Err(_) => return,
                };
                if let Ok(Ok(stream)) = tokio::time::timeout(
                    std::time::Duration::from_millis(200),
                    TcpStream::connect(&addr),
                )
                .await
                {
                    let _ = tx.send((stream, addr)).await;
                }
            });
        }
        drop(found_tx);

        match found_rx.recv().await {
            Some((stream, addr)) => {
                info!("IP scan found peer at {}", addr);
                self.connect_with_stream(stream, addr).await
            }
            None => {
                self.emit(AppEvent::SystemNotice(format!(
                    "No p2p-chat instance found at {} (tried ports {:?})",
                    ip, DEFAULT_PORTS
                )));
                Ok(())
            }
        }
    }

    /// Connect to an explicit `ip:port` address; node-id is obtained from the
    /// peer's Handshake reply (no need to know it in advance).
    async fn connect_to_addr(&self, addr_str: &str) -> Result<()> {
        let addr: SocketAddr = addr_str
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid address '{}': {}", addr_str, e))?;
        let stream = TcpStream::connect(&addr).await?;
        self.connect_with_stream(stream, addr).await
    }

    /// Route a stored registry address to the appropriate connect method.
    ///
    /// * Full `ip:port` → `connect_to_addr` (direct TCP dial)
    /// * Plain IP (default-port peer) → `connect_by_ip` (concurrent port probe)
    async fn connect_by_addr_or_ip(&self, addr_str: &str) -> Result<()> {
        if addr_str.parse::<SocketAddr>().is_ok() {
            self.connect_to_addr(addr_str).await
        } else {
            self.connect_by_ip(addr_str).await
        }
    }

    /// Complete a connection whose `TcpStream` has already been opened
    /// (used when the peer-id was not known in advance, e.g. IP discovery).
    ///
    /// Sends our Handshake, reads the peer's Handshake reply to learn their
    /// node-id, then registers the connection and spawns the I/O loops.
    async fn connect_with_stream(&self, socket: TcpStream, addr: SocketAddr) -> Result<()> {
        let (mut read_half, mut write_half) = socket.into_split();

        write_msg(
            &mut write_half,
            P2PMessage::new_handshake(
                self.node_id.clone(),
                self.username.clone(),
                self.listening_addr.to_string(),
            ),
        )
        .await?;

        let reply = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            read_msg(&mut read_half),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Handshake timeout from {}", addr))?
        .map_err(|e| anyhow::anyhow!("Read error during handshake from {}: {}", addr, e))?;

        match reply {
            Some(P2PMessage::Handshake {
                node_id: peer_id,
                username,
                listen_addr,
                ..
            }) => {
                info!(
                    "Connected to peer: '{}' ({}) @ {}",
                    peer_id, username, listen_addr
                );

                self.registry.lock().await.upsert(KnownPeer {
                    node_id: peer_id.clone(),
                    username: username.clone(),
                    address: normalize_peer_addr(&listen_addr),
                });

                let (tx, rx) = mpsc::channel::<P2PMessage>(32);
                self.connections
                    .lock()
                    .await
                    .insert(peer_id.clone(), Connection { tx });

                tokio::spawn(handle_write_loop(
                    write_half,
                    rx,
                    peer_id.clone(),
                    self.connections.clone(),
                ));
                tokio::spawn(handle_read_loop(
                    read_half,
                    self.node_id.clone(),
                    peer_id.clone(),
                    self.connections.clone(),
                    self.registry.clone(),
                    self.event_tx.clone(),
                ));

                self.emit(AppEvent::PeerConnected {
                    node_id: peer_id,
                    username,
                });
                Ok(())
            }
            _ => Err(anyhow::anyhow!(
                "Expected Handshake from {}, got unexpected message or EOF",
                addr
            )),
        }
    }

    /// Return the MPSC sender for `peer_id`, auto-connecting via the registry
    /// if the peer is known but currently offline.
    async fn get_or_connect(&self, peer_id: &str) -> Result<Option<mpsc::Sender<P2PMessage>>> {
        {
            let conns = self.connections.lock().await;
            if let Some(conn) = conns.get(peer_id) {
                return Ok(Some(conn.tx.clone()));
            }
        }

        let addr_str = self
            .registry
            .lock()
            .await
            .get(peer_id)
            .map(|p| p.address.clone());

        match addr_str {
            Some(addr_str) => {
                self.emit(AppEvent::SystemNotice(format!(
                    "'{}' not connected – reconnecting…",
                    peer_id
                )));
                self.connect_by_addr_or_ip(&addr_str).await?;
                Ok(self
                    .connections
                    .lock()
                    .await
                    .get(peer_id)
                    .map(|c| c.tx.clone()))
            }
            None => {
                self.emit(AppEvent::SystemNotice(format!(
                    "Peer '{}' is unknown. Use 'connect <ip>' or 'connect <ip:port>' first.",
                    peer_id
                )));
                Ok(None)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Messaging
// ---------------------------------------------------------------------------

impl P2PNode {
    /// Send a direct message to `to`, auto-connecting if needed.
    pub async fn send_message(&self, to: &str, content: String) -> Result<()> {
        let tx = match self.get_or_connect(to).await? {
            Some(tx) => tx,
            None => return Ok(()),
        };
        let ts = now_secs();
        let msg = P2PMessage::new_direct(self.node_id.clone(), Some(to.to_string()), content.clone());

        if tx.send(msg).await.is_err() {
            self.connections.lock().await.remove(to);
            self.emit(AppEvent::PeerDisconnected {
                node_id: to.to_string(),
                reason: "send failed — connection lost".to_string(),
            });
        } else {
            self.emit(AppEvent::MessageSent {
                to: Some(to.to_string()),
                content,
                timestamp: ts,
                ok_count: 1,
                total: 1,
                our_name: self.username.clone(),
                our_id: self.node_id.clone(),
            });
        }
        Ok(())
    }

    /// Broadcast a message to every currently connected peer.
    pub async fn broadcast_message(&self, content: String) -> Result<()> {
        let senders: Vec<(String, mpsc::Sender<P2PMessage>)> = {
            let conns = self.connections.lock().await;
            conns
                .iter()
                .map(|(id, c)| (id.clone(), c.tx.clone()))
                .collect()
        };
        let total = senders.len();
        let ts = now_secs();
        let msg = P2PMessage::new_direct(self.node_id.clone(), None, content.clone());
        let mut failed = Vec::new();
        let mut ok = 0usize;

        for (id, tx) in senders {
            if tx.send(msg.clone()).await.is_err() {
                error!("Broadcast failed for '{}': connection lost", id);
                failed.push(id);
            } else {
                ok += 1;
            }
        }

        if !failed.is_empty() {
            let mut conns = self.connections.lock().await;
            for id in &failed {
                conns.remove(id);
                warn!("Removed dead connection: {}", id);
                self.emit(AppEvent::PeerDisconnected {
                    node_id: id.clone(),
                    reason: "connection lost during broadcast".to_string(),
                });
            }
        }

        self.emit(AppEvent::MessageSent {
            to: None,
            content,
            timestamp: ts,
            ok_count: ok,
            total,
            our_name: self.username.clone(),
            our_id: self.node_id.clone(),
        });

        Ok(())
    }
}
