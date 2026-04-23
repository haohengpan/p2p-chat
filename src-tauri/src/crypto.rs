//! Cryptographic utilities — password hashing, key derivation, AES-256-GCM encryption.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

const PBKDF2_ROUNDS: u32 = 100_000;
const NONCE_LEN: usize = 12;

/// Generate a random 32-byte salt (returned as hex string).
pub fn generate_salt() -> String {
    let mut salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    hex_encode(&salt)
}

/// Hash a password with PBKDF2-HMAC-SHA256 (returned as hex string).
/// Used for login verification — stored in profile.json.
pub fn hash_password(password: &str, salt_hex: &str) -> String {
    let salt = hex_decode(salt_hex);
    let mut hash = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ROUNDS, &mut hash);
    hex_encode(&hash)
}

/// Derive a 256-bit encryption key from password + salt.
/// Uses a different context than hash_password to produce a different output.
pub fn derive_key(password: &str, salt_hex: &str) -> [u8; 32] {
    let salt = hex_decode(salt_hex);
    // Append a domain separator so the key differs from the password hash
    let mut key_salt = salt.clone();
    key_salt.extend_from_slice(b"_encryption_key");
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &key_salt, PBKDF2_ROUNDS, &mut key);
    key
}

/// Encrypt plaintext with AES-256-GCM.
/// Output format: [12-byte nonce][ciphertext+tag]
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow!("cipher init: {}", e))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, plaintext)
        .map_err(|e| anyhow!("encrypt: {}", e))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt data produced by `encrypt`.
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < NONCE_LEN {
        return Err(anyhow!("ciphertext too short"));
    }

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow!("cipher init: {}", e))?;

    let nonce = Nonce::from_slice(&data[..NONCE_LEN]);
    let ciphertext = &data[NONCE_LEN..];

    cipher.decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("decrypt: {}", e))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0))
        .collect()
}
