# Performance measurements

These results describe **separate local benchmarks**, not a single end-to-end speedup or a guarantee of public-network latency. The measurements originally appeared alongside the [SDK v4 announcement](https://erpc.global/en/news/2026/11/07/erpc-solana-stream-sdk-v4-20261107/).

| Area measured | Reported result | What it does (and does not) mean |
| --- | --- | --- |
| Rust UDP internal datagram processing | Approximately **108×** faster than the preceding local implementation | Internal processing throughput; **not** an end-to-end or network-latency comparison |
| TypeScript event queue | Approximately **4×** greater local throughput | Queue-processing performance; **not** four times faster Solana transaction delivery |
| Reconnection after a local gRPC disconnect | Median receive resumption **552 ms → 103 ms** | Specific local tonic test; **not** a cross-region recovery-time guarantee |

**Methodology and test artifacts:** [PR #129](https://github.com/ValidatorsDAO/solana-stream/pull/129). These measurements are independent and must not be multiplied or combined.

## gRPC flow control

A small HTTP/2 receive window may throttle an otherwise fast streaming consumer. SDK 5.0.0 configures **16 MiB stream and connection receive windows** with adaptive sizing disabled for Rust `ShredstreamClient::connect`. Rust consumers using the upstream re-exported `GeyserGrpcClient` need to apply `tune_geyser_grpc_builder` themselves.

The TypeScript `GeyserClient` applies these defaults in the **4.1.0 source code on `main`**. As of 2026-11-18, **npm 4.0.0 is the published package and does not include this change**. Check [npm](https://www.npmjs.com/package/@validators-dao/solana-stream-sdk) for subsequent releases. Endpoint and channel options can be overridden by callers.

Receive-window tuning does not override server limits, prevent UDP packet loss, or fix an overloaded consumer. See the [ERPC flow-control explanation](https://erpc.global/en/faq/grpc/general/) for details.

## Rust UDP dependencies in v5

The Rust SDK 5.0 `udp` feature replaces `solana-ledger` with its own Agave 4.4.1-compatible shred decoder. The release recorded a reduction from **648 to 419 resolved normal dependencies** in the relevant dependency-tree comparison. This is a build/dependency improvement, not evidence that an application will be 108× faster. Tests in the repository still use `solana-ledger` as a dev-dependency and can require additional build tooling.

See the [v5 release](https://github.com/ValidatorsDAO/solana-stream/releases/tag/v5.0.0), [migration guide](migration-v5.md), and [changelog](../CHANGELOG.md).

[Back to README](../README.md)
