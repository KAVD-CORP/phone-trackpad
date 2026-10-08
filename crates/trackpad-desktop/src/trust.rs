//! Trust store: our own Noise static keypair + the set of trusted phones.
//!
//! - Our private key lives in the OS credential store (`keyring`: Windows
//!   Credential Manager). It is never written to disk or logged.
//! - Our public key + device name live in `%APPDATA%\wtrackpad\settings.json`
//!   (public data, shown in QR codes and mDNS).
//! - Trusted peers (name + static public key + timestamps) live in
//!   `%APPDATA%\wtrackpad\trusted.json`. Public keys are not secrets;
//!   revocation = deleting the entry, effective immediately.
//! - If the keyring key and the settings file disagree (one lost), the
//!   identity rotates: a fresh keypair replaces both. Old pairings break
//!   explicitly (peer keys no longer match) rather than silently wrong.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use trackpad_core::pairing::TrustedPeer;

const SERVICE: &str = "wtrackpad";
const DEVICE_KEY_USER: &str = "device-key";
const SETTINGS_FILE: &str = "settings.json";
const TRUST_FILE: &str = "trusted.json";

#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error("credential store failed")]
    Keyring(#[from] keyring::Error),
    #[error("storage failed: {0}")]
    Io(String),
    #[error("bad stored data: {0}")]
    Format(String),
    #[error("crypto failed")]
    Crypto,
}

/// Our long-term identity.
#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SettingsFile {
    public_key: String,
    device_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrustFile {
    peers: Vec<StoredPeer>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StoredPeer {
    name: String,
    pubkey: String,
    first_seen_ms: u64,
    last_seen_ms: u64,
}

fn config_dir() -> Result<PathBuf, TrustError> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(dirs_fallback)
        .ok_or_else(|| TrustError::Io("no config dir".to_string()))?;
    let dir = base.join("wtrackpad");
    std::fs::create_dir_all(&dir).map_err(|e| TrustError::Io(e.to_string()))?;
    Ok(dir)
}

#[cfg(not(test))]
fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".config"))
}

#[cfg(test)]
fn dirs_fallback() -> Option<PathBuf> {
    None
}

fn default_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "My Computer".to_string())
}

impl DeviceIdentity {
    /// Load or create (production paths). Tests use [`DeviceIdentity::test`].
    pub fn load_or_create() -> Result<Self, TrustError> {
        Self::load_or_create_in(&config_dir()?)
    }

    fn load_or_create_in(dir: &std::path::Path) -> Result<Self, TrustError> {
        let entry = keyring::Entry::new(SERVICE, DEVICE_KEY_USER)?;
        let stored_priv = entry.get_secret().ok();
        let stored_settings = std::fs::read_to_string(dir.join(SETTINGS_FILE))
            .ok()
            .and_then(|s| serde_json::from_str::<SettingsFile>(&s).ok());

        match (stored_priv, stored_settings) {
            (Some(priv_key), Some(settings)) => {
                let public_key = B64
                    .decode(&settings.public_key)
                    .map_err(|e| TrustError::Format(e.to_string()))?;
                if priv_key.len() != 32 || public_key.len() != 32 {
                    return Err(TrustError::Format("bad key length".to_string()));
                }
                Ok(Self {
                    private_key: priv_key,
                    public_key,
                    device_name: settings.device_name,
                })
            }
            _ => {
                // Missing half or mismatch: rotate to a fresh identity.
                let (private_key, public_key) =
                    trackpad_core::crypto::generate_keypair().map_err(|_| TrustError::Crypto)?;
                entry.set_secret(&private_key)?;
                let settings = SettingsFile {
                    public_key: B64.encode(&public_key),
                    device_name: default_device_name(),
                };
                let text = serde_json::to_string_pretty(&settings)
                    .map_err(|e| TrustError::Io(e.to_string()))?;
                std::fs::write(dir.join(SETTINGS_FILE), text)
                    .map_err(|e| TrustError::Io(e.to_string()))?;
                Ok(Self {
                    private_key,
                    public_key,
                    device_name: settings.device_name,
                })
            }
        }
    }

