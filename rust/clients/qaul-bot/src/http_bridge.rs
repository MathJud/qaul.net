// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # HTTP Bridge Module
//!
//! This module provides bidirectional message bridging between qaul and a HTTP interface:
//!
//! ## Features
//!
//! ### Outgoing (qaul → HTTP)
//! - Sends messages to HTTP POST endpoint
//! - JSON message formatting with full metadata
//!
//! ### Incoming (HTTP → qaul)
//! - Receives messages via HTTP GET stream (Server-Sent Events)
//! - Parses JSON payloads with group routing information
//! - Tracks last received event ID
//! - Forward messages to specific qaul groups

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// JSON payload structure for outgoing messages (qaul → HTTP)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutgoingMessage {
    #[serde(rename = "networkId")]
    pub network_id: String,
    #[serde(rename = "botId")]
    pub bot_id: String,
    #[serde(rename = "botType")]
    pub bot_type: String,
    #[serde(rename = "groupID")]
    pub group_id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub message: String,
}

impl OutgoingMessage {
    /// Create a new OutgoingMessage from raw message data
    pub fn new(
        group_id: &[u8],
        user_id: &[u8],
        message_id: &[u8],
        message_text: &str,
    ) -> Self {
        OutgoingMessage {
            network_id: "qaul".to_string(),
            bot_id: "1312".to_string(),
            bot_type: "id".to_string(),
            group_id: bs58::encode(group_id).into_string(),
            user_id: bs58::encode(user_id).into_string(),
            message_id: bs58::encode(message_id).into_string(),
            message: message_text.to_string(),
        }
    }

    /// Serialize payload to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// JSON payload structure for incoming messages (HTTP → qaul)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IncomingMessage {
    #[serde(rename = "networkId")]
    pub network_id: String,
    #[serde(rename = "botId")]
    pub bot_id: String,
    #[serde(rename = "botType")]
    pub bot_type: String,
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "replyMessageId")]
    pub reply_message_id: String,
    #[serde(rename = "refId")]
    pub ref_id: String,
    pub message: String,
    pub direction: String,
    #[serde(rename = "eventId")]
    pub event_id: u64,
}

impl IncomingMessage {
    /// Parse JSON from HTTP stream data field
    pub fn from_json(json_str: &str) -> Result<Self, String> {
        serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse incoming message JSON: {}", e))
    }

    /// Decode base58 group ID to binary
    pub fn decode_group_id(&self) -> Result<Vec<u8>, String> {
        bs58::decode(&self.group_id)
            .into_vec()
            .map_err(|e| format!("Failed to decode base58 group ID: {}", e))
    }
}

/// HTTP bridge configuration
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub base_url: String,
    pub network_id: String,
    pub bot_id: String,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            base_url: "http://localhost:3080".to_string(),
            network_id: "qaul".to_string(),
            bot_id: "1312".to_string(),
        }
    }
}

/// HTTP bridge for forwarding qaul messages to HTTP interface
#[derive(Clone)]
pub struct HttpBridge {
    config: HttpConfig,
    last_event_id: Arc<std::sync::RwLock<u64>>,
}

impl HttpBridge {
    /// Initialize HTTP bridge with configuration
    pub fn new(config: HttpConfig, last_event_id: u64) -> Self {
        HttpBridge {
            config,
            last_event_id: Arc::new(std::sync::RwLock::new(last_event_id)),
        }
    }

    /// Get the current last event ID
    pub fn get_last_event_id(&self) -> u64 {
        *self.last_event_id.read().unwrap()
    }

    /// Update the last event ID
    pub fn update_last_event_id(&self, event_id: u64) {
        let mut lock = self.last_event_id.write().unwrap();
        if event_id > *lock {
            *lock = event_id;
        }
    }

    /// Send message to HTTP endpoint
    pub async fn send_message(
        &self,
        group_id: &[u8],
        user_id: &[u8],
        message_id: &[u8],
        message_text: &str,
    ) -> Result<(), String> {
        let message = OutgoingMessage::new(group_id, user_id, message_id, message_text);
        let json = message.to_json()
            .map_err(|e| format!("Failed to serialize message: {}", e))?;

        let url = format!("{}/api/messages", self.config.base_url);

        log::info!("[HTTP] Sending message to {}", url);
        log::debug!("[HTTP] Message payload: {}", json);

        let mut response = surf::post(&url)
            .content_type("application/json")
            .body(json)
            .await
            .map_err(|e| format!("HTTP POST failed: {}", e))?;

        if response.status().is_success() {
            log::info!("[HTTP] Message sent successfully");
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.body_string().await.unwrap_or_default();
            Err(format!("HTTP POST failed with status {}: {}", status, error_text))
        }
    }

