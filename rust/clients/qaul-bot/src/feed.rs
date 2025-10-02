// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # Feed module functions for qaul-bot

use chrono::Local;
use prost::Message;
use state::InitCell;
use std::sync::RwLock;

/// include generated protobuf RPC rust definition file
mod proto {
    include!("../../../libqaul/src/rpc/protobuf_generated/rust/qaul.rpc.feed.rs");
}

/// mutable feed state
static FEED: InitCell<RwLock<Feed>> = InitCell::new();

/// feed module function handling
pub struct Feed {
    user_id: Vec<u8>,
}

impl Feed {
    /// Initialize Feed module with user ID
    pub fn init(user_id: Vec<u8>) {
        let feed = Feed { user_id };
        FEED.set(RwLock::new(feed));
    }

    /// Send a feed message with current timestamp
    pub fn send_timestamp_message() {
        // get current date and time in human readable format
        let now = Local::now();
        let message_text = now.format("%Y-%m-%d %H:%M:%S %Z").to_string();

        log::info!("Sending feed message: {}", message_text);
        Self::send_feed_message(message_text);
    }

    /// create and send feed message via rpc
    fn send_feed_message(message_text: String) {
        // get user_id from state
        let feed = FEED.get().read().unwrap();
        let user_id = feed.user_id.clone();

        // create feed send message
        let proto_feed_message = proto::Feed {
            message: Some(proto::feed::Message::Send(proto::SendMessage {
                content: message_text,
            })),
        };

        // encode feed message
        let mut feed_buf = Vec::with_capacity(proto_feed_message.encoded_len());
        proto_feed_message
            .encode(&mut feed_buf)
            .expect("Vec<u8> provides capacity as needed");

        // create RPC message container
        let proto_rpc_message = super::rpc::proto::QaulRpc {
            module: super::rpc::proto::Modules::Feed as i32,
            request_id: String::new(),
            user_id,
            data: feed_buf,
        };

        // encode RPC message
        let mut rpc_buf = Vec::with_capacity(proto_rpc_message.encoded_len());
        proto_rpc_message
            .encode(&mut rpc_buf)
            .expect("Vec<u8> provides capacity as needed");

        // send the message
        libqaul::api::send_rpc(rpc_buf);
    }

    /// Process received RPC message
    ///
    /// Decodes received protobuf encoded binary RPC message
    /// of the feed module.
    pub fn rpc(data: Vec<u8>) {
        match proto::Feed::decode(&data[..]) {
            Ok(feed) => {
                match feed.message {
                    Some(proto::feed::Message::Received(proto_feedlist)) => {
                        // log received feed messages
                        log::info!(
                            "Received {} feed messages",
                            proto_feedlist.feed_message.len()
                        );

                        for message in proto_feedlist.feed_message {
                            log::debug!(
                                "[{}] From: {}, Content: {}",
                                message.index,
                                message.sender_id_base58,
                                message.content
                            );
                        }
                    }
                    _ => {
                        log::warn!("Unprocessable RPC feed message");
                    }
                }
            }
            Err(error) => {
                log::error!("Failed to decode feed message: {:?}", error);
            }
        }
    }
}
