// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # Nostr Bridge Module
//!
//! This module handles:
//! - Nostr keypair generation and management
//! - NIP-04 encrypted message forwarding
//! - Relay connection management
//! - Message formatting from qaul to Nostr

use crate::state::NostrKeyInfo;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};

/// Nostr key wrapper for secp256k1 keys
pub struct NostrKeys {
    keys: Keys,
}

impl NostrKeys {
    /// Generate new random Nostr keypair
    pub fn generate() -> Self {
        let keys = Keys::generate();
        NostrKeys { keys }
    }

    /// Create NostrKeys from existing secret key bytes
    pub fn from_secret_key(secret_bytes: Vec<u8>) -> Result<Self, String> {
        if secret_bytes.len() != 32 {
            return Err("Secret key must be 32 bytes".to_string());
        }

        let secret_key = SecretKey::from_slice(&secret_bytes)
            .map_err(|e| format!("Invalid secret key: {}", e))?;

        let keys = Keys::new(secret_key);
        Ok(NostrKeys { keys })
    }

    /// Create NostrKeys from hex-encoded secret key
    pub fn from_hex(hex_str: &str) -> Result<Self, String> {
        let secret_bytes =
            hex::decode(hex_str).map_err(|e| format!("Invalid hex string: {}", e))?;

        Self::from_secret_key(secret_bytes)
    }

    /// Get secret key as raw bytes
    pub fn secret_key(&self) -> &[u8] {
        self.keys.secret_key().as_secret_bytes()
    }

    /// Get public key as raw bytes
    pub fn public_key(&self) -> Vec<u8> {
        self.keys.public_key().to_bytes().to_vec()
    }

    /// Get secret key as hex string for persistence
    pub fn secret_key_hex(&self) -> String {
        hex::encode(self.secret_key())
    }

    /// Get public key as hex string
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key())
    }

    /// Encode public key to npub format (bech32)
    pub fn to_npub(&self) -> String {
        self.keys.public_key().to_bech32().unwrap()
    }

    /// Encode secret key to nsec format (bech32)
    pub fn to_nsec(&self) -> String {
        self.keys.secret_key().to_bech32().unwrap()
    }

    /// Convert to NostrKeyInfo with all key formats
    pub fn to_info(&self) -> NostrKeyInfo {
        NostrKeyInfo {
            private_key_hex: self.secret_key_hex(),
            public_key_hex: self.public_key_hex(),
            npub: self.to_npub(),
            nsec: self.to_nsec(),
        }
    }

    /// Create NostrKeys from NostrKeyInfo (loads from any available format)
    pub fn from_info(info: &NostrKeyInfo) -> Result<Self, String> {
        // Prefer loading from nsec (bech32 format), fallback to hex
        if info.nsec.starts_with("nsec1") {
            match SecretKey::from_bech32(&info.nsec) {
                Ok(secret_key) => {
                    let keys = Keys::new(secret_key);
                    return Ok(NostrKeys { keys });
                }
                Err(_) => {
                    // Fall through to hex loading
                }
            }
        }

        // Fallback to hex format
        Self::from_hex(&info.private_key_hex)
    }

    /// Get nostr-sdk Keys reference
    pub fn inner(&self) -> &Keys {
        &self.keys
    }
}

/// Nostr configuration for persistence
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NostrConfig {
    pub private_key: Option<String>,
    pub relays: Vec<String>,
    pub target_npub: String,
}

impl Default for NostrConfig {
    fn default() -> Self {
        NostrConfig {
            private_key: None,
            relays: vec![
                "wss://relay.damus.io".to_string(),
                "wss://relay.primal.net".to_string(),
            ],
            target_npub: "npub1lkekzew7rvdrcssv93dt7q5p3zlglftwy8e9wzpdhlwx7qa55d3qw8cgwq"
                .to_string(),
        }
    }
}

/// Nostr bridge for forwarding qaul messages to Nostr network
#[derive(Clone)]
pub struct NostrBridge {
    client: Client,
    keys: Keys,
    target_pubkey: PublicKey,
}

impl NostrBridge {
    /// Initialize Nostr bridge with keys and configuration
    pub async fn new(nostr_keys: NostrKeys, config: &NostrConfig) -> Result<Self, String> {
        // Parse target npub
        let target_pubkey = PublicKey::from_bech32(&config.target_npub)
            .map_err(|e| format!("Invalid target npub: {}", e))?;

        // Get owned Keys for client
        let keys = nostr_keys.keys.clone();

        // Create client with keys (pass by value, not reference)
        let client = Client::new(keys.clone());

        // Add relays
        for relay_url in &config.relays {
            client
                .add_relay(relay_url)
                .await
                .map_err(|e| format!("Failed to add relay {}: {}", relay_url, e))?;
        }

        // Connect to relays
        client.connect().await;

        log::info!("[Nostr] Connected to {} relays", config.relays.len());
        log::info!("[Nostr] Target recipient: {}", config.target_npub);

        Ok(NostrBridge {
            client,
            keys,
            target_pubkey,
        })
    }

