// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # Group module functions for qaul-bot
//!
//! This module handles:
//! - Requesting group list
//! - Tracking groups with unread messages
//! - Triggering chat message requests for groups with new content

use once_cell::sync::OnceCell;
use prost::Message;

/// include generated protobuf RPC rust definition file
mod proto {
    include!("../../../libqaul/src/rpc/protobuf_generated/rust/qaul.rpc.group.rs");
}

/// Static storage for user ID
static USER_ID: OnceCell<Vec<u8>> = OnceCell::new();

/// Group module function handling
pub struct Group {}

impl Group {
    /// Initialize the Group module with user ID
    pub fn init(user_id: Vec<u8>) {
        USER_ID.set(user_id).ok();
    }

    /// Request list of all groups
    ///
    /// This will trigger an RPC request for the group list.
    /// The response will include group metadata and unread message counts.
    pub fn request_group_list() {
        let user_id = USER_ID.get().cloned().unwrap_or_default();
        let proto_message = proto::Group {
            message: Some(proto::group::Message::GroupListRequest(
                proto::GroupListRequest {},
            )),
        };

        // encode message
        let mut buf = Vec::with_capacity(proto_message.encoded_len());
        proto_message
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");

        // wrap in RPC container
        let proto_rpc_message = super::rpc::proto::QaulRpc {
            module: super::rpc::proto::Modules::Group as i32,
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

    /// Process received RPC message
    ///
    /// Decodes received protobuf encoded binary RPC message
    /// of the group module.
    pub fn rpc(data: Vec<u8>) {
        match proto::Group::decode(&data[..]) {
            Ok(group_message) => {
                match group_message.message {
                    Some(proto::group::Message::GroupListResponse(group_list_response)) => {
                        Self::handle_group_list_response(group_list_response);
                    }
                    _ => {
                        log::debug!("Unprocessable RPC group message type");
                    }
                }
            }
            Err(error) => {
                log::error!("Failed to decode group RPC message: {:?}", error);
            }
        }
    }

    /// Handle group list response
    ///
    /// Processes the group list and requests chat conversations for groups with unread messages
    fn handle_group_list_response(response: proto::GroupListResponse) {
        if response.groups.is_empty() {
            log::debug!("No groups found");
            return;
        }

        log::debug!("Received {} groups", response.groups.len());

        // Process each group
        for group in response.groups {
            // Check if group has unread messages
            if group.unread_messages > 0 {
                let group_id_str = if group.group_id.len() == 16 {
                    match uuid::Uuid::from_slice(&group.group_id) {
                        Ok(uuid) => uuid.to_string(),
                        Err(_) => bs58::encode(&group.group_id).into_string(),
                    }
                } else {
                    bs58::encode(&group.group_id).into_string()
                };

                log::debug!(
                    "Group '{}' [{}] has {} unread messages",
                    group.group_name,
                    group_id_str,
                    group.unread_messages
                );

                // Request conversation to fetch new messages
                super::chat::Chat::request_conversation(group.group_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_module_exists() {
        // Basic test to ensure module compiles
        let _group = Group {};
    }
}
