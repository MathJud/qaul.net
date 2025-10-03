// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # Nostr Bridge Integration Tests
//!
//! Test suite for Nostr protocol integration with qaul-bot

use qaul_bot::nostr_bridge::{NostrBridge, NostrKeys};

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
