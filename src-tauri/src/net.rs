use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{anyhow, Result};
use mio::Token;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc as tokio_mpsc;

use crate::network::{self, NetEvent, Network, Poller};

// ---------------------------------------------------------------------------
// Packet — 线路协议
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Packet {
    ConnectRequest  { node_id: String, name: String, addr: SocketAddr },
    ConnectResponse { node_id: String, name: String, addr: SocketAddr },
    Chat            { from: String, to: Option<String>, content: String, timestamp: u64 },
    Disconnect      { reason: String },
    System          { content: String },
    Keepalive       { timestamp: u64 },
}

impl Packet {
    fn encode(&self) -> Result<Vec<u8>> {
        let payload = bincode::serialize(self)?;
        let len = payload.len() as u32;
        let mut buf = Vec::with_capacity(4 + payload.len());
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&payload);
        Ok(buf)
    }

    fn decode(buf: &[u8]) -> Result<Option<(Self, usize)>> {
        if buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes(buf[..4].try_into().unwrap()) as usize;
        if buf.len() < 4 + len {
            return Ok(None);
        }
        let pkt = bincode::deserialize(&buf[4..4 + len])
            .map_err(|e| anyhow!("decode error: {}", e))?;
        Ok(Some((pkt, 4 + len)))
    }
}

// ---------------------------------------------------------------------------
// Message — 帧编解码层，可 Clone，内部锁由 Network 管理
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Message {
    net: Network,
}

impl Message {
    pub fn send(&self, token: Token, packet: &Packet) -> Result<()> {
        let data = packet.encode()?;
        self.net.send(token, &data)
    }

    pub fn connect(&self, addr: SocketAddr, hello: &Packet) -> Result<()> {
        let data = hello.encode()?;
        self.net.connect(addr, Some(data))?;
        Ok(())
    }

    pub fn close(&self, token: Token) {
        self.net.close(token);
    }
}

// ---------------------------------------------------------------------------
// start_network — 启动网络线程
// ---------------------------------------------------------------------------

const POLL_TIMEOUT: Duration = Duration::from_millis(10);

pub fn start_network(
    listen_addr: SocketAddr,
) -> Result<(Message, tokio_mpsc::UnboundedReceiver<(Token, Packet)>)> {
    let (poller, network) = network::new_network(listen_addr)?;
    let message = Message { net: network.clone() };

    let (net_tx, net_rx) = tokio_mpsc::unbounded_channel();

    std::thread::spawn(move || {
        network_loop(poller, network, net_tx);
    });

    Ok((message, net_rx))
}

fn network_loop(
    mut poller: Poller,
    network: Network,
    net_tx: tokio_mpsc::UnboundedSender<(Token, Packet)>,
) {
    let mut recv_bufs: HashMap<Token, Vec<u8>> = HashMap::new();

    loop {
        let raw_events = match poller.poll(Some(POLL_TIMEOUT)) {
            Ok(events) => events,
            Err(e) => {
                eprintln!("poll error: {}", e);
                continue;
            }
        };

        if raw_events.is_empty() {
            continue;
        }

        let net_events = network.process_events(&raw_events);

        for event in net_events {
            let alive = dispatch_event(event, &mut recv_bufs, &net_tx);
            if !alive {
                return;
            }
        }
    }
}

/// Decode a single NetEvent into Packets and forward to App.
/// Returns `false` if the App channel is closed (should exit loop).
fn dispatch_event(
    event: NetEvent,
    recv_bufs: &mut HashMap<Token, Vec<u8>>,
    net_tx: &tokio_mpsc::UnboundedSender<(Token, Packet)>,
) -> bool {
    match event {
        NetEvent::Data { token, data } => decode_and_forward(token, data, recv_bufs, net_tx),
        NetEvent::Disconnected { token } => {
            recv_bufs.remove(&token);
            net_tx.send((token, Packet::Disconnect {
                reason: "connection closed".to_string(),
            })).is_ok()
        }
    }
}

/// Append raw bytes to the receive buffer, decode all complete packets,
/// and forward each to App. Returns `false` if the channel is closed.
fn decode_and_forward(
    token: Token,
    data: Vec<u8>,
    recv_bufs: &mut HashMap<Token, Vec<u8>>,
    net_tx: &tokio_mpsc::UnboundedSender<(Token, Packet)>,
) -> bool {
    let buf = recv_bufs.entry(token).or_default();
    buf.extend_from_slice(&data);

    loop {
        match Packet::decode(buf) {
            Ok(Some((pkt, consumed))) => {
                buf.drain(..consumed);
                if net_tx.send((token, pkt)).is_err() {
                    return false;
                }
            }
            Ok(None) => return true,
            Err(e) => {
                eprintln!("decode error on {:?}: {}", token, e);
                return true;
            }
        }
    }
}
