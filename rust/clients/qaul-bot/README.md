# qaul-bot

An automated bot client for the qaul network that sends periodic feed messages.

## Features

- **Automatic User Creation**: Creates a default user named "beacon-bot" on first startup if no user exists
- **RPC Polling**: Polls libqaul every second to check for incoming RPC messages
- **Periodic Feed Messages**: Sends a feed message every 10 seconds containing the current date and time in human-readable format

## Usage

```bash
cargo run --bin=qaul-bot
```

The bot will:
1. Initialize libqaul with the current working directory as storage path
2. Create a "beacon-bot" user if no user account exists
3. Start two async loops:
   - Loop 1: Polls RPC messages every 1 second
   - Loop 2: Sends feed messages every 10 seconds with current timestamp

## Dependencies

- `libqaul`: Core qaul library
- `async-std`: Async runtime
- `futures`: Async utilities
- `prost`: Protocol Buffers implementation
- `chrono`: Date and time handling
- `futures-ticker`: Ticker for periodic events

## License

AGPL-3.0
