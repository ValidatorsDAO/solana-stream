use crate::api::build_router;
use crate::handlers::processor::process_updates;
use crate::runtime::runner::run_geyser_stream;
use crate::runtime::settings::Settings;
use crate::runtime::subscription::build_subscribe_request;
use crate::state::AppState;
use crate::utils::blocktime::{create_transactions_by_slot, latency_monitor_task, BlockTimeCache};
use crate::utils::config::Config;
use crate::wallet::load_or_create_wallet;
use dotenv::dotenv;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_stream_sdk::GeyserSubscribeUpdate;
use std::{fs, net::SocketAddr, sync::Arc};
use std::sync::atomic::AtomicU64;
use tokio::sync::{mpsc, RwLock};

mod api;
mod engine;
mod handlers;
mod runtime;
mod state;
mod utils;
mod wallet;
mod webhook;

const UPDATE_CHANNEL_CAPACITY: usize = 10_000;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().ok();
    env_logger::init();

    let settings = Settings::from_env()?;
    let config_content = fs::read_to_string(&settings.config_path)?;
    let config: Config = serde_jsonc::from_str(&config_content)?;
    let request = build_subscribe_request(&config);

    // Wallet
    let keypair = load_or_create_wallet()?;
    let rpc_client = Arc::new(RpcClient::new(settings.rpc_endpoint.clone()));

    // Shared state
    let app_state = Arc::new(RwLock::new({
        let mut s = AppState::new(settings.webhook_url.clone());
        s.wallet = Some(keypair);
        s
    }));

    // Geyser infrastructure
    let transactions_by_slot = create_transactions_by_slot();
    let block_time_cache = BlockTimeCache::new(&settings.rpc_endpoint);
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
        let state = app_state.clone();
        let rpc = rpc_client.clone();
        tokio::spawn(async move {
            process_updates(updates_rx, transactions_by_slot, state, rpc).await;
        })
    };

    let geyser_handle = {
        let tracked_slot = tracked_slot.clone();
        let updates_tx = updates_tx.clone();
        tokio::spawn(run_geyser_stream(
            settings.grpc_endpoint.clone(),
            settings.x_token.clone(),
            request,
            tracked_slot,
            updates_tx,
        ))
    };

    // Axum API server
    let api_handle = {
        let state = app_state.clone();
        let rpc = rpc_client.clone();
        let port = settings.api_port;
        let api_token = settings.api_token.clone();
        tokio::spawn(async move {
            let router = build_router(state, rpc, api_token);
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            log::info!("API server listening on http://{}", addr);
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, router).await.unwrap();
        })
    };

    tokio::try_join!(latency_handle, processor_handle, geyser_handle, api_handle)?;

    Ok(())
}
