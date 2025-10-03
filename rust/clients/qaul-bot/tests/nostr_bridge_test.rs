// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # Nostr Bridge Integration Tests
//!
//! Test suite for Nostr protocol integration with qaul-bot

use qaul_bot::nostr_bridge::{NostrBridge, NostrKeys, NostrIncomingMessage};

// === PHASE 1: RED - Incoming Message Tests ===

#[test]
fn test_parse_incoming_nostr_json() {
    // Test 1: Parse valid JSON from Nostr DM
    let json_payload = r#"{
        "chat_group": "2Wh8S2wHa1m",
        "message_text": "Reply from Nostr user"
    }"#;

    let parsed = NostrIncomingMessage::from_json(json_payload).unwrap();
    assert_eq!(parsed.chat_group, "2Wh8S2wHa1m");
    assert_eq!(parsed.message_text, "Reply from Nostr user");
}

#[test]
fn test_parse_incoming_json_with_extra_fields() {
    // Test 2: JSON should ignore extra fields
    let json_payload = r#"{
        "chat_group": "ABC123xyz",
        "message_text": "Test message",
        "extra_field": "should be ignored",
        "timestamp": 1234567890
    }"#;

    let parsed = NostrIncomingMessage::from_json(json_payload).unwrap();
    assert_eq!(parsed.chat_group, "ABC123xyz");
    assert_eq!(parsed.message_text, "Test message");
}

#[test]
fn test_parse_incoming_json_missing_required_field() {
    // Test 3: Should fail if required field missing
    let json_payload = r#"{
        "chat_group": "ABC123xyz"
    }"#;

    let result = NostrIncomingMessage::from_json(json_payload);
    assert!(result.is_err());
}

#[test]
fn test_decode_base58_group_id() {
    // Test 4: Decode base58 group ID to binary
    // Use a real 16-byte UUID encoded as base58
    let uuid_bytes = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let group_id_base58 = bs58::encode(&uuid_bytes).into_string();

    let decoded = NostrIncomingMessage::decode_group_id(&group_id_base58).unwrap();
    assert!(!decoded.is_empty());

    // Verify it decodes back to the original bytes
    assert_eq!(decoded, uuid_bytes);
}

#[test]
fn test_decode_invalid_base58() {
    // Test 5: Should fail on invalid base58
    let invalid_base58 = "!!!invalid!!!";

    let result = NostrIncomingMessage::decode_group_id(invalid_base58);
    assert!(result.is_err());
}

#[test]
fn test_complete_incoming_message_flow() {
    // Test 6: Complete flow from JSON to group ID bytes
    let json_payload = r#"{
        "chat_group": "5HpnLkrF5xxTihbtBemjy6PzhL",
        "message_text": "Hello from Nostr!"
    }"#;

    // Parse JSON
    let msg = NostrIncomingMessage::from_json(json_payload).unwrap();
    assert_eq!(msg.chat_group, "5HpnLkrF5xxTihbtBemjy6PzhL");
    assert_eq!(msg.message_text, "Hello from Nostr!");

    // Decode group ID
    let group_id_bytes = msg.group_id_bytes().unwrap();
    assert!(!group_id_bytes.is_empty());

    // Verify it can be re-encoded
    let re_encoded = bs58::encode(&group_id_bytes).into_string();
    assert_eq!(re_encoded, msg.chat_group);
}

#[test]
fn test_nostr_incoming_message_struct() {
    // Test 7: Verify struct fields are accessible
    let msg = NostrIncomingMessage {
        chat_group: "ABC123".to_string(),
        message_text: "Test message".to_string(),
    };

    assert_eq!(msg.chat_group, "ABC123");
    assert_eq!(msg.message_text, "Test message");
}

#[test]
fn test_nostr_key_generation() {
    // Test 1: Generate new Nostr keypair
    let keys = NostrKeys::generate();

    // Keys should be 32 bytes (secp256k1)
    assert_eq!(keys.secret_key().len(), 32);
    assert_eq!(keys.public_key().len(), 32);

    // Public key should be deterministic from secret key
    let keys2 = NostrKeys::from_secret_key(keys.secret_key().to_vec()).unwrap();
    assert_eq!(keys.public_key(), keys2.public_key());
}

#[test]
fn test_nostr_key_serialization() {
    // Test 2: Serialize and deserialize keys
    let keys = NostrKeys::generate();

    // Convert to hex string for storage
    let secret_hex = keys.secret_key_hex();
    assert_eq!(secret_hex.len(), 64); // 32 bytes = 64 hex chars

    // Deserialize from hex
    let keys_restored = NostrKeys::from_hex(&secret_hex).unwrap();
    assert_eq!(keys.public_key(), keys_restored.public_key());
}

#[test]
fn test_npub_encoding() {
    // Test 3: Encode public key to npub format
    let keys = NostrKeys::generate();
    let npub = keys.to_npub();

    // npub should start with "npub1"
    assert!(npub.starts_with("npub1"));
    assert!(npub.len() > 20); // Bech32 encoded
}

#[test]
fn test_message_formatting() {
    // Test 4: Format qaul message for Nostr
    let sender_id = vec![1, 2, 3, 4];
    let group_id = vec![5, 6, 7, 8];
    let content = "Hello, Nostr!";
    let timestamp = 1234567890u64;

    let formatted = NostrBridge::format_message(
        &sender_id,
        &group_id,
        content,
        timestamp,
    );

    // Check message format contains all components
    assert!(formatted.contains("[qaul]"));
    assert!(formatted.contains("From:"));
    assert!(formatted.contains("Group:"));
    assert!(formatted.contains("Time:"));
    assert!(formatted.contains("Hello, Nostr!"));
}

#[test]
fn test_state_persistence_with_nostr_keys() {
    // Test 5: Ensure BotState can persist Nostr keys
    use qaul_bot::state::BotState;
    use std::fs;
    use std::env;

    let temp_dir = env::temp_dir();
    let original_dir = env::current_dir().unwrap();

    // Change to temp directory
    env::set_current_dir(&temp_dir).unwrap();

    // Clean up any existing state file
    let state_file = temp_dir.join("qaul-bot.yml");
    fs::remove_file(&state_file).ok();

    // Generate keys and save to state
    let keys = NostrKeys::generate();
    let secret_hex = keys.secret_key_hex();

    let mut state = BotState::new();
    state.set_nostr_private_key(Some(secret_hex.clone()));
    state.save().unwrap();

    // Load state and verify key persistence
    let loaded_state = BotState::load();
    assert_eq!(loaded_state.get_nostr_private_key(), Some(&secret_hex));

    // Cleanup
    fs::remove_file(&state_file).ok();
    env::set_current_dir(&original_dir).unwrap();
}
