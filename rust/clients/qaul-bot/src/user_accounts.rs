// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # User Accounts Module for qaul-bot
//!
//! This module handles RPC communication for querying user account information.

use prost::Message;

/// include generated protobuf RPC rust definition file
mod proto {
    include!("../../../libqaul/src/rpc/protobuf_generated/rust/qaul.rpc.user_accounts.rs");
}

/// User Accounts module function handling
pub struct UserAccounts {}

impl UserAccounts {
    /// Request default user account from libqaul via RPC
    ///
    /// This sends an RPC request to libqaul to query the default user account.
    /// The response will be processed asynchronously via the RPC polling loop.
    pub fn request_default_account() {
        log::info!("Requesting default user account via RPC...");

        // create request message for default account
        let proto_message = proto::UserAccounts {
            message: Some(proto::user_accounts::Message::GetDefaultUserAccount(true)),
        };

        // encode the user accounts message
        let mut buf = Vec::with_capacity(proto_message.encoded_len());
        proto_message
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");

        // wrap in RPC message container
        let proto_rpc_message = super::rpc::proto::QaulRpc {
            module: super::rpc::proto::Modules::Useraccounts as i32,
            request_id: String::new(),
            user_id: vec![], // Empty user_id for account query
            data: buf,
        };

        // encode the complete RPC message
        let mut rpc_buf = Vec::with_capacity(proto_rpc_message.encoded_len());
        proto_rpc_message
            .encode(&mut rpc_buf)
            .expect("Vec<u8> provides capacity as needed");

        // send the message via libqaul API
        libqaul::api::send_rpc(rpc_buf);
        log::debug!("Default account request sent");
    }

    /// Process received RPC message
    ///
    /// Decodes received protobuf encoded binary RPC message
    /// of the user accounts module and displays account information.
    ///
    /// # Arguments
    /// * `data` - Binary protobuf-encoded UserAccounts message
    pub fn rpc(data: Vec<u8>) {
        log::debug!("Processing UserAccounts RPC message");

        match proto::UserAccounts::decode(&data[..]) {
            Ok(user_accounts) => {
                match user_accounts.message {
                    Some(proto::user_accounts::Message::DefaultUserAccount(
                        proto_defaultuseraccount,
                    )) => {
                        Self::handle_default_account_response(proto_defaultuseraccount);
                    }
                    _ => {
                        log::warn!("Received unprocessable RPC user accounts message type");
                    }
                }
            }
            Err(error) => {
                log::error!("Failed to decode user accounts RPC message: {:?}", error);
            }
        }
    }

    /// Handle default user account response
    ///
    /// Processes the DefaultUserAccount message and displays the account information.
    ///
    /// # Arguments
    /// * `response` - The DefaultUserAccount protobuf message
    fn handle_default_account_response(response: proto::DefaultUserAccount) {
        // check if default user is set
        if response.user_account_exists {
            if let Some(my_user_account) = response.my_user_account {
                // display user account information
                println!("Your user account is:");
                println!(
                    "{}, ID[{}]",
                    my_user_account.name, my_user_account.id_base58
                );
                println!("    public key: {}", my_user_account.key_base58);

                log::info!(
                    "Default account retrieved: {} (ID: {})",
                    my_user_account.name,
                    my_user_account.id_base58
                );
            } else {
                log::error!(
                    "Unexpected message configuration: account exists but no account data provided"
                );
            }
        } else {
            log::warn!("No default user account found in system");
        }
    }
}
