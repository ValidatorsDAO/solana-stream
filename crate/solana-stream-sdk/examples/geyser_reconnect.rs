//! Stream Geyser updates with `subscribe_with_reconnect`, the recommended path in 4.0.
//!
//! ```sh
//! GEYSER_ENDPOINT=http://grpc-ams1.erpc.global cargo run --example geyser_reconnect
//! ```
use futures::StreamExt;
use solana_stream_sdk::{
    tune_geyser_grpc_builder, GeyserBackoff, GeyserGrpcClient, GeyserReconnectConfig,
    GeyserReconnectEvent, GeyserReconnectPolicy, GeyserSubscribeRequest,
    GeyserSubscribeRequestFilterSlots, GeyserUpdateOneof,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("GEYSER_ENDPOINT")
        .unwrap_or_else(|_| "http://grpc-ams1.erpc.global".to_string());

    let mut client = tune_geyser_grpc_builder(GeyserGrpcClient::build_from_shared(endpoint)?)
        // Up to 6 reconnects in a row (100 ms, 200 ms, … 3.2 s), replaying from the last
        // confirmed slot. A connection that delivers a block refills the budget.
        .set_reconnect_config(
            GeyserReconnectConfig::default()
                .with_backoff(GeyserBackoff::new(Duration::from_millis(100), 2.0, 6))
                .with_policy(GeyserReconnectPolicy::Replay { slots_behind: 0 }),
        )
        .connect()
        .await?;

    let request = GeyserSubscribeRequest {
        slots: [(
            "slots".to_string(),
            GeyserSubscribeRequestFilterSlots::default(),
        )]
        .into(),
        ..Default::default()
    };
    let (_sink, mut stream) = client.subscribe_with_reconnect(Some(request)).await?;

    while let Some(event) = stream.next().await {
        // An error here means the retry budget is spent; subscribe again or stop.
        match event? {
            GeyserReconnectEvent::Update(update) => {
                if let Some(GeyserUpdateOneof::Slot(slot)) = update.update_oneof {
                    println!("slot {} {:?}", slot.slot, slot.status());
                }
            }
            GeyserReconnectEvent::Disconnected(disconnected) => {
                // Drop what you derived from these banks: you hold only part of them. The
                // replay from `replay_from_slot` resends them whole, along with banks you
                // already hold whole, so handle duplicates.
                println!(
                    "reconnecting from {:?}; discard {:?}",
                    disconnected.replay_from_slot, disconnected.partial_banks
                );
            }
        }
    }
    Ok(())
}
