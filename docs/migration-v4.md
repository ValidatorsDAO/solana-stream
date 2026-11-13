# Migrating to 4.0

4.0 moves the SDK to the Yellowstone gRPC 17 clients and the Agave 4.4 crates, and makes
the reconnecting subscription the recommended way to stream Geyser updates.

| | 3.0 | 4.0 |
| --- | --- | --- |
| Rust `yellowstone-grpc-client` / `-proto` | 15.0.0 / 14.0.1 | 17.0.0 / 14.0.1 |
| Node.js `@triton-one/yellowstone-grpc` | 9.0.0 (napi 0.3) | 10.0.0 (napi 0.4) |
| Solana crates (`solana-entry`, `solana-ledger`, `solana-rpc-client`) | 4.2.2 | 4.4.1 |
| `solana-message` / `solana-transaction` | 4.x | 5.x |
| Rust MSRV (repository toolchain) | 1.96.1 (1.96.1) | 1.98.1 (1.99.1) |
| Node.js | 22 or 24 | 22 or later (CI: 22, 24, 26) |
| Rust feature `udp` | on by default | opt-in |

`geyser.proto` is unchanged (14.0.1), so 3.0 clients keep working against
Yellowstone gRPC 17 servers. See
[Yellowstone gRPC 17](alpenglow.md#yellowstone-grpc-17-agave-44) for what changes on the
server side.

## The recommended path: a reconnecting subscription

`subscribe_with_reconnect` (Rust) and `subscribeWithReconnect` (Node.js) reconnect by
themselves. After a dropped connection they replay from the last confirmed slot the
stream saw (or from the first request's `from_slot` before one is known), at any
commitment. Each drop yields one `Disconnected` event before the updates of the new
connection:

- `partial_banks` / `partialBanks`: banks whose content you received in part (account
  writes, transactions, entries or `CreatedBank`, but not their block, block footer or
  block meta). Discard what you derived from them. The replay resends the ones at or
  above `replay_from_slot` whole; the stream drops whatever a later connection sends of
  the others.
- The replay also resends banks you already hold whole: handle duplicates. A replayed bank
  carries only the latest write of each account, so it can have fewer account updates
  than the same bank delivered live, with the same final state.
- The server keeps a bounded replay window (150 slots by default). An outage longer than
  that cannot be replayed.
- The retry budget is `max_retries` reconnects in a row; a connection that delivers a
  block, block footer or block meta refills it. When it is spent, the stream ends with the
  last error. Subscribe again, with `from_slot` set from the last slot you saw, as the
  sample runners do.
- A request over the server's `max_decoding_message_size` ends each attempt with
  `RESOURCE_EXHAUSTED`. The stream treats that as transient and spends its budget on it
  (about 6 s with the settings below), then ends with the status. Do not subscribe
  again with the same request; the sample runners stop and report the server's message.
- The stream adds block-meta and slot filters named `__reconn` (`RECONNECT_FILTER_KEY`)
  to your request and hides the updates only they match. Do not use that filter name.

Rust ([examples/geyser_reconnect.rs](../crate/solana-stream-sdk/examples/geyser_reconnect.rs)):

```rust
use futures::StreamExt;
use solana_stream_sdk::{
    GeyserBackoff, GeyserGrpcClient, GeyserReconnectConfig, GeyserReconnectEvent,
    GeyserReconnectPolicy,
};
use std::time::Duration;

let mut client = GeyserGrpcClient::build_from_shared(endpoint)?
    .set_reconnect_config(
        GeyserReconnectConfig::default()
            .with_backoff(GeyserBackoff::new(Duration::from_millis(100), 2.0, 6))
            .with_policy(GeyserReconnectPolicy::Replay { slots_behind: 0 }),
    )
    .connect()
    .await?;
let (_sink, mut stream) = client.subscribe_with_reconnect(Some(request)).await?;
while let Some(event) = stream.next().await {
    match event? {
        GeyserReconnectEvent::Update(update) => { /* handle the update */ }
        GeyserReconnectEvent::Disconnected(disconnected) => {
            // discard disconnected.partial_banks
        }
    }
}
```

Node.js:

```ts
import { GeyserClient } from '@validators-dao/solana-stream-sdk'
import type { ReconnectEvent } from '@validators-dao/solana-stream-sdk'

const client = new GeyserClient(endpoint, xToken, undefined, {
  backoff: { initialIntervalMs: 100, multiplier: 2, maxRetries: 6 },
  policy: 'Replay',
  slotsBehind: 0,
})
await client.connect()
const stream = await client.subscribeWithReconnect(request)
stream.on('data', (event: ReconnectEvent) => {
  if (event.type === 'Disconnected') {
    // discard event.partialBanks ({ slot, bankId } as decimal strings)
    return
  }
  handle(event.update)
})
stream.on('error', (error) => {
  // the retry budget is spent: subscribe again, unless it is RESOURCE_EXHAUSTED
})
```

The sample runners show the whole loop: [client/geyser-rs](../client/geyser-rs)
(`src/runtime/runner.rs`, `src/handlers/processor.rs`) and
[client/geyser-ts](../client/geyser-ts) (`src/lib/geyserRunner.ts`, `onDisconnected` in
`src/index.ts`).

Plain `subscribe` / `subscribe_with_request` still work and still do not reconnect.

## Breaking changes

### Rust

- `yellowstone-grpc-client` 17.0.0 rewrote `subscribe_with_reconnect`. It returns a
  `ReconnectStream` of `ReconnectEvent::Update(update)` (it was
  `Update { generation, update }`) and `ReconnectEvent::Disconnected(Disconnected {
  partial_banks, replay_from_slot })`, which replaces `DiscardBanks`. Any commitment is
  accepted, and `from_slot` is allowed in the first request. `ReconnectConfig` has
  `backoff` and `policy` (`ReconnectPolicy::Replay { slots_behind }` or `NoReplay`).
  Removed upstream: `ReconnectGeyserStream`, `AutoReconnect`, `DedupStream`, `DedupState`,
  `ReconnectionPolicy`, `DiscardReason`, `SlotWinner`, `AUTORECONNECT_FILTER_KEY` and
  `ReconnectConfig::with_slot_retention`.
- New re-exports: `GeyserReconnectEvent`, `GeyserDisconnected`, `GeyserBankRef`,
  `GeyserReconnectConfig`, `GeyserReconnectPolicy`, `GeyserBackoff`,
  `GeyserReconnectStream` and `GEYSER_RECONNECT_FILTER_KEY`.
- The Solana types the SDK exposes (`VersionedTransaction`, `Entry`, the
  `solana_entry` re-export) come from the Agave 4.4.1 crates: `solana-transaction` and
  `solana-message` 5.x, `solana-entry` 4.4.1. Code that passes them to its own
  `solana-*` dependencies needs the same majors.
- The `udp` feature is no longer on by default. In 4.x it pulls in `solana-ledger`, which
  builds only against Agave's `rust-rocksdb` fork (5.0 no longer needs it: see the
  [CHANGELOG](../CHANGELOG.md)). Enable it and copy the patch from this
  repository's `Cargo.toml` into your workspace (a `[patch]` applies only in the
  workspace that declares it):

  ```toml
  [dependencies]
  solana-stream-sdk = { version = "4", features = ["udp"] }

  [patch.crates-io]
  librocksdb-sys = { git = "https://github.com/anza-xyz/rust-rocksdb", rev = "4dba80dfe9434e595a971f5b2985ff9ba4fb31e7" }
  rocksdb = { git = "https://github.com/anza-xyz/rust-rocksdb", rev = "4dba80dfe9434e595a971f5b2985ff9ba4fb31e7" }
  ```

  Without `udp`, the SDK resolves 396 crates instead of 684 and needs neither
  `solana-ledger` nor RocksDB (nor libclang to build them).
