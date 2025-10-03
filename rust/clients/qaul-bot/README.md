# qaul-bot

An automated bot client for the qaul network that sends periodic feed messages.

## Features

- **Automatic User Creation**: Creates a default user named "beacon-bot" on first startup if no user exists
- **RPC Polling**: Polls libqaul every second to check for incoming RPC messages
- **Periodic Feed Messages**: Sends a feed message every 10 seconds containing the current date and time in human-readable format
- **Group Chat Monitoring**: Monitors group conversations and displays new messages with deduplication
- **Nostr Integration**: Bidirectional message bridge between qaul and Nostr networks using NIP-04 encryption

### Nostr Bridge

The bot now includes a **Nostr-to-qaul message bridge** that enables:

#### Outgoing (qaul → Nostr)
- Forwards qaul group messages to a target Nostr recipient via encrypted DMs
- Uses NIP-04 encryption for secure message transmission
- Sends JSON payloads with full message metadata (sender, group, timestamp, content)
- Automatically connects to configured Nostr relays

#### Incoming (Nostr → qaul)
- Subscribes to encrypted direct messages on Nostr relays
- Polls for new messages every 1 second
- Decrypts NIP-04 encrypted messages automatically
- Parses JSON payloads to extract routing information
- Forwards messages to specified qaul groups based on base58-encoded group IDs

#### Message Format

**Incoming Messages (Nostr → qaul):**
```json
{
  "chat_group": "5HpnLkrF5xxTihbtBemjy6PzhL",
  "message_text": "Reply from Nostr user"
}
```

**Outgoing Messages (qaul → Nostr):**
```json
{
  "user_name": "alice",
  "qaul_id": "5Z9Kni2tixYKT",
  "chat_group": "7BcDef3uvwXYZ",
  "message_id": "abc123",
  "sent_time": "2025-10-03T14:23:45Z",
  "message_text": "Hello from qaul!"
}
```

#### Key Management

- Bot generates a persistent Nostr keypair on first run
- Keys are stored in `qaul-bot.yml` state file
- Public key (npub) is logged at startup for sharing with Nostr users
- Both hex and bech32 (npub/nsec) formats are supported

## Usage

```bash
cargo run --bin=qaul-bot
```

The bot will:
1. Initialize libqaul with the current working directory as storage path
2. Create a "beacon-bot" user if no user account exists
3. Initialize Nostr bridge and connect to relays
4. Subscribe to incoming Nostr encrypted DMs
5. Start four async loops:
   - Loop 1: Polls RPC messages every 100ms
   - Loop 2: Sends feed messages every 60 seconds with current timestamp
   - Loop 3: Polls group list every 500ms to check for new messages
   - Loop 4: Polls Nostr messages every 1 second and forwards to qaul groups

## Dependencies

- `libqaul`: Core qaul library
- `async-std`: Async runtime
- `futures`: Async utilities
- `prost`: Protocol Buffers implementation
- `chrono`: Date and time handling
- `futures-ticker`: Ticker for periodic events
- `nostr-sdk`: Nostr protocol implementation with NIP-04 support
- `bs58`: Base58 encoding for group IDs
- `serde`/`serde_json`: JSON serialization
- `serde_yaml`: State persistence

## Testing

Run the test suite:

```bash
cargo test
```

Run Nostr bridge tests specifically:

```bash
cargo test --test nostr_bridge_test
```

All tests follow TDD methodology:
- Tests written first (RED phase)
- Implementation to pass tests (GREEN phase)
- Refactoring for quality (REFACTOR phase)

## License

AGPL-3.0
