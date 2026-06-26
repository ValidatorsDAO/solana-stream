# Changelog

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
