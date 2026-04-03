use crate::api::build_router;
use crate::handlers::processor::process_updates;
use crate::runtime::runner::run_geyser_stream;
use crate::runtime::settings::Settings;
use crate::runtime::subscription::build_subscribe_request;
use crate::state::AppState;
use crate::utils::blocktime::{create_transactions_by_slot, latency_monitor_task, BlockTimeCache};
use crate::utils::config::Config;
use crate::wallet::load_or_create_wallet;
use dotenv;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_stream_sdk::GeyserSubscribeUpdate;
use std::sync::atomic::AtomicU64;
use std::{fs, net::SocketAddr, sync::Arc};
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
    dotenv::dotenv().ok();
    env_logger::init();

    let settings = Settings::from_env()?;
    let config_content = fs::read_to_string(&settings.config_path)?;
    let config: Config = serde_jsonc::from_str(&config_content)?;
    let request = build_subscribe_request(&config);

    // Wallet
    let keypair = load_or_create_wallet()?;
    let rpc_client = Arc::new(RpcClient::new(settings.rpc_endpoint.clone()));
    let send_rpc_client = Arc::new(RpcClient::new(settings.send_rpc_endpoint.clone()));
    log::info!("RPC read: {}", settings.rpc_endpoint);
    log::info!("RPC send: {}", settings.send_rpc_endpoint);

    // Optional Redis client
    let redis_client = std::env::var("REDIS_URL").ok().and_then(|url| {
        match redis::Client::open(url.as_str()) {
            Ok(client) => {
                log::info!("Redis client initialized: {}", url);
                Some(client)
            }
            Err(e) => {
                log::warn!("Failed to create Redis client ({}): {:?} — continuing without Redis", url, e);
                None
            }
        }
    });

    // Shared state
    let app_state = Arc::new(RwLock::new({
        let mut s = AppState::new(settings.webhook_url.clone());
        s.wallet = Some(keypair);
        s.redis_client = redis_client;
        s
    }));

    // Geyser infrastructure
    let transactions_by_slot = create_transactions_by_slot();
    let block_time_cache = BlockTimeCache::new(&settings.rpc_endpoint);
    let tracked_slot = Arc::new(AtomicU64::new(0));
    let (updates_tx, updates_rx) = mpsc::channel::<GeyserSubscribeUpdate>(UPDATE_CHANNEL_CAPACITY);

    // Restore positions from wallet holdings on startup
    {
        let state = app_state.clone();
        let rpc = rpc_client.clone();
        let send_rpc = send_rpc_client.clone();
        engine::restore_positions_from_wallet(state, rpc, send_rpc).await;
    }

    // Latency monitor disabled to avoid RPC rate-limit exhaustion (getBlockTime
    // fires per-slot, consuming the entire API quota and starving sell checks).
    // Re-enable once a dedicated RPC endpoint or rate-limited wrapper is added.
    let latency_handle = {
        let _block_time_cache = block_time_cache.clone();
        let _transactions_by_slot = transactions_by_slot.clone();
        tokio::spawn(async move {
            // latency_monitor_task(_block_time_cache, _transactions_by_slot).await;
            futures::future::pending::<()>().await;
        })
    };

    let processor_handle = {
        let transactions_by_slot = transactions_by_slot.clone();
        let state = app_state.clone();
        let rpc = rpc_client.clone();
        let send_rpc = send_rpc_client.clone();
        tokio::spawn(async move {
            process_updates(updates_rx, transactions_by_slot, state, rpc, send_rpc).await;
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
