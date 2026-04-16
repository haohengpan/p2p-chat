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

const HELP_LINES: &[&str] = &[
    "Commands:",
    "  connect <ip>       - Connect by IP (probes ports 9000-9004 concurrently)",
    "  connect <ip:port>  - Connect to a peer at an explicit address",
    "  connect <node_id>  - Reconnect to a known peer (registry lookup)",
    "  chat <node_id>               - Enter focused chat mode with a peer",
    "  chat                         - Exit chat mode",
    "  list                         - Show currently connected peers",
    "  peers                        - Show all known peers (with online status)",
    "  send <node_id> <message>     - Send a direct message",
    "  broadcast <message>          - Broadcast to all connected peers",
    "  quit                         - Exit the application",
    "",
    "In chat mode:",
    "  <message>     - Send message to current chat partner",
    "  /exit         - Leave chat mode",
    "  /<command>    - Run any command (e.g. /list, /peers)",
    "",
    "Keys: ↑↓ = scroll (empty input) or history | PageUp/Dn | Esc = bottom",
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

use crate::event::{AppEvent, DisplayLine, LineKind, NodeCommand};
use crate::network::format_time;

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
        }
    }
}

async fn setup_loop(
    terminal: &mut Term,
    listen_addr: SocketAddr,
    lan_ip: &str,
) -> Result<(String, String)> {
    let mut state = SetupState {
        node_id: String::new(),
        username: String::new(),
        focus: 0,
        error: None,
    };
    let mut keys = EventStream::new();

    loop {
        terminal.draw(|f| render_setup(f, &state, listen_addr, lan_ip))?;

        if let Some(ev) = keys.next().await {
            match ev? {
                Event::Key(key) => {
                    if let Some(result) = handle_setup_key(&mut state, key)? {
                        return Ok(result);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
}

fn handle_setup_key(
    state: &mut SetupState,
    key: crossterm::event::KeyEvent,
) -> Result<Option<(String, String)>> {
    state.error = None;

    match key.code {
        // Quit
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Err(anyhow::anyhow!("Setup cancelled by user"));
        }

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
        }

        // Switch field
        KeyCode::Tab | KeyCode::BackTab => {
            state.focus = 1 - state.focus;
        }
        KeyCode::Up => {
            state.focus = 0;
        }
        KeyCode::Down => {
            state.focus = 1;
        }

        // Edit active field
        KeyCode::Backspace => {
            if state.focus == 0 {
                state.node_id.pop();
            } else {
                state.username.pop();
            }
        }

        KeyCode::Char(c) => {
            if state.focus == 0 {
                // Silently reject spaces in node_id
                if c != ' ' {
                    state.node_id.push(c);
                }
            } else {
                state.username.push(c);
            }
        }

        _ => {}
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
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(dialog);
    f.render_widget(block, dialog);

    if inner.height < 10 || inner.width < 32 {
        f.render_widget(
            Paragraph::new("Terminal too small — please resize."),
            inner,
        );
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
        Paragraph::new(lan_text).style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        chunks[1],
    );

    // ── node_id ───────────────────────────────────────────────────────────
    let (nid_label, nid_border) = field_styles(state.focus == 0);
    f.render_widget(
        Paragraph::new("Node ID  (unique identifier, no spaces):")
            .style(nid_label),
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
        Paragraph::new("Username  (display name visible to peers):")
            .style(uname_label),
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
    let cx = (input_area.x + 1 + text_len as u16)
        .min(input_area.x + input_area.width.saturating_sub(2));
    let cy = input_area.y + 1;
    f.set_cursor_position((cx, cy));
}

/// Returns `(label_style, border_style)` for a focused / unfocused field.
fn field_styles(focused: bool) -> (Style, Style) {
    if focused {
        (
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Yellow),
        )
    } else {
        (
            Style::default().fg(Color::White),
            Style::default().fg(Color::DarkGray),
        )
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
        }
    }

    fn push(&mut self, line: DisplayLine) {
        if self.messages.len() >= MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(line);
    }

    fn system(&mut self, text: impl Into<String>) {
        self.push(DisplayLine {
            kind: LineKind::System,
            text: text.into(),
        });
    }
}

/// Run the main chat TUI.  Reuses `terminal` which was already set up by
/// `run_setup` (still in raw mode / alternate screen).  Restores the terminal
/// on exit.
///
/// Startup banner is built here from the node identity and listen address.
pub async fn run_tui(
    mut terminal: Term,
    event_rx: mpsc::UnboundedReceiver<AppEvent>,
    cmd_tx: mpsc::UnboundedSender<NodeCommand>,
    listen_addr: SocketAddr,
    node_id: String,
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
    app.system("Commands: help | connect | chat | list | peers | send | broadcast | quit");
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
    terminal: &mut Term,
    app: &mut App,
    mut event_rx: mpsc::UnboundedReceiver<AppEvent>,
    cmd_tx: mpsc::UnboundedSender<NodeCommand>,
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
                        if handle_app_event(app, ev) {
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

    let scroll_hint = if app.scroll_offset > 0 {
        format!(" ↑{} ", app.scroll_offset)
    } else {
        String::new()
    };

    f.render_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title + &scroll_hint)),
        chunks[0],
    );

    // ── input bar ──────────────────────────────────────────────────────────
    let (prompt, bar_title) = if let Some(ref p) = app.current_chat {
        (
            format!("[{}]> {}", p, app.input),
            " Chat mode — type to send | /exit to leave ",
        )
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
    app: &mut App,
    key: crossterm::event::KeyEvent,
    cmd_tx: &mpsc::UnboundedSender<NodeCommand>,
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
        }

        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let _ = cmd_tx.send(NodeCommand::Quit);
            return Ok(true);
        }

        KeyCode::Char(c) => {
            app.history_idx = None;
            app.input.push(c);
        }

        KeyCode::Backspace => {
            app.input.pop();
        }

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
        }

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
        }

        KeyCode::PageUp => {
            app.scroll_offset = app.scroll_offset.saturating_add(10);
        }

        KeyCode::PageDown => {
            app.scroll_offset = app.scroll_offset.saturating_sub(10);
        }

        KeyCode::Esc => {
            app.scroll_offset = 0;
        }

        _ => {}
    }
    Ok(false)
}

