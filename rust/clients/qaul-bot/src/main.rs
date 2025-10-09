// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # qaul-bot - Automated Feed Message Bot
//!
//! This bot automatically:
//! - Creates a default user "beacon-bot" on first startup
//! - Queries and displays account information via RPC
//! - Polls libqaul every second for incoming RPC messages
//! - Sends a feed message every 10 seconds with current date/time
//! - Monitors group chats and displays new messages every 1 second

use futures::prelude::*;
use futures_ticker::Ticker;
use std::{thread, time::Duration};

mod chat;
mod feed;
mod group;
mod http_bridge;
mod rpc;
mod state;
mod user_accounts;

use chat::Chat;
use feed::Feed;
use group::Group;
use http_bridge::{HttpBridge, HttpConfig};
use rpc::Rpc;
use state::BotState;
use user_accounts::UserAccounts;

/// Event Types of the async loop
enum EventType {
    RpcPoll,
    SendFeedMessage,
    PollGroupList,
    PollHttpMessages,
}

#[async_std::main]
async fn main() {
    // get current working directory
    let path = std::env::current_dir().unwrap();
    let storage_path = path.as_path().to_str().unwrap().to_string();

    // start libqaul in new thread and save configuration file to current working path
    // Note: libqaul initializes its own logger, so we don't need to initialize one here
    libqaul::api::start_with_config(storage_path, None);

    // wait until libqaul finished initializing
    while libqaul::api::initialization_finished() == false {
        // wait a little while
        thread::sleep(Duration::from_millis(10));
    }

    // if no account exists, create beacon-bot user
    if libqaul::node::user_accounts::UserAccounts::len() == 0 {
        log::info!("Creating default user: beacon-bot");
        libqaul::node::user_accounts::UserAccounts::create("beacon-bot".to_string());
    }

    // get the default user ID
    let user_account = libqaul::node::user_accounts::UserAccounts::get_default_user();
    if user_account.is_none() {
        log::error!("Failed to get user account");
        return;
    }
    let user_account = user_account.unwrap();
    let user_id_bytes = user_account.id.to_bytes();
    log::info!(
        "Using user ID: {}",
        bs58::encode(&user_id_bytes).into_string()
    );

    // initialize feed module
    Feed::init(user_id_bytes.clone());

    // initialize chat module with user ID
    Chat::init(user_id_bytes.clone());

    // initialize group module with user ID
    Group::init(user_id_bytes);

    // Initialize HTTP bridge
    let http_bridge = initialize_http_bridge();
    log::info!("[HTTP] Bridge initialized successfully");
    // Store bridge in global state for use by Chat module
    chat::set_http_bridge(http_bridge.clone()).await;

    // query account information via RPC
    UserAccounts::request_default_account();

    // create ticker for polling RPC every 1 second
    let mut rpc_ticker = Ticker::new(Duration::from_millis(100));

    // create ticker for sending feed messages every 10 seconds
    let mut feed_ticker = Ticker::new(Duration::from_secs(60));

    // create ticker for polling group list every 1 second
    let mut group_ticker = Ticker::new(Duration::from_millis(500));

    // create ticker for polling HTTP messages every 1 second
    let mut http_ticker = Ticker::new(Duration::from_secs(1));

    log::info!("qaul-bot started successfully");
    log::info!("Polling RPC every 1 second");
    log::info!("Sending feed messages every 10 seconds");
    log::info!("Monitoring group chats every 1 second");
    log::info!("Polling HTTP messages every 1 second");

    // main event loop
    loop {
        let evt = {
            let rpc_fut = rpc_ticker.next().fuse();
            let feed_fut = feed_ticker.next().fuse();
            let group_fut = group_ticker.next().fuse();
            let http_fut = http_ticker.next().fuse();

            futures::pin_mut!(rpc_fut);
            futures::pin_mut!(feed_fut);
            futures::pin_mut!(group_fut);
            futures::pin_mut!(http_fut);

            futures::select! {
                _rpc_tick = rpc_fut => Some(EventType::RpcPoll),
                _feed_tick = feed_fut => Some(EventType::SendFeedMessage),
                _group_tick = group_fut => Some(EventType::PollGroupList),
                _http_tick = http_fut => Some(EventType::PollHttpMessages),
            }
        };

        if let Some(event) = evt {
            match event {
                EventType::RpcPoll => {
                    // poll for RPC messages
                    match libqaul::api::receive_rpc() {
                        Ok(data) => {
                            Rpc::received_message(data);
                        }
                        _ => {}
                    }
                }
                EventType::SendFeedMessage => {
                    // send feed message with current date/time
                    Feed::send_timestamp_message();
                }
                EventType::PollGroupList => {
                    // request group list to check for new messages
                    Group::request_group_list();
                }
                EventType::PollHttpMessages => {
                    // poll for HTTP messages and forward to qaul
                    let bridge_clone = http_bridge.clone();
                    async_std::task::spawn(async move {
                        log::debug!("[HTTP] Starting poll for new messages");
                        match bridge_clone.receive_messages().await {
                            Ok(messages) => {
                                if messages.is_empty() {
                                    log::debug!("[HTTP] No new messages in this poll");
                                } else {
                                    log::info!("[HTTP] Received {} new messages", messages.len());

                                    for msg in messages {
                                        log::info!(
                                            "[HTTP→qaul] Processing message - groupId: {}, message: '{}', eventId: {}",
                                            msg.group_id,
                                            msg.message,
                                            msg.event_id
                                        );

                                        // Send message to qaul group
                                        if let Err(e) = Chat::send_message_to_group(
                                            &msg.group_id,
                                            msg.message.clone(),
                                        ) {
                                            log::error!(
                                                "[HTTP→qaul] Failed to send message to group {}: {}",
                                                msg.group_id,
                                                e
                                            );
                                        } else {
                                            log::info!(
                                                "[HTTP→qaul] Successfully forwarded message to group {}: '{}'",
                                                msg.group_id,
                                                msg.message
                                            );

                                            // Update last event ID
                                            bridge_clone.update_last_event_id(msg.event_id);
                                            log::info!("[HTTP] Updated last event ID to: {}", msg.event_id);

                                            // Save to state file
                                            let mut bot_state = BotState::load();
                                            bot_state.set_last_event_id(msg.event_id);
                                            if let Err(e) = bot_state.save() {
                                                log::error!("[HTTP] Failed to save state: {}", e);
                                            } else {
                                                log::debug!("[HTTP] Saved state with event ID: {}", msg.event_id);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!("[HTTP] Error polling messages: {}", e);
                            }
                        }
                    });
                }
            }
        }
    }
}

/// Initialize HTTP bridge with configuration and last event ID from state
fn initialize_http_bridge() -> HttpBridge {
    // Load bot state to get last event ID
    let bot_state = BotState::load();
    let last_event_id = bot_state.get_last_event_id();

    log::info!("[HTTP] Last event ID from state: {}", last_event_id);

    // Create HTTP configuration
    let config = HttpConfig::default();

    log::info!("[HTTP] Base URL: {}", config.base_url);
    log::info!("[HTTP] Network ID: {}", config.network_id);
    log::info!("[HTTP] Bot ID: {}", config.bot_id);

    // Initialize HTTP bridge
    HttpBridge::new(config, last_event_id)
}
