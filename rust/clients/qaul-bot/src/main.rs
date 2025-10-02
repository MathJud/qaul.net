// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # qaul-bot - Automated Feed Message Bot
//!
//! This bot automatically:
//! - Creates a default user "beacon-bot" on first startup
//! - Queries and displays account information via RPC
//! - Polls libqaul every second for incoming RPC messages
//! - Sends a feed message every 10 seconds with current date/time

use futures::prelude::*;
use futures_ticker::Ticker;
use std::{thread, time::Duration};

mod rpc;
mod feed;
mod user_accounts;

use rpc::Rpc;
use feed::Feed;
use user_accounts::UserAccounts;

/// Event Types of the async loop
enum EventType {
    RpcPoll,
    SendFeedMessage,
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
    log::info!("Using user ID: {}", bs58::encode(&user_id_bytes).into_string());

    // initialize feed module
    Feed::init(user_id_bytes);

    // query account information via RPC
    UserAccounts::request_default_account();

    // create ticker for polling RPC every 1 second
    let mut rpc_ticker = Ticker::new(Duration::from_secs(1));

    // create ticker for sending feed messages every 10 seconds
    let mut feed_ticker = Ticker::new(Duration::from_secs(10));

    log::info!("qaul-bot started successfully");
    log::info!("Polling RPC every 1 second");
    log::info!("Sending feed messages every 10 seconds");

    // main event loop
    loop {
        let evt = {
            let rpc_fut = rpc_ticker.next().fuse();
            let feed_fut = feed_ticker.next().fuse();

            futures::pin_mut!(rpc_fut);
            futures::pin_mut!(feed_fut);

            futures::select! {
                _rpc_tick = rpc_fut => Some(EventType::RpcPoll),
                _feed_tick = feed_fut => Some(EventType::SendFeedMessage),
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
            }
        }
    }
}
