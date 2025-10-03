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
mod nostr_bridge;
mod rpc;
mod state;
mod user_accounts;

use chat::Chat;
use feed::Feed;
use group::Group;
use nostr_bridge::{NostrBridge, NostrConfig, NostrKeys};
use rpc::Rpc;
use state::BotState;
use user_accounts::UserAccounts;

/// Event Types of the async loop
enum EventType {
    RpcPoll,
    SendFeedMessage,
    PollGroupList,
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

    // Initialize Nostr bridge
    let nostr_bridge = initialize_nostr_bridge().await;
    if let Some(bridge) = &nostr_bridge {
        log::info!("[Nostr] Bridge initialized successfully");
        // Store bridge in global state for use by Chat module
        chat::set_nostr_bridge(bridge.clone()).await;
    } else {
        log::warn!("[Nostr] Failed to initialize bridge - continuing without Nostr integration");
    }

    // query account information via RPC
    UserAccounts::request_default_account();

    // create ticker for polling RPC every 1 second
    let mut rpc_ticker = Ticker::new(Duration::from_millis(100));

    // create ticker for sending feed messages every 10 seconds
    let mut feed_ticker = Ticker::new(Duration::from_secs(60));

    // create ticker for polling group list every 1 second
    let mut group_ticker = Ticker::new(Duration::from_millis(500));

    log::info!("qaul-bot started successfully");
    log::info!("Polling RPC every 1 second");
    log::info!("Sending feed messages every 10 seconds");
    log::info!("Monitoring group chats every 1 second");

    // main event loop
    loop {
        let evt = {
            let rpc_fut = rpc_ticker.next().fuse();
            let feed_fut = feed_ticker.next().fuse();
            let group_fut = group_ticker.next().fuse();

            futures::pin_mut!(rpc_fut);
            futures::pin_mut!(feed_fut);
            futures::pin_mut!(group_fut);

            futures::select! {
                _rpc_tick = rpc_fut => Some(EventType::RpcPoll),
                _feed_tick = feed_fut => Some(EventType::SendFeedMessage),
                _group_tick = group_fut => Some(EventType::PollGroupList),
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
            }
        }
    }
}

/// Initialize Nostr bridge with key generation/loading and relay connections
async fn initialize_nostr_bridge() -> Option<NostrBridge> {
    // Load or create bot state
    let mut bot_state = BotState::load();

    // Load or generate Nostr keys
    let nostr_keys = match bot_state.get_nostr_keys() {
        Some(key_info) => {
            // Load existing key from complete key info
            match NostrKeys::from_info(key_info) {
                Ok(keys) => {
                    log::info!("[Nostr] Loaded existing keypair from state");
                    log::info!("[Nostr] Public key (npub): {}", keys.to_npub());
                    log::info!("[Nostr] All key formats available (private_key_hex, public_key_hex, npub, nsec)");
                    keys
                }
                Err(e) => {
                    log::error!("[Nostr] Failed to load keypair: {}", e);
                    log::info!("[Nostr] Generating new keypair");
                    let new_keys = NostrKeys::generate();
                    let npub = new_keys.to_npub();
                    log::info!("[Nostr] Generated new public key (npub): {}", npub);

                    // Save complete key info to state
                    bot_state.set_nostr_keys(Some(new_keys.to_info()));
                    if let Err(e) = bot_state.save() {
                        log::error!("[Nostr] Failed to save state: {}", e);
                    } else {
                        log::info!("[Nostr] Saved complete key information to state");
                    }

                    new_keys
                }
            }
        }
        None => {
            // Generate new keypair on first run
            log::info!("[Nostr] No existing keypair found - generating new one");
            let new_keys = NostrKeys::generate();
            let npub = new_keys.to_npub();
            log::info!("[Nostr] Generated public key (npub): {}", npub);

            // Save complete key info to state
            bot_state.set_nostr_keys(Some(new_keys.to_info()));
            if let Err(e) = bot_state.save() {
                log::error!("[Nostr] Failed to save state: {}", e);
            } else {
                log::info!("[Nostr] Saved complete key information to state");
            }

            new_keys
        }
    };

    // Create Nostr configuration
    let config = NostrConfig::default();

    log::info!("[Nostr] Connecting to {} relays:", config.relays.len());
    for relay in &config.relays {
        log::info!("[Nostr]   - {}", relay);
    }

    // Initialize Nostr bridge
    match NostrBridge::new(nostr_keys, &config).await {
        Ok(bridge) => {
            log::info!("[Nostr] Successfully connected to relays");
            log::info!("[Nostr] Target recipient: {}", config.target_npub);
            Some(bridge)
        }
        Err(e) => {
            log::error!("[Nostr] Failed to initialize bridge: {}", e);
            None
        }
    }
}
