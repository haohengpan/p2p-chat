use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum P2PMessage {
    /// Handshake message - first message sent when connecting.
    /// Both sides exchange a Handshake so each peer can record the other's
    /// listening address and username in the local peer registry.
    Handshake {
        node_id: String,         // Node identifier
        username: String,        // User display name
        listen_addr: String,     // Our TCP listening address (ip:port) for reconnecting
        timestamp: u64,
    },

    /// Direct message (point-to-point)
    Direct {
        from: String,           // Sender node ID
        to: Option<String>,      // Receiver node ID (None for broadcast)
        content: String,         // Message content
        timestamp: u64,
    },

    /// System message
    System {
        content: String,         // System information
        timestamp: u64,
    },

    /// Keepalive message
    Keepalive {
        timestamp: u64,
    },

    /// Disconnect message
    Disconnect {
        reason: String,          // Disconnection reason
        timestamp: u64,
    },
}

impl P2PMessage {
    pub fn new_handshake(node_id: String, username: String, listen_addr: String) -> Self {
        P2PMessage::Handshake {
            node_id,
            username,
            listen_addr,
            timestamp: Self::now(),
        }
    }

    pub fn new_direct(from: String, to: Option<String>, content: String) -> Self {
        P2PMessage::Direct {
            from,
            to,
            content,
            timestamp: Self::now(),
        }
    }

    pub fn new_system(content: String) -> Self {
        P2PMessage::System {
            content,
            timestamp: Self::now(),
        }
    }

    pub fn new_keepalive() -> Self {
        P2PMessage::Keepalive {
            timestamp: Self::now(),
        }
    }

    pub fn new_disconnect(reason: String) -> Self {
        P2PMessage::Disconnect {
            reason,
            timestamp: Self::now(),
        }
    }

    pub fn timestamp(&self) -> u64 {
        match self {
            P2PMessage::Handshake { timestamp, .. }
            | P2PMessage::Direct { timestamp, .. }
            | P2PMessage::System { timestamp, .. }
            | P2PMessage::Keepalive { timestamp, .. }
            | P2PMessage::Disconnect { timestamp, .. } => *timestamp,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }

    pub fn is_handshake(&self) -> bool {
        matches!(self, P2PMessage::Handshake { .. })
    }

    pub fn is_direct(&self) -> bool {
        matches!(self, P2PMessage::Direct { .. })
    }

    pub fn is_system(&self) -> bool {
        matches!(self, P2PMessage::System { .. })
    }

    pub fn is_keepalive(&self) -> bool {
        matches!(self, P2PMessage::Keepalive { .. })
    }

    pub fn is_disconnect(&self) -> bool {
        matches!(self, P2PMessage::Disconnect { .. })
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}