//! Terminal UI – ratatui + crossterm.
//!
//! Two public entry points:
//!   `run_setup`  – collects node-id / username before the node starts.
//!                  Returns the raw answers **and** the already-configured
//!                  Terminal so `run_tui` can reuse it without a visible flash.
//!   `run_tui`    – the main chat interface.
//!
//! Layout (main chat)
//! ──────────────────
//!   ┌─ P2P Chat | Online: alice, bob ──────────────────── ↑3 ─┐
//!   │  [14:32:05] Alice (alice): hello                         │  ← cyan
//!   │  [14:32:10] You  (bob) → alice: hi                       │  ← green
//!   │  [SYSTEM] charlie disconnected                           │  ← yellow
//!   ├──────────────────────────────────────────────────────────┤
//!   │  [alice]> _                                              │
//!   └──────────────────────────────────────────────────────────┘
//!
//! Key bindings (chat)
//! ────────────────────
//!   Enter          – submit (command or chat message)
//!   Ctrl-C         – graceful quit
//!   ↑/↓  (empty)   – scroll messages
//!   ↑/↓  (input)   – command-history navigation
//!   PageUp/Down    – scroll ±10 lines
//!   Esc            – snap to latest
//!   Backspace      – delete last char

use std::collections::VecDeque;
use std::net::SocketAddr;

// ---------------------------------------------------------------------------
// Rendering types  (terminal-UI-specific; not part of the node↔UI protocol)
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

const HELP_LINES: &[&str] = &[
    "Commands:",
    "  connect <addr>               - Connect (ip:port, ip, or node_id)",
    "  disconnect <node_id>         - Disconnect from a peer",
    "  chat <node_id>               - Enter chat mode with a peer",
    "  chat                         - Exit chat mode",
    "  send <node_id> <message>     - Send a direct message",
    "  history [node_id]            - Show chat history",
    "  list / peers                 - Show connected peers",
    "  quit                         - Exit the application",
    "",
    "In chat mode:",
    "  <message>     - Send message to current chat partner",
    "  /exit         - Leave chat mode",
    "  /<command>    - Run any command (e.g. /list, /history)",
    "",
    "Keys: ↑↓ = scroll/history | PageUp/Dn | Esc = bottom",
];

use anyhow::Result;
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use tokio::sync::mpsc;

use crate::api::{Command, Content, MessageStatus, Notify, NoticeLevel};

fn format_time(ts: u64) -> String {
    let secs = ts % 60;
    let mins = (ts / 60) % 60;
    let hours = (ts / 3600) % 24;
    format!("{:02}:{:02}:{:02}", hours, mins, secs)
}

const MAX_MESSAGES: usize = 500;

// ---------------------------------------------------------------------------
// Shared terminal type alias
// ---------------------------------------------------------------------------

pub type Term = Terminal<CrosstermBackend<std::io::Stdout>>;

// ============================================================================
// SECTION 1 – Setup screen
// ============================================================================

struct SetupState {
    node_id: String,
    username: String,
    /// 0 = node_id field active, 1 = username field active.
    focus: usize,
    error: Option<String>,
}

/// Try to get the primary outgoing LAN IP via a non-sending UDP trick.
pub fn local_ip() -> String {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    if let Some(sock) = s {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(a) = sock.local_addr() {
                return a.ip().to_string();
            }
        }
    }
    "unknown".to_string()
}

/// Show the setup dialog, collect node-id and username, return them together
/// with the still-open `Terminal` (raw mode stays active — no visible flash).
///
/// On error (e.g. Ctrl-C), the terminal is restored before the error
/// propagates.
pub async fn run_setup(listen_addr: SocketAddr) -> Result<(String, String, Term)> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let lan_ip = local_ip();

    match setup_loop(&mut terminal, listen_addr, &lan_ip).await {
        Ok((node_id, username)) => Ok((node_id, username, terminal)),
        Err(e) => {
            // Restore terminal before propagating the error
            let _ = disable_raw_mode();
            let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
            let _ = terminal.show_cursor();
            Err(e)
        },
    }
}

