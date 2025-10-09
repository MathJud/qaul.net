# Nostr Bridge Testing Guide

## Bug Fixes Applied

### 1. **Subscription Filter Fixed**
**Problem**: The original subscription filter was using `.pubkey(my_pubkey)` which filters for events AUTHORED by that pubkey, not events sent TO that pubkey.

**Fix**: Changed to use `.custom_tag()` with the 'p' tag to properly filter encrypted DMs where we are the recipient:
```rust
let filter = Filter::new()
    .kind(Kind::EncryptedDirectMessage)
    .custom_tag(
        nostr_sdk::SingleLetterTag::lowercase(nostr_sdk::Alphabet::P),
        my_pubkey.to_hex()
    )
    .since(since_timestamp);
```

### 2. **Historical Messages Support**
**Problem**: The subscription didn't include a `since` timestamp, so it might miss messages sent before the bot started.

**Fix**: Added a `since` timestamp set to 24 hours ago to fetch recent messages:
```rust
let twenty_four_hours_ago = now.saturating_sub(24 * 60 * 60);
let since_timestamp = Timestamp::from(twenty_four_hours_ago);
```

### 3. **Enhanced Logging**
**Problem**: Limited visibility into whether messages are being received from the Nostr gateway.

**Fix**: Added comprehensive logging to track:
- Total notifications received
- DM count (old vs new)
- Decryption success/failure
- Message parsing results
- Sender information with timestamps

## Testing the Nostr Bridge

### Prerequisites
1. A Nostr client or relay testing tool
2. The bot's npub (public key) - found in logs when bot starts
3. A Nostr account that can send encrypted DMs

### Step 1: Start the Bot
```bash
cd /home/sojus/src/vibe/cashu/qaul.net/rust/clients/qaul-bot
cargo run
```

### Step 2: Find the Bot's Public Key
Look for this line in the startup logs:
```
[Nostr] Our public key (npub): npub1...
```

Copy this npub - you'll need it to send messages TO the bot.

### Step 3: Send a Test Message
Using a Nostr client (like Damus, Amethyst, or nostr-tools), send an encrypted DM to the bot's npub with this JSON format:

```json
{
  "chat_group": "5HpnLkrF5xxTihbtBemjy6PzhL",
  "message_text": "Test message from Nostr"
}
```

Replace `5HpnLkrF5xxTihbtBemjy6PzhL` with the base58-encoded group ID of a valid qaul group.

### Step 4: Monitor the Logs
Watch for these log messages indicating successful reception:

1. **Subscription confirmation** (on startup):
```
[Nostr] Subscribed to incoming encrypted DMs sent to us
[Nostr] Our public key (npub): npub1...
[Nostr] Fetching messages from last 24 hours
```

2. **Message received** (when DM arrives):
```
[Nostr] New DM detected from npub1... (timestamp: ...)
[Nostr] Successfully decrypted DM
[Nostr] Parsed incoming message for group: 5HpnLkrF5xxTihbtBemjy6PzhL
[Nostr→qaul] Forwarding message to group: 5HpnLkrF5xxTihbtBemjy6PzhL
[Nostr→qaul] Successfully forwarded: Test message from Nostr
```

3. **Polling summary** (every 1 second when messages are present):
```
[Nostr Poll] 3 notifications, 1 DMs (0 old, 1 new), 1 parsed messages
```

### Step 5: Verify Message Forwarding
Check that the message appears in the qaul group chat specified in the JSON payload.

## Debugging

### If no messages are received:

1. **Check relay connection**:
   - Look for `[Nostr] Connected to N relays` in startup logs
   - Default relays: `wss://relay.damus.io`, `wss://relay.primal.net`

2. **Verify the target npub**:
   - Check `config.yaml` for the `target_npub` field
   - This should match the Nostr account you're sending FROM

3. **Enable debug logging**:
   - Set `RUST_LOG=debug` environment variable
   - Look for detailed filter and notification information

4. **Check timestamp filtering**:
   - Messages older than 24 hours won't be fetched
   - Messages with timestamps before `last_seen_timestamp` are skipped

5. **Verify JSON format**:
   - Ensure the encrypted DM contains valid JSON
   - Both `chat_group` and `message_text` fields are required

### Common Issues:

- **"Failed to parse message JSON"**: The decrypted content is not valid JSON
- **"Failed to decrypt message"**: The message wasn't encrypted for the bot's public key
- **"Failed to decode group ID"**: The `chat_group` value is not valid base58
- **"Skipping old DM"**: The message timestamp is older than `last_seen_timestamp`

## Expected Behavior

The bot should:
1. ✅ Subscribe to encrypted DMs on startup
2. ✅ Poll for new messages every 1 second
3. ✅ Decrypt incoming NIP-04 encrypted DMs
4. ✅ Parse JSON payloads from decrypted content
5. ✅ Forward messages to the specified qaul group
6. ✅ Track the last seen timestamp to avoid duplicates
7. ✅ Log detailed information about the message processing pipeline

## Testing Checklist

- [ ] Bot starts without errors
- [ ] Nostr bridge initializes successfully
- [ ] Bot logs its public key (npub)
- [ ] Bot subscribes to encrypted DMs
- [ ] Sending a test DM triggers log messages
- [ ] DM is successfully decrypted
- [ ] JSON payload is correctly parsed
- [ ] Message is forwarded to the qaul group
- [ ] Subsequent polling doesn't re-process the same message

## Next Steps

If the bot is receiving messages successfully, you can:
1. Test with multiple groups
2. Verify bidirectional communication (qaul → Nostr)
3. Test edge cases (malformed JSON, invalid group IDs)
4. Monitor performance under load
