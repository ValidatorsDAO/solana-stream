# Alpenglow migration

## When

Alpenglow activates on Solana mainnet-beta at epoch 1064, expected around
2026-10-21 13:00–15:00 UTC. The changes below apply from the first Alpenglow slot.

Rust SDK 3.0.0 (crates.io `solana-stream-sdk`) and Node.js SDK 3.0.0
(`@validators-dao/solana-stream-sdk`) are the releases that handle them. They are a
release candidate and are not published yet.

## Entry streams carry block markers

A deshredded payload is either an entry batch or a block marker. A marker payload
has an entry count of 0 followed by a V1 marker: `BlockHeader`, `BlockFooter`,
`UpdateParent` or `GenesisCertificate`. The wire layout is `BlockComponent` /
`VersionedBlockMarker` / `BlockMarkerV1` in `solana-entry`
(`block_component.rs`).

In 2.0.0 such a payload decoded as 0 entries without an error: `decode_entries`
returns `Ok` with an empty vector for `UpdateParent` and `BlockFooter` bytes. A 2.0.0
client therefore does not crash, but it cannot see markers.

### UDP shreds (Rust)

Use `deshred_shreds_to_component` instead of `deshred_shreds_to_entries`. It returns a
`BlockComponentPayload`:

```rust
use solana_entry::block_component::{VersionedBlockMarker, VersionedUpdateParent};
use solana_stream_sdk::shreds_udp::ShredReadyBatch;
use solana_stream_sdk::{deshred_shreds_to_component, BlockComponentPayload, BlockMarkerKind};

fn handle(ready: &ShredReadyBatch) {
    match deshred_shreds_to_component(&ready.shreds) {
        Ok(BlockComponentPayload::Entries(entries)) => {
            // Transactions, as before. An empty vector is the empty entry batch.
            let _ = entries;
        }
        Ok(BlockComponentPayload::BlockMarker { kind, marker }) => {
            if kind == BlockMarkerKind::UpdateParent {
                if let Some(VersionedBlockMarker::V1(marker)) = &marker {
                    if let Some(VersionedUpdateParent::V1(update)) = marker.as_update_parent() {
                        println!("slot {} moved to parent {}", ready.key.slot, update.new_parent_slot);
                    }
                }
            }
        }
        Err(error) => {
            // The shreds did not deshred, or an entry batch did not decode.
            let _ = error;
        }
    }
}
```

- `BlockMarkerKind` is `BlockFooter`, `BlockHeader`, `UpdateParent`,
  `GenesisCertificate` or `Unknown`. `Unknown` is a zero entry count followed by bytes
  that are not a V1 marker, or a V1 marker with a variant this SDK does not know.
  A caller that guessed where a segment starts (a mid-batch slice can begin with zero
  bytes) must treat `Unknown` as undecodable, not as a marker.
- `marker` is `None` when the marker body does not decode, and always for `Unknown`.
- `classify_block_marker(&[u8]) -> Option<BlockMarkerKind>` and
  `decode_block_component(&[u8])` work on the deshredded bytes directly. `None` from
  `classify_block_marker` means an entry batch, including the empty batch `[0u8; 8]`.
- `deshred_shreds_to_entries` keeps its signature and returns an empty vector for a
  marker payload.
- `run_shreds_udp` counts markers per kind (`ShredMetrics::block_marker_count(kind)`),
  logs them (an `UpdateParent` marker with its new parent slot) and keeps them out of
  the entry path. It emits no watch events for a marker. The sample clients `shreds-udp-rs` and `generic_logger` log the marker
  kind.

### Shredstream gRPC

The gRPC client is unchanged. `decode_entries` (Rust) and `decodeSolanaEntries`
(Node.js) are the entry decoders for the `entries` bytes; treat a frame that decodes to
0 entries as a frame without transactions. `decode_block_component` takes the same byte
slice as `decode_entries` if you need the marker kind.

## UpdateParent

An `UpdateParent` marker means the leader restarted the slot on a new parent. Entries of
that slot received before the marker belong to the abandoned bank and must not be
treated as part of the block. The SDK does not retract entries it has already returned;
it reports the marker and the caller decides.

- UDP: on `BlockMarkerKind::UpdateParent` for slot S, discard what you derived from
  slot S so far.
- Geyser: set `include_update_parent` on the entry filter and handle
  `EntryUpdateParent`. In `geyser.proto` 14.0.1, `SubscribeUpdateEntryUpdateParent`
  "Invalidates entries for cleared_bank_id, including entries that arrive after this
  update"; the replacement bank id is reported by the `CreatedBank` slot update. Join
  updates on `(slot, bank_id)`.