async fn setup_loop(
    terminal: &mut Term, listen_addr: SocketAddr, lan_ip: &str,
) -> Result<(String, String)> {
    let mut state =
        SetupState { node_id: String::new(), username: String::new(), focus: 0, error: None };
    let mut keys = EventStream::new();

    loop {
        terminal.draw(|f| render_setup(f, &state, listen_addr, lan_ip))?;

        if let Some(ev) = keys.next().await {
            match ev? {
                Event::Key(key) => {
                    if let Some(result) = handle_setup_key(&mut state, key)? {
                        return Ok(result);
                    }
                },
                Event::Resize(_, _) => {},
                _ => {},
            }
        }
    }
}

fn handle_setup_key(
    state: &mut SetupState, key: crossterm::event::KeyEvent,
) -> Result<Option<(String, String)>> {
    state.error = None;

    match key.code {
        // Quit
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Err(anyhow::anyhow!("Setup cancelled by user"));
        },

        // Confirm / advance
        KeyCode::Enter => {
            if state.focus == 0 {
                let nid = state.node_id.trim().to_string();
                if nid.is_empty() {
                    state.error = Some("Node ID cannot be empty".into());
                } else {
                    state.node_id = nid;
                    state.focus = 1;
                }
            } else {
                let nid = state.node_id.trim().to_string();
                let uname = state.username.trim().to_string();
                if nid.is_empty() {
                    state.focus = 0;
                    state.error = Some("Node ID cannot be empty".into());
                } else if uname.is_empty() {
                    state.error = Some("Username cannot be empty".into());
                } else {
                    return Ok(Some((nid, uname)));
                }
            }
        },

        // Switch field
        KeyCode::Tab | KeyCode::BackTab => {
            state.focus = 1 - state.focus;
        },
        KeyCode::Up => {
            state.focus = 0;
        },
        KeyCode::Down => {
            state.focus = 1;
        },

        // Edit active field
        KeyCode::Backspace => {
            if state.focus == 0 {
                state.node_id.pop();
            } else {
                state.username.pop();
            }
        },

        KeyCode::Char(c) => {
            if state.focus == 0 {
                // Silently reject spaces in node_id
                if c != ' ' {
                    state.node_id.push(c);
                }
            } else {
                state.username.push(c);
            }
        },

        _ => {},
    }
    Ok(None)
}