    /// Receive messages from HTTP GET stream (Server-Sent Events)
    /// Returns a vector of new messages
    pub async fn receive_messages(&self) -> Result<Vec<IncomingMessage>, String> {
        let url = format!(
            "{}/api/messages/out/{}/{}",
            self.config.base_url,
            self.config.network_id,
            self.config.bot_id
        );

        log::info!("[HTTP] Polling messages from {}", url);

        // Make a non-blocking request
        let mut response = surf::get(&url)
            .await
            .map_err(|e| {
                log::error!("[HTTP] GET request failed: {}", e);
                format!("HTTP GET failed: {}", e)
            })?;

        let status = response.status();
        log::info!("[HTTP] Response status: {} - {}", status, status.canonical_reason());

        if !status.is_success() {
            let error_body = response.body_string().await.unwrap_or_default();
            log::error!("[HTTP] Error response body: {}", error_body);
            return Err(format!("HTTP GET failed with status {}: {}", status, error_body));
        }

        // Check content type
        if let Some(content_type) = response.content_type() {
            log::debug!("[HTTP] Content-Type: {:?}", content_type);
        }

        // For SSE streams, read line by line with a timeout
        // The stream stays open, so we read until no more lines arrive
        use async_std::io::prelude::*;
        use async_std::io::BufReader;

        let body_reader = response.take_body();
        let mut buf_reader = BufReader::new(body_reader);
        let mut body = String::new();
        let mut line = String::new();

        // Read lines with timeout - collect data for up to 200ms
        let deadline = async_std::task::sleep(std::time::Duration::from_millis(200));
        futures::pin_mut!(deadline);

        loop {
            line.clear();

            let read_fut = buf_reader.read_line(&mut line);
            futures::pin_mut!(read_fut);

            match futures::future::select(read_fut, deadline).await {
                futures::future::Either::Left((result, remaining_deadline)) => {
                    match result {
                        Ok(0) => {
                            // EOF reached
                            log::debug!("[HTTP] EOF reached on stream");
                            break;
                        }
                        Ok(n) => {
                            log::debug!("[HTTP] Read {} bytes: {:?}", n, line.trim());
                            body.push_str(&line);
                            // Reset deadline for next line
                            deadline = remaining_deadline;
                        }
                        Err(e) => {
                            log::error!("[HTTP] Error reading line: {}", e);
                            break;
                        }
                    }
                }
                futures::future::Either::Right(_) => {
                    // Timeout reached
                    log::debug!("[HTTP] Timeout reached, done reading");
                    break;
                }
            }
        }

        log::info!("[HTTP] Received body ({} bytes, {} lines)", body.len(), body.lines().count());
        if !body.is_empty() {
            log::info!("[HTTP] Body content: {}",
                if body.len() > 500 {
                    format!("{}... (truncated)", &body[..500])
                } else {
                    body.clone()
                });
        } else {
            log::info!("[HTTP] Body is empty - no events in stream");
        }

        // Parse Server-Sent Events format
        let messages = self.parse_sse_stream(&body)?;

        if !messages.is_empty() {
            log::info!("[HTTP] Parsed {} messages from stream", messages.len());
        }

        Ok(messages)
    }

