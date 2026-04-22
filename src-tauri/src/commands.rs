//! Tauri command handlers — each maps to an api::Command variant.

use crate::api::{Command, Content};
use crate::bridge;

/// Initialize backend with user identity. Called once from the setup screen.
#[tauri::command]
pub fn setup(
    handle: tauri::AppHandle,
    node_id: String,
    username: String,
    port: u16,
) -> Result<SetupResult, String> {
    if bridge::is_started() {
        return Err("Backend already started".to_string());
    }

    let listen_addr = crate::resolve_listen_addr(port)
        .map_err(|e| e.to_string())?;
    let lan_ip = crate::local_ip();

    bridge::start_backend(handle, node_id, username, port)
        .map_err(|e| e.to_string())?;

    Ok(SetupResult {
        listen_addr: listen_addr.to_string(),
        lan_ip,
    })
}

#[derive(serde::Serialize)]
pub struct SetupResult {
    pub listen_addr: String,
    pub lan_ip: String,
}

#[tauri::command]
pub fn connect(addr: String) -> Result<(), String> {
    tracing::info!("connect command received: {}", addr);
    bridge::cmd_tx()
        .send(Command::Connect { addr })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn disconnect(peer_id: String) -> Result<(), String> {
    bridge::cmd_tx()
        .send(Command::Disconnect { peer_id })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn send_message(conv_id: String, msg_id: String, content: String) -> Result<(), String> {
    bridge::cmd_tx()
        .send(Command::SendMessage {
            conv_id,
            msg_id,
            content: Content::Text(content),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_history(conv_id: String, before: Option<u64>, limit: u32) -> Result<(), String> {
    bridge::cmd_tx()
        .send(Command::GetHistory { conv_id, before, limit })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_peers() -> Result<(), String> {
    bridge::cmd_tx()
        .send(Command::ListPeers)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn shutdown() -> Result<(), String> {
    bridge::cmd_tx()
        .send(Command::Shutdown)
        .map_err(|e| e.to_string())
}
