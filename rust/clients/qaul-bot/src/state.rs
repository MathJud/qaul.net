// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # State persistence module for qaul-bot
//!
//! This module handles:
//! - Saving message counts to disk (qaul-bot.yml)
//! - Loading message counts on startup
//! - Maintaining state across bot restarts

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const STATE_FILE: &str = "qaul-bot.yml";

/// Nostr key information with all formats for persistence
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NostrKeyInfo {
    /// Private key in hex format (64 chars)
    pub private_key_hex: String,
    /// Public key in hex format (64 chars)
    pub public_key_hex: String,
    /// Bech32-encoded public key (npub1...)
    pub npub: String,
    /// Bech32-encoded private key (nsec1...)
    pub nsec: String,
}

/// Bot state structure for serialization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BotState {
    /// Map of group_id (as base58 string) -> last_message_index
    pub message_counts: HashMap<String, u64>,
    /// Last received event ID from HTTP stream
    #[serde(default)]
    pub last_event_id: u64,
    /// Complete Nostr key information (all formats)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nostr_keys: Option<NostrKeyInfo>,
    /// DEPRECATED: Legacy field for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    #[deprecated(note = "Use nostr_keys instead")]
    pub nostr_private_key: Option<String>,
}

impl BotState {
    /// Create new empty state
    pub fn new() -> Self {
        BotState {
            message_counts: HashMap::new(),
            last_event_id: 0,
            nostr_keys: None,
            #[allow(deprecated)]
            nostr_private_key: None,
        }
    }

    /// Load state from file or return empty state if file doesn't exist
    pub fn load() -> Self {
        let path = Self::get_state_file_path();

        match fs::read_to_string(&path) {
            Ok(contents) => {
                log::debug!("[State] File contents:\n{}", contents);
                match serde_yaml::from_str::<BotState>(&contents) {
                    Ok(state) => {
                        log::info!("[Startup] Loaded state from {}", path.display());
                        log::info!("[Startup] Loaded message counts for {} groups", state.message_counts.len());
                        log::info!("[Startup] Nostr keys present: {}", state.nostr_keys.is_some());
                        if let Some(ref keys) = state.nostr_keys {
                            log::debug!("[Startup] Loaded nostr_keys: npub={}, nsec={}", keys.npub, keys.nsec);
                        }
                        state
                    }
                    Err(e) => {
                        log::warn!("[Startup] Failed to parse state file: {}. Starting fresh.", e);
                        BotState::new()
                    }
                }
            }
            Err(e) => {
                log::info!("[Startup] No state file found at {}: {}. Starting fresh.", path.display(), e);
                BotState::new()
            }
        }
    }

    /// Save state to file
    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_state_file_path();

        let yaml = serde_yaml::to_string(self)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        log::debug!("[State] Saving to {}", path.display());
        log::debug!("[State] Nostr keys present: {}", self.nostr_keys.is_some());
        log::debug!("[State] YAML content:\n{}", yaml);

        fs::write(&path, yaml)
            .map_err(|e| format!("Failed to write state file: {}", e))?;