    /// Parse Server-Sent Events (SSE) format
    /// Format:
    /// id: 1
    /// event: message
    /// data: {"json":"payload"}
    ///
    fn parse_sse_stream(&self, text: &str) -> Result<Vec<IncomingMessage>, String> {
        let mut messages = Vec::new();
        let mut current_id: Option<u64> = None;
        let mut current_data: Option<String> = None;

        let last_seen_id = self.get_last_event_id();

        for line in text.lines() {
            let line = line.trim();

            if line.is_empty() {
                // Empty line signals end of an event
                if let (Some(id), Some(data)) = (current_id, current_data.take()) {
                    // Only process messages with ID greater than last seen
                    if id > last_seen_id {
                        match IncomingMessage::from_json(&data) {
                            Ok(msg) => {
                                log::info!("[HTTP] Parsed message with event_id: {}", id);
                                messages.push(msg);
                            }
                            Err(e) => {
                                log::warn!("[HTTP] Failed to parse message: {}", e);
                            }
                        }
                    } else {
                        log::debug!("[HTTP] Skipping old message with id: {} (last_seen: {})", id, last_seen_id);
                    }
                }
                current_id = None;
                continue;
            }

            if let Some(id_str) = line.strip_prefix("id:") {
                current_id = id_str.trim().parse().ok();
            } else if let Some(data_str) = line.strip_prefix("data:") {
                current_data = Some(data_str.trim().to_string());
            }
            // Ignore "event:" lines and other fields
        }

        // Handle last event if stream doesn't end with empty line
        if let (Some(id), Some(data)) = (current_id, current_data) {
            if id > last_seen_id {
                match IncomingMessage::from_json(&data) {
                    Ok(msg) => {
                        log::info!("[HTTP] Parsed message with event_id: {}", id);
                        messages.push(msg);
                    }
                    Err(e) => {
                        log::warn!("[HTTP] Failed to parse message: {}", e);
                    }
                }
            }
        }

        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outgoing_message_serialization() {
        let group_id = vec![1, 2, 3, 4];
        let user_id = vec![5, 6, 7, 8];
        let message_id = vec![9, 10, 11, 12];
        let message_text = "Hello from qaul!";

        let msg = OutgoingMessage::new(&group_id, &user_id, &message_id, message_text);
        let json = msg.to_json().unwrap();

        // Verify JSON contains all required fields
        assert!(json.contains("networkId"));
        assert!(json.contains("qaul"));
        assert!(json.contains("botId"));
        assert!(json.contains("1312"));
        assert!(json.contains("groupID"));
        assert!(json.contains("userId"));
        assert!(json.contains("messageID"));
        assert!(json.contains("message"));
        assert!(json.contains("Hello from qaul!"));
    }

    #[test]
    fn test_incoming_message_parsing() {
        let json = r#"{
            "networkId":"qaul",
            "botId":"1312",
            "botType":"id",
            "groupId":"5HpnLkrF5xxTihbtBemjy6PzhL",
            "userId":"abc123",
            "replyMessageId":"msg456",
            "refId":"1760035764818",
            "message":"Test message",
            "direction":"out",
            "eventId":1
        }"#;

        let msg = IncomingMessage::from_json(json).unwrap();
        assert_eq!(msg.network_id, "qaul");
        assert_eq!(msg.bot_id, "1312");
        assert_eq!(msg.group_id, "5HpnLkrF5xxTihbtBemjy6PzhL");
        assert_eq!(msg.message, "Test message");
        assert_eq!(msg.event_id, 1);
    }

    #[test]
    fn test_sse_parsing() {
        let config = HttpConfig::default();
        let bridge = HttpBridge::new(config, 0);

        let sse_data = r#"id: 1
event: message
data: {"networkId":"qaul","botId":"1312","botType":"id","groupId":"5HpnLkrF5xxTihbtBemjy6PzhL","userId":"abc123","replyMessageId":"msg456","refId":"1760035764818","message":"Test message 1","direction":"out","eventId":1}

id: 2
event: message
data: {"networkId":"qaul","botId":"1312","botType":"id","groupId":"5HpnLkrF5xxTihbtBemjy6PzhL","userId":"abc123","replyMessageId":"msg456","refId":"1760036362338","message":"Test message 2","direction":"out","eventId":2}
"#;

        let messages = bridge.parse_sse_stream(sse_data).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].message, "Test message 1");
        assert_eq!(messages[0].event_id, 1);
        assert_eq!(messages[1].message, "Test message 2");
        assert_eq!(messages[1].event_id, 2);
    }

    #[test]
    fn test_sse_parsing_with_last_event_id() {
        let config = HttpConfig::default();
        let bridge = HttpBridge::new(config, 1); // Last seen ID is 1

        let sse_data = r#"id: 1
event: message
data: {"networkId":"qaul","botId":"1312","botType":"id","groupId":"5HpnLkrF5xxTihbtBemjy6PzhL","userId":"abc123","replyMessageId":"msg456","refId":"1760035764818","message":"Old message","direction":"out","eventId":1}

id: 2
event: message
data: {"networkId":"qaul","botId":"1312","botType":"id","groupId":"5HpnLkrF5xxTihbtBemjy6PzhL","userId":"abc123","replyMessageId":"msg456","refId":"1760036362338","message":"New message","direction":"out","eventId":2}
"#;

        let messages = bridge.parse_sse_stream(sse_data).unwrap();
        assert_eq!(messages.len(), 1); // Only the new message
        assert_eq!(messages[0].message, "New message");
        assert_eq!(messages[0].event_id, 2);
    }

    #[test]
    fn test_last_event_id_tracking() {
        let config = HttpConfig::default();
        let bridge = HttpBridge::new(config, 0);

        assert_eq!(bridge.get_last_event_id(), 0);

        bridge.update_last_event_id(5);
        assert_eq!(bridge.get_last_event_id(), 5);

        // Should not decrease
        bridge.update_last_event_id(3);
        assert_eq!(bridge.get_last_event_id(), 5);

        // Should increase
        bridge.update_last_event_id(10);
        assert_eq!(bridge.get_last_event_id(), 10);
    }
}