    /// Format qaul message for Nostr relay
    pub fn format_message(
        sender_id: &[u8],
        group_id: &[u8],
        content: &str,
        timestamp: u64,
    ) -> String {
        let sender_base58 = bs58::encode(sender_id).into_string();
        let group_base58 = bs58::encode(group_id).into_string();

        // Convert Unix timestamp to human-readable format
        let datetime = chrono::DateTime::from_timestamp(timestamp as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| format!("timestamp: {}", timestamp));

        format!(
            "[qaul] Message received\nFrom: {}\nGroup: {}\nTime: {}\n\n{}",
            sender_base58, group_base58, datetime, content
        )
    }

    /// Send encrypted DM to target recipient using NIP-04
    pub async fn send_encrypted_dm(&self, message: &str) -> Result<(), String> {
        log::info!("send_encrypted_dm: {}", message);

        // Encrypt message using NIP-04
        let encrypted_content =
            nip04::encrypt(&self.keys.secret_key(), &self.target_pubkey, message)
                .map_err(|e| format!("Failed to encrypt message: {}", e))?;

        // Create encrypted DM event (kind 4)
        let event = EventBuilder::new(Kind::EncryptedDirectMessage, encrypted_content)
            .tag(Tag::public_key(self.target_pubkey))
            .sign_with_keys(&self.keys)
            .map_err(|e| format!("Failed to sign event: {}", e))?;

        // Send to all connected relays
        self.client
            .send_event(&event)
            .await
            .map_err(|e| format!("Failed to send event: {}", e))?;

        log::info!("[Nostr] Sent encrypted DM ({} bytes)", message.len());

        Ok(())
    }

    /// Forward qaul message to Nostr network
    pub async fn forward_qaul_message(
        &self,
        sender_id: &[u8],
        group_id: &[u8],
        content: &str,
        timestamp: u64,
    ) -> Result<(), String> {
        let formatted_message = Self::format_message(sender_id, group_id, content, timestamp);
        self.send_encrypted_dm(&formatted_message).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nostr_keys_generation() {
        let keys = NostrKeys::generate();
        assert_eq!(keys.secret_key().len(), 32);
        assert_eq!(keys.public_key().len(), 32);
    }

    #[test]
    fn test_nostr_keys_serialization() {
        let keys = NostrKeys::generate();
        let hex = keys.secret_key_hex();

        assert_eq!(hex.len(), 64); // 32 bytes = 64 hex chars

        let restored = NostrKeys::from_hex(&hex).unwrap();
        assert_eq!(keys.public_key(), restored.public_key());
    }

    #[test]
    fn test_npub_format() {
        let keys = NostrKeys::generate();
        let npub = keys.to_npub();

        assert!(npub.starts_with("npub1"));
        assert!(npub.len() > 20);
    }

    #[test]
    fn test_message_formatting() {
        let sender = vec![1, 2, 3, 4];
        let group = vec![5, 6, 7, 8];
        let content = "Hello Nostr!";
        let timestamp = 1696348800; // 2023-10-03 14:00:00 UTC

        let formatted = NostrBridge::format_message(&sender, &group, content, timestamp);

        assert!(formatted.contains("[qaul]"));
        assert!(formatted.contains("From:"));
        assert!(formatted.contains("Group:"));
        assert!(formatted.contains("Time:"));
        assert!(formatted.contains("Hello Nostr!"));
    }

    #[test]
    fn test_config_default() {
        let config = NostrConfig::default();

        assert_eq!(config.relays.len(), 2);
        assert!(config.relays.contains(&"wss://relay.damus.io".to_string()));
        assert!(config.relays.contains(&"wss://relay.primal.net".to_string()));
        assert!(config.target_npub.starts_with("npub1"));
    }

    #[test]
    fn test_nostr_key_info_conversion() {
        let keys = NostrKeys::generate();

        // Convert to info
        let info = keys.to_info();

        // Verify all fields are populated
        assert_eq!(info.private_key_hex.len(), 64); // 32 bytes in hex
        assert_eq!(info.public_key_hex.len(), 64);
        assert!(info.npub.starts_with("npub1"));
        assert!(info.nsec.starts_with("nsec1"));

        // Load from info
        let restored = NostrKeys::from_info(&info).unwrap();

        // Verify keys match
        assert_eq!(keys.public_key(), restored.public_key());
        assert_eq!(keys.secret_key(), restored.secret_key());
        assert_eq!(keys.to_npub(), restored.to_npub());
    }

    #[test]
    fn test_public_key_hex() {
        let keys = NostrKeys::generate();
        let hex = keys.public_key_hex();

        assert_eq!(hex.len(), 64); // 32 bytes = 64 hex chars
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_nsec_format() {
        let keys = NostrKeys::generate();
        let nsec = keys.to_nsec();

        assert!(nsec.starts_with("nsec1"));
        assert!(nsec.len() > 20);
    }
}