fn route_input(
    app: &mut App,
    input: String,
    cmd_tx: &mpsc::UnboundedSender<NodeCommand>,
) -> Result<bool> {
    if let Some(ref partner) = app.current_chat.clone() {
        if let Some(cmd_str) = input.strip_prefix('/') {
            let cmd_str = cmd_str.trim();
            if cmd_str == "exit" || cmd_str == "chat" {
                app.system(format!("Left chat with '{}'", partner));
                app.current_chat = None;
                let _ = cmd_tx.send(NodeCommand::Chat(None));
                return Ok(false);
            }
            return dispatch(app, cmd_str, cmd_tx);
        }
        let _ = cmd_tx.send(NodeCommand::SendMessage {
            to: partner.clone(),
            content: input,
        });
        return Ok(false);
    }
    let raw = input.strip_prefix('/').unwrap_or(&input);
    dispatch(app, raw, cmd_tx)
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

fn dispatch(
    app: &mut App,
    cmd: &str,
    cmd_tx: &mpsc::UnboundedSender<NodeCommand>,
) -> Result<bool> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(false);
    }

    match parts[0] {
        "quit" | "exit" => {
            let _ = cmd_tx.send(NodeCommand::Quit);
            return Ok(true);
        }
        "help" => {
            for line in HELP_LINES {
                app.system(*line);
            }
        }
        "list" => {
            let _ = cmd_tx.send(NodeCommand::ListConnected);
        }
        "peers" => {
            let _ = cmd_tx.send(NodeCommand::ListPeers);
        }
        "connect" => {
            if parts.len() != 2 {
                app.system("Usage: connect <ip>  |  connect <ip:port>  |  connect <node_id>");
            } else if parts[1].parse::<std::net::IpAddr>().is_ok() {
                // Pure IP → probe DEFAULT_PORTS concurrently
                let _ = cmd_tx.send(NodeCommand::ConnectByIp {
                    ip: parts[1].to_string(),
                });
            } else if parts[1].parse::<std::net::SocketAddr>().is_ok() {
                // ip:port → connect directly, node-id from handshake
                let _ = cmd_tx.send(NodeCommand::Connect {
                    addr: parts[1].to_string(),
                });
            } else {
                // node_id → registry lookup
                let _ = cmd_tx.send(NodeCommand::ConnectById {
                    node_id: parts[1].to_string(),
                });
            }
        }
        "chat" => {
            if parts.len() >= 2 {
                let node_id = parts[1].to_string();
                app.current_chat = Some(node_id.clone());
                app.system(format!(
                    "Entering chat with '{}'. Type to send, /exit to leave.",
                    node_id
                ));
                let _ = cmd_tx.send(NodeCommand::Chat(Some(node_id)));
            } else if let Some(ref p) = app.current_chat.clone() {
                let p = p.clone();
                app.current_chat = None;
                app.system(format!("Left chat with '{}'", p));
                let _ = cmd_tx.send(NodeCommand::Chat(None));
            } else {
                app.system("Not in chat mode");
            }
        }
        "send" => {
            if parts.len() >= 3 {
                let _ = cmd_tx.send(NodeCommand::SendMessage {
                    to: parts[1].to_string(),
                    content: parts[2..].join(" "),
                });
            } else {
                app.system("Usage: send <node_id> <message>");
            }
        }
        "broadcast" => {
            if parts.len() >= 2 {
                let _ = cmd_tx.send(NodeCommand::BroadcastMessage {
                    content: parts[1..].join(" "),
                });
            } else {
                app.system("Usage: broadcast <message>");
            }
        }
        other => {
            app.system(format!(
                "Unknown command '{}'. Type 'help' for available commands.",
                other
            ));
        }
    }

    Ok(false)
}

