//! Tauri command handlers — each maps to an api::Command variant.

use crate::api::{Command, Content};
use crate::bridge;
use crate::crypto;
use crate::storage::Profile;

/// List all registered profiles (for login screen).
#[tauri::command]
pub fn list_profiles() -> Vec<Profile> {
    bridge::storage().list_profiles()
}

/// Load saved peers for a specific user (before backend starts).
#[tauri::command]
pub fn get_saved_peers() -> Vec<crate::api::PeerInfo> {
    bridge::storage().load_peers()
}

/// Load saved chat history for a conversation.
#[tauri::command]
pub fn load_history(conv_id: String) -> Vec<crate::api::MessageInfo> {
    bridge::storage().load_history(&conv_id)
}

/// Register a new account and start backend.
#[tauri::command]
pub fn setup(
    handle: tauri::AppHandle,
    node_id: String,
    username: String,
    password: String,
    port: u16,
) -> Result<SetupResult, String> {
    if bridge::is_started() {
        return Err("Backend already started".to_string());
    }

    let listen_addr = crate::resolve_listen_addr(port)
        .map_err(|e| e.to_string())?;
    let lan_ip = crate::local_ip();

    // Generate salt, hash password, derive encryption key
    let salt = crypto::generate_salt();
    let password_hash = crypto::hash_password(&password, &salt);
    let key = crypto::derive_key(&password, &salt);

    let profile = Profile {
        node_id: node_id.clone(),
        username: username.clone(),
        port,
        password_hash,
        salt,
    };
    if let Err(e) = bridge::storage().save_profile(&profile) {
        tracing::error!("save profile: {}", e);
    }
    if let Err(e) = bridge::storage().set_active_user(&node_id, key) {
        tracing::error!("set active user: {}", e);
    }

    bridge::start_backend(handle, node_id, username, port)
        .map_err(|e| e.to_string())?;

    Ok(SetupResult {
        listen_addr: listen_addr.to_string(),
        lan_ip,
    })
}

/// Login to an existing account and start backend.
#[tauri::command]
pub fn login(
    handle: tauri::AppHandle,
    node_id: String,
    password: String,
) -> Result<SetupResult, String> {
    if bridge::is_started() {
        return Err("Backend already started".to_string());
    }

    let profile = bridge::storage().load_profile(&node_id)
        .ok_or_else(|| format!("Profile '{}' not found", node_id))?;

    // Verify password
    let hash = crypto::hash_password(&password, &profile.salt);
    if hash != profile.password_hash {
        return Err("Incorrect password".to_string());
    }

    let listen_addr = crate::resolve_listen_addr(profile.port)
        .map_err(|e| e.to_string())?;
    let lan_ip = crate::local_ip();

    // Derive encryption key from password
    let key = crypto::derive_key(&password, &profile.salt);

    if let Err(e) = bridge::storage().set_active_user(&node_id, key) {
        tracing::error!("set active user: {}", e);
    }

    bridge::start_backend(handle, node_id, profile.username, profile.port)
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
            conv_id, msg_id,
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
