# Migrating to 5.0

5.0 changes only the Rust SDK's `udp` feature. If you do not enable `udp`, moving from
4.x is a version bump.

| | 4.x | 5.0 |
|---|---|---|
| Shred decoding behind `udp` | `solana-ledger` 4.4.1 | the SDK's own `solana_stream_sdk::shred` |
| `udp` builds RocksDB, OpenSSL, native-tls, bzip2 | yes | no |
| `[patch.crates-io]` for `rust-rocksdb`, libclang | needed | not needed |
| Shred type in the UDP API | `solana_ledger::shred::Shred` | `solana_stream_sdk::shred::Shred` |

## Dependencies

```toml
[dependencies]
solana-stream-sdk = { version = "5.0.0", features = ["udp"] }
```

Remove the `librocksdb-sys` and `rocksdb` lines under `[patch.crates-io]`, and
`solana-ledger`, unless something else in your workspace needs them. Tests that make
shreds with Agave's `Shredder` still need `solana-ledger`, as a dev-dependency (with the
patch, and libclang to build it); this repository does that.

## The UDP API

`deshred_shreds_to_entries`, `deshred_shreds_to_component`, `ShredReadyBatch::shreds`,
`DecodedShred::shred` and the receiver's batches take or carry
`solana_stream_sdk::shred::Shred`. Their names and what they do are unchanged, and the
sample clients need no source change.

## From `solana_ledger::shred` to `solana_stream_sdk::shred`

The module decodes and checks shreds as `solana-ledger` 4.4.1 does (the same bytes are
accepted and rejected, for the same reason), and keeps Agave's names for what it has.
It does not make, recover, re-sign or filter shreds. Replace the import, then:

### `Shred`

Kept, with the SDK's `ShredError` and `ShredType` where Agave's signature names its own:
`new_from_serialized_shred` (it takes any `impl Into<Vec<u8>>`: a `Vec<u8>` as it is,
or a `&[u8]`, a `shred::Payload` or a `bytes::Bytes`, converted into a `Vec<u8>`, which
copies the bytes unless a `Bytes` holds the only reference to them), `sanitize`, `merkle_root`, `chained_merkle_root`, `verify`, `slot`,
`index`, `version`, `fec_set_index`, `signature`, `parent`, `data`, `shred_type`,
`is_data`, `is_code`, `last_in_slot`, `data_complete`, and the variants
`Shred::ShredCode` and `Shred::ShredData`.

| `solana_ledger::shred::Shred` | `solana_stream_sdk::shred::Shred` |
|---|---|
| `proof_size()` returns `Result<u8, Error>` | returns `u8` (drop the `?`) |
| `payload()` returns Agave's `Payload` | returns `shred::Payload`: it derefs to `[u8]` and has `as_ref()`, but no `bytes` field, `as_mut`, `get_mut`, `truncate`, `into_bytes_mut` or `From<Vec<u8>>`/`From<Bytes>`/`From<BytesMut>` (make a shred from bytes with `new_from_serialized_shred`). Agave's `copy_to_packet`, `to_packet` and `to_bytes_packet` exist only with its `dev-context-only-utils` feature; there are none here. |
| `into_payload()` | `payload().clone()`, or `Vec::<u8>::from(payload().clone())` |
| `id()` (`ShredId`) | `(shred.slot(), shred.index(), shred.shred_type())` |
| `has_correct_proof_size()` | `shred.proof_size() == shred::PROOF_ENTRIES_FOR_32_32_BATCH` |
| `retransmitter_signature_offset()` | none; `retransmitter_signature()` reads the signature |
| `is_shred_duplicate(&other)` | none |
| (crate-private there) | `retransmitter_signature()`, `variant()` |

### `layout` (also `wire`)

Kept, with the SDK's `ShredError` and `ShredType` where Agave's signature names its own:
`get_shred_type`, `get_slot`, `get_index`, `get_fec_set_index`, `get_merkle_root`,
`get_retransmitter_signature`, `is_retransmitter_signed_variant`.

| `solana_ledger::shred::layout` | `solana_stream_sdk::shred::layout` |
|---|---|
| `get_shred(packet)` takes `Into<PacketRef>` | takes the packet's bytes, `&[u8]` |
| `get_flags` returns `ShredFlags` | returns the flags byte, `u8`; test it with `shred::FLAG_DATA_COMPLETE_SHRED`, `FLAG_LAST_SHRED_IN_SLOT`, `FLAG_SHRED_TICK_REFERENCE_MASK` |
| `get_shred_id` | `get_slot`, `get_index`, `get_shred_type` |
| `get_shred_and_repair_nonce`, `get_common_header_bytes`, `get_shred_mut` | none: read the bytes after `get_shred(packet)` |
| `set_retransmitter_signature`, `resign_shred`, `resign_packet` | none (the SDK does not sign) |
| (crate-private there) | `get_signature`, `get_version`, `get_parent_offset`, `get_coding_header`, `get_data`, `get_chained_merkle_root`, `get_shred_variant` |
| `sigverify_shreds::verify_shred_cpu(packet, &leaders, &cache)` | `verify_shred(&packet, &leader)`, with the leader of `get_slot(&packet)` from your leader schedule |

### Other items

| `solana_ledger::shred` | `solana_stream_sdk::shred` |
|---|---|
| `Error` | `ShredError`. Its messages are Agave's, except `TruncatedHeaders` (Agave: the header deserializer's `WincodeRead` error) and `InvalidErasureShardIndex` (Agave prints the headers). Agave's errors from making or recovering shreds have no counterpart. |
| `ShredFlags` | the `u8` constants `FLAG_SHRED_TICK_REFERENCE_MASK`, `FLAG_DATA_COMPLETE_SHRED`, `FLAG_LAST_SHRED_IN_SLOT`; `ShredData::flags()`, `ShredData::reference_tick()` |
| `ShredType`: `u8::from(t)`, `ShredType::try_from(byte)` | `t as u8`; no `TryFrom<u8>` (`ShredVariant::try_from(byte)` reads a variant byte) |
| `Shredder::deshred(shreds)` | `shred::deshred(shreds)`: the same rules and bytes |
| `SIZE_OF_NONCE`, `SIZE_OF_DATA_SHRED_HEADERS`, `DATA_SHREDS_PER_FEC_BLOCK`, `CODING_SHREDS_PER_FEC_BLOCK`, `MAX_DATA_SHREDS_PER_SLOT`, `MAX_CODE_SHREDS_PER_SLOT` | kept |
| `ShredData::capacity`, `ShredCode::capacity` | none (the payload sizes are `DATA_SHRED_PAYLOAD_SIZE`, `CODE_SHRED_PAYLOAD_SIZE`) |
| `ShredId`, `ErasureSetId`, `Nonce`, `SHREDS_PER_FEC_BLOCK`, `MAX_FEC_SETS_PER_SLOT`, `Shredder` (making shreds), `ReedSolomonCache`, `ProcessShredsStats`, `ShredFetchStats`, `merkle_tree`, `filter` | none |

## Leader signatures

The UDP receiver decodes and checks shreds but does not check that the slot's leader
signed them, in 4.x or 5.0. `run_shreds_udp` has no hook for such a check: what it emits
is unverified. To drop shreds the leader did not sign, write the receive loop yourself,
as `client/shreds-udp-rs` does: `UdpShredReceiver::recv_raw`, then
`shred::wire::verify_shred` on the datagram, with the leader of `wire::get_slot` from a
leader schedule you get from an RPC node you trust, then `decode_udp_datagram` and
`insert_shred`.
