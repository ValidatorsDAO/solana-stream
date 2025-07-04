use bs58;
use chrono::{DateTime, TimeZone, Utc};
use futures::future::join_all;
use log::info;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_stream_sdk::{GeyserSubscribeUpdate, GeyserUpdateOneof};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct BlockTimeCache {
    rpc_client: Arc<RpcClient>,
    cache: Arc<Mutex<HashMap<u64, i64>>>,
    fetching: Arc<Mutex<HashSet<u64>>>,
}

impl BlockTimeCache {
    pub fn new(rpc_endpoint: &str) -> Self {
        Self {
            rpc_client: Arc::new(RpcClient::new(rpc_endpoint.to_string())),
            cache: Arc::new(Mutex::new(HashMap::new())),
            fetching: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn get_block_time(&self, slot: u64) -> Option<i64> {
        {
            let cache = self.cache.lock().await;
            if let Some(time) = cache.get(&slot) {
                return Some(*time);
            }
        }

        {
            let mut fetching = self.fetching.lock().await;
            if fetching.contains(&slot) {
                return None;
            }
            fetching.insert(slot);
        }

        let block_time_result = self.rpc_client.get_block_time(slot).await;

        let block_time = match block_time_result {
            Ok(time) => Some(time),
            Err(err) => {
                if format!("{:?}", err).contains("Block not available") {
                    None
                } else {
                    log::error!("Error fetching block time for slot {}: {:?}", slot, err);
                    None
                }
            }
        };

        {
            let mut cache = self.cache.lock().await;
            if let Some(time) = block_time {
                cache.insert(slot, time);
                if cache.len() > 20 {
                    if let Some(oldest_slot) = cache.keys().next().cloned() {
                        cache.remove(&oldest_slot);
                    }
                }
            }
        }

        let mut fetching = self.fetching.lock().await;
        fetching.remove(&slot);

        block_time
    }
}

pub async fn prepare_log_message(
    msg: &GeyserSubscribeUpdate,
    transactions_by_slot: &Arc<Mutex<BTreeMap<u64, Vec<(String, DateTime<Utc>)>>>>,
) {
    match &msg.update_oneof {
        Some(GeyserUpdateOneof::Transaction(tx_info)) => {
            let received_time = Utc::now();
            let slot = tx_info.slot;
            if let Some(tx) = &tx_info.transaction {
                if let Some(inner_tx) = &tx.transaction {
                    if let Some(sig) = inner_tx.signatures.first() {
                        let sig_str = bs58::encode(sig).into_string();
                        transactions_by_slot
                            .lock()
                            .await
                            .entry(slot)
                            .or_default()
                            .push((sig_str, received_time));
                    }
                }
            }
        }
        _ => {}
    }
}

pub async fn latency_monitor_task(
    block_time_cache: BlockTimeCache,
    transactions_by_slot: Arc<Mutex<BTreeMap<u64, Vec<(String, DateTime<Utc>)>>>>,
) {
    const MAX_LATENCIES: usize = 420;
    let mut latency_buffer = Vec::new();

    loop {
        tokio::time::sleep(std::time::Duration::from_millis(420)).await;

        let slots: Vec<u64> = transactions_by_slot.lock().await.keys().cloned().collect();

        let block_time_futures = slots.iter().map(|&slot| {
            let value = block_time_cache.clone();
            async move {
                let block_time = value.get_block_time(slot).await;
                (slot, block_time)
            }
        });

        let slot_block_times = join_all(block_time_futures).await;

        for (slot, block_time_unix_opt) in slot_block_times {
            if let Some(block_time_unix) = block_time_unix_opt {
                let block_time = Utc.timestamp_opt(block_time_unix, 0).unwrap();
                let txs = transactions_by_slot
                    .lock()
                    .await
                    .remove(&slot)
                    .unwrap_or_default();

                for (sig, recv_time) in txs {
                    let latency = recv_time
                        .signed_duration_since(block_time)
                        .num_milliseconds()
                        .saturating_sub(500);
                    latency_buffer.push(latency);
                    if latency_buffer.len() > MAX_LATENCIES {
                        latency_buffer.remove(0);
                    }

                    let avg_latency =
                        latency_buffer.iter().sum::<i64>() as f64 / latency_buffer.len() as f64;

                    info!(
                        "Slot: {}\nTx: {}\n⏰ BlockTime: {}\n📥 ReceivedAt: {}\n🚀 Adjusted Latency: {} ms\n📊 Average Latency (latest {}): {:.2} ms\n",
                        slot,
                        sig,
                        block_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        recv_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        latency,
                        latency_buffer.len(),
                        avg_latency
                    );
                }
            }
        }
    }
}
