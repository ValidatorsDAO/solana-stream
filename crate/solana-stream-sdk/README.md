<p align="center">
  <a href="https://slv.dev/" target="_blank">
    <img src="https://storage.validators.solutions/SolanaStreamSDK.jpg" alt="SolanaStreamSDK" />
  </a>
  <a href="https://twitter.com/intent/follow?screen_name=ValidatorsDAO" target="_blank">
    <img src="https://img.shields.io/twitter/follow/ValidatorsDAO.svg?label=Follow%20@ValidatorsDAO" alt="Follow @ValidatorsDAO" />
  </a>
  <a href="https://crates.io/crates/solana-stream-sdk">
    <img alt="Crate" src="https://img.shields.io/crates/v/solana-stream-sdk?label=solana-stream-sdk&color=fc8d62&logo=rust">
  </a>
  <a href="https://crates.io/crates/solana-stream-sdk">
    <img alt="Downloads" src="https://img.shields.io/crates/d/solana-stream-sdk?color=66c2a5">
  </a>
  <a aria-label="License" href="https://github.com/ValidatorsDAO/solana-stream/blob/main/LICENSE.txt">
    <img alt="" src="https://badgen.net/badge/license/Apache/blue">
  </a>
  <a aria-label="Code of Conduct" href="https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md">
    <img alt="" src="https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg">
  </a>
</p>

# @validators-dao/solana-stream-sdk

# Solana Stream SDK

A Rust SDK for streaming Solana Data by Validators DAO.
This SDK provides a simple and efficient way to connect to Shredstream service and Geyser gRPC service, allowing you to subscribe to real-time Solana entries and transactions.

<a href="https://solana.com/">
  <img src="https://storage.slv.dev/PoweredBySolana.svg" alt="Powered By Solana" width="200px" height="95px">
</a>

## Features

- **Easy-to-use API** - Simple wrapper around the Shredstream protocols and Geyser gRPC
- **Async Support** - Built with tokio for async/await patterns
- **Type Safety** - Strongly typed Rust interfaces
- **Error Handling** - Comprehensive error types with proper error propagation
- **Streaming** - Efficient streaming of Solana entries and transactions

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
solana-stream-sdk = "0.6.0"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
dotenvy = "0.15"  # Optional: for loading environment variables from .env files
```

## Usage

### Quick Start Guide for Sample Shreds Client

Follow these steps to quickly run the sample client provided in this repository:

1. **Clone the repository**

```bash
git clone https://github.com/ValidatorsDAO/solana-stream.git
cd solana-stream
```

2. **Create a `.env` file** (placed in the project root)

```env
SHREDS_ENDPOINT=https://shreds-ams.erpc.global
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

For Geyser gRPC:

