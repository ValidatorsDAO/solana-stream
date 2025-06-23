use crate::{
    blocktime::{latency_monitor_task, prepare_log_message, BlockTimeCache},
    config::{commitment_from_str, Config},
};
use anyhow::Context;
use backoff::{future::retry, ExponentialBackoff};
use dotenv::dotenv;
use env_logger;
use futures::{SinkExt, StreamExt};
use log::{error, info};
use serde_jsonc;
use solana_stream_sdk::{
    GeyserGrpcClient, GeyserSubscribeRequest, GeyserSubscribeRequestFilterAccounts,
    GeyserSubscribeRequestFilterBlocks, GeyserSubscribeRequestFilterBlocksMeta,
    GeyserSubscribeRequestFilterEntry, GeyserSubscribeRequestFilterSlots,
    GeyserSubscribeRequestFilterTransactions,
};
use std::{collections::BTreeMap, env, fs, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tonic::transport::ClientTlsConfig;

mod blocktime;
mod config;

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

    let latency_handle = {
        let block_time_cache = block_time_cache.clone();
        let transactions_by_slot = transactions_by_slot.clone();
        tokio::spawn(async move {
            latency_monitor_task(block_time_cache, transactions_by_slot).await;
        })
    };

    let geyser_handle = {
        let transactions_by_slot = transactions_by_slot.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = async {
                    let mut client = retry(ExponentialBackoff::default(), || {
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
                    })
                    .await?;

                    let (mut sink, mut stream) = client.subscribe().await?;
                    sink.send(request.clone()).await?;

                    while let Some(message) = stream.next().await {
                        match message {
                            Ok(msg) => prepare_log_message(&msg, &transactions_by_slot).await,
                            Err(e) => {
                                error!("Stream error: {:?}, reconnecting...", e);
                                break;
                            }
                        }
                    }

                    info!("Stream ended, reconnecting...");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    Ok::<(), anyhow::Error>(())
                }
                .await
                {
                    error!("Failed to handle stream: {:?}", e);
                }
            }
        })
    };

    tokio::try_join!(latency_handle, geyser_handle)?;

    Ok(())
}
