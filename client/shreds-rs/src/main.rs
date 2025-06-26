use blocktime::prepare_log_message;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use solana_stream_sdk::{CommitmentLevel, ShredstreamClient};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;

mod blocktime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    env_logger::init();
    let endpoint = env::var("SHREDS_ENDPOINT")
        .unwrap_or_else(|_| "https://shreds-ams-9.erpc.global".to_string());

    let mut client = ShredstreamClient::connect(&endpoint).await?;

    // The filter is experimental
    let request = ShredstreamClient::create_entries_request_for_accounts(
        vec![],
        vec![],
        vec![],
        Some(CommitmentLevel::Processed),
    );

    let mut stream = client.subscribe_entries(request).await?;

    let transactions_by_slot = Arc::new(Mutex::new(
        HashMap::<u64, Vec<(String, DateTime<Utc>)>>::new(),
    ));
    let block_time_cache = blocktime::BlockTimeCache::new(
        &env::var("SOLANA_RPC_ENDPOINT")
            .unwrap_or("https://api.mainnet-beta.solana.com".to_string()),
    );

    let latency_handle = {
        let block_time_cache = block_time_cache.clone();
        let transactions_by_slot = transactions_by_slot.clone();
        tokio::spawn(async move {
            blocktime::latency_monitor_task(block_time_cache, transactions_by_slot).await;
        })
    };

    let stream_handle = tokio::spawn(async move {
        while let Some(slot_entry) = stream.next().await {
            match slot_entry {
                Ok(data) => {
                    let slot = data.slot;
                    prepare_log_message(slot, &transactions_by_slot).await;

                    // You can see data with deserializing like below
                    // let entries =
                    //     match bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&slot_entry.entries) {
                    //         Ok(e) => e,
                    //         Err(e) => {
                    //             println!("Deserialization failed with err: {e}");
                    //             continue;
                    //         }
                    //     };
                    // let transactions = entries
                    //     .iter()
                    //     .flat_map(|e| e.transactions.iter())
                    //     .collect::<Vec<_>>();
                    // println!("transactions: {:?}", transactions[0]);
                    // println!(
                    //     "slot {}, entries: {}, transactions: {}",
                    //     slot_entry.slot,
                    //     entries.len(),
                    //     entries.iter().map(|e| e.transactions.len()).sum::<usize>()
                    // );
                }
                Err(_) => {
                    break;
                }
            }
        }
    });

    tokio::try_join!(latency_handle, stream_handle)?;

    Ok(())
}
