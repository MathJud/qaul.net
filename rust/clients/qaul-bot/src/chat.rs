// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # Chat module functions for qaul-bot
//!
//! This module handles:
//! - Tracking last seen message indices per conversation
//! - Requesting chat conversations
//! - Displaying only new messages (deduplication)

use once_cell::sync::{Lazy, OnceCell};
use prost::Message;
use std::collections::HashMap;
use std::sync::RwLock;

/// include generated protobuf RPC rust definition file
mod proto {
    include!("../../../libqaul/src/rpc/protobuf_generated/rust/qaul.rpc.chat.rs");
}

/// Mutable chat state
static CHAT_STATE: Lazy<RwLock<ChatState>> = Lazy::new(|| RwLock::new(ChatState::new()));

/// Static storage for user ID
static USER_ID: OnceCell<Vec<u8>> = OnceCell::new();

/// Chat state tracking
pub struct ChatState {
    /// Map of conversation_id -> last_seen_message_index
    last_seen_indices: HashMap<Vec<u8>, u64>,
}

impl ChatState {
    fn new() -> Self {
        ChatState {
            last_seen_indices: HashMap::new(),
        }
    }
}

/// Chat module function handling
pub struct Chat {}

impl Chat {
    /// Initialize Chat module with user ID
    ///
    /// Note: State is automatically initialized on first access via Lazy.
    pub fn init(user_id: Vec<u8>) {
        // Force initialization of the lazy static
        let _ = &*CHAT_STATE;
        // Store user ID for RPC requests
        USER_ID.set(user_id).ok();
    }

    /// Send a chat message to a group/conversation
    ///
    /// This constructs and sends a chat message via RPC to the specified group.
    pub fn send_message(group_id: Vec<u8>, message_text: String) {
        let user_id = USER_ID.get().cloned().unwrap_or_default();

        // Create chat send message
        let proto_message = proto::Chat {
            message: Some(proto::chat::Message::Send(proto::ChatMessageSend {
                group_id: group_id.clone(),
                content: message_text,
            })),
        };

        // Encode message
        let mut buf = Vec::with_capacity(proto_message.encoded_len());
        proto_message
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");

        // Wrap in RPC container
        let proto_rpc_message = super::rpc::proto::QaulRpc {
            module: super::rpc::proto::Modules::Chat as i32,
            request_id: String::new(),
            user_id,
            data: buf,
        };

        // Encode RPC message
        let mut rpc_buf = Vec::with_capacity(proto_rpc_message.encoded_len());
        proto_rpc_message
            .encode(&mut rpc_buf)
            .expect("Vec<u8> provides capacity as needed");

        // Send via libqaul API
        libqaul::api::send_rpc(rpc_buf);

        log::info!(
            "[Bot] Sent message to group {}",
            if group_id.len() == 16 {
                match uuid::Uuid::from_slice(&group_id) {
                    Ok(uuid) => uuid.to_string(),
                    Err(_) => bs58::encode(&group_id).into_string(),
                }
            } else {
                bs58::encode(&group_id).into_string()
            }
        );
    }

    /// Request chat conversation for a group
    ///
    /// Requests messages from the last seen index onwards.
    /// If no messages have been seen yet, starts from index 0.
    pub fn request_conversation(group_id: Vec<u8>) {
        // Get last seen index for this conversation
        let last_index = {
            let state = CHAT_STATE.read().unwrap();
            state.last_seen_indices.get(&group_id).copied().unwrap_or(0)
        };

        Self::send_conversation_request(group_id, last_index);
    }

