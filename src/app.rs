use std::collections::HashMap;
use std::net::SocketAddr;

use anyhow::Result;
use mio::Token;
use tokio::sync::mpsc;

use crate::api::{Command, MessageStatus, Notify, NoticeLevel, NotifySender, PeerInfo};
use crate::net::{self, Message, Packet};
use crate::node::Node;

const DEFAULT_PORTS: &[u16] = &[9000, 9001, 9002, 9003, 9004];

pub struct App {
    pub node_id: String,
    pub name: String,
    pub listen_addr: SocketAddr,
    pub node_list: HashMap<String, Node>,
    message: Message,
    notify_tx: NotifySender,
}

impl App {
    pub fn new(
        node_id: String,
        name: String,
        listen_addr: SocketAddr,
        notify_tx: mpsc::UnboundedSender<Notify>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<(Token, Packet)>)> {
        let (message, net_rx) = net::start_network(listen_addr)?;
        let notify_tx = NotifySender::new(notify_tx);

        let app = Self {
            node_id, name, listen_addr,
            node_list: HashMap::new(),
            message, notify_tx,
        };
        Ok((app, net_rx))
    }

    fn notice(&self, level: NoticeLevel, content: String) {
        self.notify_tx.emit(Notify::Notice { level, content });
    }

    pub async fn run(
        &mut self,
        mut net_rx: mpsc::UnboundedReceiver<(Token, Packet)>,
        mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    ) -> Result<()> {
        loop {
            tokio::select! {
                Some((token, packet)) = net_rx.recv() => {
                    self.handle_packet(token, packet)?;
                }
                Some(cmd) = cmd_rx.recv() => {
                    if self.handle_command(cmd)? {
                        return Ok(());
                    }
                }
                else => break,
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Packet dispatch
// ---------------------------------------------------------------------------

impl App {
    fn handle_packet(&mut self, token: Token, packet: Packet) -> Result<()> {
        match packet {
            Packet::ConnectRequest { node_id, name, addr } =>
                self.on_connect_request(token, node_id, name, addr),
            Packet::ConnectResponse { node_id, name, addr } =>
                self.on_connect_response(token, node_id, name, addr),
            Packet::Chat { from, content, timestamp, .. } =>
                self.on_chat(from, content, timestamp),
            Packet::Disconnect { .. } =>
                self.on_disconnect(token),
            Packet::System { content } => {
                self.notice(NoticeLevel::Info, content);
                Ok(())
            }
            Packet::Keepalive { .. } => Ok(()),
        }
    }

    fn on_connect_request(&mut self, token: Token, node_id: String, name: String, addr: SocketAddr) -> Result<()> {
        let reply = Packet::ConnectResponse {
            node_id: self.node_id.clone(),
            name: self.name.clone(),
            addr: self.listen_addr,
        };
        let node = self.create_node(node_id.clone(), name.clone(), addr, token);
        node.send_packet(&reply)?;
        self.notify_tx.emit(Notify::PeerOnline {
            peer_id: node_id, peer_name: name, addr,
        });
        Ok(())
    }

    fn on_connect_response(&mut self, token: Token, node_id: String, name: String, addr: SocketAddr) -> Result<()> {
        self.create_node(node_id.clone(), name.clone(), addr, token);
        self.notify_tx.emit(Notify::PeerOnline {
            peer_id: node_id, peer_name: name, addr,
        });
        Ok(())
    }

    fn on_chat(&mut self, from: String, content: String, timestamp: u64) -> Result<()> {
        if let Some(node) = self.node_list.get_mut(&from) {
            // Generate msg_id for incoming messages (sender doesn't provide one yet)
            let msg_id = format!("{}-{}", from, timestamp);
            node.handle_chat(msg_id, content, timestamp);
        }
        Ok(())
    }

    fn on_disconnect(&mut self, token: Token) -> Result<()> {
        if let Some(id) = self.node_id_by_token(token) {
            if let Some(node) = self.node_list.get(&id) {
                node.close();
            }
            self.notify_tx.emit(Notify::PeerOffline { peer_id: id });
        }
        self.node_list.retain(|_, n| n.token != token);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

impl App {
    fn handle_command(&mut self, cmd: Command) -> Result<bool> {
        match cmd {
            Command::Shutdown => return Ok(true),
            Command::Connect { addr } => self.cmd_connect(&addr)?,
            Command::Disconnect { peer_id } => self.cmd_disconnect(&peer_id)?,
            Command::SendMessage { conv_id, msg_id, content } => {
                self.cmd_send_message(conv_id, msg_id, content)?;
            }
            Command::GetHistory { conv_id, before, limit } => {
                self.cmd_get_history(&conv_id, before, limit);
            }
            Command::ListPeers => self.cmd_list_peers(),
        }
        Ok(false)
    }

    /// Smart connect: detects ip:port, bare ip (probe ports), or node_id.
    fn cmd_connect(&mut self, addr: &str) -> Result<()> {
        if let Ok(sock) = addr.parse::<SocketAddr>() {
            return self.do_connect(sock);
        }
        if let Some(node) = self.node_list.get(addr) {
            let sock = node.addr;
            return self.do_connect(sock);
        }
        self.do_connect_ip(addr)
    }

    fn do_connect(&mut self, addr: SocketAddr) -> Result<()> {
        let hello = Packet::ConnectRequest {
            node_id: self.node_id.clone(),
            name: self.name.clone(),
            addr: self.listen_addr,
        };
        self.message.connect(addr, &hello)?;
        self.notice(NoticeLevel::Info, format!("Connecting to {}...", addr));
        Ok(())
    }

    fn do_connect_ip(&mut self, ip: &str) -> Result<()> {
        let hello = Packet::ConnectRequest {
            node_id: self.node_id.clone(),
            name: self.name.clone(),
            addr: self.listen_addr,
        };
        for &port in DEFAULT_PORTS {
            let addr_str = format!("{}:{}", ip, port);
            if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                let _ = self.message.connect(addr, &hello);
            }
        }
        self.notice(NoticeLevel::Info,
            format!("Connecting to {} (trying ports {:?})...", ip, DEFAULT_PORTS),
        );
        Ok(())
    }

    fn cmd_disconnect(&mut self, peer_id: &str) -> Result<()> {
        if let Some(node) = self.node_list.get(peer_id) {
            node.close();
            self.notify_tx.emit(Notify::PeerOffline { peer_id: peer_id.to_string() });
            self.node_list.remove(peer_id);
        } else {
            self.notice(NoticeLevel::Error, format!("Unknown peer '{}'", peer_id));
        }
        Ok(())
    }

    fn cmd_send_message(&mut self, conv_id: String, msg_id: String, content: crate::api::Content) -> Result<()> {
        let text = match &content {
            crate::api::Content::Text(s) => s.clone(),
        };
        let our_id = self.node_id.clone();
        let timestamp = now();

        match self.node_list.get_mut(&conv_id) {
            Some(node) => {
                match node.send_chat(&our_id, &msg_id, &text, timestamp) {
                    Ok(()) => {
                        self.notify_tx.emit(Notify::MessageAck {
                            msg_id, status: MessageStatus::Sent,
                        });
                    }
                    Err(e) => {
                        self.notify_tx.emit(Notify::MessageAck {
                            msg_id, status: MessageStatus::Failed(e.to_string()),
                        });
                    }
                }
            }
            None => {
                self.notify_tx.emit(Notify::MessageAck {
                    msg_id, status: MessageStatus::Failed(format!("Unknown peer '{}'", conv_id)),
                });
            }
        }
        Ok(())
    }

    fn cmd_get_history(&self, conv_id: &str, before: Option<u64>, limit: u32) {
        let messages = if let Some(node) = self.node_list.get(conv_id) {
            node.get_history(before, limit)
        } else {
            Vec::new()
        };
        self.notify_tx.emit(Notify::History {
            conv_id: conv_id.to_string(),
            messages,
        });
    }

    fn cmd_list_peers(&self) {
        let peers: Vec<PeerInfo> = self.node_list.values()
            .map(|n| PeerInfo {
                peer_id: n.node_id.clone(),
                peer_name: n.name.clone(),
                addr: n.addr,
            })
            .collect();
        self.notify_tx.emit(Notify::PeerList { peers });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl App {
    fn create_node(&mut self, node_id: String, name: String, addr: SocketAddr, token: Token) -> &mut Node {
        self.node_list.insert(node_id.clone(), Node::new(
            node_id.clone(), name, addr, token,
            self.message.clone(), self.notify_tx.clone(),
        ));
        self.node_list.get_mut(&node_id).unwrap()
    }

    fn node_id_by_token(&self, token: Token) -> Option<String> {
        self.node_list.values()
            .find(|n| n.token == token)
            .map(|n| n.node_id.clone())
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
