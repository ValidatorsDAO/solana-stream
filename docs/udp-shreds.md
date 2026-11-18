# UDP Shreds: Rust receiver and program watcher

Receive raw Solana shreds over UDP, assemble entries, and build program-specific consumers. This is the lower-level path: choose it when you need early signals and are prepared to handle packet loss, out-of-order data, and pre-finalization observations.

**Start elsewhere?** [Compare stream types](../README.md#choose-your-stream) · [Geyser gRPC starter](../client/geyser-rs/README.md) · [Shreds gRPC starter](../client/shreds-rs/README.md)

## Quick start

Use a sender that can forward Solana shreds to your listener's reachable IP and UDP port. The repository does not provide a live UDP feed.

1. Configure `client/shreds-udp-rs/settings.jsonc` (including `bind_addr` and optional watch targets). This file is embedded at build time; rebuild after editing.
2. Supply secrets through environment variables rather than committing them.
3. Run the generic logger, which has no application-specific watch targets unless you set them.

```bash
git clone https://github.com/ValidatorsDAO/solana-stream.git
cd solana-stream
export SOLANA_RPC_ENDPOINT=https://api.mainnet-beta.solana.com
cargo run -p shreds-udp-rs --bin generic_logger
```

To watch your own program instead of all entries:

```bash
GENERIC_WATCH_PROGRAM_IDS=YOUR_PROGRAM_ID \
  cargo run -p shreds-udp-rs --bin generic_logger
```

Point your Shreds sender at the listener's `ip:port`. No packets means no events to decode. The default binary (`cargo run -p shreds-udp-rs`) uses the pump.fun-oriented convenience watcher; use `generic_logger` for a general starting point.

For a standalone project based on the published Rust crate, see [`temp-release/shreds-udp-rs`](../temp-release/shreds-udp-rs/README.md). SDK 5.0.0's `udp` feature does not pull in `solana-ledger`, RocksDB or libclang for consumers.

## Processing model

```text
UDP datagrams -> decode/prefilter -> FEC batch -> deshred
                                            |-> entries -> watch/detail -> your sink
                                            |-> Alpenglow block marker
```

The components are composable:

- `UdpShredReceiver` binds a UDP socket and receives timestamped datagrams.
- `decode_udp_datagram` performs receive-side prefiltering.
- `insert_shred` with `ShredsUdpState` buffers and assembles FEC batches.
- `deshred_shreds_to_component` distinguishes decoded entries from Alpenglow block markers. Do not pass block markers to transaction watchers.
- `collect_watch_events` processes decoded transactions; customize the watch set with `ProgramWatchConfig`, `watch_config_no_defaults()`, or detailers. Connect the resulting detections to your own sink.

A convenience wrapper, `handle_pumpfun_watcher`, composes the stages with application-specific defaults. For other applications, prefer the modular pipeline.

## Minimal receiver

The following examples were moved from the root README; they demonstrate two integration styles.

### Convenience API

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

### Modular API with custom watch targets

```rust
use solana_stream_sdk::shreds_udp::{
    collect_watch_events, decode_udp_datagram, deshred_shreds_to_component, insert_shred,
    DeshredPolicy, ShredInsertOutcome, ShredSource, ShredsUdpConfig, ShredsUdpState,
};
use solana_stream_sdk::txn::{ProgramWatchConfig, SplTokenMintFinder};
use solana_stream_sdk::{BlockComponentPayload, UdpShredReceiver};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cfg = ShredsUdpConfig::from_env();
    let mut receiver = UdpShredReceiver::bind(&cfg.bind_addr, None).await?;
    let policy = DeshredPolicy { require_code_match: cfg.require_code_match };
    let state = ShredsUdpState::new(&cfg);
    let watch_cfg = Arc::new(
        ProgramWatchConfig::new(vec![], vec![]) // define your own watch set
            .with_mint_finder(Arc::new(SplTokenMintFinder))
            .with_detailers(Vec::new()),
    );

    loop {
        let datagram = receiver.recv_raw().await?;
        if let Some(decoded) = decode_udp_datagram(&datagram, &state, &cfg).await {
            if let ShredInsertOutcome::Ready(ready) =
                insert_shred(decoded, &datagram, &state, &cfg, &policy).await
            {
                match deshred_shreds_to_component(&ready.shreds) {
                    Ok(BlockComponentPayload::Entries(entries)) => {
                        let txs: Vec<_> =
                            entries.iter().flat_map(|e| e.transactions.iter()).collect();
                        let _hits =
                            collect_watch_events(ready.key.slot, &txs, watch_cfg.as_ref(), 0);
                    }
                    Ok(BlockComponentPayload::BlockMarker { kind, .. }) => {
                        // An Alpenglow block marker (UpdateParent, BlockFooter, ...), not transactions.
                        println!("slot {} block marker {:?}", ready.key.slot, kind);
                    }
                    Err(_) => {
                        state.remove_batch(&ready.key).await;
                        state.mark_suppressed(ready.key).await;
                        continue;
                    }
                }
                state.remove_batch(&ready.key).await;
                if matches!(ready.source, ShredSource::Data) {
                    state.mark_completed(ready.key).await;
                }
            }
        }
    }
}
```

## Configuration and log output

The sample's `settings.jsonc` controls the bind address, logging, FEC validation policy, slot windows, watched program IDs and authorities, token programs, and optional thresholds. Keep RPC credentials in environment variables.

The generic logger can be customized with `GENERIC_WATCH_PROGRAM_IDS` and `GENERIC_WATCH_AUTHORITIES`. Optional logging flags include `SHREDS_UDP_LOG_*` and the latency tracker (`SHREDS_UDP_ENABLE_LATENCY=1`).

The pump.fun-oriented sample uses these log symbols: `🎯` program match, `🐣` authority match/create, `🟢` buy, `🔻` sell, `🪙` other and `❓` incomplete or unknown amount. SOL amounts in these logs are instruction limits, **not confirmed execution fills**. Vote traffic is skipped by default.

## Operational caveats

- **Pre-finalization:** Observations may include failed transactions; packets can arrive out of order or be lost. Do not treat UDP receipt as proof of finalized execution.
- **Leader signatures:** The SDK's standard UDP receive loop, including these samples, does **not automatically verify** that a shred was signed by the slot leader. SDK 5.0.0 exposes `shred::wire::verify_shred`. Obtain the slot leader from a trusted leader schedule and validate each packet in your own receive loop when authenticity matters. See [leader-signature guidance](migration-v5.md#leader-signatures).
- **Alpenglow:** `deshred_shreds_to_component` returns either entries or a block marker. The older `deshred_shreds_to_entries` returns an empty vector for markers and cannot report their kind. See [Alpenglow changes](alpenglow.md).
- **Transaction v1:** Use a compatible decoder (SDK 2.0+). Invalid message-version, unexpected EOF, or alias-encoding errors often indicate a codec mismatch. Packet lengths around 1203/1228 bytes can be normal Merkle shreds; they do not alone prove truncation. See [Transaction v1](transaction-v1.md).
- **Verification and throughput:** Keep receive-side work small and downstream queues bounded. UDP transport does not guarantee delivery.

## Source and further reading

[UDP sample README](../client/shreds-udp-rs/README.md) · [Standalone starter](../temp-release/shreds-udp-rs/README.md) · [Rust 5.0 migration](migration-v5.md) · [SDK crate](../crate/solana-stream-sdk/README.md)
