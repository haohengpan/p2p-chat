use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Registry, Token};

const LISTENER_TOKEN: Token = Token(0);
const EVENTS_CAPACITY: usize = 128;

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

enum ConnState {
    Connecting,
    Connected,
}

struct Connection {
    stream: TcpStream,
    state: ConnState,
    pending_hello: Option<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// NetEvent
// ---------------------------------------------------------------------------

pub enum NetEvent {
    Data { token: Token, data: Vec<u8> },
    Disconnected { token: Token },
}

// ---------------------------------------------------------------------------
// Poller — 网络线程独占，不需要锁
// ---------------------------------------------------------------------------

pub struct Poller {
    poll: Poll,
    events: Events,
}

impl Poller {
    /// 阻塞等待 I/O 事件，返回原始 (token, readable, writable)。
    /// 在 **不持有任何锁** 的情况下调用。
    pub fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<(Token, bool, bool)>> {
        self.poll.poll(&mut self.events, timeout)?;
        Ok(self.events
            .iter()
            .map(|e| (e.token(), e.is_readable(), e.is_writable()))
            .collect())
    }
}

// ---------------------------------------------------------------------------
// NetInner — 内部状态，不直接暴露
// ---------------------------------------------------------------------------

struct NetInner {
    registry: Registry,
    listener: TcpListener,
    connections: HashMap<Token, Connection>,
    next_token: usize,
    free_tokens: Vec<Token>,
}

impl NetInner {
    fn process_events(&mut self, raw_events: &[(Token, bool, bool)]) -> Vec<NetEvent> {
        let mut out = Vec::new();

        for &(token, readable, writable) in raw_events {
            match token {
                LISTENER_TOKEN => {
                    if let Err(e) = self.accept_all() {
                        eprintln!("accept error: {}", e);
                    }
                }
                token => {
                    if writable {
                        if let Err(e) = self.handle_writable(token, &mut out) {
                            eprintln!("writable error on {:?}: {}", token, e);
                        }
                    }
                    if readable {
                        self.handle_readable(token, &mut out);
                    }
                }
            }
        }
        out
    }

    fn accept_all(&mut self) -> Result<()> {
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let token = self.alloc_token();
                    self.registry
                        .register(&mut stream, token, Interest::READABLE)?;
                    self.connections.insert(token, Connection {
                        stream,
                        state: ConnState::Connected,
                        pending_hello: None,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(anyhow!("accept error: {}", e)),
            }
        }
        Ok(())
    }

    fn handle_writable(&mut self, token: Token, out: &mut Vec<NetEvent>) -> Result<()> {
        let conn = match self.connections.get_mut(&token) {
            Some(c) if matches!(c.state, ConnState::Connecting) => c,
            _ => return Ok(()),
        };

        if conn.stream.peer_addr().is_err() {
            self.remove(token);
            out.push(NetEvent::Disconnected { token });
            return Ok(());
        }

        conn.state = ConnState::Connected;
        let hello = conn.pending_hello.take();
        self.registry
            .reregister(&mut conn.stream, token, Interest::READABLE)?;

        if let Some(data) = hello {
            let conn = self.connections.get_mut(&token).unwrap();
            if conn.stream.write_all(&data).is_err() {
                self.remove(token);
                out.push(NetEvent::Disconnected { token });
            }
        }
        Ok(())
    }

    fn handle_readable(&mut self, token: Token, out: &mut Vec<NetEvent>) {
        let conn = match self.connections.get_mut(&token) {
            Some(c) => c,
            None => return,
        };

        let mut tmp = [0u8; 4096];
        let mut data = Vec::new();

        loop {
            match conn.stream.read(&mut tmp) {
                Ok(0) => {
                    self.remove(token);
                    out.push(NetEvent::Disconnected { token });
                    return;
                }
                Ok(n) => data.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.remove(token);
                    out.push(NetEvent::Disconnected { token });
                    return;
                }
            }
        }

        if !data.is_empty() {
            out.push(NetEvent::Data { token, data });
        }
    }

    fn send(&mut self, token: Token, data: &[u8]) -> Result<()> {
        let conn = self.connections.get_mut(&token)
            .ok_or_else(|| anyhow!("no connection for token {:?}", token))?;
        conn.stream.write_all(data)?;
        Ok(())
    }

    fn connect(&mut self, addr: SocketAddr, hello: Option<Vec<u8>>) -> Result<Token> {
        let mut stream = TcpStream::connect(addr)?;
        let token = self.alloc_token();
        self.registry
            .register(&mut stream, token, Interest::READABLE | Interest::WRITABLE)?;
        self.connections.insert(token, Connection {
            stream,
            state: ConnState::Connecting,
            pending_hello: hello,
        });
        Ok(token)
    }

    fn close(&mut self, token: Token) {
        self.remove(token);
    }

    fn alloc_token(&mut self) -> Token {
        if let Some(token) = self.free_tokens.pop() {
            return token;
        }
        let t = Token(self.next_token);
        self.next_token += 1;
        t
    }

    fn remove(&mut self, token: Token) {
        if let Some(mut conn) = self.connections.remove(&token) {
            let _ = self.registry.deregister(&mut conn.stream);
            self.free_tokens.push(token);
        }
    }
}

// ---------------------------------------------------------------------------
// Network — 线程安全外壳，每次调用自动加锁解锁
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Network {
    inner: Arc<Mutex<NetInner>>,
}

/// 创建 Poller（网络线程独占）和 Network（线程安全，可 Clone）。
pub fn new_network(addr: SocketAddr) -> Result<(Poller, Network)> {
    let poll = Poll::new()?;
    let registry = poll.registry().try_clone()?;

    let mut listener = TcpListener::bind(addr)?;
    poll.registry()
        .register(&mut listener, LISTENER_TOKEN, Interest::READABLE)?;

    let poller = Poller {
        poll,
        events: Events::with_capacity(EVENTS_CAPACITY),
    };

    let network = Network {
        inner: Arc::new(Mutex::new(NetInner {
            registry,
            listener,
            connections: HashMap::new(),
            next_token: 1,
            free_tokens: Vec::new(),
        })),
    };

    Ok((poller, network))
}

impl Network {
    /// 处理原始 poll 事件，返回高层 NetEvent（微秒级持锁）。
    pub fn process_events(&self, raw_events: &[(Token, bool, bool)]) -> Vec<NetEvent> {
        self.inner.lock().unwrap().process_events(raw_events)
    }

    pub fn send(&self, token: Token, data: &[u8]) -> Result<()> {
        self.inner.lock().unwrap().send(token, data)
    }

    pub fn connect(&self, addr: SocketAddr, hello: Option<Vec<u8>>) -> Result<Token> {
        self.inner.lock().unwrap().connect(addr, hello)
    }

    pub fn close(&self, token: Token) {
        self.inner.lock().unwrap().close(token);
    }
}
