use crate::handlers::processor::process_updates;
use crate::runtime::runner::run_geyser_stream;
use crate::runtime::settings::Settings;
use crate::runtime::subscription::build_subscribe_request;
use crate::utils::blocktime::{create_transactions_by_slot, latency_monitor_task, BlockTimeCache};
use crate::utils::config::Config;
use dotenv::dotenv;
use env_logger;
use serde_jsonc;
use solana_stream_sdk::GeyserReconnectEvent;
use std::sync::atomic::AtomicU64;
use std::{fs, sync::Arc};
use tokio::sync::mpsc;

mod handlers;
mod runtime;
mod utils;

const UPDATE_CHANNEL_CAPACITY: usize = 10_000;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().ok();
    env_logger::init();

    let settings = Settings::from_env()?;
    let config_content = fs::read_to_string(&settings.config_path)?;
    let config: Config = serde_jsonc::from_str(&config_content)?;
    let request = build_subscribe_request(&config);

    let transactions_by_slot = create_transactions_by_slot();
    let block_time_cache = BlockTimeCache::new(&settings.rpc_endpoint);
    let tracked_slot = Arc::new(AtomicU64::new(0));
    let (events_tx, events_rx) = mpsc::channel::<GeyserReconnectEvent>(UPDATE_CHANNEL_CAPACITY);

    let latency_handle = {
        let block_time_cache = block_time_cache.clone();
        let transactions_by_slot = transactions_by_slot.clone();
        tokio::spawn(async move {
            latency_monitor_task(block_time_cache, transactions_by_slot).await;
        })
    };

    let processor_handle = {
        // Add your detection/trade logic in src/handlers/processor.rs.
        let transactions_by_slot = transactions_by_slot.clone();
        tokio::spawn(async move {
            process_updates(events_rx, transactions_by_slot).await;
        })
    };

    let geyser_handle = {
        let tracked_slot = tracked_slot.clone();
        let events_tx = events_tx.clone();
        tokio::spawn(run_geyser_stream(
            settings.grpc_endpoint.clone(),
            settings.x_token.clone(),
            request,
            tracked_slot,
            events_tx,
        ))
    };

    // The Geyser runner returns only on an error that reconnecting cannot fix; exit
    // with it (status 1) instead of idling without a stream.
    tokio::select! {
        result = geyser_handle => result?,
        result = latency_handle => result.map_err(anyhow::Error::from),
        result = processor_handle => result.map_err(anyhow::Error::from),
    }
}
