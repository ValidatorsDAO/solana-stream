use crate::config::Config;
use anyhow::Context;
use backoff::{future::retry, ExponentialBackoff};
use chrono::{DateTime, TimeZone, Utc};
use dotenv::dotenv;
use env_logger;
use futures::{SinkExt, StreamExt};
use log::info;
use serde_jsonc;
use solana_signature::Signature;
use solana_stream_sdk::{
    GeyserCommitmentLevel, GeyserGrpcClient, GeyserSubscribeRequest,
    GeyserSubscribeRequestFilterAccounts, GeyserSubscribeRequestFilterBlocks,
    GeyserSubscribeRequestFilterBlocksMeta, GeyserSubscribeRequestFilterEntry,
    GeyserSubscribeRequestFilterSlots, GeyserSubscribeRequestFilterTransactions,
    GeyserSubscribeUpdate, GeyserUpdateOneof,
};
use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    time::Duration,
};
use tonic::transport::ClientTlsConfig;

mod config;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().ok();
    env::set_var(
        env_logger::DEFAULT_FILTER_ENV,
        env::var_os(env_logger::DEFAULT_FILTER_ENV).unwrap_or_else(|| "info".into()),
    );
    env_logger::init();

    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "config.jsonc".to_string());
    let endpoint = env::var("GRPC_ENDPOINT").context("GRPC_ENDPOINT is missing")?;
    let x_token = env::var("X_TOKEN").ok();

    let config_content = fs::read_to_string(config_path)?;
    let config: Config = serde_jsonc::from_str(&config_content)?;

    let request = GeyserSubscribeRequest {
        commitment: config.commitment.as_ref().map(|c| commitment_from_str(c)),
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
        transactions_status: HashMap::new(),
        accounts_data_slice: vec![],
        from_slot: None,
        ping: None,
    };

    let mut messages: BTreeMap<u64, (Option<DateTime<Utc>>, Vec<(String, DateTime<Utc>)>)> =
        BTreeMap::new();
    let mut current_slot: u64 = 0;
    let mut latencies: Vec<i64> = Vec::new();

    loop {
        let endpoint = endpoint.clone();
        let x_token = x_token.clone();
        let request = request.clone();

        let mut client = retry(ExponentialBackoff::default(), move || {
            let endpoint = endpoint.clone();
            let x_token = x_token.clone();

            async move {
                let mut builder = GeyserGrpcClient::build_from_shared(endpoint.clone())
                    .context("failed to create client builder")?;

                if let Some(token) = x_token {
                    builder = builder
                        .x_token(Some(token))
                        .context("failed to set x_token")?;
                }

                if endpoint.starts_with("https://") {
                    builder = builder
                        .tls_config(ClientTlsConfig::new().with_native_roots())
                        .context("failed to configure TLS")?;
                }

                builder = builder
                    .initial_stream_window_size(Some(1_048_576))
                    .initial_connection_window_size(Some(4_194_304))
                    .http2_adaptive_window(true)
                    .tcp_nodelay(true);

                builder.connect().await.map_err(|e| {
                    log::error!("Connection failed: {:?}, retrying...", e);
                    backoff::Error::transient(anyhow::anyhow!(e))
                })
            }
        })
        .await?;

        let (mut sink, mut stream) = client.subscribe().await?;
        sink.send(request).await?;

        while let Some(message) = stream.next().await {
            match message {
                Ok(msg) => log_message(&msg, &mut messages, &mut current_slot, &mut latencies),
                Err(e) => {
                    log::error!("Stream error: {:?}, reconnecting...", e);
                    break;
                }
            }
        }

        log::info!("Stream ended, attempting to reconnect...");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn commitment_from_str(commitment: &str) -> i32 {
    match commitment {
        "Processed" => GeyserCommitmentLevel::Processed as i32,
        "Confirmed" => GeyserCommitmentLevel::Confirmed as i32,
        "Finalized" => GeyserCommitmentLevel::Finalized as i32,
        _ => GeyserCommitmentLevel::Processed as i32,
    }
}

fn log_message(
    msg: &GeyserSubscribeUpdate,
    messages: &mut BTreeMap<u64, (Option<DateTime<Utc>>, Vec<(String, DateTime<Utc>)>)>,
    current_slot: &mut u64,
    latencies: &mut Vec<i64>,
) {
    let received_time = Utc::now();

    match &msg.update_oneof {
        Some(GeyserUpdateOneof::TransactionStatus(tx)) => {
            let entry = messages.entry(tx.slot).or_default();
            let sig = Signature::try_from(tx.signature.as_slice())
                .unwrap()
                .to_string();

            if let Some(block_time) = entry.0 {
                let latency = received_time
                    .signed_duration_since(block_time)
                    .num_milliseconds()
                    .saturating_sub(500);
                latencies.push(latency);
                let avg_latency =
                    latencies.iter().copied().sum::<i64>() as f64 / latencies.len() as f64;

                info!(
                    "[Received: {}] TransactionStatus | Signature: {}\n  - Block Time: {}\n  - Adjusted Latency: {} ms\n - Avg Latency: {:.2} ms",
                    received_time,
                    sig,
                    block_time,
                    latency,
                    avg_latency
                );
            } else {
                entry.1.push((sig, received_time));
            }
        }
        Some(GeyserUpdateOneof::BlockMeta(block_meta)) => {
            let entry = messages.entry(block_meta.slot).or_default();
            entry.0 = block_meta.block_time.map(|obj| {
                Utc.timestamp_opt(obj.timestamp, 0)
                    .single()
                    .expect("valid timestamp")
            });
            *current_slot = block_meta.slot;

            if let Some(block_time) = entry.0 {
                for (sig, tx_received_time) in &entry.1 {
                    let latency = tx_received_time
                        .signed_duration_since(block_time)
                        .num_milliseconds()
                        .saturating_sub(500);
                    latencies.push(latency);
                    let avg_latency =
                        latencies.iter().copied().sum::<i64>() as f64 / latencies.len() as f64;

                    info!(
                        "[Received: {}] Transaction | Signature: {}\n  - Block Time: {}\n  - Adjusted Latency: {} ms\n - Avg Latency: {:.2} ms",
                        tx_received_time,
                        sig,
                        block_time,
                        latency,
                        avg_latency
                    );
                }
                entry.1.clear();
            }

            while let Some(slot) = messages.keys().next().cloned() {
                if slot < block_meta.slot.saturating_sub(20) {
                    messages.remove(&slot);
                } else {
                    break;
                }
            }
        }
        Some(GeyserUpdateOneof::Transaction(tx_info)) => {
            let slot = tx_info.slot;
            let sig = tx_info
                .transaction
                .as_ref()
                .and_then(|tx| tx.transaction.as_ref())
                .and_then(|inner_tx| inner_tx.signatures.first())
                .map(|sig| bs58::encode(sig).into_string())
                .unwrap_or_else(|| "Unknown".into());

            messages
                .entry(slot)
                .or_default()
                .1
                .push((sig, received_time));
        }
        _ => {}
    }
}
