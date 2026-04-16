//! Async network event handlers and display helpers.
//!
//! This module owns the three long-running per-connection tasks
//! (`handle_incoming_connection`, `handle_read_loop`, `handle_write_loop`)
//! and exposes `format_time` / `now_secs` as utilities used by both the
//! node layer and the UI.

use anyhow::Result;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::connection::{read_msg, write_msg, Connection, ConnectionMap};
use crate::event::AppEvent;
use crate::message::P2PMessage;
use crate::registry::{normalize_peer_addr, KnownPeer, RegistryRef};

// ---------------------------------------------------------------------------
// Time / display helpers
// ---------------------------------------------------------------------------

/// Format a Unix timestamp (seconds) as `HH:MM:SS` (UTC).
pub fn format_time(unix_secs: u64) -> String {
    let s = unix_secs % 86400;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Current Unix timestamp in seconds.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Incoming connection handler (server side)
// ---------------------------------------------------------------------------

/// Perform the server-side handshake for a freshly accepted TCP connection,
/// persist the peer to the registry, send back our own Handshake, then spawn
/// read / write tasks.
pub async fn handle_incoming_connection(
    socket: TcpStream,
    node_id: String,
    username: String,
    listen_addr: String,
    connections: ConnectionMap,
    registry: RegistryRef,
    event_tx: mpsc::UnboundedSender<AppEvent>,
) -> Result<()> {
    let (mut read_half, write_half) = socket.into_split();

    match read_msg(&mut read_half).await? {
        Some(P2PMessage::Handshake {
            node_id: peer_id,
            username: peer_username,
            listen_addr: peer_addr,
            ..
        }) => {
            info!("Handshake from {} ({}) @ {}", peer_id, peer_username, peer_addr);

            // Persist the connecting peer (normalise port before storing)
            registry.lock().await.upsert(KnownPeer {
                node_id: peer_id.clone(),
                username: peer_username.clone(),
                address: normalize_peer_addr(&peer_addr),
            });

            let (tx, rx) = mpsc::channel::<P2PMessage>(32);

            // Reply with our Handshake so the connector can update its registry
            tx.send(P2PMessage::new_handshake(
                node_id.clone(),
                username.clone(),
                listen_addr,
            ))
            .await?;

            tokio::spawn(handle_write_loop(
                write_half,
                rx,
                peer_id.clone(),
                connections.clone(),
            ));

            connections
                .lock()
                .await
                .insert(peer_id.clone(), Connection { tx });

            // Notify UI: peer connected
            let _ = event_tx.send(AppEvent::PeerConnected {
                node_id: peer_id.clone(),
                username: peer_username.clone(),
            });

            let peer_id_clone = peer_id.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    handle_read_loop(read_half, node_id, peer_id_clone, connections, registry, event_tx).await
                {
                    error!("Read-loop error for {}: {}", peer_id, e);
                }
            });
        }
        other => error!("Expected Handshake, got: {:?}", other),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-connection loops
// ---------------------------------------------------------------------------

/// Receive frames from a peer and forward events to the UI.
///
/// On the outbound path the first message is the Handshake reply; we use it
/// to update the registry and emit `PeerConnected`.  Subsequent messages are
/// Direct / System / Disconnect / Keepalive.
pub async fn handle_read_loop(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    _my_node_id: String,
    peer_id: String,
    connections: ConnectionMap,
    registry: RegistryRef,
    event_tx: mpsc::UnboundedSender<AppEvent>,
) -> Result<()> {
    loop {
        match read_msg(&mut reader).await {
            Ok(Some(msg)) => match msg {
                // Handshake reply (outbound-connection path only)
                P2PMessage::Handshake {
                    node_id: recv_id,
                    username: recv_username,
                    listen_addr: recv_addr,
                    ..
                } => {
                    info!("Handshake reply: {} ({}) @ {}", recv_id, recv_username, recv_addr);
                    registry.lock().await.upsert(KnownPeer {
                        node_id: recv_id.clone(),
                        username: recv_username.clone(),
                        address: normalize_peer_addr(&recv_addr),
                    });
                    let _ = event_tx.send(AppEvent::PeerConnected {
                        node_id: recv_id,
                        username: recv_username,
                    });
                }

                P2PMessage::Direct {
                    from,
                    to,
                    content,
                    timestamp,
                } => {
                    // Resolve sender's display name from registry
                    let from_name = {
                        let reg = registry.lock().await;
                        reg.get(&from)
                            .map(|p| p.username.clone())
                            .unwrap_or_else(|| from.clone())
                    };
                    let _ = event_tx.send(AppEvent::MessageReceived {
                        from_id: from,
                        from_name,
                        to,
                        content,
                        timestamp,
                    });
                }

                P2PMessage::System { content, .. } => {
                    let _ = event_tx.send(AppEvent::SystemNotice(content));
                }

                P2PMessage::Keepalive { .. } => {}

                P2PMessage::Disconnect { reason, .. } => {
                    info!("Peer {} disconnected: {}", peer_id, reason);
                    connections.lock().await.remove(&peer_id);
                    let _ = event_tx.send(AppEvent::PeerDisconnected {
                        node_id: peer_id.clone(),
                        reason,
                    });
                    break;
                }
            },

            Ok(None) => {
                info!("Peer {} closed connection", peer_id);
                connections.lock().await.remove(&peer_id);
                let _ = event_tx.send(AppEvent::PeerDisconnected {
                    node_id: peer_id.clone(),
                    reason: "connection closed".to_string(),
                });
                break;
            }

            Err(e) => {
                info!("Peer {} read error: {}", peer_id, e);
                connections.lock().await.remove(&peer_id);
                let _ = event_tx.send(AppEvent::PeerDisconnected {
                    node_id: peer_id.clone(),
                    reason: format!("read error: {}", e),
                });
                break;
            }
        }
    }
    Ok(())
}

/// Drain the MPSC channel and write frames to the TCP stream.
pub async fn handle_write_loop(
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    mut rx: mpsc::Receiver<P2PMessage>,
    peer_id: String,
    connections: ConnectionMap,
) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = write_msg(&mut writer, msg).await {
            error!("Write error for {}: {}", peer_id, e);
            connections.lock().await.remove(&peer_id);
            break;
        }
    }
}
