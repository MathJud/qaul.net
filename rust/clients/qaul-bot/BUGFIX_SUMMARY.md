# Nostr Bridge Bug Fix Summary

## Issues Fixed

### 1. **Incorrect Subscription Filter** ⚠️ CRITICAL BUG
**Location**: `src/nostr_bridge.rs`, line 297-299

**Problem**: 
The subscription filter was using `.pubkey(my_pubkey)` which subscribes to events WHERE the author is `my_pubkey`. This means the bot was subscribing to its own messages, not messages sent TO it.

**Root Cause**:
In Nostr, encrypted DMs (kind 4) have:
- `pubkey` field: The sender (author) of the message
- `p` tag: The recipient of the message

The original code filtered by author, not recipient.

**Fix**:
Changed to use `.custom_tag()` to filter by the 'p' tag (recipient):
```rust
// BEFORE (WRONG):
let filter = Filter::new()
    .kind(Kind::EncryptedDirectMessage)
    .pubkey(my_pubkey);  // This filters by AUTHOR, not recipient!

// AFTER (CORRECT):
let filter = Filter::new()
    .kind(Kind::EncryptedDirectMessage)
    .custom_tag(
        nostr_sdk::SingleLetterTag::lowercase(nostr_sdk::Alphabet::P),
        my_pubkey.to_hex()  // Filter by recipient ('p' tag)
    )
    .since(since_timestamp);
```

**Impact**: 
The bot can now receive messages sent TO it from the Nostr gateway. Previously, it would never receive incoming messages because it was subscribing to the wrong events.

---

### 2. **Missing Historical Messages** 📅
**Location**: `src/nostr_bridge.rs`, line 302

**Problem**: 
The subscription had no `since` parameter, so it would only receive NEW messages from the moment of subscription. Messages sent before the bot started were lost.

**Fix**:
Added a `since` timestamp set to 24 hours ago:
```rust
let twenty_four_hours_ago = now.saturating_sub(24 * 60 * 60);
let since_timestamp = Timestamp::from(twenty_four_hours_ago);

let filter = Filter::new()
    .kind(Kind::EncryptedDirectMessage)
    .custom_tag(...)
    .since(since_timestamp);  // Fetch messages from last 24 hours
```

**Impact**: 
The bot now fetches messages from the last 24 hours on startup, so messages sent while the bot was offline won't be lost.

---

### 3. **Insufficient Logging** 🔍
**Location**: `src/nostr_bridge.rs`, line 321-409

**Problem**: 
Limited visibility into whether the polling mechanism was working. Hard to debug if messages aren't being received.

**Fix**:
Added comprehensive logging:
- Total notifications received
- DM count (old vs new)
- Sender information with npub and timestamp
- Decryption success/failure messages
- JSON parsing results
- Poll summary statistics

**Example logs**:
```
[Nostr] New DM detected from npub1abc... (timestamp: 1696348800)
[Nostr] Successfully decrypted DM
[Nostr] Parsed incoming message for group: 5HpnLkrF5xxTihbtBemjy6PzhL
[Nostr Poll] 3 notifications, 1 DMs (0 old, 1 new), 1 parsed messages
```

**Impact**: 
Much easier to debug and verify that the nostr bridge is working correctly.

---

## Code Changes

### Files Modified:
1. `clients/qaul-bot/src/nostr_bridge.rs`
   - Fixed subscription filter to use 'p' tag
   - Added `since` timestamp for historical messages
   - Enhanced logging throughout `check_for_messages()`

2. `clients/qaul-bot/src/chat.rs`
   - Fixed unused variable warning

### Files Created:
1. `clients/qaul-bot/NOSTR_BRIDGE_TESTING.md` - Testing guide
2. `clients/qaul-bot/BUGFIX_SUMMARY.md` - This document

---

## Testing Status

✅ **Build**: Compiles successfully with no errors
⏳ **Runtime**: Needs testing with actual Nostr relay
⏳ **Integration**: Needs verification that messages are forwarded to qaul groups

---

## How to Verify the Fix

1. Start the bot: `cargo run --package qaul-bot`
2. Look for the bot's npub in startup logs
3. Send an encrypted DM to that npub with JSON payload
4. Watch logs for message reception and forwarding
5. Verify message appears in the target qaul group

See `NOSTR_BRIDGE_TESTING.md` for detailed testing instructions.

---

## Technical Notes

### Nostr Filter Behavior
- `.pubkey(pk)`: Filters events AUTHORED by `pk`
- `.custom_tag(SingleLetterTag::P, pk)`: Filters events with 'p' tag = `pk` (recipient)
- `.since(timestamp)`: Only return events after this timestamp

### NIP-04 Encrypted DM Structure
```
{
  "id": "event_id",
  "pubkey": "sender_public_key",     // Author
  "created_at": 1696348800,
  "kind": 4,                          // Encrypted DM
  "tags": [["p", "recipient_pubkey"]], // Recipient
  "content": "encrypted_content",
  "sig": "signature"
}
```

The fix ensures we filter by the 'p' tag (recipient), not the `pubkey` field (author).

---

## Future Improvements

1. **Configurable fetch window**: Make the 24-hour lookback configurable
2. **Persistent timestamp**: Save `last_seen_timestamp` to disk to survive restarts
3. **Multiple senders**: Support receiving from multiple Nostr accounts, not just `target_npub`
4. **Relay health monitoring**: Track which relays are responding
5. **Retry logic**: Automatically reconnect if relay connection drops

---

## Related Documentation

- Nostr Protocol: https://github.com/nostr-protocol/nips
- NIP-04 (Encrypted DMs): https://github.com/nostr-protocol/nips/blob/master/04.md
- nostr-sdk documentation: https://docs.rs/nostr-sdk/