```env
GRPC_ENDPOINT=https://your.geyser.endpoint
X_TOKEN=your_token # Optional
SOLANA_RPC_ENDPOINT="https://edge.erpc.global?api-key=YOUR_API_KEY"
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

3. **Run the sample client**

```bash
cargo run -p shreds-rs
```

Example code:

- [shreds-rs example](https://github.com/ValidatorsDAO/solana-stream/blob/main/client/shreds-rs/src/main.rs)

For Geyser gRPC:

```bash
cd client/geyser-rs
RUST_LOG=info cargo run
```

Example code:

- [geyser-rs example](https://github.com/ValidatorsDAO/solana-stream/blob/main/client/geyser-rs/src/main.rs)

A 7-day free trial for Shreds endpoints is available by joining the Validators DAO Discord community. Please try it out: [https://discord.gg/C7ZQSrCkYR](https://discord.gg/C7ZQSrCkYR)

### Basic Example

```rust
use solana_stream_sdk::{CommitmentLevel, ShredstreamClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to the Shredstream proxy
    let mut client = ShredstreamClient::connect("https://shreds-ams.erpc.global").await?;

    // Create a subscription request for a specific account
    let request = ShredstreamClient::create_entries_request_for_account(
        "L1ocbjmuFUQDVwwUWi8HjXjg1RYEeN58qQx6iouAsGF",
        Some(CommitmentLevel::Processed),
    );

    // Subscribe to entries stream
    let mut stream = client.subscribe_entries(request).await?;

    // Process incoming entries
    while let Some(entry) = stream.message().await? {
        println!("Received entry for slot: {}", entry.slot);

        // Deserialize entries
        let entries = bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&entry.entries)?;

        for entry in entries {
            println!("Entry has {} transactions", entry.transactions.len());
        }
    }

    Ok(())
}
```

### Using Environment Variables

Create a `.env` file in your project root:

```env
SHREDS_ENDPOINT=https://shreds-ams.erpc.global
```

Then use it in your code:

```rust
use solana_stream_sdk::{CommitmentLevel, ShredstreamClient};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Get the shreds endpoint from environment variable
    let endpoint = env::var("SHREDS_ENDPOINT")
        .unwrap_or_else(|_| "https://shreds-ams.erpc.global".to_string());

    let mut client = ShredstreamClient::connect(&endpoint).await?;

    let request = ShredstreamClient::create_entries_request_for_account(
        "L1ocbjmuFUQDVwwUWi8HjXjg1RYEeN58qQx6iouAsGF",
        Some(CommitmentLevel::Processed),
    );

    let mut stream = client.subscribe_entries(request).await?;

    while let Some(entry) = stream.message().await? {
        println!("Received entry for slot: {}", entry.slot);

        let entries = bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&entry.entries)?;

        for entry in entries {
            println!("Entry has {} transactions", entry.transactions.len());
        }
    }

    Ok(())
}
```

### Basic Example (Geyser gRPC)

```rust
use solana_stream_sdk::{GeyserGrpcClient, GeyserSubscribeRequest, GeyserCommitmentLevel};
use std::collections::HashMap;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("GRPC_ENDPOINT")?;
    let client = GeyserGrpcClient::build_from_shared(endpoint)?.connect().await?;

    let request = GeyserSubscribeRequest {
        commitment: Some(GeyserCommitmentLevel::Processed as i32),
        accounts: HashMap::new(),
        transactions: HashMap::new(),
        slots: HashMap::new(),
        blocks: HashMap::new(),
        blocks_meta: HashMap::new(),
        entry: HashMap::new(),
        transactions_status: HashMap::new(),
        accounts_data_slice: vec![],
        from_slot: None,
        ping: None,
    };

    let (mut sink, mut stream) = client.subscribe().await?;
    sink.send(request).await?;

    while let Some(update) = stream.next().await {
        println!("Received: {:?}", update?);
    }

    Ok(())
}
```

### Custom Subscription Request

```rust
use solana_stream_sdk::{
    CommitmentLevel, SubscribeEntriesRequest, SubscribeRequestFilterAccounts,
    SubscribeRequestFilterSlots, SubscribeRequestFilterTransactions, ShredstreamClient
};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = ShredstreamClient::connect("https://shreds-ams.erpc.global").await?;

    // Create custom subscription filters
    let mut accounts = HashMap::new();
    accounts.insert(
        "my-filter".to_string(),
        SubscribeRequestFilterAccounts {
            account: vec!["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string()], // USDC
            owner: vec![],
            filters: vec![],
            nonempty_txn_signature: None,
        },
    );

    let mut slots = HashMap::new();
    slots.insert(
        "slot-filter".to_string(),
        SubscribeRequestFilterSlots {
            filter_by_commitment: Some(true),
            interslot_updates: Some(false),
        },
    );

    let request = SubscribeEntriesRequest {
        accounts,
        transactions: HashMap::new(),
        slots,
        commitment: Some(CommitmentLevel::Confirmed as i32),
    };

    let mut stream = client.subscribe_entries(request).await?;

    while let Some(entry) = stream.message().await? {
        println!("Slot: {}, Entry data: {} bytes", entry.slot, entry.entries.len());
    }

    Ok(())
}
```

### Custom Subscription Request (Geyser gRPC)

```rust
use solana_stream_sdk::{
    GeyserSubscribeRequest,
    GeyserSubscribeRequestFilterAccounts,
    GeyserCommitmentLevel,
    GeyserGrpcClient,
};
use std::collections::HashMap;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("GRPC_ENDPOINT")?;
    let client = GeyserGrpcClient::build_from_shared(endpoint)?.connect().await?;

    let mut accounts = HashMap::new();
    accounts.insert(
        "example".to_string(),
        GeyserSubscribeRequestFilterAccounts {
            account: vec!["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string()],
            owner: vec![],
            filters: vec![],
            nonempty_txn_signature: None,
        },
    );

    let request = GeyserSubscribeRequest {
        commitment: Some(GeyserCommitmentLevel::Confirmed as i32),
        accounts,
        transactions: HashMap::new(),
        slots: HashMap::new(),
        blocks: HashMap::new(),
        blocks_meta: HashMap::new(),
        entry: HashMap::new(),
        transactions_status: HashMap::new(),
        accounts_data_slice: vec![],
        from_slot: None,
        ping: None,
    };

    let (mut sink, mut stream) = client.subscribe().await?;
    sink.send(request).await?;

    while let Some(update) = stream.next().await {
        println!("Update: {:?}", update?);
    }

    Ok(())
}
```

## API Reference

### `ShredstreamClient`

The main client for connecting to the Shredstream services.

#### Methods

- `connect(endpoint: impl AsRef<str>) -> Result<Self>` – Connect to a Shredstream endpoint and initialize the client.
- `subscribe_entries(&mut self, request: SubscribeEntriesRequest) -> Result<impl Stream>` – Subscribe to real-time Solana entries.
- `create_entries_request_for_account(account: impl AsRef<str>, commitment: Option<CommitmentLevel>) -> SubscribeEntriesRequest` – Helper to create account-specific subscription requests.
- `create_empty_entries_request() -> SubscribeEntriesRequest` – Create an empty request for further customization.

### `GeyserGrpcClient`

Client for interacting with Solana via the Geyser gRPC service.

#### Methods

- `build_from_shared(endpoint: impl Into<String>) -> Result<GeyserGrpcClient>` – Initialize the client builder with a shared endpoint URL.
- `connect() -> Result<GeyserGrpcClient>` – Establish a connection to the configured gRPC endpoint.
- `subscribe() -> Result<(Sink, Stream)>` – Open a bidirectional subscription stream to Geyser for real-time data exchange.

### Error Types

The SDK provides a comprehensive `SolanaStreamError` enum that covers:

- `Transport` – Network or transport errors
- `Status` – gRPC status errors
- `Serialization` – Data serialization and deserialization errors
- `Connection` – Connection-related issues
- `Configuration` – Invalid configuration errors
- `IO` – Input/output errors
- `SerdeJsonc` – Errors related to parsing JSONC
- `InvalidUri` – URI parsing errors
- `Builder` – Errors during client builder initialization
- `SendError` – Errors during message sending
- `Client` – Errors within the Geyser gRPC client
- `UrlParse` – URL parsing errors

### Re-exported Types

For convenience, the following types are re-exported:

#### Shreds Protocol

- `CommitmentLevel`
- `SubscribeEntriesRequest`
- `SubscribeRequestFilterAccounts`
- `SubscribeRequestFilterSlots`
- `SubscribeRequestFilterTransactions`

#### Geyser gRPC Protocol

- `GeyserCommitmentLevel`
- `GeyserSubscribeRequest`
- `GeyserSubscribeRequestFilterAccounts`
- `GeyserSubscribeRequestFilterBlocks`
- `GeyserSubscribeRequestFilterBlocksMeta`
- `GeyserSubscribeRequestFilterEntry`
- `GeyserSubscribeRequestFilterSlots`
- `GeyserSubscribeRequestFilterTransactions`
- `GeyserSubscribeUpdate`
- `GeyserSubscribeUpdateAccountInfo`
- `GeyserSubscribeUpdateEntry`
- `GeyserSubscribeUpdateTransactionInfo`

## Requirements

- Rust 1.70+
- Tokio runtime for async operations

## ⚠️ Experimental Filtering Feature Notice

The filtering functionality provided by this SDK is currently experimental. Occasionally, data may not be fully available, and filters may not be applied correctly.

If you encounter such cases, please report them by opening an issue at: https://github.com/ValidatorsDAO/solana-stream/issues

Your feedback greatly assists our debugging efforts and overall improvement of this feature.

Other reports and suggestions are also highly appreciated.

You can also join discussions or share feedback on Validators DAO's Discord community:
https://discord.gg/C7ZQSrCkYR

## License

The package is available as open source under the terms of the
[Apache-2.0 License](https://www.apache.org/licenses/LICENSE-2.0).

## Code of Conduct

Everyone interacting in the Validators DAO project’s codebases, issue trackers, chat rooms
and mailing lists is expected to follow the
[code of conduct](https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md).
