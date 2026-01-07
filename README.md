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

## What's New (TypeScript v1.0.0)

- Yellowstone Geyser gRPC connection upgraded to an NAPI-RS-powered client for better backpressure
- Improved backpressure handling and up to 4x streaming efficiency (400% improvement)
- Faster real-time Geyser streams for TypeScript clients with lower overhead

## Production-Ready Geyser Client (TypeScript Best Practices)

- Ping/Pong handling to keep Yellowstone gRPC streams alive
- Exponential reconnect backoff plus `fromSlot` gap recovery
- Bounded in-memory queue with drop logging for backpressure safety
- Hot-swappable subscriptions via a JSON file (no reconnect)
- Optional runtime metrics logging (rates, queue size, drops)
- Default filters drop vote/failed transactions to reduce traffic

Tip: start with slots, then add filters as needed. When resuming from `fromSlot`,
duplicates are expected.

## Package Structure

### Rust Clients

- **client/geyser-rs/**: Rust client using Geyser plugin (gRPC)
- **client/shreds-rs/**: Rust client for Shredstream over gRPC
- **client/shreds-udp-rs/**: Minimal UDP shred listener; includes pump.fun token-mint detection example

### TypeScript Clients

- **client/geyser-ts/**: TypeScript client using Geyser plugin (gRPC)
- **client/shreds-ts/**: TypeScript client for Shredstream over gRPC

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
# Optional: required only if your endpoint enforces auth
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
solana-stream-sdk = "1.1.1"
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

## Shreds UDP Pump.fun Watcher (Rust)

`client/shreds-udp-rs` listens for Shredstream over **UDP** and highlights watched programs (defaults to pump.fun). Settings live in `client/shreds-udp-rs/settings.jsonc` and are embedded at build time; secrets like RPC can be overridden via environment variables.

Quick start:
```bash
export SOLANA_RPC_ENDPOINT=https://api.mainnet-beta.solana.com   # pass secrets via env only
cargo run -p shreds-udp-rs                                       # settings already in settings.jsonc
```

Log legend:
- Prefix: `🎯` program hit, `🐣` authority hit (`🎯🐣` means both)
- Action: `🐣` create, `🟢` buy, `🔻` sell, `🪙` other, `❓` unknown/missing amounts
- Votes skipped by default (`skip_vote_txs=true`)
- `pump_min_lamports` can suppress small pump.fun buy/sell logs
- UDP shreds are processed directly; not dependent on RPC commitment. Failed transactions may still appear; missing fields show as `❓`.

Components from `crate/solana-stream-sdk` (5 layers):
- Config loader (`ShredsUdpConfig`): reads JSONC/env and builds `ProgramWatchConfig` (pump.fun defaults; composite mint finder = pump.fun accounts + SPL Token MintTo/Initialize). Use `watch_config_no_defaults()` to opt out of pump.fun fallbacks.
- Receiver (`UdpShredReceiver`): minimal UDP socket reader with timestamps.
- Pipeline (5 layers): ① receive/prefilter (`decode_udp_datagram`) → ② FEC buffer (`insert_shred` + `ShredsUdpState`) → ③ deshred (`deshred_shreds_to_entries`) → ④ watcher/detail (`collect_watch_events` + detailers) → ⑤ sink (logs/custom hooks).
- One-call convenience: `handle_pumpfun_watcher` wraps the same 5 layers (pump.fun defaults).
- Customize sink/detailer: via `ProgramWatchConfig::with_detailers(...)` or replace the sink with your own hook.
- Vote filtering: by default `skip_vote_txs=true`, so vote-only shreds/txs are dropped early.
- Samples: `cargo run -p shreds-udp-rs` (pump.fun defaults, one-call wrapper) or `cargo run -p shreds-udp-rs --bin generic_logger` (pump.fun-free logger; set `GENERIC_WATCH_PROGRAM_IDS` / `GENERIC_WATCH_AUTHORITIES` to watch your own programs).

Design notes
- Layered pipeline (5 layers): ① UDP receive → ② FEC buffer/pre-deshred → ③ deshred → ④ watcher (mint extraction) → ⑤ detailer/sink (labeling + log output). Each stage can be swapped or reused.
- Pure UDP/FEC path: single-purpose deshredder tuned for Agave merkle sizing; leaves ledger/rpc out of the hot path.
- Config is JSONC/env: secrets (RPC) in env, behavior (watch ids, logging) in JSONC; defaults prefill pump.fun watch ids.
- Pump filters: optional `pump_min_lamports` to log only pump.fun buy/sell with SOL amount above a threshold; logs also show `sol:` when amount is parsed.
- Composable stages: receiver → deshred → watcher → detailer → sink; each stage can be swapped or reused.
- Signal-first logging: emoji at a glance, vote-filtered by default, and mint-level detail with adapters (pump.fun).
- Small, dependency-light SDK crate backing a CLI client; intended to embed into larger consumers as well.

Quick choices:
- Want a one-call, pump.fun-ready loop? Use `handle_pumpfun_watcher` in your own binary and set watch IDs/env as needed. This matches the out-of-the-box behavior shown in the screenshots.
- Need to act on detections (e.g., push to a queue, custom filtering, alternate watchers/detailers)? Use the modular pipeline (`decode_udp_datagram` → `insert_shred` → `deshred_shreds_to_entries` → `collect_watch_events`) and hook your own sink right after detection (see `client/shreds-udp-rs` custom hook example).

Minimal usage example (Rust):
```rust
use solana_stream_sdk::shreds_udp::{ShredsUdpConfig, ShredsUdpState, DeshredPolicy, handle_pumpfun_watcher};
use solana_stream_sdk::UdpShredReceiver;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cfg = ShredsUdpConfig::from_env(); // reads SHREDS_UDP_CONFIG jsonc too
    let mut receiver = UdpShredReceiver::bind(&cfg.bind_addr, None).await?;
    let policy = DeshredPolicy { require_code_match: cfg.require_code_match };
    let watch_cfg = Arc::new(cfg.watch_config());
    let state = ShredsUdpState::new(&cfg);
    loop {
        handle_pumpfun_watcher(&mut receiver, &state, &cfg, policy, watch_cfg.clone()).await?;
    }
}
```

Modular pipeline example (pump.fun opt-out):
```rust
use solana_stream_sdk::shreds_udp::{
    collect_watch_events, decode_udp_datagram, deshred_shreds_to_entries, insert_shred,
    DeshredPolicy, ShredInsertOutcome, ShredSource, ShredsUdpConfig, ShredsUdpState,
};
use solana_stream_sdk::txn::{ProgramWatchConfig, SplTokenMintFinder};
use solana_stream_sdk::UdpShredReceiver;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cfg = ShredsUdpConfig::from_env();
    let mut receiver = UdpShredReceiver::bind(&cfg.bind_addr, None).await?;
    let policy = DeshredPolicy { require_code_match: cfg.require_code_match };
    let state = ShredsUdpState::new(&cfg);
    let watch_cfg = Arc::new(
        ProgramWatchConfig::new(vec![], vec![]) // opt-out of pump.fun defaults
            .with_mint_finder(Arc::new(SplTokenMintFinder))
            .with_detailers(Vec::new()),
    );

    loop {
        let datagram = receiver.recv_raw().await?;
        if let Some(decoded) = decode_udp_datagram(&datagram, &state, &cfg).await {
            if let ShredInsertOutcome::Ready(ready) =
                insert_shred(decoded, &datagram, &state, &cfg, &policy).await
            {
                let entries = deshred_shreds_to_entries(&ready.shreds)?;
                let txs: Vec<_> = entries.iter().flat_map(|e| e.transactions.iter()).collect();
                let _hits = collect_watch_events(ready.key.slot, &txs, watch_cfg.as_ref(), 0);
                state.remove_batch(&ready.key).await;
                if matches!(ready.source, ShredSource::Data) {
                    state.mark_completed(ready.key).await;
                }
            }
        }
    }
}
```

## ⚠️ Experimental Filtering Feature Notice

Filtering remains experimental on the **Shreds gRPC** path (`shreds-rs`, `shreds-ts`): requests should send empty filter maps because shreds-side filters are not usable yet. Geyser gRPC filters are fine. For workloads that need filtering, prefer the high-speed, customizable UDP shreds pipeline described above. Occasionally, data may not be fully available, and filters may not be applied correctly on the shreds gRPC path.

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
