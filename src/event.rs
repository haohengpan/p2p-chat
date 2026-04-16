//! Event types for UI ↔ Node communication.
//!
//! `AppEvent`  – emitted by the node/network layer, consumed by the UI.
//! `NodeCommand` – emitted by the UI, consumed by the node.
//! `DisplayLine` / `LineKind` – unit of display in the message pane.

// ---------------------------------------------------------------------------
// AppEvent
// ---------------------------------------------------------------------------

/// Everything the node/network layer can tell the UI.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// A direct or broadcast message arrived from a remote peer.
    MessageReceived {
        from_id: String,
        from_name: String,
        /// `None` means broadcast.
        to: Option<String>,
        content: String,
        timestamp: u64,
    },
    /// A message we sent (for echo display in the message pane).
    MessageSent {
        /// `None` means broadcast.
        to: Option<String>,
        content: String,
        timestamp: u64,
        ok_count: usize,
        total: usize,
        our_name: String,
        our_id: String,
    },
    /// A peer connected (inbound or after successful outbound handshake).
    PeerConnected { node_id: String, username: String },
    /// A peer disconnected.
    PeerDisconnected { node_id: String, reason: String },
    /// General informational notice (no special formatting needed).
    SystemNotice(String),
    /// One line of output from a command (list, peers, help …).
    CommandOutput(String),
    /// The node finished its graceful-shutdown sequence.
    NodeShutdown,
}

// ---------------------------------------------------------------------------
// NodeCommand
// ---------------------------------------------------------------------------

/// Commands the UI sends to the node's command loop.
#[derive(Debug)]
pub enum NodeCommand {
    SendMessage { to: String, content: String },
    BroadcastMessage { content: String },
    /// Direct connect by explicit `ip:port`; node-id is obtained from the handshake.
    Connect { addr: String },
    /// Reconnect to a peer already known in the registry.
    ConnectById { node_id: String },
    /// Connect by IP only — probe DEFAULT_PORTS concurrently to find the instance.
    ConnectByIp { ip: String },
    /// `Some(node_id)` → enter chat with that peer (auto-connect if needed).
    /// `None` → leave chat mode (node side no-op, UI handles state).
    Chat(Option<String>),
    ListPeers,
    ListConnected,
    Quit,
}

// ---------------------------------------------------------------------------
// DisplayLine
// ---------------------------------------------------------------------------

/// Visual category of a line in the message pane.
#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    Incoming,
    Outgoing,
    System,
}

/// A single rendered line in the message pane.
#[derive(Debug, Clone)]
pub struct DisplayLine {
    pub kind: LineKind,
    pub text: String,
}