fn render_setup(f: &mut Frame, state: &SetupState, listen_addr: SocketAddr, lan_ip: &str) {
    let area = f.area();

    // Outer: 10% vertical padding on each side, 12% horizontal on each side
    let vert = Layout::vertical([
        Constraint::Percentage(10),
        Constraint::Min(0),
        Constraint::Percentage(10),
    ])
    .split(area);

    let horiz = Layout::horizontal([
        Constraint::Percentage(12),
        Constraint::Min(0),
        Constraint::Percentage(12),
    ])
    .split(vert[1]);

    let dialog = horiz[1];

    // Outer dialog box
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " P2P Chat — Setup ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(dialog);
    f.render_widget(block, dialog);

    if inner.height < 10 || inner.width < 32 {
        f.render_widget(Paragraph::new("Terminal too small — please resize."), inner);
        return;
    }

    // Inner content layout  (2 address lines now)
    let chunks = Layout::vertical([
        Constraint::Length(1), // "Local:" address line
        Constraint::Length(1), // "LAN:"   address line
        Constraint::Length(1), // blank
        Constraint::Length(1), // node_id label
        Constraint::Length(3), // node_id input
        Constraint::Length(1), // blank
        Constraint::Length(1), // username label
        Constraint::Length(3), // username input
        Constraint::Length(1), // blank
        Constraint::Length(1), // hint / error
        Constraint::Min(0),    // leftover space
    ])
    .split(inner);

    // ── address info (two lines) ──────────────────────────────────────────
    let port = listen_addr.port();
    f.render_widget(
        Paragraph::new(format!("Local  127.0.0.1:{}   (same machine)", port))
            .style(Style::default().fg(Color::Green)),
        chunks[0],
    );
    let lan_text = if lan_ip == "unknown" {
        format!("LAN    <ip>:{}   (share with peers on your network)", port)
    } else {
        format!("LAN    {}:{}   ← share this address with peers", lan_ip, port)
    };
    f.render_widget(
        Paragraph::new(lan_text)
            .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        chunks[1],
    );

    // ── node_id ───────────────────────────────────────────────────────────
    let (nid_label, nid_border) = field_styles(state.focus == 0);
    f.render_widget(
        Paragraph::new("Node ID  (unique identifier, no spaces):").style(nid_label),
        chunks[3],
    );
    f.render_widget(
        Paragraph::new(state.node_id.as_str())
            .block(Block::default().borders(Borders::ALL).border_style(nid_border)),
        chunks[4],
    );

    // ── username ──────────────────────────────────────────────────────────
    let (uname_label, uname_border) = field_styles(state.focus == 1);
    f.render_widget(
        Paragraph::new("Username  (display name visible to peers):").style(uname_label),
        chunks[6],
    );
    f.render_widget(
        Paragraph::new(state.username.as_str())
            .block(Block::default().borders(Borders::ALL).border_style(uname_border)),
        chunks[7],
    );

    // ── hint / error ──────────────────────────────────────────────────────
    if let Some(ref err) = state.error {
        f.render_widget(
            Paragraph::new(err.as_str()).style(Style::default().fg(Color::Red)),
            chunks[9],
        );
    } else {
        let hint = match state.focus {
            0 => "Tab / ↓ / Enter → next field   |   Ctrl-C to quit",
            _ => "Tab / ↑ → prev field   |   Enter → start chatting!",
        };
        f.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            chunks[9],
        );
    }

    // ── cursor ────────────────────────────────────────────────────────────
    let (input_area, text_len) = if state.focus == 0 {
        (chunks[4], state.node_id.len())
    } else {
        (chunks[7], state.username.len())
    };
    let cx =
        (input_area.x + 1 + text_len as u16).min(input_area.x + input_area.width.saturating_sub(2));
    let cy = input_area.y + 1;
    f.set_cursor_position((cx, cy));
}

/// Returns `(label_style, border_style)` for a focused / unfocused field.
fn field_styles(focused: bool) -> (Style, Style) {
    if focused {
        (
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Yellow),
        )
    } else {
        (Style::default().fg(Color::White), Style::default().fg(Color::DarkGray))
    }
}

// ============================================================================
// SECTION 2 – Main chat screen
// ============================================================================

struct App {
    messages: VecDeque<DisplayLine>,
    input: String,
    /// `Some(node_id)` while in chat mode.
    current_chat: Option<String>,
    /// node_ids of connected peers (shown in title bar).
    online_peers: Vec<String>,
    /// Distance from bottom; 0 = follow latest.
    scroll_offset: usize,
    history: Vec<String>,
    history_idx: Option<usize>,
    /// Counter for generating unique client-side message IDs.
    msg_seq: u64,
}

impl App {
    fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            input: String::new(),
            current_chat: None,
            online_peers: Vec::new(),
            scroll_offset: 0,
            history: Vec::new(),
            history_idx: None,
            msg_seq: 0,
        }
    }

    fn next_msg_id(&mut self) -> String {
        self.msg_seq += 1;
        format!("msg-{}", self.msg_seq)
    }

    fn push(&mut self, line: DisplayLine) {
        if self.messages.len() >= MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(line);
    }

    fn system(&mut self, text: impl Into<String>) {
        self.push(DisplayLine { kind: LineKind::System, text: text.into() });
    }
}

