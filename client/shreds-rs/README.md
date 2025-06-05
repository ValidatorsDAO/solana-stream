# Shreds-RS

A Rust client for streaming Solana shreds data using the published `solana-stream-sdk` crate.

## Quick Start

### Prerequisites

- Rust 1.70+
- Access to a Solana shreds streaming endpoint

### Installation

1. Clone or download this project
2. Set up environment variables:

```bash
cp .env.example .env
# Edit .env with your configuration
```

3. Run the client:

```bash
cargo run
```

## Configuration

Create a `.env` file with the following configuration:

```env
SHREDS_ENDPOINT=https://shreds-ams.erpc.global
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

## Usage

The client will connect to the configured shreds endpoint and stream entries for the specified account.

Default target account: `L1ocbjmuFUQDVwwUWi8HjXjg1RYEeN58qQx6iouAsGF`

To modify the target account, edit `src/main.rs`:

```rust
let request = ShredstreamClient::create_entries_request_for_account(
    "YOUR_ACCOUNT_ADDRESS_HERE",
    Some(CommitmentLevel::Processed),
);
```

## Dependencies

This project uses the published `solana-stream-sdk` crate:

- `solana-stream-sdk = "0.2.5"` - Main SDK for Solana streaming
- `tokio` - Async runtime
- `dotenvy` - Environment variable loading
- `solana-entry` - Solana entry types
- `bincode` - Serialization

## Example Output

```
Slot: 12345, Entries: 3
  Entry has 2 transactions
  Entry has 1 transactions
  Entry has 0 transactions
```

## Development

Build the project:

```bash
cargo build
```

Run in development mode:

```bash
cargo run
```

## License

MIT License

## More Information

For more details about the Solana Stream SDK, visit:

- [GitHub Repository](https://github.com/elsoul/solana-stream)
- [Crates.io](https://crates.io/crates/solana-stream-sdk)