- `SolanaStreamError::InvalidUri` wraps `http` 1.x `InvalidUri` (it was `http` 0.2).
- MSRV 1.98.1, the one Agave 4.4.1 requires.

### Node.js

- `@triton-one/yellowstone-grpc` 10.0.0 (napi 0.4.0 binaries). `subscribeWithReconnect()`
  yields `{ type: 'Update', update }` and
  `{ type: 'Disconnected', partialBanks, replayFromSlot }`. The `DiscardBanks` event and
  `generation` are gone. Reconnect options are `{ backoff, policy, slotsBehind }`, any
  commitment is accepted, and `AUTORECONNECT_FILTER_KEY` is now `RECONNECT_FILTER_KEY`.
- New exports: `ClientReconnectDuplexStream`, `RECONNECT_FILTER_KEY`, and the types
  `BankRef`, `ChannelOptions`, `ReconnectEvent`, `ReconnectOptions`, `SubscribeRequest`
  and `SubscribeUpdate`.
- `engines.node` is `>=22`.

### Sample clients

- `client/geyser-rs`: `run_geyser_stream` sends `GeyserReconnectEvent`s (it sent
  `GeyserSubscribeUpdate`s) and `process_updates` handles `Disconnected`. The `backoff`
  crate (unmaintained) is no longer used.
- `client/geyser-ts`: `runGeyser` takes `onDisconnected`, and `createEnvClient` passes the
  reconnect options. A `Disconnected` event is never dropped by the bounded queue. The
  request type is the SDK's `SubscribeRequest` (add `blockFooter: {}`).
- `client/shreds-udp-rs` enables the `udp` feature.

## Building on macOS

Rust 1.99 is built on LLVM 23. If `rustc` stops with `LLVM version mismatch: this compiler
was built for LLVM 23, but LLVM 21.1.8 was found at /opt/homebrew/.../libLLVM.dylib`,
`DYLD_LIBRARY_PATH` points at Homebrew's LLVM: remove it from your shell profile. Earlier
versions of the `client/geyser-rs` README recommended setting it. `LIBCLANG_PATH` alone is
enough for bindgen (only the `udp` feature needs it).

## Starters (`temp-release/`)

`slv bot init` copies `temp-release/<template>/` from the `main` branch, and those
starters install the published packages. They use 4.0.0 (published on 2026-11-06), and
CI checks them against it.

- `geyser-rs`, `geyser-ts`: the same code as `client/geyser-rs` and `client/geyser-ts`.
- `shreds-udp-rs`: enables `udp` and carries the rust-rocksdb `[patch.crates-io]`
  lines. Keep them if you move the crate into another workspace.
- `trade-app`: SDK 4.0, but its own runner. It trades at processed commitment without
  de-duplication, and the 4.0 runner replays from the last confirmed slot after each
  reconnect, which would resend recent pool creations. Add de-duplication before you
  switch it to `subscribe_with_reconnect`.
- `geyser-ts`, `shreds-ts`: standalone pnpm projects. Their `pnpm-workspace.yaml` turns
  esbuild's install script off; without a decision, pnpm 11 stops `pnpm install`
  (`ERR_PNPM_IGNORED_BUILDS`).
