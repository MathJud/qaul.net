// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # Nostr Bridge Module
//!
//! This module provides bidirectional message bridging between qaul and Nostr networks:
//!
//! ## Features
//!
//! ### Outgoing (qaul → Nostr)
//! - Nostr keypair generation and management
//! - NIP-04 encrypted message forwarding to target recipients
//! - Relay connection management
//! - JSON message formatting with full metadata
//!
//! ### Incoming (Nostr → qaul)
//! - Subscribe to encrypted DMs on Nostr relays
//! - Poll for new messages every 1 second
//! - Decrypt NIP-04 encrypted messages
//! - Parse JSON payloads with group routing information
//! - Forward messages to specific qaul groups
//!
//! ## Expected JSON Format
//!
//! ### Incoming Messages (Nostr → qaul)
//! ```json
//! {
//!   "chat_group": "5HpnLkrF5xxTihbtBemjy6PzhL",  // Base58 encoded group ID
//!   "message_text": "Reply from Nostr user"
//! }
//! ```
//!
//! ### Outgoing Messages (qaul → Nostr)
//! ```json
//! {
//!   "user_name": "alice",
//!   "qaul_id": "5Z9Kni2tixYKT",
//!   "chat_group": "7BcDef3uvwXYZ",
//!   "message_id": "abc123",
//!   "sent_time": "2025-10-03T14:23:45Z",
//!   "message_text": "Hello from qaul!"
//! }
//! ```

use crate::state::NostrKeyInfo;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};

/// JSON payload structure for incoming Nostr messages to qaul
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NostrIncomingMessage {
    pub chat_group: String,    // Base58 encoded group ID
    pub message_text: String,
}

impl NostrIncomingMessage {
    /// Parse JSON from decrypted Nostr DM
    pub fn from_json(json_str: &str) -> Result<Self, String> {
        serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse incoming message JSON: {}", e))
    }

    /// Decode base58 group ID to binary
    pub fn decode_group_id(group_id_base58: &str) -> Result<Vec<u8>, String> {
        bs58::decode(group_id_base58)
            .into_vec()
            .map_err(|e| format!("Failed to decode base58 group ID: {}", e))
    }

    /// Get decoded group ID as binary
    pub fn group_id_bytes(&self) -> Result<Vec<u8>, String> {
        Self::decode_group_id(&self.chat_group)
    }
}

/// JSON payload structure for qaul messages forwarded to Nostr
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QaulMessagePayload {
    pub user_name: String,
    pub qaul_id: String,       // Base58 encoded sender ID
    pub chat_group: String,    // Base58 encoded group ID
    pub message_id: String,    // Base58 encoded message ID
    pub sent_time: String,     // ISO 8601 timestamp
    pub message_text: String,
}

impl QaulMessagePayload {
    /// Create a new QaulMessagePayload from raw message data
    pub fn new(
        user_name: &str,
        sender_id: &[u8],
        group_id: &[u8],
        message_id: &[u8],
        timestamp: u64,
        message_text: &str,
    ) -> Self {
        QaulMessagePayload {
            user_name: user_name.to_string(),
            qaul_id: bs58::encode(sender_id).into_string(),
            chat_group: bs58::encode(group_id).into_string(),
            message_id: bs58::encode(message_id).into_string(),
            sent_time: Self::format_timestamp(timestamp),
            message_text: message_text.to_string(),
        }
    }