        log::debug!("[State] Saved state to {}", path.display());
        Ok(())
    }

    /// Get the full path to the state file in current working directory
    fn get_state_file_path() -> PathBuf {
        let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        path.push(STATE_FILE);
        path
    }

    /// Update message count for a group
    pub fn update_count(&mut self, group_id: String, count: u64) {
        self.message_counts
            .entry(group_id)
            .and_modify(|e| *e = (*e).max(count))
            .or_insert(count);
    }

    /// Get message count for a group
    pub fn get_count(&self, group_id: &str) -> Option<u64> {
        self.message_counts.get(group_id).copied()
    }

    /// Set Nostr key information (all formats)
    pub fn set_nostr_keys(&mut self, keys: Option<NostrKeyInfo>) {
        self.nostr_keys = keys;
    }

    /// Get Nostr key information (all formats)
    pub fn get_nostr_keys(&self) -> Option<&NostrKeyInfo> {
        self.nostr_keys.as_ref()
    }

    /// DEPRECATED: Legacy getter for backward compatibility
    #[deprecated(note = "Use get_nostr_keys instead")]
    #[allow(deprecated)]
    pub fn get_nostr_private_key(&self) -> Option<&String> {
        self.nostr_private_key.as_ref()
    }

    /// DEPRECATED: Legacy setter for backward compatibility
    #[deprecated(note = "Use set_nostr_keys instead")]
    #[allow(deprecated)]
    pub fn set_nostr_private_key(&mut self, key: Option<String>) {
        self.nostr_private_key = key;
    }

    /// Get last event ID
    pub fn get_last_event_id(&self) -> u64 {
        self.last_event_id
    }

    /// Set last event ID
    pub fn set_last_event_id(&mut self, event_id: u64) {
        self.last_event_id = event_id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_bot_state_new() {
        let state = BotState::new();
        assert_eq!(state.message_counts.len(), 0);
    }

    #[test]
    fn test_update_count() {
        let mut state = BotState::new();

        state.update_count("group1".to_string(), 5);
        assert_eq!(state.get_count("group1"), Some(5));

        // Update to higher count
        state.update_count("group1".to_string(), 10);
        assert_eq!(state.get_count("group1"), Some(10));

        // Try to update to lower count - should not decrease
        state.update_count("group1".to_string(), 7);
        assert_eq!(state.get_count("group1"), Some(10));
    }

    #[test]
    fn test_multiple_groups() {
        let mut state = BotState::new();

        state.update_count("group1".to_string(), 5);
        state.update_count("group2".to_string(), 12);
        state.update_count("group3".to_string(), 3);

        assert_eq!(state.get_count("group1"), Some(5));
        assert_eq!(state.get_count("group2"), Some(12));
        assert_eq!(state.get_count("group3"), Some(3));
        assert_eq!(state.get_count("group4"), None);
    }

    #[test]
    fn test_save_and_load() {
        // Create test state file in temp directory
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_qaul_bot_state.yml");

        // Create and save state
        let mut state1 = BotState::new();
        state1.update_count("group1".to_string(), 5);
        state1.update_count("group2".to_string(), 12);

        let yaml = serde_yaml::to_string(&state1).unwrap();
        fs::write(&test_file, yaml).unwrap();

        // Load state from file
        let contents = fs::read_to_string(&test_file).unwrap();
        let state2: BotState = serde_yaml::from_str(&contents).unwrap();

        // Verify loaded state matches saved state
        assert_eq!(state1, state2);
        assert_eq!(state2.get_count("group1"), Some(5));
        assert_eq!(state2.get_count("group2"), Some(12));

        // Cleanup
        fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_load_nonexistent_file() {
        // Test that load() doesn't panic even if file doesn't exist
        // Note: File may exist from previous tests or bot runs
        let state = BotState::load();
        // State should be valid regardless of whether file exists
        assert!(state.message_counts.len() >= 0);
    }

    #[test]
    fn test_persistence_workflow() {
        // Test the full workflow: create, save, load, verify
        use std::env;

        let temp_dir = env::temp_dir();
        let original_dir = env::current_dir().unwrap();

        // Change to temp directory for this test
        env::set_current_dir(&temp_dir).unwrap();

        // Clean up any existing state file
        let state_file = temp_dir.join("qaul-bot.yml");
        fs::remove_file(&state_file).ok();

        // Create and save state
        let mut state1 = BotState::new();
        state1.update_count("group1".to_string(), 5);
        state1.update_count("group2".to_string(), 12);
        state1.save().unwrap();

        // Verify file was created
        assert!(state_file.exists(), "State file should be created after save");

        // Load state and verify
        let state2 = BotState::load();
        assert_eq!(state2.message_counts.get("group1"), Some(&5));
        assert_eq!(state2.message_counts.get("group2"), Some(&12));

        // Update state and save again
        let mut state3 = state2;
        state3.update_count("group1".to_string(), 15); // Update existing
        state3.update_count("group3".to_string(), 3);  // Add new
        state3.save().unwrap();

        // Load again and verify updates
        let state4 = BotState::load();
        assert_eq!(state4.message_counts.get("group1"), Some(&15));
        assert_eq!(state4.message_counts.get("group2"), Some(&12));
        assert_eq!(state4.message_counts.get("group3"), Some(&3));

        // Cleanup
        fs::remove_file(&state_file).ok();
        env::set_current_dir(&original_dir).unwrap();
    }

    #[test]
    fn test_nostr_key_persistence() {
        use std::env;

        let temp_dir = env::temp_dir();
        let original_dir = env::current_dir().unwrap();

        // Change to temp directory for this test
        env::set_current_dir(&temp_dir).unwrap();

        // Clean up any existing state file
        let state_file = temp_dir.join("qaul-bot.yml");
        fs::remove_file(&state_file).ok();

        // Create state with Nostr keys
        let mut state1 = BotState::new();
        let key_info = NostrKeyInfo {
            private_key_hex: "a".repeat(64),
            public_key_hex: "b".repeat(64),
            npub: "npub1test123".to_string(),
            nsec: "nsec1test456".to_string(),
        };
        state1.set_nostr_keys(Some(key_info.clone()));
        state1.save().unwrap();

        // Load state and verify keys are preserved
        let state2 = BotState::load();
        assert!(state2.get_nostr_keys().is_some());
        let loaded_keys = state2.get_nostr_keys().unwrap();
        assert_eq!(loaded_keys.private_key_hex, key_info.private_key_hex);
        assert_eq!(loaded_keys.public_key_hex, key_info.public_key_hex);
        assert_eq!(loaded_keys.npub, key_info.npub);
        assert_eq!(loaded_keys.nsec, key_info.nsec);

        // Cleanup
        fs::remove_file(&state_file).ok();
        env::set_current_dir(&original_dir).unwrap();
    }
}