- Geyser deshred stream: `include_update_parent` on the deshred transactions filter adds
  `SubscribeUpdateDeshredUpdateParent` updates, which precede the transactions from the
  data set at the `UpdateParent` FEC-set boundary.

## Vote transactions

Alpenglow votes are not vote transactions in blocks. Vote-transaction counts and
vote-based filters drop to zero.

## Block footer over Geyser

Subscribe with a `block_footer` filter (`include_certificates`). The server sends
`SubscribeUpdateBlockFooter` as its own update: `slot`, `bank_id`, `bank_hash`,
`block_producer_time_nanos`, `block_user_agent` and, when `include_certificates` is set,
the certificates. It does not wait for the block it belongs to; join it with other
per-block updates on `(slot, bank_id)`.

Rust:

```rust
use solana_stream_sdk::{
    GeyserSubscribeRequest, GeyserSubscribeRequestFilterBlockFooter,
    GeyserSubscribeRequestFilterEntry,
};

let request = GeyserSubscribeRequest {
    entry: [(
        "entries".to_string(),
        GeyserSubscribeRequestFilterEntry {
            include_update_parent: Some(true),
        },
    )]
    .into(),
    block_footer: [(
        "footer".to_string(),
        GeyserSubscribeRequestFilterBlockFooter {
            include_certificates: Some(true),
        },
    )]
    .into(),
    ..Default::default()
};
```

Node.js:

```ts
stream.write({
  accounts: {},
  slots: {},
  transactions: {},
  transactionsStatus: {},
  blocks: {},
  blocksMeta: {},
  entry: { entries: { includeUpdateParent: true } },
  blockFooter: { footer: { includeCertificates: true } },
  accountsDataSlice: [],
})
```

## Breaking changes

### Rust

- `GeyserSubscribeRequest` has a new `block_footer` field,
  `GeyserSubscribeRequestFilterEntry` has a new `include_update_parent` field and
  `GeyserSubscribeRequestFilterDeshredTransactions` has a new `include_update_parent`
  field. Struct literals that list every field need the new field or
  `..Default::default()`.
- `GeyserUpdateOneof` has the new variants `BlockFooter` and `EntryUpdateParent`.
  Exhaustive matches need arms.
- `bank_id` fields on the slot, account, transaction, transaction status, entry, block
  and block meta updates.
- The re-exported `yellowstone-grpc-client` moves from 13.5.0 to 15.0.0 and
  `yellowstone-grpc-proto` from 12.7.0 to 14.0.1.
- New re-exports: `GeyserSubscribeRequestFilterBlockFooter`, `GeyserUpdateBlockFooter`,
  `GeyserUpdateEntryUpdateParent` and `GeyserSubscribeUpdateDeshredUpdateParent`.
- Unchanged: the Solana crates stay at 4.2.2 (`solana-entry` 4.2.2 already contains
  `block_component`) and the toolchain stays at Rust 1.96.1.

### Node.js

`@validators-dao/solana-stream-sdk` 3.0.0 depends on `@triton-one/yellowstone-grpc`
9.0.0 (it was 7.0.1) and exports `SubscribeRequestFilterBlockFooter`.

- `ReconnectOptions` is `{ backoff? }` only; `enabled`, `slotRetention` and `policy`
  are removed.
- `SubscribeRequest.blockFooter` is a required map in the TypeScript type
  (`blockFooter: {}`).
- New `Client.subscribeWithReconnect`. Its stream emits `ReconnectEvent` values, which
  are either `{ type: "Update", generation, update }` or
  `{ type: "DiscardBanks", banks, reason, replacement, winners }`. Apply `DiscardBanks`
  before the replacement updates that follow it.
- The stream classes now extend a shared `SubscriptionDuplexStream` base class, and
  `on("data", listener)` is typed with the update type.
- `SubscribeUpdate` has the new `blockFooter` and `entryUpdateParent` fields.

## Versions

| Package | Version | Change |
| --- | --- | --- |
| crates.io `solana-stream-sdk` | 3.0.0 | Block markers, Yellowstone 15.0.0 / 14.0.1 |
| npm `@validators-dao/solana-stream-sdk` | 3.0.0 | `@triton-one/yellowstone-grpc` 9.0.0 |
| npm `@validators-dao/solana-entry-decoder` | 2.5.0 | Unchanged, not republished |
| npm `@validators-dao/solana-shreds-client` | 2.0.0 | Unchanged, not republished |