/// Run the main chat TUI.  Reuses `terminal` which was already set up by
/// `run_setup` (still in raw mode / alternate screen).  Restores the terminal
/// on exit.
///
/// Startup banner is built here from the node identity and listen address.
pub async fn run_tui(
    mut terminal: Term, event_rx: mpsc::UnboundedReceiver<Notify>,
    cmd_tx: mpsc::UnboundedSender<Command>, listen_addr: SocketAddr, node_id: String,
    username: String,
) -> Result<()> {
    let port = listen_addr.port();
    let lan_ip = local_ip();
    let node_info = [
        format!("Node: {} ({})", node_id, username),
        format!("Listening → local: 127.0.0.1:{}   LAN: {}:{}", port, lan_ip, port),
        "Share the LAN address with peers so they can connect to you.".to_string(),
    ];

    let mut app = App::new();
    let sep = "─".repeat(71);
    app.system(sep.clone());
    for line in &node_info {
        app.system(line.as_str());
    }
    app.system("Commands: help | connect | chat | send | history | list | quit");
    app.system("Keys: ↑↓ scroll/history | PageUp/Dn | Esc=bottom | Ctrl-C quit");
    app.system(sep);

    let result = chat_loop(&mut terminal, &mut app, event_rx, cmd_tx).await;

    // Always restore terminal
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

// ---------------------------------------------------------------------------
// Chat event loop
// ---------------------------------------------------------------------------

async fn chat_loop(
    terminal: &mut Term, app: &mut App, mut event_rx: mpsc::UnboundedReceiver<Notify>,
    cmd_tx: mpsc::UnboundedSender<Command>,
) -> Result<()> {
    let mut keys = EventStream::new();

    loop {
        terminal.draw(|f| render_chat(f, app))?;

        tokio::select! {
            maybe = keys.next() => {
                match maybe {
                    Some(Ok(Event::Key(key))) => {
                        if handle_key(app, key, &cmd_tx)? {
                            break;
                        }
                    }
                    Some(Ok(Event::Resize(_, _))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::error!("Terminal I/O error: {}", e);
                        break;
                    }
                    None => break,
                }
            }

            msg = event_rx.recv() => {
                match msg {
                    Some(ev) => {
                        if handle_notify(app, ev) {
                            terminal.draw(|f| render_chat(f, app))?;
                            break;
                        }
                    }
                    None => {
                        app.system("[SYSTEM] Node exited unexpectedly");
                        terminal.draw(|f| render_chat(f, app))?;
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Chat rendering
// ---------------------------------------------------------------------------

fn render_chat(f: &mut Frame, app: &App) {
    let area = f.area();

    let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(area);

    // ── title ──────────────────────────────────────────────────────────────
    let title = if app.online_peers.is_empty() {
        " P2P Chat (no peers online) ".to_string()
    } else {
        format!(" P2P Chat | Online: {} ", app.online_peers.join(", "))
    };

    // ── message window ─────────────────────────────────────────────────────
    let inner_h = chunks[0].height.saturating_sub(2) as usize;
    let total = app.messages.len();
    let start = if total <= inner_h {
        0
    } else {
        let max_scroll = total - inner_h;
        let clamped = app.scroll_offset.min(max_scroll);
        max_scroll - clamped
    };

    let items: Vec<ListItem> = app
        .messages
        .iter()
        .skip(start)
        .take(inner_h)
        .map(|line| {
            let color = match line.kind {
                LineKind::Incoming => Color::Cyan,
                LineKind::Outgoing => Color::Green,
                LineKind::System => Color::Yellow,
            };
            ListItem::new(line.text.as_str()).style(Style::default().fg(color))
        })
        .collect();

    let scroll_hint =
        if app.scroll_offset > 0 { format!(" ↑{} ", app.scroll_offset) } else { String::new() };

    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(title + &scroll_hint)),
        chunks[0],
    );

    // ── input bar ──────────────────────────────────────────────────────────
    let (prompt, bar_title) = if let Some(ref p) = app.current_chat {
        (format!("[{}]> {}", p, app.input), " Chat mode — type to send | /exit to leave ")
    } else {
        (format!("> {}", app.input), " Command ")
    };

    f.render_widget(
        Paragraph::new(prompt.as_str())
            .block(Block::default().borders(Borders::ALL).title(bar_title)),
        chunks[1],
    );

    // Cursor at end of prompt (clamped to visible area)
    let max_cx = chunks[1].x + chunks[1].width.saturating_sub(2);
    let cx = (chunks[1].x + 1 + prompt.len() as u16).min(max_cx);
    f.set_cursor_position((cx, chunks[1].y + 1));
}

// ---------------------------------------------------------------------------
// Keyboard handling
// ---------------------------------------------------------------------------

fn handle_key(
    app: &mut App, key: crossterm::event::KeyEvent, cmd_tx: &mpsc::UnboundedSender<Command>,
) -> Result<bool> {
    match key.code {
        KeyCode::Enter => {
            let input = app.input.trim().to_string();
            if input.is_empty() {
                return Ok(false);
            }
            if app.history.last().map(String::as_str) != Some(&input) {
                app.history.push(input.clone());
            }
            app.history_idx = None;
            app.input.clear();
            app.scroll_offset = 0;
            return route_input(app, input, cmd_tx);
        },

        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let _ = cmd_tx.send(Command::Shutdown);
            return Ok(true);
        },

        KeyCode::Char(c) => {
            app.history_idx = None;
            app.input.push(c);
        },

        KeyCode::Backspace => {
            app.input.pop();
        },

        KeyCode::Up => {
            if app.input.is_empty() {
                app.scroll_offset += 1;
            } else {
                let len = app.history.len();
                if len > 0 {
                    let idx = match app.history_idx {
                        None => len - 1,
                        Some(0) => 0,
                        Some(i) => i - 1,
                    };
                    app.history_idx = Some(idx);
                    app.input = app.history[idx].clone();
                }
            }
        },

        KeyCode::Down => {
            if app.input.is_empty() {
                app.scroll_offset = app.scroll_offset.saturating_sub(1);
            } else if let Some(idx) = app.history_idx {
                if idx + 1 < app.history.len() {
                    let ni = idx + 1;
                    app.history_idx = Some(ni);
                    app.input = app.history[ni].clone();
                } else {
                    app.history_idx = None;
                    app.input.clear();
                }
            }
        },

        KeyCode::PageUp => {
            app.scroll_offset = app.scroll_offset.saturating_add(10);
        },

        KeyCode::PageDown => {
            app.scroll_offset = app.scroll_offset.saturating_sub(10);
        },

        KeyCode::Esc => {
            app.scroll_offset = 0;
        },

        _ => {},
    }
    Ok(false)
}

fn route_input(
    app: &mut App, input: String, cmd_tx: &mpsc::UnboundedSender<Command>,
) -> Result<bool> {
    if let Some(ref partner) = app.current_chat.clone() {
        if let Some(cmd_str) = input.strip_prefix('/') {
            let cmd_str = cmd_str.trim();
            if cmd_str == "exit" || cmd_str == "chat" {
                app.system(format!("Left chat with '{}'", partner));
                app.current_chat = None;
                return Ok(false);
            }
            return dispatch(app, cmd_str, cmd_tx);
        }
        let msg_id = app.next_msg_id();
        // Optimistic update: show message immediately
        let t = format_time(crate::app::now());
        let text = format!("[{}] → {}: {}", t, partner, input);
        app.push(DisplayLine { kind: LineKind::Outgoing, text });
        let _ = cmd_tx.send(Command::SendMessage {
            conv_id: partner.clone(),
            msg_id,
            content: Content::Text(input),
        });
        return Ok(false);
    }
    let raw = input.strip_prefix('/').unwrap_or(&input);
    dispatch(app, raw, cmd_tx)
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

fn dispatch(app: &mut App, cmd: &str, cmd_tx: &mpsc::UnboundedSender<Command>) -> Result<bool> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(false);
    }

    match parts[0] {
        "quit" | "exit" => {
            let _ = cmd_tx.send(Command::Shutdown);
            return Ok(true);
        },
        "help" => {
            for line in HELP_LINES {
                app.system(*line);
            }
        },
        "list" | "peers" => {
            let _ = cmd_tx.send(Command::ListPeers);
        },
        "connect" => {
            if parts.len() != 2 {
                app.system("Usage: connect <ip>  |  connect <ip:port>  |  connect <node_id>");
            } else {
                let _ = cmd_tx.send(Command::Connect { addr: parts[1].to_string() });
            }
        },
        "disconnect" => {
            if parts.len() != 2 {
                app.system("Usage: disconnect <node_id>");
            } else {
                let _ = cmd_tx.send(Command::Disconnect { peer_id: parts[1].to_string() });
            }
        },
        "chat" => {
            if parts.len() >= 2 {
                let node_id = parts[1].to_string();
                app.current_chat = Some(node_id.clone());
                app.system(format!(
                    "Entering chat with '{}'. Type to send, /exit to leave.",
                    node_id
                ));
            } else if let Some(ref p) = app.current_chat.clone() {
                let p = p.clone();
                app.current_chat = None;
                app.system(format!("Left chat with '{}'", p));
            } else {
                app.system("Not in chat mode");
            }
        },
        "send" => {
            if parts.len() >= 3 {
                let msg_id = app.next_msg_id();
                let to = parts[1];
                let body = parts[2..].join(" ");
                let t = format_time(crate::app::now());
                let text = format!("[{}] → {}: {}", t, to, body);
                app.push(DisplayLine { kind: LineKind::Outgoing, text });
                let _ = cmd_tx.send(Command::SendMessage {
                    conv_id: to.to_string(),
                    msg_id,
                    content: Content::Text(body),
                });
            } else {
                app.system("Usage: send <node_id> <message>");
            }
        },
        "history" => {
            if parts.len() >= 2 {
                let _ = cmd_tx.send(Command::GetHistory {
                    conv_id: parts[1].to_string(),
                    before: None,
                    limit: 32,
                });
            } else if let Some(ref p) = app.current_chat {
                let _ = cmd_tx.send(Command::GetHistory {
                    conv_id: p.clone(),
                    before: None,
                    limit: 32,
                });
            } else {
                app.system("Usage: history <node_id>");
            }
        },
        other => {
            app.system(format!("Unknown command '{}'. Type 'help' for available commands.", other));
        },
    }

    Ok(false)
}

// ---------------------------------------------------------------------------
// AppEvent → UI state
// ---------------------------------------------------------------------------

fn handle_notify(app: &mut App, event: Notify) -> bool {
    match event {
        Notify::MessageReceived { conv_id, msg } => {
            let t = format_time(msg.timestamp);
            let text_content = match &msg.content {
                Content::Text(s) => s.as_str(),
            };
            let text = format!("[{}] {} ({}): {}", t, msg.from, conv_id, text_content);
            app.push(DisplayLine { kind: LineKind::Incoming, text });
        },

        Notify::MessageAck { msg_id, status } => {
            match status {
                MessageStatus::Sent => {},
                MessageStatus::Failed(err) => {
                    app.system(format!("Send failed [{}]: {}", msg_id, err));
                },
            }
        },

        Notify::PeerOnline { peer_id, peer_name, addr } => {
            if !app.online_peers.contains(&peer_id) {
                app.online_peers.push(peer_id.clone());
            }
            app.system(format!("{} ({}) connected from {}", peer_name, peer_id, addr));
        },

        Notify::PeerOffline { peer_id } => {
            app.online_peers.retain(|p| p != &peer_id);
            app.system(format!("{} disconnected", peer_id));
        },

        Notify::PeerList { peers } => {
            if peers.is_empty() {
                app.system("No connected peers".to_string());
            } else {
                app.system(format!("Connected peers ({}):", peers.len()));
                for p in peers {
                    app.push(DisplayLine {
                        kind: LineKind::System,
                        text: format!("  {} ({}) - {}", p.peer_name, p.peer_id, p.addr),
                    });
                }
            }
        },

        Notify::History { conv_id, messages } => {
            if messages.is_empty() {
                app.system(format!("No history for '{}'", conv_id));
            } else {
                app.system(format!("History for '{}' ({} messages):", conv_id, messages.len()));
                for msg in messages {
                    let t = format_time(msg.timestamp);
                    let text_content = match &msg.content {
                        Content::Text(s) => s.as_str(),
                    };
                    let status_tag = match &msg.status {
                        MessageStatus::Sent => "",
                        MessageStatus::Failed(_) => " [FAILED]",
                    };
                    let text = format!("  [{}] {} ({}): {}{}", t, msg.from, msg.msg_id, text_content, status_tag);
                    app.push(DisplayLine { kind: LineKind::System, text });
                }
            }
        },

        Notify::Notice { level, content } => {
            let prefix = match level {
                NoticeLevel::Info => "",
                NoticeLevel::Error => "[ERROR] ",
            };
            app.system(format!("{}{}", prefix, content));
        },
    }
    false
}
