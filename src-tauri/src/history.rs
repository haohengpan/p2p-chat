//! HistoryManager — unified in-memory cache + disk persistence for chat history.
//!
//! All history operations go through this module. Node no longer owns history,
//! and App no longer needs to manually call persist in multiple places.

use std::collections::{HashMap, VecDeque};

use crate::api::MessageInfo;
use crate::bridge;

const HISTORY_LIMIT: usize = 128;

pub struct HistoryManager {
    /// peer_id → in-memory message buffer
    chats: HashMap<String, VecDeque<MessageInfo>>,
}

impl HistoryManager {
    /// Create from saved history loaded at startup.
    pub fn new(saved: HashMap<String, Vec<MessageInfo>>) -> Self {
        let chats = saved
            .into_iter()
            .map(|(peer_id, msgs)| {
                let mut buf = VecDeque::with_capacity(HISTORY_LIMIT);
                for msg in msgs {
                    buf.push_back(msg);
                }
                while buf.len() > HISTORY_LIMIT {
                    buf.pop_front();
                }
                (peer_id, buf)
            })
            .collect();
        Self { chats }
    }

    /// Add a message (incoming or outgoing). Persists to disk immediately.
    pub fn add_message(&mut self, peer_id: &str, msg: MessageInfo) {
        let buf = self.chats.entry(peer_id.to_string()).or_default();
        if buf.len() >= HISTORY_LIMIT {
            buf.pop_front();
        }
        buf.push_back(msg);
        self.persist(peer_id);
    }

    /// Query history for a peer with optional pagination.
    pub fn get_history(&self, peer_id: &str, before: Option<u64>, limit: u32) -> Vec<MessageInfo> {
        let buf = match self.chats.get(peer_id) {
            Some(b) => b,
            None => return Vec::new(),
        };
        let iter = buf.iter().rev();
        let iter: Box<dyn Iterator<Item = &MessageInfo>> = if let Some(ts) = before {
            Box::new(iter.filter(move |m| m.timestamp < ts))
        } else {
            Box::new(iter)
        };
        iter.take(limit as usize)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Flush all peers' history to disk (for shutdown).
    pub fn persist_all(&self) {
        for peer_id in self.chats.keys() {
            self.persist(peer_id);
        }
    }

    /// Flush a specific peer's history to disk.
    fn persist(&self, peer_id: &str) {
        if let Some(buf) = self.chats.get(peer_id) {
            let msgs: Vec<MessageInfo> = buf.iter().cloned().collect();
            if let Err(e) = bridge::storage().save_history(peer_id, &msgs) {
                tracing::error!("save history for {}: {}", peer_id, e);
            }
        }
    }
}
