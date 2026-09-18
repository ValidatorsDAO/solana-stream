# Transaction v1 migration

## Released

Rust SDK 2.0.0 (crates.io `solana-stream-sdk`), Node.js SDK/client 2.0.0
(`@validators-dao/solana-stream-sdk`, `@validators-dao/solana-shreds-client`) and
the Node.js entry decoder 2.5.0 (`@validators-dao/solana-entry-decoder`) were
published on 2026-09-17. Transaction v1 has been live on Solana mainnet since
epoch 1035 (2026-09-15); update to these versions to decode it.

Solana Transaction v1 changes the transaction wire layout. Its version prefix is
at the beginning of the transaction and signatures follow the message. A legacy
bincode/serde transaction decoder can report EOF, compact-length overflow or an
apparently invalid message version when given valid v1 data.

Use the current Agave wire schema for all transaction versions. Merely upgrading
bincode or continuing to use an older `solana-entry` does not add v1 support.

## Rust

Build this release with Rust 1.96.1, matching the Agave 4.2.2 toolchain.

```toml
solana-stream-sdk = "2.0.0"
```

For a gRPC-only client, disable the optional UDP reconstruction dependencies:

```toml
solana-stream-sdk = { version = "2.0.0", default-features = false }
```

Replace calls to `bincode::deserialize::<Vec<Entry>>` with:

```rust
let entries = solana_stream_sdk::decode_entries(&slot_entry.entries)?;
for entry in entries {
    for transaction in entry.transactions {
        // Handle legacy, v0 and v1 messages, including v1 transaction config.
        println!("{:?}", transaction.message);
    }
}
```

`Entry` and `VersionedTransaction` are re-exported by this SDK. Version 2 moves
the Rust public types to the Agave 4.2.2-compatible modular Solana crates; callers
must update any direct type dependencies and exhaustive message matches.
UDP and gRPC use the same entry decoder. The UDP receiver and reconstruction
helpers remain available with the default `udp` feature.

Decoding does not establish signature validity, confirmation, or finality.

## Node.js

Upgrade the native `@validators-dao/solana-entry-decoder` along with
`@validators-dao/solana-stream-sdk`. The `decodeSolanaEntries(Buffer)` entry point
is unchanged. Entry JSON retains `num_hashes`, `hash`, and `transactions`, and
v1 messages include their transaction configuration.

When present, a v1 message's `config.priorityFee` is a decimal string, including
`"0"`. This preserves all 64-bit lamport values without JavaScript number
rounding. Use `BigInt` for exact arithmetic; an unspecified priority fee remains
`null` in the native decoder's JSON, distinct from an explicit `"0"`.

The Geyser dependencies also move to Rust client 13.5/protobuf 12.7 and Node.js
client 7.0.1. These preserve the optional v1 transaction configuration, including
an empty configuration whose presence distinguishes v1 from v0.

Use matching published platform artifacts for your operating system and CPU;
a JavaScript-only package update cannot replace an older native decoder.

### Building Linux native addons from source

Install Zig 0.15.2 and the Rust targets below, then run the normal package scripts
from the repository root after `pnpm install`:

```bash
rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
pnpm --dir package/solana-entry-decoder run build:linux:x64
pnpm --dir package/solana-entry-decoder run build:linux:arm64
pnpm --dir package/solana-shreds-client run build:linux:x64
pnpm --dir package/solana-shreds-client run build:linux:arm64
```

The scripts target glibc 2.17 and keep build-time tools such as `protoc` on the
host compiler during cross-compilation. The current addons do not link OpenSSL;
these builds no longer require a separate OpenSSL build. Set `HOST_CC` and
`HOST_CXX` if the host C/C++ compilers are not available as `cc` and `c++`, and
use `CARGO_BUILD_JOBS` to override the default of four build jobs.

A successful cross-build does not replace runtime testing on the target machine.

## Provider independence

The SDK connects to the endpoint supplied by the caller and generates its gRPC
types from protobuf definitions checked into this repository. It does not
require a Jito Block Engine connection, Jito authentication service, Jito proxy
binary, or a dependency fetched from a Jito Git repository.

The existing `shredstream` protobuf package, `ShredstreamProxy.SubscribeEntries`
method and field numbers remain compatible with existing endpoints. Protocol
compatibility does not require using any particular upstream provider.

This SDK contains client-side streaming and decoding support. It does not
publish or include a provider's private forwarding service.

## References

- [SIMD-0385: Transaction v1](https://github.com/solana-foundation/solana-improvement-documents/blob/main/proposals/0385-transaction-v1.md)
- [Agave Entry 4.2.2](https://docs.rs/solana-entry/4.2.2/solana_entry/entry/struct.Entry.html)
