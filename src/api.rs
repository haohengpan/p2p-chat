//! Gateway API — unified interface between App and UI.
//!
//! Two channels:
//!   Command  (UI → App)  — user-initiated actions
//!   Notify   (App → UI)  — real-time push events + query results

use std::net::SocketAddr;

use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// NotifySender — shared by App and Node to push events to UI
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct NotifySender(mpsc::UnboundedSender<Notify>);

impl NotifySender {
    pub fn new(tx: mpsc::UnboundedSender<Notify>) -> Self {
        Self(tx)
    }

    pub fn emit(&self, event: Notify) {
        let _ = self.0.send(event);
    }
}

// ---------------------------------------------------------------------------
// Command  (UI → App)
// ---------------------------------------------------------------------------

pub enum Command {
    // -- Peer management --

    /// Connect to a peer. App resolves ip:port, bare ip, or node_id.
    Connect { addr: String },

    /// Disconnect from a peer.
    Disconnect { peer_id: String },

    // -- Messaging --

    /// Send a message in a conversation.
    /// `msg_id` is client-generated for optimistic UI updates.
    SendMessage { conv_id: String, msg_id: String, content: Content },

    /// Request chat history for a conversation.
    GetHistory { conv_id: String, before: Option<u64>, limit: u32 },

    // -- Queries --

    /// List connected peers.
    ListPeers,

    // -- Lifecycle --

    /// Graceful shutdown.
    Shutdown,
}

// ---------------------------------------------------------------------------
// Notify  (App → UI)
// ---------------------------------------------------------------------------

pub enum Notify {
    // -- Peer status --

    PeerOnline {
        peer_id: String,
        peer_name: String,
        addr: SocketAddr,
    },

    PeerOffline {
        peer_id: String,
    },

    // -- Message lifecycle --

    /// Incoming message from a peer.
    MessageReceived {
        conv_id: String,
        msg: MessageInfo,
    },

    /// Acknowledgement that a sent message was delivered or failed.
    MessageAck {
        msg_id: String,
        status: MessageStatus,
    },

    // -- Query results --

    PeerList {
        peers: Vec<PeerInfo>,
    },

    History {
        conv_id: String,
        messages: Vec<MessageInfo>,
    },

    // -- System --

    Notice {
        level: NoticeLevel,
        content: String,
    },
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    // Future: File, Image, etc.
}

#[derive(Debug, Clone)]
pub enum MessageStatus {
    /// Successfully sent to peer.
    Sent,
    /// Send failed.
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct MessageInfo {
    pub msg_id: String,
    pub from: String,
    pub content: Content,
    pub timestamp: u64,
    pub status: MessageStatus,
}

pub struct PeerInfo {
    pub peer_id: String,
    pub peer_name: String,
    pub addr: SocketAddr,
}

pub enum NoticeLevel {
    Info,
    Error,
}
