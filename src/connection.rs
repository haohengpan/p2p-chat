//! Low-level TCP connection types and binary wire-protocol helpers.
//!
//! Higher-level loop logic (read / write / accept) lives in [`crate::network`].

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};

use crate::message::P2PMessage;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Hard limit on a single serialised message.  Prevents OOM from
/// malformed or malicious packets.
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10 MB

// ---------------------------------------------------------------------------
// Connection types
// ---------------------------------------------------------------------------

/// One live connection: writing is done by sending to this channel.
pub struct Connection {
    pub tx: mpsc::Sender<P2PMessage>,
}

/// Thread-safe map of active connections keyed by peer `node_id`.
pub type ConnectionMap = Arc<Mutex<HashMap<String, Connection>>>;

// ---------------------------------------------------------------------------
// Wire protocol: length-prefixed frames
// ---------------------------------------------------------------------------
//
// Frame layout:
//   ┌──────────────────┬─────────────────────────┐
//   │  length  (4 B)   │  body  (length bytes)   │
//   │  big-endian u32  │  bincode-serialised msg  │
//   └──────────────────┴─────────────────────────┘

/// Serialise `message` and write it as a length-prefixed frame.
pub async fn write_msg(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    message: P2PMessage,
) -> Result<()> {
    let bytes = message.to_bytes()?;
    if bytes.len() > MAX_MESSAGE_SIZE {
        return Err(anyhow::anyhow!(
            "Message too large: {} bytes (max {})",
            bytes.len(),
            MAX_MESSAGE_SIZE
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    writer.write_all(&bytes).await?;
    Ok(())
}

/// Read one length-prefixed frame.  Returns `None` on clean EOF /
/// connection reset.
pub async fn read_msg(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
) -> Result<Option<P2PMessage>> {
    let mut len_buf = [0u8; 4];
    if reader.read_exact(&mut len_buf).await.is_err() {
        return Ok(None);
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(anyhow::anyhow!(
            "Received frame too large: {} bytes – possible attack or protocol error",
            len
        ));
    }
    let mut buf = vec![0u8; len];
    if reader.read_exact(&mut buf).await.is_err() {
        return Ok(None);
    }
    Ok(Some(P2PMessage::from_bytes(&buf)?))
}
