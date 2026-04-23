//! Persistent storage — profile, peers, chat history, logs.
//!
//! Data is stored under the platform-standard app data directory:
//!   Windows: %APPDATA%/com.p2pchat.app/
//!   Linux:   ~/.config/com.p2pchat.app/
//!
//! Layout (per-user):
//!   p2p-chat.log                       — shared log
//!   users/{node_id}/profile.json       — {node_id, username, port, password_hash, salt}
//!   users/{node_id}/peers.json         — [{peer_id, peer_name, addr}]
//!   users/{node_id}/history/{peer}.bin — encrypted chat history

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::api::{MessageInfo, PeerInfo};
use crate::crypto;

// ---------------------------------------------------------------------------
// Profile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub node_id: String,
    pub username: String,
    pub port: u16,
    /// PBKDF2 hash of password (hex-encoded), for login verification.
    #[serde(default)]
    pub password_hash: String,
    /// Random salt used for password hashing and key derivation (hex-encoded).
    #[serde(default)]
    pub salt: String,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub struct Storage {
    /// Root app data directory.
    dir: PathBuf,
    /// Currently active user's node_id (set after login/setup).
    active_user: RwLock<Option<String>>,
    /// Encryption key derived from password (set after login/setup).
    encryption_key: RwLock<Option<[u8; 32]>>,
}

impl Storage {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            active_user: RwLock::new(None),
            encryption_key: RwLock::new(None),
        }
    }

    /// Ensure base directories exist.
    pub fn init(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(self.dir.join("users"))?;
        Ok(())
    }

    /// Set the active user and encryption key after login.
    pub fn set_active_user(&self, node_id: &str, key: [u8; 32]) -> anyhow::Result<()> {
        let user_dir = self.user_dir(node_id);
        std::fs::create_dir_all(user_dir.join("history"))?;
        *self.active_user.write().unwrap() = Some(node_id.to_string());
        *self.encryption_key.write().unwrap() = Some(key);
        Ok(())
    }

    /// Path to the shared log file.
    pub fn log_path(&self) -> PathBuf {
        self.dir.join("p2p-chat.log")
    }

    pub fn data_dir(&self) -> &Path {
        &self.dir
    }

    /// Get the user-specific directory.
    fn user_dir(&self, node_id: &str) -> PathBuf {
        self.dir.join("users").join(Self::sanitize(node_id))
    }

    /// Get the active user's directory.
    fn active_dir(&self) -> PathBuf {
        let guard = self.active_user.read().unwrap();
        let node_id = guard.as_deref().expect("no active user");
        self.user_dir(node_id)
    }

    /// Get the encryption key. Panics if not set.
    fn key(&self) -> [u8; 32] {
        self.encryption_key.read().unwrap().expect("encryption key not set")
    }

    fn sanitize(name: &str) -> String {
        name.chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect()
    }

    // -- Profiles --

    /// List all registered profiles.
    pub fn list_profiles(&self) -> Vec<Profile> {
        let users_dir = self.dir.join("users");
        let mut profiles = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&users_dir) {
            for entry in entries.flatten() {
                let path = entry.path().join("profile.json");
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(p) = serde_json::from_str::<Profile>(&data) {
                        profiles.push(p);
                    }
                }
            }
        }
        profiles
    }

    /// Load a specific user's profile.
    pub fn load_profile(&self, node_id: &str) -> Option<Profile> {
        let path = self.user_dir(node_id).join("profile.json");
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Save profile for a user.
    pub fn save_profile(&self, profile: &Profile) -> anyhow::Result<()> {
        let user_dir = self.user_dir(&profile.node_id);
        std::fs::create_dir_all(&user_dir)?;
        let path = user_dir.join("profile.json");
        let data = serde_json::to_string_pretty(profile)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    // -- Peers (active user) --

    pub fn load_peers(&self) -> Vec<PeerInfo> {
        let path = self.active_dir().join("peers.json");
        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    pub fn save_peers(&self, peers: &[PeerInfo]) -> anyhow::Result<()> {
        let path = self.active_dir().join("peers.json");
        let data = serde_json::to_string_pretty(peers)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    // -- Encrypted chat history (active user) --

    pub fn load_history(&self, peer_id: &str) -> Vec<MessageInfo> {
        let path = self.history_path(peer_id);
        let ciphertext = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };
        match crypto::decrypt(&self.key(), &ciphertext) {
            Ok(plaintext) => serde_json::from_slice(&plaintext).unwrap_or_default(),
            Err(e) => {
                tracing::error!("decrypt history for {}: {}", peer_id, e);
                Vec::new()
            }
        }
    }

    pub fn load_all_history(&self) -> HashMap<String, Vec<MessageInfo>> {
        let mut all = HashMap::new();
        let history_dir = self.active_dir().join("history");
        if let Ok(entries) = std::fs::read_dir(&history_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "bin") {
                    if let Some(peer_id) = path.file_stem().and_then(|s| s.to_str()) {
                        let ciphertext = match std::fs::read(&path) {
                            Ok(d) => d,
                            Err(_) => continue,
                        };
                        match crypto::decrypt(&self.key(), &ciphertext) {
                            Ok(plaintext) => {
                                if let Ok(msgs) = serde_json::from_slice::<Vec<MessageInfo>>(&plaintext) {
                                    all.insert(peer_id.to_string(), msgs);
                                }
                            }
                            Err(e) => {
                                tracing::error!("decrypt history for {}: {}", peer_id, e);
                            }
                        }
                    }
                }
            }
        }
        all
    }

    pub fn save_history(&self, peer_id: &str, messages: &[MessageInfo]) -> anyhow::Result<()> {
        let path = self.history_path(peer_id);
        let plaintext = serde_json::to_vec(messages)?;
        let ciphertext = crypto::encrypt(&self.key(), &plaintext)?;
        std::fs::write(path, ciphertext)?;
        Ok(())
    }

    fn history_path(&self, peer_id: &str) -> PathBuf {
        self.active_dir().join("history").join(format!("{}.bin", Self::sanitize(peer_id)))
    }
}
