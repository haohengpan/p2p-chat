use std::collections::VecDeque;
use std::net::SocketAddr;

use anyhow::Result;
use mio::Token;

use crate::api::{Content, MessageInfo, MessageStatus, Notify, NotifySender};
use crate::net::{Message, Packet};

const HISTORY_LIMIT: usize = 128;

pub struct Node {
    pub node_id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub token: Token,
    pub history: VecDeque<MessageInfo>,
    message: Message,
    notify_tx: NotifySender,
}

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
            history: VecDeque::new(),
            message, notify_tx,
        }
    }

    // -- Incoming message handling --

    /// Process an incoming chat message: record history, notify UI.
    pub fn handle_chat(&mut self, msg_id: String, content: String, timestamp: u64) {
        let msg = MessageInfo {
            msg_id,
            from: self.node_id.clone(),
            content: Content::Text(content),
            timestamp,
            status: MessageStatus::Sent,
        };
        self.record(msg.clone());
        self.notify_tx.emit(Notify::MessageReceived {
            conv_id: self.node_id.clone(),
            msg,
        });
    }

    // -- Outgoing operations --

    /// Send a chat message to this peer.
    /// Records outgoing history. Returns Ok/Err; caller emits MessageAck.
    pub fn send_chat(&mut self, from_id: &str, msg_id: &str, content: &str, timestamp: u64) -> Result<()> {
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
        self.record(msg);
        Ok(())
    }

    /// Send a raw packet to this peer (e.g. ConnectResponse).
    pub fn send_packet(&self, packet: &Packet) -> Result<()> {
        self.message.send(self.token, packet)
    }

    /// Close the network connection for this peer.
    pub fn close(&self) {
        self.message.close(self.token);
    }

    /// Get chat history for this peer/conversation.
    pub fn get_history(&self, before: Option<u64>, limit: u32) -> Vec<MessageInfo> {
        let iter = self.history.iter().rev();
        let iter: Box<dyn Iterator<Item = &MessageInfo>> = if let Some(ts) = before {
            Box::new(iter.filter(move |m| m.timestamp < ts))
        } else {
            Box::new(iter)
        };
        iter.take(limit as usize).cloned().collect::<Vec<_>>().into_iter().rev().collect()
    }

    // -- Internal --

    fn record(&mut self, msg: MessageInfo) {
        if self.history.len() >= HISTORY_LIMIT {
            self.history.pop_front();
        }
        self.history.push_back(msg);
    }
}
