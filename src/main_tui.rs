//! Entry point – parse args, init logging, resolve port, run setup TUI,
//! then hand control to `App`.

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{error, info};

mod api;
mod app;
mod net;
mod network;
mod node;
mod ui;

use app::App;

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

const DEFAULT_PORTS: &[u16] = &[9000, 9001, 9002, 9003, 9004];

#[derive(Parser, Debug)]
#[command(name = "p2p-chat")]
#[command(about = "A peer-to-peer chat application")]
struct Args {
    /// Listening port. 0 (default) = try DEFAULT_PORTS [9000-9004] in order.
    #[arg(short = 'P', long, default_value = "0")]
    port: u16,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let log_file = std::fs::OpenOptions::new()
        .create(true).append(true).open("p2p-chat.log")?;
    tracing_subscriber::fmt().with_writer(log_file).with_ansi(false).init();

    let args = Args::parse();

    // ── Resolve listen address ───────────────────────────────────────────────
    let listen_addr = resolve_listen_addr(args.port)?;

    // ── Setup TUI: collect node_id / username ────────────────────────────────
    let (node_id, username, terminal) = ui::run_setup(listen_addr).await?;
    info!("Setup complete: node='{}' username='{}'", node_id, username);

    // ── Create channels ──────────────────────────────────────────────────────
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    // ── Create and start App ─────────────────────────────────────────────────
    let (mut app, net_rx) = App::new(
        node_id.clone(), username.clone(), listen_addr, event_tx,
    )?;

    let app_handle = tokio::spawn(async move {
        if let Err(e) = app.run(net_rx, cmd_rx).await {
            error!("App error: {}", e);
        }
    });

    // ── TUI (blocks until quit) ──────────────────────────────────────────────
    if let Err(e) = ui::run_tui(terminal, event_rx, cmd_tx, listen_addr, node_id, username).await {
        error!("TUI error: {}", e);
    }

    // ── Wait for the app loop to drain ───────────────────────────────────────
    let _ = tokio::time::timeout(Duration::from_secs(2), app_handle).await;
    Ok(())
}

/// Try to find an available port. If `port` is non-zero, use it directly.
/// Otherwise probe DEFAULT_PORTS and return the first available one.
fn resolve_listen_addr(port: u16) -> Result<SocketAddr> {
    if port != 0 {
        return Ok(SocketAddr::from(([0, 0, 0, 0], port)));
    }
    for &p in DEFAULT_PORTS {
        let addr = SocketAddr::from(([0, 0, 0, 0], p));
        if std::net::TcpListener::bind(addr).is_ok() {
            return Ok(addr);
        }
    }
    anyhow::bail!("No available port in {:?}", DEFAULT_PORTS)
}