    /// Send conversation request via RPC
    fn send_conversation_request(group_id: Vec<u8>, last_index: u64) {
        let user_id = USER_ID.get().cloned().unwrap_or_default();

        let proto_message = proto::Chat {
            message: Some(proto::chat::Message::ConversationRequest(
                proto::ChatConversationRequest {
                    group_id,
                    last_index,
                },
            )),
        };

        // encode message
        let mut buf = Vec::with_capacity(proto_message.encoded_len());
        proto_message
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");

        // wrap in RPC container
        let proto_rpc_message = super::rpc::proto::QaulRpc {
            module: super::rpc::proto::Modules::Chat as i32,
            request_id: String::new(),
            user_id,
            data: buf,
        };

        // encode RPC message
        let mut rpc_buf = Vec::with_capacity(proto_rpc_message.encoded_len());
        proto_rpc_message
            .encode(&mut rpc_buf)
            .expect("Vec<u8> provides capacity as needed");

        // send via libqaul API
        libqaul::api::send_rpc(rpc_buf);
    }

    /// Get last seen message index for a conversation
    ///
    /// Returns None if conversation has never been seen
    pub fn get_last_seen_index(group_id: &[u8]) -> Option<u64> {
        let state = CHAT_STATE.read().unwrap();
        state.last_seen_indices.get(group_id).copied()
    }

    /// Update last seen index for a conversation
    pub fn update_last_seen_index(group_id: Vec<u8>, index: u64) {
        let mut state = CHAT_STATE.write().unwrap();
        state
            .last_seen_indices
            .entry(group_id)
            .and_modify(|e| *e = (*e).max(index))
            .or_insert(index);
    }

    /// Process received RPC message
    ///
    /// Decodes received protobuf encoded binary RPC message
    /// of the chat module and displays only NEW messages.
    pub fn rpc(data: Vec<u8>) {
        match proto::Chat::decode(&data[..]) {
            Ok(chat) => {
                match chat.message {
                    Some(proto::chat::Message::ConversationList(proto_conversation)) => {
                        Self::handle_conversation_response(proto_conversation);
                    }
                    _ => {
                        log::debug!("Unprocessable RPC chat message type");
                    }
                }
            }
            Err(error) => {
                log::error!("Failed to decode chat RPC message: {:?}", error);
            }
        }
    }

    /// Handle conversation response and display new messages
    fn handle_conversation_response(conversation: proto::ChatConversationList) {
        let group_id = conversation.group_id.clone();

        // Get last seen index for deduplication
        let last_seen = {
            let state = CHAT_STATE.read().unwrap();
            state.last_seen_indices.get(&group_id).copied().unwrap_or(0)
        };

        // Filter and display only new messages
        let new_messages: Vec<_> = conversation
            .message_list
            .into_iter()
            .filter(|msg| msg.index > last_seen)
            .collect();

        if new_messages.is_empty() {
            return; // No new messages
        }

        // Convert group_id to UUID for display
        let group_id_str = if group_id.len() == 16 {
            match uuid::Uuid::from_slice(&group_id) {
                Ok(uuid) => uuid.to_string(),
                Err(_) => bs58::encode(&group_id).into_string(),
            }
        } else {
            bs58::encode(&group_id).into_string()
        };

        // Get bot's user ID for comparison
        let bot_user_id = USER_ID.get().cloned().unwrap_or_default();

        // Display new messages and send auto-replies
        for message in &new_messages {
            let content = Self::extract_message_content(&message.content);
            let sender_id = bs58::encode(&message.sender_id).into_string();

            log::info!(
                "[Group: {}] New message from {}: {}",
                group_id_str,
                sender_id,
                content
            );

            // Send auto-reply only if message is not from the bot itself
            if message.sender_id != bot_user_id {
                // Only auto-reply to actual chat messages, not system events
                if Self::is_user_message(&message.content) {
                    log::info!(
                        "[Bot] Sending auto-reply to group {}",
                        group_id_str
                    );
                    Self::send_message(
                        group_id.clone(),
                        "thanks for this message, we will process it".to_string(),
                    );
                }
            }
        }

        // Update last seen index to the highest message index received
        if let Some(last_message) = new_messages.last() {
            Self::update_last_seen_index(group_id, last_message.index);
        }
    }

