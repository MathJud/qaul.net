// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # RPC client functions for qaul-bot

use prost::Message;

/// include generated protobuf RPC rust definition file
pub mod proto {
    include!("../../../libqaul/src/rpc/protobuf_generated/rust/qaul.rpc.rs");
}

/// RPC message communication between client and libqaul
pub struct Rpc {}

impl Rpc {
    /// receive an rpc message from libqaul
    pub fn received_message(data: Vec<u8>) {
        match proto::QaulRpc::decode(&data[..]) {
            Ok(message) => {
                log::info!("qaul rpc message received");

                match proto::Modules::try_from(message.module) {
                    Ok(proto::Modules::Feed) => {
                        super::feed::Feed::rpc(message.data);
                    }
                    Ok(proto::Modules::Node) => {
                        log::info!("Node module message received");
                    }
                    Ok(proto::Modules::Useraccounts) => {
                        super::user_accounts::UserAccounts::rpc(message.data);
                    }
                    Ok(proto::Modules::Users) => {
                        log::info!("Users module message received");
                    }
                    Ok(proto::Modules::Router) => {
                        log::info!("Router module message received");
                    }
                    Ok(proto::Modules::Chat) => {
                        log::info!("Chat module message received");
                    }
                    Ok(proto::Modules::Connections) => {
                        log::info!("Connections module message received");
                    }
                    Ok(proto::Modules::Ble) => {
                        log::info!("BLE module message received");
                    }
                    Ok(proto::Modules::Debug) => {
                        log::info!("Debug module message received");
                    }
                    Ok(proto::Modules::Chatfile) => {
                        log::info!("ChatFile module message received");
                    }
                    Ok(proto::Modules::Group) => {
                        log::info!("Group module message received");
                    }
                    Ok(proto::Modules::Rtc) => {
                        log::info!("RTC module message received");
                    }
                    Ok(proto::Modules::Dtn) => {
                        log::info!("DTN module message received");
                    }
                    Ok(proto::Modules::Rpc) => {
                        log::info!("RPC module message received");
                    }
                    Ok(proto::Modules::None) => {}
                    Err(_) => {
                        log::warn!("Unknown module in RPC message");
                    }
                }
            }
            Err(error) => {
                log::error!("Failed to decode RPC message: {:?}", error);
            }
        }
    }
}
