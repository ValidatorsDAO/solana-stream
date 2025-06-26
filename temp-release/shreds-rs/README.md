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
SOLANA_RPC_ENDPOINT="https://edge.erpc.global?api-key=YOUR_API_KEY"
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

## Usage

```rust
    let mut client = ShredstreamClient::connect(&endpoint).await?;

    // The filter is experimental
    let request = ShredstreamClient::create_entries_request_for_accounts(
        vec![],
        vec![],
        vec![],
        Some(CommitmentLevel::Processed),
    );

    let mut stream = client.subscribe_entries(request).await?;
```

## Dependencies

This project uses the published `solana-stream-sdk` crate:

- `solana-stream-sdk = "0.5.1"` - Main SDK for Solana streaming
- `tokio` - Async runtime
- `dotenvy` - Environment variable loading
- `solana-entry` - Solana entry types
- `bincode` - Serialization

## Example Output

```
Slot 349218153, Entry #14
  ⏰ BlockTime: 2025-06-26T00:57:41.000Z
  📥 ReceivedAt: 2025-06-26T00:57:42.466Z
  🚀 Adjusted Latency: 966 ms

Slot 349218153, Entry #15
  ⏰ BlockTime: 2025-06-26T00:57:41.000Z
  📥 ReceivedAt: 2025-06-26T00:57:42.477Z
  🚀 Adjusted Latency: 977 ms

📊 Average Latency (last 420 entries): 665.11 ms

Slot 349218154, Entry #1
  ⏰ BlockTime: 2025-06-26T00:57:42.000Z
  📥 ReceivedAt: 2025-06-26T00:57:42.506Z
  🚀 Adjusted Latency: 6 ms

📊 Average Latency (last 420 entries): 664.33 ms
```

## ⚠️ Experimental Filtering Feature Notice

The filtering functionality provided by this SDK is currently experimental. Occasionally, data may not be fully available, and filters may not be applied correctly.

If you encounter such cases, please report them by opening an issue at: https://github.com/ValidatorsDAO/solana-stream/issues

Your feedback greatly assists our debugging efforts and overall improvement of this feature.

Other reports and suggestions are also highly appreciated.

You can also join discussions or share feedback on Validators DAO's Discord community:
https://discord.gg/C7ZQSrCkYR

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
