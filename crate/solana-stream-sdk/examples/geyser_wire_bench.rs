//! Measure a Geyser subscription: updates and decoded protobuf bytes over a fixed time,
//! with or without response compression. Run two at once with different `COMPRESSION`
//! to compare them on the same traffic, and read the bytes on the wire from the OS
//! (`nettop -P -L 1 -J bytes_in -p <pid>` on macOS) and CPU time from `/usr/bin/time`.
//!
//! ```sh
//! GEYSER_ENDPOINT=http://grpc-ams1.erpc.global COMPRESSION=zstd SECONDS=30 \
//!   cargo run --release --example geyser_wire_bench
//! ```
//!
//! `COMPRESSION` is `none` (default), `zstd` or `gzip`. `PROGRAM` is the account the
//! transaction filter includes (default: the pump.fun program the sample clients watch).
//! The output also gives what each message would weigh compressed one by one, as gRPC
//! compresses them (zstd level 3, gzip level 6), and the CPU time that took.
use futures::StreamExt;
use prost::Message;
use solana_stream_sdk::{
    GeyserGrpcClient, GeyserSubscribeRequest, GeyserSubscribeRequestFilterTransactions,
    GeyserUpdateOneof,
};
use std::time::{Duration, Instant};
use tonic::codec::CompressionEncoding;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("GEYSER_ENDPOINT")?;
    let compression = std::env::var("COMPRESSION").unwrap_or_else(|_| "none".into());
    let seconds: u64 = std::env::var("SECONDS").map_or(Ok(30), |s| s.parse())?;
    let program = std::env::var("PROGRAM")
        .unwrap_or_else(|_| "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string());

    let mut builder = GeyserGrpcClient::build_from_shared(endpoint)?;
    builder = match compression.as_str() {
        "zstd" => builder.accept_compressed(CompressionEncoding::Zstd),
        "gzip" => builder.accept_compressed(CompressionEncoding::Gzip),
        _ => builder,
    };
    let mut client = builder.connect().await?;

    let request = GeyserSubscribeRequest {
        transactions: [(
            "program".to_string(),
            GeyserSubscribeRequestFilterTransactions {
                vote: Some(false),
                failed: Some(false),
                account_include: vec![program],
                ..Default::default()
            },
        )]
        .into(),
        ..Default::default()
    };
    let (_sink, mut stream) = client.subscribe_with_request(Some(request)).await?;

    let started = Instant::now();
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let (mut updates, mut transactions, mut decoded_bytes) = (0u64, 0u64, 0u64);
    let (mut zstd_bytes, mut gzip_bytes) = (0u64, 0u64);
    let (mut zstd_time, mut gzip_time) = (Duration::ZERO, Duration::ZERO);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            message = stream.next() => match message {
                Some(Ok(update)) => {
                    updates += 1;
                    let encoded = update.encode_to_vec();
                    decoded_bytes += encoded.len() as u64;
                    let t = Instant::now();
                    zstd_bytes += zstd::bulk::compress(&encoded, 3)?.len() as u64;
                    zstd_time += t.elapsed();
                    let t = Instant::now();
                    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
                    std::io::Write::write_all(&mut gz, &encoded)?;
                    gzip_bytes += gz.finish()?.len() as u64;
                    gzip_time += t.elapsed();
                    if matches!(update.update_oneof, Some(GeyserUpdateOneof::Transaction(_))) {
                        transactions += 1;
                    }
                }
                Some(Err(status)) => return Err(status.into()),
                None => break,
            },
        }
    }
    println!(
        "{{\"compression\":\"{compression}\",\"seconds\":{:.1},\"updates\":{updates},\"transactions\":{transactions},\"decoded_bytes\":{decoded_bytes},\"per_message_zstd3_bytes\":{zstd_bytes},\"zstd3_cpu_ms\":{},\"per_message_gzip6_bytes\":{gzip_bytes},\"gzip6_cpu_ms\":{}}}",
        started.elapsed().as_secs_f64(),
        zstd_time.as_millis(),
        gzip_time.as_millis()
    );
    // Stay alive (HOLD seconds, default 15) so the OS counters of this process can be read.
    let hold: u64 = std::env::var("HOLD").map_or(Ok(15), |s| s.parse())?;
    tokio::time::sleep(Duration::from_secs(hold)).await;
    Ok(())
}
