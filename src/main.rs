//! Entry point – parse args, init logging, bind port, run setup TUI, then
//! wire the node and chat TUI together.
//!
//! Port binding strategy and P2P-port constants live in `node.rs`; all
//! user-visible startup text is assembled in `ui.rs`.

use anyhow::Result;
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{info, error};

mod connection;
mod event;
mod message;
mod network;
mod node;
mod registry;
mod ui;

use event::{AppEvent, NodeCommand};
use node::P2PNode;

// ---------------------------------------------------------------------------
// CLI arguments (all optional)
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "p2p-chat")]
#[command(about = "A peer-to-peer chat application")]
struct Args {
    /// Listening port.  0 (default) = try DEFAULT_PORTS [9000-9004] in order.
    #[arg(short = 'P', long, default_value = "0")]
    port: u16,

    /// Peers to connect to on startup (format: ip:port:node_id)
    #[arg(short, long, num_args = 0..)]
    peers: Vec<String>,

    /// Path to the peer-registry JSON file
    #[arg(long, default_value = "known_peers.json")]
    peers_file: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    // Redirect all tracing output to a log file so it does not corrupt the TUI.
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("p2p-chat.log")?;
    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    let args = Args::parse();

    // ── Bind listener ────────────────────────────────────────────────────────
    let listener = node::bind_listener(args.port).await?;
    let listen_addr = listener.local_addr()?;

    // ── Setup TUI: collect node_id / username ────────────────────────────────
    let (node_id, username, terminal) = ui::run_setup(listen_addr).await?;
    info!("Setup complete: node='{}' username='{}'", node_id, username);

    // ── Create channels and node ─────────────────────────────────────────────
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<NodeCommand>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AppEvent>();

    let node = P2PNode::new(
        node_id.clone(),
        username.clone(),
        listen_addr,
        args.peers,
        &args.peers_file,
        event_tx,
    )?;

    let node_handle = tokio::spawn(async move {
        if let Err(e) = node.start(cmd_rx, listener).await {
            error!("Node error: {}", e);
        }
    });

    // ── Main chat TUI (blocks until user quits) ───────────────────────────────
    ui::run_tui(terminal, event_rx, cmd_tx, listen_addr, node_id, username).await?;

    // Give the node up to 2 s to finish graceful shutdown.
    let _ = tokio::time::timeout(tokio::time::Duration::from_secs(2), node_handle).await;

    Ok(())
}