// ---------------------------------------------------------------------------
// AppEvent → UI state
// ---------------------------------------------------------------------------

fn handle_app_event(app: &mut App, event: AppEvent) -> bool {
    match event {
        AppEvent::MessageReceived {
            from_id,
            from_name,
            to,
            content,
            timestamp,
        } => {
            let t = format_time(timestamp);
            let text = match to {
                Some(_) => format!("[{}] {} ({}): {}", t, from_name, from_id, content),
                None => format!("[{}] {} ({}) → all: {}", t, from_name, from_id, content),
            };
            app.push(DisplayLine {
                kind: LineKind::Incoming,
                text,
            });
        }

        AppEvent::MessageSent {
            to,
            content,
            timestamp,
            ok_count,
            total,
            our_name,
            our_id,
        } => {
            let t = format_time(timestamp);
            let text = match to {
                Some(ref dest) => {
                    format!("[{}] {} ({}) → {}: {}", t, our_name, our_id, dest, content)
                }
                None => format!(
                    "[{}] {} ({}) → all [{}/{}]: {}",
                    t, our_name, our_id, ok_count, total, content
                ),
            };
            app.push(DisplayLine {
                kind: LineKind::Outgoing,
                text,
            });
        }

        AppEvent::PeerConnected { node_id, username } => {
            if !app.online_peers.contains(&node_id) {
                app.online_peers.push(node_id.clone());
            }
            app.system(format!("[SYSTEM] {} ({}) connected", node_id, username));
        }

        AppEvent::PeerDisconnected { node_id, reason } => {
            app.online_peers.retain(|p| p != &node_id);
            app.system(format!("[SYSTEM] {} disconnected: {}", node_id, reason));
        }

        AppEvent::SystemNotice(msg) => {
            app.system(format!("[SYSTEM] {}", msg));
        }

        AppEvent::CommandOutput(line) => {
            app.push(DisplayLine {
                kind: LineKind::System,
                text: line,
            });
        }

        AppEvent::NodeShutdown => {
            app.system("[SYSTEM] Node has shut down gracefully.");
            return true;
        }
    }
    false
}
