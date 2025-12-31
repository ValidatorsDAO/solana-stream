use crate::{
    blocktime::{latency_monitor_task, prepare_log_message, BlockTimeCache},
    config::{commitment_from_str, Config},
};
use anyhow::Context;
use backoff::backoff::Backoff;
use backoff::{future::retry, ExponentialBackoff};
use dotenv::dotenv;
use env_logger;
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use serde_jsonc;
use solana_stream_sdk::{
    GeyserGrpcClient, GeyserSubscribeRequest, GeyserSubscribeRequestFilterAccounts,
    GeyserSubscribeRequestFilterBlocks, GeyserSubscribeRequestFilterBlocksMeta,
    GeyserSubscribeRequestFilterEntry, GeyserSubscribeRequestFilterSlots,
    GeyserSubscribeRequestFilterTransactions, GeyserSubscribeUpdate, GeyserUpdateOneof,
};
use solana_stream_sdk::yellowstone_grpc_proto::geyser::SubscribeRequestPing;
use std::{collections::BTreeMap, env, fs, sync::Arc, time::Duration};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, Mutex};
use tonic::transport::ClientTlsConfig;

mod blocktime;
mod config;

const UPDATE_CHANNEL_CAPACITY: usize = 10_000;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().ok();
    env_logger::init();

    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "config.jsonc".to_string());
    let grpc_endpoint = env::var("GRPC_ENDPOINT").context("GRPC_ENDPOINT is missing")?;
    let x_token = env::var("X_TOKEN").ok();
    let rpc_endpoint = env::var("SOLANA_RPC_ENDPOINT")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    let config_content = fs::read_to_string(config_path)?;
    let config: Config = serde_jsonc::from_str(&config_content)?;

    let request = GeyserSubscribeRequest {
        commitment: config.commitment.as_deref().map(commitment_from_str),
        transactions: config
            .transactions
            .iter()
            .map(|(k, v)| (k.clone(), GeyserSubscribeRequestFilterTransactions::from(v)))
            .collect(),
        accounts: config
            .accounts
            .iter()
            .map(|(k, v)| (k.clone(), GeyserSubscribeRequestFilterAccounts::from(v)))
            .collect(),
        slots: config
            .slots
            .iter()
            .map(|(k, v)| (k.clone(), GeyserSubscribeRequestFilterSlots::from(v)))
            .collect(),
        blocks: config
            .blocks
            .iter()
            .map(|(k, v)| (k.clone(), GeyserSubscribeRequestFilterBlocks::from(v)))
            .collect(),
        blocks_meta: config
            .blocks_meta
            .iter()
            .map(|(k, v)| (k.clone(), GeyserSubscribeRequestFilterBlocksMeta::from(v)))
            .collect(),
        entry: config
            .entry
            .iter()
            .map(|(k, v)| (k.clone(), GeyserSubscribeRequestFilterEntry::from(v)))
            .collect(),
        transactions_status: Default::default(),
        accounts_data_slice: vec![],
        from_slot: None,
        ping: None,
    };

    let transactions_by_slot = Arc::new(Mutex::new(BTreeMap::new()));
    let block_time_cache = BlockTimeCache::new(&rpc_endpoint);
    let tracked_slot = Arc::new(AtomicU64::new(0));
    let (updates_tx, updates_rx) = mpsc::channel::<GeyserSubscribeUpdate>(UPDATE_CHANNEL_CAPACITY);

    let latency_handle = {
        let block_time_cache = block_time_cache.clone();
        let transactions_by_slot = transactions_by_slot.clone();
        tokio::spawn(async move {
            latency_monitor_task(block_time_cache, transactions_by_slot).await;
        })
    };

    let processor_handle = {
        let transactions_by_slot = transactions_by_slot.clone();
        tokio::spawn(async move {
            process_updates(updates_rx, transactions_by_slot).await;
        })
    };

    let geyser_handle = {
        let tracked_slot = tracked_slot.clone();
        let updates_tx = updates_tx.clone();
        tokio::spawn(async move {
            let mut reconnect_backoff = ExponentialBackoff {
                max_elapsed_time: None,
                ..ExponentialBackoff::default()
            };

            loop {
                let stream_result = async {
                    let mut client = retry(
                        ExponentialBackoff {
                            max_elapsed_time: None,
                            ..ExponentialBackoff::default()
                        },
                        || {
                            let grpc_endpoint = grpc_endpoint.clone();
                            let x_token = x_token.clone();
                            async move {
                                let mut builder =
                                    GeyserGrpcClient::build_from_shared(grpc_endpoint.clone())?;
                                if let Some(token) = x_token {
                                    builder = builder.x_token(Some(token))?;
                                }
                                if grpc_endpoint.starts_with("https://") {
                                    builder = builder
                                        .tls_config(ClientTlsConfig::new().with_native_roots())?;
                                }
                                builder.connect().await.map_err(backoff::Error::transient)
                            }
                        },
                    )
                    .await?;

                    let mut subscribe_request = request.clone();
                    let resume_from_slot = tracked_slot.load(Ordering::Relaxed);
                    subscribe_request.from_slot = if resume_from_slot > 0 {
                        Some(resume_from_slot.saturating_sub(1))
                    } else {
                        None
                    };

                    let (mut sink, mut stream) = client.subscribe().await?;
                    sink.send(subscribe_request).await?;

                    let mut saw_non_heartbeat = false;

                    while let Some(message) = stream.next().await {
                        match message {
                            Ok(update) => {
                                if matches!(update.update_oneof, Some(GeyserUpdateOneof::Ping(_))) {
                                    if let Err(err) = sink
                                        .send(GeyserSubscribeRequest {
                                            ping: Some(SubscribeRequestPing { id: 1 }),
                                            ..Default::default()
                                        })
                                        .await
                                    {
                                        warn!("Failed to respond to ping: {:?}", err);
                                        break;
                                    }
                                    continue;
                                }

                                if matches!(update.update_oneof, Some(GeyserUpdateOneof::Pong(_))) {
                                    continue;
                                }

                                update_tracked_slot(&update, &tracked_slot);
                                saw_non_heartbeat = true;

                                if let Err(err) = updates_tx.try_send(update) {
                                    warn!("Dropping update due to full channel: {:?}", err);
                                }
                            }
                            Err(e) => {
                                return Err(anyhow::Error::from(e));
                            }
                        }
                    }

                    info!("Stream ended, reconnecting...");
                    Ok::<bool, anyhow::Error>(saw_non_heartbeat)
                }
                .await;

                if let Ok(true) = stream_result {
                    reconnect_backoff.reset();
                }

                if let Err(e) = stream_result {
                    error!("Failed to handle stream: {:?}", e);
                }

                if let Some(delay) = reconnect_backoff.next_backoff() {
                    tokio::time::sleep(delay).await;
                } else {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        })
    };

    tokio::try_join!(latency_handle, processor_handle, geyser_handle)?;

    Ok(())
}

async fn process_updates(
    mut updates_rx: mpsc::Receiver<GeyserSubscribeUpdate>,
    transactions_by_slot: Arc<Mutex<BTreeMap<u64, Vec<(String, chrono::DateTime<chrono::Utc>)>>>>,
) {
    while let Some(update) = updates_rx.recv().await {
        prepare_log_message(&update, &transactions_by_slot).await;
    }
}

fn update_tracked_slot(update: &GeyserSubscribeUpdate, tracked_slot: &AtomicU64) {
    let maybe_slot = match &update.update_oneof {
        Some(GeyserUpdateOneof::Transaction(tx)) => Some(tx.slot),
        Some(GeyserUpdateOneof::Slot(slot)) => Some(slot.slot),
        Some(GeyserUpdateOneof::Block(block)) => Some(block.slot),
        Some(GeyserUpdateOneof::BlockMeta(block_meta)) => Some(block_meta.slot),
        Some(GeyserUpdateOneof::Account(account)) => Some(account.slot),
        Some(GeyserUpdateOneof::Entry(entry)) => Some(entry.slot),
        _ => None,
    };

    if let Some(slot) = maybe_slot {
        tracked_slot.fetch_max(slot, Ordering::Relaxed);
    }
}
