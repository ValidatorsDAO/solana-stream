use crate::config::Config;
use dotenv::dotenv;
use futures::{SinkExt, StreamExt};
use serde_jsonc;
use solana_stream_sdk::{
    GeyserCommitmentLevel, GeyserGrpcClient, GeyserSubscribeRequest,
    GeyserSubscribeRequestFilterAccounts, GeyserSubscribeRequestFilterBlocks,
    GeyserSubscribeRequestFilterBlocksMeta, GeyserSubscribeRequestFilterEntry,
    GeyserSubscribeRequestFilterSlots, GeyserSubscribeRequestFilterTransactions, SolanaStreamError,
};
use std::{collections::HashMap, env, fs};
use tonic::transport::ClientTlsConfig;

mod config;

#[tokio::main]
async fn main() -> Result<(), SolanaStreamError> {
    dotenv().ok();
    env_logger::init();

    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "config.jsonc".to_string());
    let endpoint = env::var("GRPC_ENDPOINT").expect("GRPC_ENDPOINT is missing");
    let x_token = env::var("X_TOKEN").ok();

    let config_content = fs::read_to_string(config_path)?;
    let config: Config = serde_jsonc::from_str(&config_content)?;

    let mut builder = GeyserGrpcClient::build_from_shared(endpoint.clone())?;

    if let Some(token) = x_token {
        builder = builder.x_token(Some(token))?;
    }

    if endpoint.starts_with("https://") {
        builder = builder.tls_config(ClientTlsConfig::new().with_native_roots())?;
    }

    let mut client = builder.connect().await?;

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

    let (mut sink, mut stream) = client.subscribe().await?;
    sink.send(request).await?;

    while let Some(message) = stream.next().await {
        log::info!("Received: {:?}", message?);
    }

    Ok(())
}

fn commitment_from_str(commitment: &str) -> i32 {
    match commitment {
        "Processed" => GeyserCommitmentLevel::Processed as i32,
        "Confirmed" => GeyserCommitmentLevel::Confirmed as i32,
        "Finalized" => GeyserCommitmentLevel::Finalized as i32,
        _ => GeyserCommitmentLevel::Processed as i32,
    }
}