    /// Check if message content is a user chat message (not system event or file)
    fn is_user_message(content: &[u8]) -> bool {
        if let Ok(content_message) = proto::ChatContentMessage::decode(content) {
            matches!(
                content_message.message,
                Some(proto::chat_content_message::Message::ChatContent(_))
            )
        } else {
            false
        }
    }

    /// Extract text content from message
    fn extract_message_content(content: &[u8]) -> String {
        if let Ok(content_message) = proto::ChatContentMessage::decode(content) {
            match content_message.message {
                Some(proto::chat_content_message::Message::ChatContent(chat_content)) => {
                    return chat_content.text;
                }
                Some(proto::chat_content_message::Message::FileContent(file_content)) => {
                    return format!("[File: {}]", file_content.file_name);
                }
                Some(proto::chat_content_message::Message::GroupEvent(group_event)) => {
                    return Self::format_group_event(group_event);
                }
                None => {}
            }
        }
        "[Unable to decode message]".to_string()
    }

    /// Format group event into human-readable text
    fn format_group_event(event: proto::GroupEvent) -> String {
        match proto::GroupEventType::try_from(event.event_type) {
            Ok(proto::GroupEventType::Joined) => {
                format!("[User joined: {}]", bs58::encode(event.user_id).into_string())
            }
            Ok(proto::GroupEventType::Left) => {
                format!("[User left: {}]", bs58::encode(event.user_id).into_string())
            }
            Ok(proto::GroupEventType::Removed) => "[You were removed]".to_string(),
            Ok(proto::GroupEventType::Created) => "[Group created]".to_string(),
            Ok(proto::GroupEventType::InviteAccepted) => "[Invite accepted]".to_string(),
            _ => "[Group event]".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_state_initialization() {
        let state = ChatState::new();
        assert_eq!(state.last_seen_indices.len(), 0);
    }

    #[test]
    fn test_get_last_seen_index_none() {
        Chat::init(vec![1, 2, 3, 4]); // Dummy user ID for test
        // Use a unique group_id to avoid test interference
        let group_id = vec![255, 1, 0, 0];
        assert_eq!(Chat::get_last_seen_index(&group_id), None);
    }

    #[test]
    fn test_update_last_seen_index() {
        Chat::init(vec![1, 2, 3, 4]); // Dummy user ID for test
        // Use a unique group_id to avoid test interference
        let group_id = vec![255, 2, 0, 0];

        Chat::update_last_seen_index(group_id.clone(), 5);
        assert_eq!(Chat::get_last_seen_index(&group_id), Some(5));

        // Update to higher index
        Chat::update_last_seen_index(group_id.clone(), 10);
        assert_eq!(Chat::get_last_seen_index(&group_id), Some(10));
    }

    #[test]
    fn test_update_last_seen_index_not_decreasing() {
        Chat::init(vec![1, 2, 3, 4]); // Dummy user ID for test
        // Use a unique group_id to avoid test interference
        let group_id = vec![255, 3, 0, 0];

        Chat::update_last_seen_index(group_id.clone(), 10);
        assert_eq!(Chat::get_last_seen_index(&group_id), Some(10));

        // Try to update to lower index - should stay at 10
        Chat::update_last_seen_index(group_id.clone(), 5);
        assert_eq!(Chat::get_last_seen_index(&group_id), Some(10));
    }

    #[test]
    fn test_multiple_conversations() {
        Chat::init(vec![1, 2, 3, 4]); // Dummy user ID for test
        // Use unique group_ids to avoid test interference
        let group_id_1 = vec![255, 4, 0, 0];
        let group_id_2 = vec![255, 5, 0, 0];

        Chat::update_last_seen_index(group_id_1.clone(), 5);
        Chat::update_last_seen_index(group_id_2.clone(), 10);

        assert_eq!(Chat::get_last_seen_index(&group_id_1), Some(5));
        assert_eq!(Chat::get_last_seen_index(&group_id_2), Some(10));
    }
}
