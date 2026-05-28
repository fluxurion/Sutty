//! Session configuration management with AES-256-GCM encrypted passwords.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Derive a 32-byte encryption key from a fixed seed.
fn derive_key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sutty-session-v2-fixed-key");
    let hash = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&hash);
    key
}

/// Encrypt bytes → base64-encoded string (nonce || ciphertext).
fn encrypt(plaintext: &[u8]) -> Result<String> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("encryption failed: {:?}", e))?;
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &combined))
}

/// Decrypt base64-encoded string → bytes.
fn decrypt(encoded: &str) -> Result<Vec<u8>> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    let combined = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
        .context("base64 decode failed")?;
    if combined.len() < 12 {
        anyhow::bail!("ciphertext too short");
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed: {:?}", e))
}

/// A saved SSH session — password is stored encrypted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Encrypted password (or None).
    #[serde(default)]
    pub encrypted_password: Option<String>,
    /// Path to private key (or None).
    #[serde(default)]
    pub key_file: Option<String>,
    /// Timestamp of last use (epoch seconds).
    #[serde(default)]
    pub last_used: u64,
}

impl SessionConfig {
    /// Decrypt and return the stored password, if any.
    pub fn password(&self) -> Option<String> {
        self.encrypted_password
            .as_ref()
            .and_then(|enc| decrypt(enc).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    /// Set the password, encrypting it for storage.
    pub fn set_password(&mut self, plain: &str) {
        match encrypt(plain.as_bytes()) {
            Ok(enc) => self.encrypted_password = Some(enc),
            Err(_) => {} // silently fail — password won't be stored
        }
    }
}

/// Persistent session store + last-used tracking.
pub struct SessionManager {
    path: PathBuf,
    sessions: Vec<SessionConfig>,
    /// Name of the last-used session (for auto-load).
    pub last_session: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    sessions: Vec<SessionConfig>,
    last_session: Option<String>,
}

impl SessionManager {
    pub fn load() -> Result<Self> {
        let path = sessions_path();
        let (sessions, last_session) = if path.exists() {
            let data = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let store: StoreFile = serde_json::from_str(&data).unwrap_or(StoreFile {
                sessions: vec![],
                last_session: None,
            });
            (store.sessions, store.last_session)
        } else {
            (vec![], None)
        };
        Ok(Self { path, sessions, last_session })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = StoreFile {
            sessions: self.sessions.clone(),
            last_session: self.last_session.clone(),
        };
        let data = serde_json::to_string_pretty(&store)?;
        std::fs::write(&self.path, data)?;
        Ok(())
    }

    pub fn sessions(&self) -> &[SessionConfig] {
        &self.sessions
    }

    pub fn find(&self, name: &str) -> Option<&SessionConfig> {
        self.sessions.iter().find(|s| s.name == name)
    }

    pub fn upsert(&mut self, config: SessionConfig) -> Result<()> {
        if let Some(existing) = self.sessions.iter_mut().find(|s| s.name == config.name) {
            *existing = config;
        } else {
            self.sessions.push(config);
        }
        self.save()
    }

    pub fn delete(&mut self, name: &str) -> Result<()> {
        self.sessions.retain(|s| s.name != name);
        if self.last_session.as_deref() == Some(name) {
            self.last_session = None;
        }
        self.save()
    }

    /// Mark a session as last-used and persist.
    pub fn touch_last_used(&mut self, name: &str) -> Result<()> {
        self.last_session = Some(name.to_string());
        if let Some(s) = self.sessions.iter_mut().find(|s| s.name == name) {
            s.last_used = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }
        self.save()
    }
}

fn sessions_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("sutty").join("sessions.json")
}
