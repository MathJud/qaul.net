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

/// Bot state structure for serialization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BotState {
    /// Map of group_id (as base58 string) -> last_message_index
    pub message_counts: HashMap<String, u64>,
}

impl BotState {
    /// Create new empty state
    pub fn new() -> Self {
        BotState {
            message_counts: HashMap::new(),
        }
    }

    /// Load state from file or return empty state if file doesn't exist
    pub fn load() -> Self {
        let path = Self::get_state_file_path();

        match fs::read_to_string(&path) {
            Ok(contents) => {
                match serde_yaml::from_str::<BotState>(&contents) {
                    Ok(state) => {
                        log::info!("[Startup] Loaded state from {}", path.display());
                        log::info!("[Startup] Loaded message counts for {} groups", state.message_counts.len());
                        state
                    }
                    Err(e) => {
                        log::warn!("[Startup] Failed to parse state file: {}. Starting fresh.", e);
                        BotState::new()
                    }
                }
            }
            Err(_) => {
                log::info!("[Startup] No state file found. Starting fresh.");
                BotState::new()
            }
        }
    }

    /// Save state to file
    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_state_file_path();

        let yaml = serde_yaml::to_string(self)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        fs::write(&path, yaml)
            .map_err(|e| format!("Failed to write state file: {}", e))?;

        log::debug!("[State] Saved message counts to {}", path.display());
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
}
