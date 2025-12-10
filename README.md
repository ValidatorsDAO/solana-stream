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
  <a href="https://www.npmjs.com/package/@validators-dao/solana-stream-sdk">
    <img alt="NPM Version" src="https://img.shields.io/npm/v/@validators-dao/solana-stream-sdk?color=268bd2&label=version&logo=npm">
  </a>
  <a aria-label="License" href="https://github.com/ValidatorsDAO/solana-stream/blob/main/LICENSE.txt">
    <img alt="" src="https://badgen.net/badge/license/Apache/blue">
  </a>
  <a aria-label="Code of Conduct" href="https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md">
    <img alt="" src="https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg">
  </a>
</p>

# Solana Stream SDK

A collection of Rust and TypeScript packages for Solana stream data, operated by ValidatorsDAO. This repository is published as open-source software (OSS) and is freely available for anyone to use.

<a href="https://solana.com/">
  <img src="https://storage.slv.dev/PoweredBySolana.svg" alt="Powered By Solana" width="200px" height="95px">
</a>

## Overview

This project provides libraries and tools for streaming real-time data from the Solana blockchain. It supports both Geyser and Shreds approaches, making it easier for developers to access Solana data streams.

## Package Structure

### Rust Clients

- **client/geyser-rs/**: Rust client using Geyser plugin
- **client/shreds-rs/**: Rust client using Shreds

### TypeScript Clients

- **client/geyser-ts/**: TypeScript client using Geyser plugin
- **client/shreds-ts/**: TypeScript client using Shreds

### SDK Packages

- **crate/solana-stream-sdk/**: Rust SDK for Solana stream functionality
- **package/solana-stream-sdk/**: TypeScript SDK for Solana stream functionality

## Getting Started

### Prerequisites

- Node.js (for TypeScript packages)
- Rust (for Rust packages)
- pnpm (for package management)

### Installation

For the entire workspace:

```bash
git clone https://github.com/ValidatorsDAO/solana-stream.git
cd solana-stream
pnpm install
```

### Geyser Client Example – TypeScript

Create a `.env` file at `client/geyser-ts/.env` with your environment variables:

```env
X_TOKEN=YOUR_X_TOKEN
GEYSER_ENDPOINT=https://grpc-ams.erpc.global
SOLANA_RPC_ENDPOINT="https://edge.erpc.global?api-key=YOUR_API_KEY"
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

Next, build and run the client:

```bash
pnpm -F @validators-dao/solana-stream-sdk build
pnpm -F geyser-ts dev
```

- A 1-day free trial for the Geyser gRPC endpoints is available by joining the Validators DAO Discord community. Please try it out:

[https://discord.gg/C7ZQSrCkYR](https://discord.gg/C7ZQSrCkYR)

### Quick Start Guide for Sample Shreds Client - Rust

**Create a `.env` file** (placed in the project root)

```env
SHREDS_ENDPOINT=https://shreds-ams.erpc.global
SOLANA_RPC_ENDPOINT="https://edge.erpc.global?api-key=YOUR_API_KEY"
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

**Run the sample client**

```bash
cargo run -p shreds-rs
```

The sample code can be found at:

[https://github.com/ValidatorsDAO/solana-stream/blob/main/client/shreds-rs/src/main.rs](https://github.com/ValidatorsDAO/solana-stream/blob/main/client/shreds-rs/src/main.rs)

A 1-day free trial for the Shreds endpoints is available by joining the Validators DAO Discord community. Please try it out: [https://discord.gg/C7ZQSrCkYR](https://discord.gg/C7ZQSrCkYR)

#### Usage with solana-stream-sdk

You can also use the published crate in your own projects:

```toml
[dependencies]
solana-stream-sdk = "0.6.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
dotenvy = "0.15"
solana-entry = "3.0.12"
bincode = "1.3.3"
```

```rust
use solana_stream_sdk::{CommitmentLevel, ShredstreamClient};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Connect to shreds endpoint
    let endpoint = env::var("SHREDS_ENDPOINT")
        .unwrap_or_else(|_| "https://shreds-ams.erpc.global".to_string());
    let mut client = ShredstreamClient::connect(&endpoint).await?;

    // Create subscription for specific account
    let request = ShredstreamClient::create_entries_request_for_account(
        "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
        Some(CommitmentLevel::Processed),
    );

    // Subscribe to entries stream
    let mut stream = client.subscribe_entries(request).await?;

    // Process incoming entries
    while let Some(entry) = stream.message().await? {
        let entries = bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&entry.entries)?;
        println!("Slot: {}, Entries: {}", entry.slot, entries.len());

        for entry in entries {
            println!("  Entry has {} transactions", entry.transactions.len());
        }
    }

    Ok(())
}
```

For specific packages, navigate to the package directory and install dependencies.

## ⚠️ Experimental Filtering Feature Notice

The filtering functionality provided by this SDK is currently experimental. Occasionally, data may not be fully available, and filters may not be applied correctly.

If you encounter such cases, please report them by opening an issue at: https://github.com/ValidatorsDAO/solana-stream/issues

Your feedback greatly assists our debugging efforts and overall improvement of this feature.

Other reports and suggestions are also highly appreciated.

You can also join discussions or share feedback on Validators DAO's Discord community:
https://discord.gg/C7ZQSrCkYR

## Development

This project uses a monorepo structure with both Rust and TypeScript components:

- **Rust packages**: Managed with Cargo
- **TypeScript packages**: Managed with pnpm workspaces
- **Unified configuration**: Shared TypeScript and Prettier configurations

### Building

```bash
# Build all TypeScript packages
pnpm build

# Build Rust packages
cargo build
```

## Usage

Each package contains its own documentation and usage examples. Please refer to the individual package READMEs for specific implementation details.

## Contributing

We welcome contributions from the community! This project is continuously updated and improved.

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Submit a pull request

## About ValidatorsDAO

This project is operated and maintained by ValidatorsDAO, focused on providing robust tools and infrastructure for the Solana ecosystem.

https://discord.gg/pw7kuJNDKq

## Updates

This repository is actively maintained and will receive continuous updates to improve functionality and add new features.

## License

The package is available as open source under the terms of the
[Apache-2.0 License](https://www.apache.org/licenses/LICENSE-2.0).

## Code of Conduct

Everyone interacting in the Validators DAO project’s codebases, issue trackers, chat rooms
and mailing lists is expected to follow the
[code of conduct](https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md).
