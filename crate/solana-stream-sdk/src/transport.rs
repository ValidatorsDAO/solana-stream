//! Throughput-oriented defaults for Solana gRPC consumers.
//!
//! HTTP/2 has separate stream-level and connection-level receive windows. A
//! small value on either level caps throughput at roughly window / RTT.
//! The fixed 16 MiB profile is based on ERPC's published 16 KiB vs 16 MiB
//! comparison. It is a transport default, not a substitute for a fast reader,
//! an adequate host NIC or a bounded application queue.

use tonic::transport::Endpoint;
use yellowstone_grpc_client::GeyserGrpcBuilder;

/// Default window for BOTH HTTP/2 stream and connection receive flow control.
pub const HIGH_THROUGHPUT_GRPC_WINDOW_BYTES: u32 = 16 * 1024 * 1024;

/// Tune an existing Yellowstone Geyser builder. Further builder calls can override this.
/// This does not alter the upstream `GeyserGrpcClient` re-export's own defaults.
pub fn tune_geyser_grpc_builder(builder: GeyserGrpcBuilder) -> GeyserGrpcBuilder {
    builder
        .initial_stream_window_size(HIGH_THROUGHPUT_GRPC_WINDOW_BYTES)
        .initial_connection_window_size(HIGH_THROUGHPUT_GRPC_WINDOW_BYTES)
        .http2_adaptive_window(false)
}

/// Tune a tonic endpoint used by Shredstream clients. Further calls can override.
pub fn tune_grpc_endpoint(endpoint: Endpoint) -> Endpoint {
    endpoint
        .initial_stream_window_size(HIGH_THROUGHPUT_GRPC_WINDOW_BYTES)
        .initial_connection_window_size(HIGH_THROUGHPUT_GRPC_WINDOW_BYTES)
        .http2_adaptive_window(false)
}