    #[cfg(test)]
    fn test() -> Self {
        let (private_key, public_key) = trackpad_core::crypto::generate_keypair().expect("keygen");
        Self {
            private_key,
            public_key,
            device_name: "test-pc".to_string(),
        }
    }
}

/// The set of trusted phones, persisted as JSON.
#[derive(Debug, Default)]
pub struct TrustStore {
    peers: Vec<TrustedPeer>,
}

impl TrustStore {
    pub fn load() -> Result<Self, TrustError> {
        Self::load_from(&config_dir()?)
    }

    fn load_from(dir: &std::path::Path) -> Result<Self, TrustError> {
        let text = std::fs::read_to_string(dir.join(TRUST_FILE))
            .unwrap_or_else(|_| r#"{"peers":[]}"#.to_string());
        let file: TrustFile =
            serde_json::from_str(&text).map_err(|e| TrustError::Format(e.to_string()))?;
        let mut peers = Vec::new();
        for p in file.peers {
            let raw = B64
                .decode(&p.pubkey)
                .map_err(|e| TrustError::Format(e.to_string()))?;
            if raw.len() != 32 {
                return Err(TrustError::Format("bad peer key".to_string()));
            }
            let mut pubkey = [0u8; 32];
            pubkey.copy_from_slice(&raw);
            peers.push(TrustedPeer {
                name: p.name,
                pubkey,
                first_seen_ms: p.first_seen_ms,
                last_seen_ms: p.last_seen_ms,
            });
        }
        Ok(Self { peers })
    }

    fn save_to(&self, dir: &std::path::Path) -> Result<(), TrustError> {
        let file = TrustFile {
            peers: self
                .peers
                .iter()
                .map(|p| StoredPeer {
                    name: p.name.clone(),
                    pubkey: B64.encode(p.pubkey),
                    first_seen_ms: p.first_seen_ms,
                    last_seen_ms: p.last_seen_ms,
                })
                .collect(),
        };
        let text =
            serde_json::to_string_pretty(&file).map_err(|e| TrustError::Io(e.to_string()))?;
        std::fs::write(dir.join(TRUST_FILE), text).map_err(|e| TrustError::Io(e.to_string()))
    }

    pub fn save(&self) -> Result<(), TrustError> {
        self.save_to(&config_dir()?)
    }

    pub fn peers(&self) -> &[TrustedPeer] {
        &self.peers
    }

    pub fn is_trusted(&self, pubkey: &[u8]) -> bool {
        self.peers.iter().any(|p| p.matches(pubkey))
    }

    pub fn add(&mut self, peer: TrustedPeer) {
        if let Some(existing) = self.peers.iter_mut().find(|p| p.matches(&peer.pubkey)) {
            existing.name = peer.name;
            existing.last_seen_ms = peer.last_seen_ms;
        } else {
            self.peers.push(peer);
        }
    }

    /// Revoke by public key. Returns true if anything was removed.
    pub fn revoke(&mut self, pubkey: &[u8]) -> bool {
        let before = self.peers.len();
        self.peers.retain(|p| !p.matches(pubkey));
        self.peers.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wtrackpad-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    #[test]
    fn trust_add_revoke_round_trip() {
        let dir = tmpdir("trust");
        let mut store = TrustStore::load_from(&dir).expect("empty load");
        assert!(!store.is_trusted(&[1u8; 32]));
        store.add(TrustedPeer {
            name: "phone".to_string(),
            pubkey: [1u8; 32],
            first_seen_ms: 10,
            last_seen_ms: 20,
        });
        assert!(store.is_trusted(&[1u8; 32]));
        store.save_to(&dir).expect("save");
        let reloaded = TrustStore::load_from(&dir).expect("reload");
        assert!(reloaded.is_trusted(&[1u8; 32]));
        assert_eq!(reloaded.peers()[0].name, "phone");
        let mut reloaded = reloaded;
        assert!(reloaded.revoke(&[1u8; 32]));
        assert!(!reloaded.revoke(&[1u8; 32]));
        assert!(!reloaded.is_trusted(&[1u8; 32]));
    }

    #[test]
    fn identity_test_has_valid_keys() {
        let id = DeviceIdentity::test();
        assert_eq!(id.private_key.len(), 32);
        assert_eq!(id.public_key.len(), 32);
    }
}