    /// Convert timestamp to ISO 8601 format
    pub fn format_timestamp(timestamp: u64) -> String {
        use chrono::SecondsFormat;

        chrono::DateTime::from_timestamp(timestamp as i64, 0)
            .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    /// Serialize payload to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

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
    last_seen_timestamp: std::sync::Arc<std::sync::RwLock<u64>>,
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
            last_seen_timestamp: std::sync::Arc::new(std::sync::RwLock::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )),
        })
    }

    /// Start listening for incoming encrypted DMs
    pub async fn start_listening(&self) -> Result<(), String> {
        // Subscribe to encrypted DMs sent to our public key
        let my_pubkey = self.keys.public_key();

        // Calculate timestamp for 24 hours ago to get recent messages
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let twenty_four_hours_ago = now.saturating_sub(24 * 60 * 60);
        let since_timestamp = Timestamp::from(twenty_four_hours_ago);

        // Create filter for encrypted DMs sent TO us
        // For encrypted DMs, the recipient's pubkey is in the 'p' tag
        let filter = Filter::new()
            .kind(Kind::EncryptedDirectMessage)
            .custom_tag(
                nostr_sdk::SingleLetterTag::lowercase(nostr_sdk::Alphabet::P),
                my_pubkey.to_hex()
            )
            .since(since_timestamp);  // Get recent messages from last 24h

        self.client
            .subscribe(filter, None)
            .await
            .map_err(|e| format!("Failed to subscribe to DMs: {}", e))?;

        log::info!("[Nostr] Subscribed to incoming encrypted DMs sent to us");
        log::info!("[Nostr] Our public key (npub): {}", my_pubkey.to_bech32().unwrap());
        log::info!("[Nostr] Fetching messages from last 24 hours");
        Ok(())
    }

    /// Check for new incoming messages and return parsed messages
    pub async fn check_for_messages(&self) -> Result<Vec<NostrIncomingMessage>, String> {
        let mut messages = Vec::new();
        let mut total_notifications = 0;
        let mut dm_count = 0;
        let mut old_dm_count = 0;

        // Get current timestamp for filtering
        let last_seen = {
            let lock = self.last_seen_timestamp.read().unwrap();
            *lock
        };

        // Get notifications from relays
        let mut notifications = self.client.notifications();

        // Process incoming events (non-blocking)
        while let Ok(notification) = notifications.try_recv() {
            total_notifications += 1;
            
            if let RelayPoolNotification::Event { event, .. } = notification {
                // Count all DMs, even old ones
                if event.kind == Kind::EncryptedDirectMessage {
                    dm_count += 1;
                    
                    // Check if this is a new message
                    if event.created_at.as_u64() <= last_seen {
                        old_dm_count += 1;
                        log::debug!(
                            "[Nostr] Skipping old DM (timestamp: {}, last_seen: {})",
                            event.created_at.as_u64(),
                            last_seen
                        );
                        continue;
                    }

                    log::info!(
                        "[Nostr] New DM detected from {} (timestamp: {})",
                        event.pubkey.to_bech32().unwrap_or_else(|_| "unknown".to_string()),
                        event.created_at.as_u64()
                    );

                    // Decrypt the message
                    match nip04::decrypt(&self.keys.secret_key(), &event.pubkey, &event.content) {
                        Ok(decrypted_content) => {
                            log::info!("[Nostr] Successfully decrypted DM");
                            log::debug!("[Nostr] Decrypted content: {}", decrypted_content);

                            // Parse JSON payload
                            match NostrIncomingMessage::from_json(&decrypted_content) {
                                Ok(msg) => {
                                    log::info!(
                                        "[Nostr] Parsed incoming message for group: {}",
                                        msg.chat_group
                                    );
                                    messages.push(msg);
                                }
                                Err(e) => {
                                    log::error!("[Nostr] Failed to parse message JSON: {}", e);
                                    log::debug!("[Nostr] Decrypted content: {}", decrypted_content);
                                }
                            }

                            // Update last seen timestamp
                            let mut lock = self.last_seen_timestamp.write().unwrap();
                            *lock = event.created_at.as_u64();
                        }
                        Err(e) => {
                            log::error!("[Nostr] Failed to decrypt message: {}", e);
                        }
                    }
                }
            }
        }

        // Log summary every 10 polls (to avoid spam, but still informative)
        if total_notifications > 0 || messages.len() > 0 {
            log::debug!(
                "[Nostr Poll] {} notifications, {} DMs ({} old, {} new), {} parsed messages",
                total_notifications,
                dm_count,
                old_dm_count,
                dm_count - old_dm_count,
                messages.len()
            );
        }

        Ok(messages)
    }

    /// Format qaul message for Nostr relay (legacy plain text format)
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

    /// Format qaul message as JSON for Nostr relay (new format with metadata)
    pub fn format_message_json(
        user_name: &str,
        sender_id: &[u8],
        group_id: &[u8],
        message_id: &[u8],
        content: &str,
        timestamp: u64,
    ) -> String {
        let payload = QaulMessagePayload::new(
            user_name,
            sender_id,
            group_id,
            message_id,
            timestamp,
            content,
        );

        payload.to_json().unwrap_or_else(|e| {
            log::error!("[Nostr] Failed to serialize message to JSON: {}", e);
            // Fallback to plain text format
            Self::format_message(sender_id, group_id, content, timestamp)
        })
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

    /// Forward qaul message to Nostr network (legacy plain text format)
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

    /// Forward qaul message to Nostr network as JSON with full metadata
    pub async fn forward_qaul_message_json(
        &self,
        sender_id: &[u8],
        group_id: &[u8],
        message_id: &[u8],
        content: &str,
        timestamp: u64,
    ) -> Result<(), String> {
        // Use sender_id as user_name fallback (in production, this should come from Users module)
        let user_name = bs58::encode(sender_id).into_string();

        let json_message = Self::format_message_json(
            &user_name,
            sender_id,
            group_id,
            message_id,
            content,
            timestamp,
        );

        self.send_encrypted_dm(&json_message).await
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

    // === NEW TESTS FOR JSON MESSAGE PAYLOAD ===

    #[test]
    fn test_qaul_message_payload_serialization() {
        let payload = QaulMessagePayload {
            user_name: "alice".to_string(),
            qaul_id: "5Z9Kni2tixYKT".to_string(),
            chat_group: "7BcDef3uvwXYZ".to_string(),
            message_id: "abc123".to_string(),
            sent_time: "2025-10-03T14:23:45Z".to_string(),
            message_text: "Hello from qaul!".to_string(),
        };

        let json = payload.to_json().unwrap();

        // Verify JSON contains all required fields
        assert!(json.contains("user_name"));
        assert!(json.contains("alice"));
        assert!(json.contains("qaul_id"));
        assert!(json.contains("chat_group"));
        assert!(json.contains("message_id"));
        assert!(json.contains("sent_time"));
        assert!(json.contains("message_text"));
        assert!(json.contains("Hello from qaul!"));

        // Verify it's valid JSON by deserializing
        let parsed: QaulMessagePayload = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.user_name, "alice");
        assert_eq!(parsed.message_text, "Hello from qaul!");
    }

    #[test]
    fn test_qaul_message_payload_all_fields_present() {
        let payload = QaulMessagePayload::new(
            "bob",
            &vec![1, 2, 3, 4],
            &vec![5, 6, 7, 8],
            &vec![9, 10, 11, 12],
            1727964225,
            "Test message"
        );

        // Verify all 6 required fields are populated
        assert!(!payload.user_name.is_empty());
        assert!(!payload.qaul_id.is_empty());
        assert!(!payload.chat_group.is_empty());
        assert!(!payload.message_id.is_empty());
        assert!(!payload.sent_time.is_empty());
        assert_eq!(payload.message_text, "Test message");
    }

    #[test]
    fn test_json_message_formatting() {
        let sender = vec![1, 2, 3, 4];
        let group = vec![5, 6, 7, 8];
        let message_id = vec![9, 10, 11, 12];
        let content = "Hello Nostr!";
        let timestamp = 1727964225;

        let json_message = NostrBridge::format_message_json(
            "alice",
            &sender,
            &group,
            &message_id,
            content,
            timestamp
        );

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json_message).unwrap();
        assert_eq!(parsed["user_name"].as_str().unwrap(), "alice");
        assert_eq!(parsed["message_text"].as_str().unwrap(), "Hello Nostr!");
        assert!(parsed["qaul_id"].as_str().unwrap().len() > 0);
        assert!(parsed["chat_group"].as_str().unwrap().len() > 0);
        assert!(parsed["message_id"].as_str().unwrap().len() > 0);
        assert!(parsed["sent_time"].as_str().is_some());
    }

    #[test]
    fn test_timestamp_formatting_iso8601() {
        let timestamp = 1727964225; // 2024-10-03 14:23:45 UTC
        let formatted = QaulMessagePayload::format_timestamp(timestamp);

        // Verify ISO 8601 format
        assert!(formatted.ends_with("Z"));
        assert!(formatted.contains("T"));
        assert!(formatted.contains("-"));
        assert!(formatted.contains(":"));
    }

    #[test]
    fn test_complete_json_message_example() {
        // This test demonstrates the complete JSON message format
        let sender_id = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let group_id = vec![9, 10, 11, 12, 13, 14, 15, 16];
        let message_id = vec![17, 18, 19, 20, 21, 22, 23, 24];
        let content = "Hello from qaul network!";
        let timestamp = 1727964225; // 2024-10-03 14:03:45 UTC

        let json_message = NostrBridge::format_message_json(
            "alice",
            &sender_id,
            &group_id,
            &message_id,
            content,
            timestamp,
        );

        // Parse and verify the complete JSON structure
        let parsed: serde_json::Value = serde_json::from_str(&json_message).unwrap();

        // Verify all required fields are present
        assert_eq!(parsed["user_name"].as_str().unwrap(), "alice");
        assert!(parsed["qaul_id"].as_str().unwrap().len() > 0);
        assert!(parsed["chat_group"].as_str().unwrap().len() > 0);
        assert!(parsed["message_id"].as_str().unwrap().len() > 0);
        assert_eq!(parsed["sent_time"].as_str().unwrap(), "2024-10-03T14:03:45Z");
        assert_eq!(parsed["message_text"].as_str().unwrap(), "Hello from qaul network!");

        // Print example for documentation
        println!("\nExample JSON message format:");
        println!("{}", json_message);
    }
}
