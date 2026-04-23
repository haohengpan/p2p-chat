use std::sync::OnceLock;

use anyhow::Result;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::api::Command;
use crate::app::App;
use crate::storage::Storage;

/// Global command sender — accessed by Tauri command handlers.
static CMD_TX: OnceLock<mpsc::UnboundedSender<Command>> = OnceLock::new();

/// Global storage — shared with command handlers for load_profile etc.
static STORAGE: OnceLock<Storage> = OnceLock::new();

/// Get the global command sender. Panics if backend is not started.
pub fn cmd_tx() -> &'static mpsc::UnboundedSender<Command> {
    CMD_TX.get().expect("backend not started — call setup first")
}

/// Get the global storage.
pub fn storage() -> &'static Storage {
    STORAGE.get().expect("storage not initialized")
}

/// Check if backend has been started.
pub fn is_started() -> bool {
    CMD_TX.get().is_some()
}

/// Initialize storage (called early, before setup).
pub fn init_storage(storage: Storage) {
    let _ = STORAGE.set(storage);
}

/// Start the backend: create App, spawn event loop, forward Notify to Tauri events.
pub fn start_backend(
    handle: AppHandle,
    node_id: String,
    username: String,
    port: u16,
) -> Result<()> {
    let listen_addr = crate::resolve_listen_addr(port)?;

    let (notify_tx, mut notify_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    // Store global sender (only once)
    let _ = CMD_TX.set(cmd_tx);

    // Load saved data for this user
    let saved_history = storage().load_all_history();
    let saved_peers = storage().load_peers();

    let (mut app, net_rx) = App::new(
        node_id, username, listen_addr, notify_tx, saved_history, saved_peers,
    )?;

    // Spawn App::run in Tauri's tokio runtime
    tauri::async_runtime::spawn(async move {
        if let Err(e) = app.run(net_rx, cmd_rx).await {
            tracing::error!("App error: {}", e);
        }
    });

    // Forward Notify events to the frontend via Tauri events
    tauri::async_runtime::spawn(async move {
        while let Some(notify) = notify_rx.recv().await {
            tracing::info!("emit notify: {:?}", notify);
            if let Err(e) = handle.emit("notify", &notify) {
                tracing::error!("emit error: {}", e);
            }
        }
    });

    tracing::info!("Backend started on {}", listen_addr);
    Ok(())
}
