use crate::state::{AppState, PositionStatus, TradeConfig, PUMPSWAP_AMM};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

pub type SharedState = Arc<RwLock<AppState>>;

pub fn build_router(state: SharedState, rpc_client: Arc<RpcClient>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/config", get(get_config))
        .route("/api/config", put(put_config))
        .route("/api/trade/start", post(post_trade_start))
        .route("/api/trade/stop", post(post_trade_stop))
        .route("/api/trade/status", get(get_trade_status))
        .route("/api/logs", get(get_logs))
        .route("/api/watch-address", put(put_watch_address))
        .route("/api/wallet", get(get_wallet))
        .layer(cors)
        .with_state((state, rpc_client))
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn get_config(
    State((state, _)): State<(SharedState, Arc<RpcClient>)>,
) -> impl IntoResponse {
    let s = state.read().await;
    Json(s.config.clone())
}

#[derive(Debug, Deserialize)]
pub struct PartialTradeConfig {
    pub buy_amount_lamports: Option<u64>,
    pub sell_multiplier: Option<f64>,
    pub slippage_bps: Option<u64>,
    pub max_positions: Option<usize>,
}

async fn put_config(
    State((state, _)): State<(SharedState, Arc<RpcClient>)>,
    Json(body): Json<PartialTradeConfig>,
) -> impl IntoResponse {
    let mut s = state.write().await;
    if let Some(v) = body.buy_amount_lamports {
        s.config.buy_amount_lamports = v;
    }
    if let Some(v) = body.sell_multiplier {
        s.config.sell_multiplier = v;
    }
    if let Some(v) = body.slippage_bps {
        s.config.slippage_bps = v;
    }
    if let Some(v) = body.max_positions {
        s.config.max_positions = v;
    }
    Json(s.config.clone())
}

#[derive(Serialize)]
struct StartResponse {
    started: bool,
    message: String,
    wallet_pubkey: Option<String>,
    balance_sol: Option<f64>,
}

async fn post_trade_start(
    State((state, rpc_client)): State<(SharedState, Arc<RpcClient>)>,
) -> impl IntoResponse {
    let (has_wallet, pubkey, buy_amount_lamports) = {
        let s = state.read().await;
        let pubkey = s.wallet.as_ref().map(|kp| kp.pubkey());
        (pubkey.is_some(), pubkey, s.config.buy_amount_lamports)
    };

    if !has_wallet {
        return (
            StatusCode::BAD_REQUEST,
            Json(StartResponse {
                started: false,
                message: "No wallet loaded. Place wallet.json in the working directory.".to_string(),
                wallet_pubkey: None,
                balance_sol: None,
            }),
        );
    }

    let pubkey = pubkey.unwrap();
    let balance = rpc_client.get_balance(&pubkey).await.unwrap_or(0);
    let balance_sol = balance as f64 / 1e9;

    if balance < buy_amount_lamports + 10_000_000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(StartResponse {
                started: false,
                message: format!(
                    "Insufficient funds. Send at least {:.4} SOL to {}",
                    (buy_amount_lamports + 10_000_000) as f64 / 1e9,
                    pubkey
                ),
                wallet_pubkey: Some(pubkey.to_string()),
                balance_sol: Some(balance_sol),
            }),
        );
    }

    let mut s = state.write().await;
    s.running = true;
    (
        StatusCode::OK,
        Json(StartResponse {
            started: true,
            message: "Trading started.".to_string(),
            wallet_pubkey: Some(pubkey.to_string()),
            balance_sol: Some(balance_sol),
        }),
    )
}

#[derive(Serialize)]
struct StopResponse {
    stopped: bool,
}

async fn post_trade_stop(
    State((state, _)): State<(SharedState, Arc<RpcClient>)>,
) -> impl IntoResponse {
    let mut s = state.write().await;
    s.running = false;
    Json(StopResponse { stopped: true })
}

#[derive(Serialize)]
struct TradeStatus {
    running: bool,
    active_positions: usize,
    wallet_balance: Option<f64>,
}

async fn get_trade_status(
    State((state, rpc_client)): State<(SharedState, Arc<RpcClient>)>,
) -> impl IntoResponse {
    let (running, active_positions, pubkey) = {
        let s = state.read().await;
        let active = s
            .positions
            .iter()
            .filter(|p| p.status == PositionStatus::Active || p.status == PositionStatus::Selling)
            .count();
        let pubkey = s.wallet.as_ref().map(|kp| kp.pubkey());
        (s.running, active, pubkey)
    };

    let wallet_balance = if let Some(pk) = pubkey {
        rpc_client
            .get_balance(&pk)
            .await
            .ok()
            .map(|b| b as f64 / 1e9)
    } else {
        None
    };

    Json(TradeStatus {
        running,
        active_positions,
        wallet_balance,
    })
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

async fn get_logs(
    State((state, _)): State<(SharedState, Arc<RpcClient>)>,
    Query(query): Query<LogsQuery>,
) -> impl IntoResponse {
    let s = state.read().await;
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);
    let logs: Vec<_> = s.trade_logs.iter().rev().skip(offset).take(limit).cloned().collect();
    Json(logs)
}

#[derive(Deserialize)]
pub struct WatchAddressBody {
    pub address: String,
}

#[derive(Serialize)]
struct WatchAddressResponse {
    watch_address: String,
}

async fn put_watch_address(
    State((state, _)): State<(SharedState, Arc<RpcClient>)>,
    Json(body): Json<WatchAddressBody>,
) -> impl IntoResponse {
    match Pubkey::from_str(&body.address) {
        Ok(pubkey) => {
            let mut s = state.write().await;
            s.watch_address = pubkey;
            (
                StatusCode::OK,
                Json(WatchAddressResponse {
                    watch_address: pubkey.to_string(),
                }),
            )
        }
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(WatchAddressResponse {
                watch_address: format!("Invalid address: {}", body.address),
            }),
        ),
    }
}

#[derive(Serialize)]
struct WalletInfo {
    pubkey: Option<String>,
    balance_sol: Option<f64>,
    message: String,
}

async fn get_wallet(
    State((state, rpc_client)): State<(SharedState, Arc<RpcClient>)>,
) -> impl IntoResponse {
    let pubkey = {
        let s = state.read().await;
        s.wallet.as_ref().map(|kp| kp.pubkey())
    };

    match pubkey {
        Some(pk) => {
            let balance = rpc_client
                .get_balance(&pk)
                .await
                .ok()
                .map(|b| b as f64 / 1e9);
            let message = if balance.unwrap_or(0.0) == 0.0 {
                format!("Wallet has no funds. Send SOL to {} to start trading.", pk)
            } else {
                "Wallet loaded.".to_string()
            };
            Json(WalletInfo {
                pubkey: Some(pk.to_string()),
                balance_sol: balance,
                message,
            })
        }
        None => Json(WalletInfo {
            pubkey: None,
            balance_sol: None,
            message: "No wallet found. Place wallet.json in the working directory.".to_string(),
        }),
    }
}
