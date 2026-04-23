use std::net::SocketAddr;

use anyhow::Result;
use mio::Token;

use crate::api::{Content, MessageInfo, MessageStatus, Notify, NotifySender};
use crate::net::{Message, Packet};

pub struct Node {
    pub node_id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub token: Token,
    pub online: bool,
    message: Message,
    notify_tx: NotifySender,
}

/// Dummy token for offline nodes (never matches a real connection).
pub const OFFLINE_TOKEN: Token = Token(usize::MAX);

impl Node {
    pub fn new(
        node_id: String,
        name: String,
        addr: SocketAddr,
        token: Token,
        message: Message,
        notify_tx: NotifySender,
    ) -> Self {
        Self {
            node_id, name, addr, token,
            online: true,
            message, notify_tx,
        }
    }

    /// Create an offline node (loaded from saved peers, not yet connected).
    pub fn new_offline(
        node_id: String,
        name: String,
        addr: SocketAddr,
        message: Message,
        notify_tx: NotifySender,
    ) -> Self {
        Self {
            node_id, name, addr,
            token: OFFLINE_TOKEN,
            online: false,
            message, notify_tx,
        }
    }

    /// Bring this node online with a new connection token.
    pub fn set_online(&mut self, token: Token, name: String, addr: SocketAddr) {
        self.token = token;
        self.name = name;
        self.addr = addr;
        self.online = true;
    }

    /// Mark this node as offline.
    pub fn set_offline(&mut self) {
        self.online = false;
        self.token = OFFLINE_TOKEN;
    }

    /// Process an incoming chat message: build MessageInfo and notify UI.
    /// Returns the MessageInfo so App can record it in HistoryManager.
    pub fn handle_chat(&self, msg_id: String, content: String, timestamp: u64) -> MessageInfo {
        let msg = MessageInfo {
            msg_id,
            from: self.node_id.clone(),
            content: Content::Text(content),
            timestamp,
            status: MessageStatus::Sent,
        };
        self.notify_tx.emit(Notify::MessageReceived {
            conv_id: self.node_id.clone(),
            msg: msg.clone(),
        });
        msg
    }

    /// Send a chat message to this peer.
    /// Returns the MessageInfo on success so App can record it in HistoryManager.
    pub fn send_chat(&self, from_id: &str, msg_id: &str, content: &str, timestamp: u64) -> Result<MessageInfo> {
        let pkt = Packet::Chat {
            from: from_id.to_string(),
            to: Some(self.node_id.clone()),
            content: content.to_string(),
            timestamp,
        };
        self.message.send(self.token, &pkt)?;
        let msg = MessageInfo {
            msg_id: msg_id.to_string(),
            from: from_id.to_string(),
            content: Content::Text(content.to_string()),
            timestamp,
            status: MessageStatus::Sent,
        };
        Ok(msg)
    }

    /// Send a raw packet to this peer (e.g. ConnectResponse).
    pub fn send_packet(&self, packet: &Packet) -> Result<()> {
        self.message.send(self.token, packet)
    }

    /// Close the network connection for this peer.
    pub fn close(&self) {
        if self.online {
            self.message.close(self.token);
        }
    }
}
