# Changelog

## Unreleased

### Rust SDK 5.0.0: the `udp` feature without `solana-ledger` (breaking)

The crate version is 5.0.0 on `main`; it is not published yet.

- The SDK decodes shreds itself: `solana_stream_sdk::shred` (behind `udp`) replaces `solana-ledger` in the UDP receiver. It decodes the Merkle shreds Agave 4.4.1 sends (chained, resigned or not; Agave 4.4.1 no longer accepts legacy or unchained shreds, and neither does the SDK), makes the checks Agave's `sanitize` makes in the same order with the same error messages, computes the Merkle root from a shred and its proof, checks the leader's signature, and deshreds. Each piece names the `solana-ledger` 4.4.1 function it follows.
- `udp` no longer builds `solana-ledger`, and with it RocksDB, OpenSSL, `native-tls`, `hyper-tls` and bzip2 (`libbz2-rs-sys`, licence `bzip2-1.0.6`). It needs no `[patch.crates-io]` and no libclang. It resolves 419 crates instead of 648 (`cargo tree -p solana-stream-sdk --features udp -e normal`). It keeps `solana-rpc-client` (leader schedule, block times), whose `reqwest` uses rustls. CI checks that the feature builds none of these.
- Breaking: `deshred_shreds_to_entries`, `deshred_shreds_to_component`, `ShredReadyBatch::shreds`, `DecodedShred::shred` and the receiver's batches use `solana_stream_sdk::shred::Shred` instead of `solana_ledger::shred::Shred`. The function names and what they do are unchanged. `Shred` keeps Agave's variants (`ShredCode`, `ShredData`) and accessors; `payload()` returns `shred::Payload`, which derefs to `[u8]`.
- New API in `solana_stream_sdk::shred`: `Shred::new_from_serialized_shred`, `sanitize`, `merkle_root`, `chained_merkle_root`, `retransmitter_signature`, `verify(&leader)`; `deshred` (Agave's `Shredder::deshred`); `ShredError`; `wire` (also `layout`): `get_slot`, `get_index`, `get_fec_set_index`, `get_signature`, `get_merkle_root`, … and `verify_shred(packet, &leader)`, the check Agave's `sigverify_shreds::verify_shred_cpu` makes on a received packet.
- Tests: `tests/shred_differential.rs` runs the SDK and `solana-ledger` 4.4.1 on the same bytes and requires the same answer, on 14,912 shreds from Agave's `Shredder`, 36,547 corrupted copies, 88,600 header values at the limits `sanitize` checks, 6,848 shreds with every proof size, 18,000 deshred lists and 30,000 random inputs. `solana-ledger` is a dev-dependency only; the workspace keeps the `rust-rocksdb` `[patch.crates-io]` for it, and `cargo test` in this repository still needs libclang.

Moving from 4.x (only if you enable `udp`):

1. `solana-stream-sdk = { version = "5.0.0", features = ["udp"] }`. Remove the `librocksdb-sys` and `rocksdb` lines under `[patch.crates-io]`, and `solana-ledger`, unless something else in your workspace needs them.
2. Replace `solana_ledger::shred::{Shred, layout}` with `solana_stream_sdk::shred::{Shred, layout}`; `shred::Error` is `shred::ShredError`, `Shredder::deshred(…)` is `shred::deshred(…)`. Checking a received packet's signature: `shred::wire::verify_shred(&packet, &leader)` with the leader of `wire::get_slot(&packet)`.
3. `client/shreds-udp-rs` needs no source change. `temp-release/shreds-udp-rs` stays on the published 4.0.0 until 5.0.0 is published; then it needs only `Cargo.toml` changes: SDK 5.0.0, no `solana-ledger`, no `[patch.crates-io]`.

### Other changes

- `package/solana-shreds-client` (the source of the npm native client, which does not use `udp`) requires the SDK at 5.0.0 so that it keeps building against this repository's crate. The workspace optimizes `solana-ed25519` and `sha2` in its dev builds: the shred tests check hundreds of thousands of signatures, which take 5 ms each unoptimized.
- Starters (`temp-release/`, which `slv bot init` copies from `main`) move to the published 4.0.0:
  - `geyser-rs` and `geyser-ts` are now the same code as `client/geyser-rs` and `client/geyser-ts`: the reconnecting subscription with `Disconnected` handling, redacted logs, the plaintext-token warning, and the faster TypeScript queue.
  - `shreds-rs`, `shreds-ts` and `shreds-udp-rs` move to SDK 4.0. `shreds-udp-rs` enables the `udp` feature and carries the rust-rocksdb `[patch.crates-io]` lines it needs.
  - The Rust starters declare `rust-version = "1.98.1"`.
- `trade-app` builds again. `ultima-swap-pumpfun` is pinned to `dbbe5f6e` (later commits add a required field to `SellParams` / `BuyParams`, so `cargo build` failed). It moves to SDK 4.0 and `dotenvy`, and keeps its own runner: it trades at processed commitment without de-duplication, and the 4.0 runner replays from the last confirmed slot after each reconnect, which would resend recent pool creations.
- TypeScript starters are standalone pnpm projects, as users get them. Each has a `pnpm-workspace.yaml` that decides esbuild's install script: without one, pnpm 11 stops `pnpm install` with `ERR_PNPM_IGNORED_BUILDS`. They are no longer packages of the repository's workspace.
- CI installs, type-checks, tests and builds the TypeScript starters on their own, and runs `cargo check --all-targets` on the four Rust starters against the published SDK.
- Docs: `client/shreds-rs` and the crate README named SDK 3.0.0 and Rust 1.96.1; now SDK 4.0 and Rust 1.98.1.

## 4.0.0 - 2026-11-06 — Yellowstone gRPC 17 clients, Agave 4.4

Published 2026-11-06: crates.io `solana-stream-sdk` 4.0.0; npm `@validators-dao/solana-stream-sdk` 4.0.0 (published from a workstation, so without a provenance statement; later releases go through `.github/workflows/release.yml`). `@validators-dao/solana-shreds-client` 2.0.0 and `@validators-dao/solana-entry-decoder` 2.5.0 are unchanged and not republished. Migration: [docs/migration-v4.md](docs/migration-v4.md).

- Rust: `yellowstone-grpc-client` 15.0.0 → 17.0.0 (`yellowstone-grpc-proto` stays 14.0.1). `subscribe_with_reconnect` yields `ReconnectEvent::Update(update)` and `ReconnectEvent::Disconnected { partial_banks, replay_from_slot }` at any commitment; re-exported as `GeyserReconnectEvent`, `GeyserDisconnected`, `GeyserBankRef`, `GeyserReconnectConfig`, `GeyserReconnectPolicy`, `GeyserBackoff`, `GeyserReconnectStream` and `GEYSER_RECONNECT_FILTER_KEY`. New example `geyser_reconnect`.
- Rust: Solana crates 4.2.2 → 4.4.1 (`solana-entry`, `solana-ledger`, `solana-rpc-client`), `solana-message` / `solana-transaction` 5.x, `wincode` 0.6, `http` 1.x in `SolanaStreamError::InvalidUri`. MSRV 1.98.1; the repository toolchain is Rust 1.99.1.
- Rust: the `udp` feature is opt-in. It needs Agave's `rust-rocksdb` patch in the workspace that enables it (`solana-ledger` 4.4.1 does not build against crates.io `rocksdb`). Without it the SDK resolves 396 crates instead of 684.
- Node.js: `@triton-one/yellowstone-grpc` 9.0.0 → 10.0.0 (napi 0.4.0). `subscribeWithReconnect()` yields `Update` and `Disconnected { partialBanks, replayFromSlot }` events; reconnect options take `policy` and `slotsBehind`. New exports: `ClientReconnectDuplexStream`, `RECONNECT_FILTER_KEY` and the `BankRef`, `ChannelOptions`, `ReconnectEvent`, `ReconnectOptions`, `SubscribeRequest`, `SubscribeUpdate` types. `engines.node` is `>=22`; CI runs Node.js 22, 24 and 26.
- Sample clients `client/geyser-rs` and `client/geyser-ts` stream with the reconnecting subscription and hand `Disconnected` to user code (never dropped by the bounded queue); they subscribe again with `from_slot` when the stream gives up, and stop on `RESOURCE_EXHAUSTED`. `client/geyser-rs` no longer uses the unmaintained `backoff` crate.
- Native packages: the sources of `@validators-dao/solana-entry-decoder` (now on `solana-entry` 4.4.1) and `@validators-dao/solana-shreds-client` (now on SDK 4.0) moved; their published binaries (2.5.0, 2.0.0) are unchanged and need a rebuild for a new release.
- `temp-release/` (served by `slv bot init`) still uses 3.x; moving it to 4.0 is the next step.
- macOS: the `client/geyser-rs` README no longer recommends `DYLD_LIBRARY_PATH=/opt/homebrew/opt/llvm/lib`, which makes Rust 1.99 fail with `LLVM version mismatch`.

### Performance (measured)

- UDP shreds: `insert_shred` pruned the `completed` and `suppressed` TTL maps on every datagram, reading the clock once per entry; `completed` keeps every FEC set finished in the last 30 s. The maps are now pruned at most every 250 ms, and a lookup checks the entry's own age, so TTLs are unchanged. Pipeline bench (`bench_udp_pipeline`: 300 slots of 20 FEC sets, 403,200 datagrams, decode → insert → deshred, median of 5 runs): 94,236 → 855–881 ns per datagram (11k → ~1.15M datagrams/s), same output.
- TypeScript runner: the consumer takes queued events synchronously instead of awaiting `next()` per event, and yields to the event loop every 1024 events. `bench/queue.bench.ts` (2M events, Node 24): 5.7–7.4 → 24.8–29.4 M events/s.
- Measured without a code change: reconnect resume time after a dropped connection (`bench_reconnect_gap`) is a median 103 ms with the 4.0 runner against 552 ms with the 3.0 runner. Requesting zstd/gzip responses from `grpc-ams1.erpc.global` changes nothing on the wire (the server sends uncompressed); `examples/geyser_wire_bench.rs` measures that the template's filter would shrink by 60% with per-message zstd.

### Security

- Credentials stay out of logs: `solana_stream_sdk::redact` (`redact_url`, `redact_urls`) strips credentials, path and query from URLs. `ShredsUdpConfig::describe()` logged `rpc=<endpoint>` as it was and the UDP block-time lookup logged RPC errors with the request URL, both including any `?api-key=`; they now log the redacted form. The sample clients log the endpoint and RPC errors the same way (`src/utils/redact.ts` in the TypeScript clients) and warn when `X_TOKEN` would go unencrypted over `http://` to another host.
- CI: gitleaks over the whole history and the tracked-file hygiene gate from elsoul/erpc-sdk (`scripts/run-gitleaks.sh`, `scripts/check-oss-hygiene.sh`, with a self-test), `cargo deny check` (`deny.toml`: advisories, licenses, bans, and sources limited to crates.io plus Agave's rust-rocksdb fork) and `pnpm audit`.
- Dependencies: RustSec vulnerabilities in `Cargo.lock` 25 → 2 (with the 4.0 update), and the remaining two (`ed25519-dalek` 1.0.1, `curve25519-dalek` 3.2.0) come only through Agave's `agave-precompiles` in the opt-in `udp` build; `deny.toml` records why each accepted advisory is accepted. `dotenv` (unmaintained) → `dotenvy`. `pnpm audit` 16 → 0: `tsup` 8.5.1 → 9.0.0, `npm-run-all` (unmaintained) → `npm-run-all2`.
- Release: `.github/workflows/release.yml` publishes with OIDC trusted publishing (no stored registry token) and npm provenance; manual dispatch, dry run by default. `@validators-dao/solana-stream-sdk` sets `publishConfig.provenance`.
- `SECURITY.md` and `CONTRIBUTING.md`.
- Removed `temp-release/geyser-ts3.tar.gz`, `shreds-rs3.tar.gz` and `shreds-ts.tar.gz`: no longer served (`slv` ≥ 0.13.14 copies `temp-release/<dir>/`; the old download location returns 404) and they still named Shreds and gRPC hosts that do not resolve.

### Yellowstone gRPC 17 readiness and templates

- Yellowstone gRPC 17 (Agave 4.4): document what changes for clients — `from_slot` replays only from the first request of a stream, commitment statuses and `CreatedBank` follow the content at confirmed/finalized, and an oversized request ends the stream with `RESOURCE_EXHAUSTED` ([docs/alpenglow.md](docs/alpenglow.md#yellowstone-grpc-17-agave-44)). No SDK package changes: `geyser.proto` is unchanged.
- Geyser sample runners (`client/geyser-rs`, `client/geyser-ts` and their `temp-release/` copies, including `trade-app`): stop with the server's message when the server ends the stream with `RESOURCE_EXHAUSTED`, instead of reconnecting forever (Rust) or up to `maxRetries` (TypeScript). The binaries exit with status 1.
- Templates and docs: replace the Shreds and Geyser gRPC hosts that no longer resolve (`shreds-ams`, `shreds-ams-9`, `grpc-ams`, `grpc`) with `http://shreds-ams6-1.erpc.global` and `http://grpc-ams1.erpc.global`. `scripts/check-erpc-endpoints.mjs` fails CI on a host that is not in service.
- Add CI (GitHub Actions): endpoint check, TypeScript SDK and Geyser template tests, Rust workspace tests.

## 3.0.0 - 2026-10-21 — Alpenglow

Published 2026-10-21: crates.io `solana-stream-sdk` 3.0.0; npm `@validators-dao/solana-stream-sdk` 3.0.0. `@validators-dao/solana-shreds-client` 2.0.0 and `@validators-dao/solana-entry-decoder` 2.5.0 are unchanged. Migration: [docs/alpenglow.md](docs/alpenglow.md).

- Rust SDK 3.0.0 adds the block-marker API for Alpenglow: `classify_block_marker`, `decode_block_component`, `BlockComponentPayload`, `BlockMarkerKind` and `deshred_shreds_to_component`. `BlockMarkerKind` is `#[non_exhaustive]`, so a `match` on it needs a wildcard arm. `deshred_shreds_to_entries` keeps its signature and returns an empty `Vec` for a marker payload.
- Rust: re-export the `solana-entry` crate as `solana_stream_sdk::solana_entry`, so the marker types inside `BlockComponentPayload` need no extra dependency.
- The UDP runner (`run_shreds_udp`) counts markers by kind (`ShredMetrics::block_marker_count`), logs an `UpdateParent` marker with its new parent slot and keeps marker payloads out of the entry path. The sample UDP clients (`shreds-udp-rs`, `generic_logger`) log the marker kind.
- Update the Rust Yellowstone dependencies to client 15.0.0/protobuf 14.0.1 and re-export `GeyserSubscribeRequestFilterBlockFooter`, `GeyserUpdateBlockFooter`, `GeyserUpdateEntryUpdateParent` and `GeyserSubscribeUpdateDeshredUpdateParent`. The new protobuf fields and `UpdateOneof` variants are breaking for struct literals and exhaustive matches (see the [migration guide](docs/alpenglow.md)).
- Rust: `subscribe` and `subscribe_with_request` of `yellowstone-grpc-client` no longer reconnect, even when `GeyserGrpcBuilder::set_reconnect_config` is set (in 13.5.0 that setting made them reconnect). The setting still compiles but the stream ends at the first disconnect. Use `subscribe_with_reconnect` (processed commitment, no `from_slot`; handle `ReconnectEvent::DiscardBanks`) or write your own reconnect loop.
- Node.js: calling `SubscribeRequest.encode` directly with a request that has no `blockFooter` throws; `stream.write` fills the missing field first and is not affected.
- Node.js SDK 3.0.0 moves to `@triton-one/yellowstone-grpc` 9.0.0 and exports `SubscribeRequestFilterBlockFooter`.
- Node.js: add `classifyBlockMarker(payload)` and the `BlockMarkerKind` type, so `entries` bytes can be checked for an Alpenglow block marker before `decodeSolanaEntries` (which decodes a marker as an empty list).
- Rust and Node.js: a block marker followed by zero padding after its declared length is classified (and, in Rust, decoded) as its kind.
- Unchanged: the Solana crates stay at 4.2.2 (`solana-entry` 4.2.2 already contains `block_component`) and Rust stays at 1.96.1. `@validators-dao/solana-entry-decoder` 2.5.0 and `@validators-dao/solana-shreds-client` 2.0.0 are not republished.

## 2.0.0 - 2026-09-17 — Transaction v1

Published 2026-09-17: crates.io `solana-stream-sdk` 2.0.0; npm `@validators-dao/solana-stream-sdk` 2.0.0, `@validators-dao/solana-shreds-client` 2.0.0, `@validators-dao/solana-entry-decoder` 2.5.0 (native decoders rebuilt for darwin/linux × x64/arm64). Migration: [docs/transaction-v1.md](docs/transaction-v1.md).

- Rust SDK 2.0.0, Node.js SDK/client 2.0.0 and entry decoder 2.5.0 (see the [migration guide](docs/transaction-v1.md)).
- Decode legacy, v0 and v1 entries with the Agave 4.2.2 wire schema in Rust, UDP reconstruction and the native Node.js decoder. Use `solana_stream_sdk::decode_entries` instead of a legacy `bincode` decoder.
- Move Rust public transaction types to the compatible modular Solana crates and require Rust 1.96.1. The optional `udp` feature remains enabled by default.
- Update Yellowstone dependencies to Rust client 13.5/protobuf 12.7 and Node.js client 7.0.1, preserving v1 configuration presence and values.
- Return a present Node.js v1 `config.priorityFee` as an exact decimal string, including zero, preserving values above JavaScript's safe integer limit.
- Keep the existing Shredstream gRPC method and field numbers, provider-independent endpoint selection and local protobuf definitions.

## 1.4.0 - 2026-06-26

- Updated the Rust Yellowstone client to `yellowstone-grpc-client@13.1.1` and protobuf crate to `yellowstone-grpc-proto@12.5.0`, matching the current stable upstream 13.x client line.
- Removed legacy `yellowstone_grpc_proto::plugin` re-export aliases because upstream 12.5.0 no longer ships the `plugin` module.
- Added public aliases for Yellowstone 13.x deshred subscription types, token-account expansion controls, and cuckoo compressed account filter helpers.
- Updated Rust Geyser sample builders for new optional protobuf fields: `token_accounts`, `cuckoo_accounts_filter`, and `cuckoo_account_include`.
- Confirmed the TypeScript SDK remains on current upstream `@triton-one/yellowstone-grpc@5.0.9`.

## 1.3.0 - 2026-06-26

- Updated the TypeScript SDK and starter clients to Solana Kit (`@solana/kit`) for RPC block-time lookups.
- Updated the TypeScript Yellowstone client dependency to `@triton-one/yellowstone-grpc@5.0.9`.
- Updated the TypeScript build toolchain to pnpm 11, TypeScript 6, tsup 8.5, and Node 24 typings.
- Updated Rust lockfile dependencies within the current Agave 3.x compatible line and moved the workspace toolchain to Rust 1.89.
- Reduced known RustSec findings versus the previous lockfile without adding new advisories; remaining findings are inherited from Agave/Solana transitive dependencies.
- Documented the Yellowstone status: Agave 4.0.2 server/plugin support is available upstream in `rpcpool/yellowstone-grpc` `v13.3.0+solana.4.0.2`, while this SDK keeps the Rust Yellowstone public API on 10.x until a breaking 13.x migration removes or replaces the legacy `plugin` re-exports.

## 1.2.2 - 2026-06-25

- Fixed Direct Shreds UDP merge test coverage to match the create-first detail backfill behavior.
- Published `@validators-dao/solana-stream-sdk@1.2.2` to npm to align with the already published Rust crate.
