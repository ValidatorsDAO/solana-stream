use crate::state::{AppState, PositionStatus, PUMPSWAP_AMM};
use axum::{
    extract::{Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
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

#[derive(Clone)]
pub struct ApiContext {
    pub state: SharedState,
    pub rpc_client: Arc<RpcClient>,
    pub api_token: Option<String>,
}

pub fn build_router(state: SharedState, rpc_client: Arc<RpcClient>, api_token: Option<String>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let ctx = ApiContext { state, rpc_client, api_token };

    Router::new()
        .route("/api/config", get(get_config))
        .route("/api/config", put(put_config))
        .route("/api/trade/start", post(post_trade_start))
        .route("/api/trade/stop", post(post_trade_stop))
        .route("/api/trade/status", get(get_trade_status))
        .route("/api/logs", get(get_logs))
        .route("/api/watch-address", put(put_watch_address))
        .route("/api/wallet", get(get_wallet))
        .route_layer(middleware::from_fn_with_state(ctx.clone(), auth_middleware))
        .layer(cors)
        .with_state(ctx)
}

/// Bearer token middleware. If API_TOKEN is set, require it on every request.
async fn auth_middleware(
    State(ctx): State<ApiContext>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    if let Some(expected_token) = &ctx.api_token {
        let auth_header = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let provided = auth_header.strip_prefix("Bearer ").unwrap_or("");
        if provided != expected_token {
            return (StatusCode::UNAUTHORIZED, "Invalid or missing API token").into_response();
        }
    }
    next.run(request).await
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn get_config(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let s = ctx.state.read().await;
    Json(s.config.clone())
}

#[derive(Debug, Deserialize)]
pub struct PartialTradeConfig {
    pub buy_amount_lamports: Option<u64>,
    pub sell_multiplier: Option<f64>,
    pub slippage_bps: Option<u64>,
    pub max_positions: Option<usize>,
}

async fn put_config(State(ctx): State<ApiContext>, Json(body): Json<PartialTradeConfig>) -> impl IntoResponse {
    let mut s = ctx.state.write().await;
    if let Some(v) = body.buy_amount_lamports { s.config.buy_amount_lamports = v; }
    if let Some(v) = body.sell_multiplier { s.config.sell_multiplier = v; }
    if let Some(v) = body.slippage_bps { s.config.slippage_bps = v; }
    if let Some(v) = body.max_positions { s.config.max_positions = v; }
    Json(s.config.clone())
}

#[derive(Serialize)]
struct StartResponse {
    started: bool,
    message: String,
    wallet_pubkey: Option<String>,
    balance_sol: Option<f64>,
}

async fn post_trade_start(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let (has_wallet, pubkey, buy_amount_lamports) = {
        let s = ctx.state.read().await;
        let pk = s.wallet.as_ref().map(|kp| kp.pubkey());
        (pk.is_some(), pk, s.config.buy_amount_lamports)
    };

    if !has_wallet {
        return (StatusCode::BAD_REQUEST, Json(StartResponse {
            started: false,
            message: "No wallet loaded.".to_string(),
            wallet_pubkey: None,
            balance_sol: None,
        }));
    }

    let pubkey = pubkey.unwrap();
    let balance = ctx.rpc_client.get_balance(&pubkey).await.unwrap_or(0);
    let balance_sol = balance as f64 / 1e9;

    if balance < buy_amount_lamports + 10_000_000 {
        return (StatusCode::BAD_REQUEST, Json(StartResponse {
            started: false,
            message: format!("Insufficient funds. Send at least {:.4} SOL to {}", (buy_amount_lamports + 10_000_000) as f64 / 1e9, pubkey),
            wallet_pubkey: Some(pubkey.to_string()),
            balance_sol: Some(balance_sol),
        }));
    }

    let mut s = ctx.state.write().await;
    s.running = true;
    (StatusCode::OK, Json(StartResponse {
        started: true,
        message: "Trading started.".to_string(),
        wallet_pubkey: Some(pubkey.to_string()),
        balance_sol: Some(balance_sol),
    }))
}

async fn post_trade_stop(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let mut s = ctx.state.write().await;
    s.running = false;
    Json(serde_json::json!({ "stopped": true }))
}

async fn get_trade_status(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let (running, active, pubkey) = {
        let s = ctx.state.read().await;
        let pk = s.wallet.as_ref().map(|kp| kp.pubkey());
        (s.running, s.active_position_count(), pk)
    };

    let wallet_balance = if let Some(pk) = pubkey {
        ctx.rpc_client.get_balance(&pk).await.ok().map(|b| b as f64 / 1e9)
    } else {
        None
    };

    Json(serde_json::json!({
        "running": running,
        "active_positions": active,
        "wallet_balance": wallet_balance,
    }))
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

async fn get_logs(State(ctx): State<ApiContext>, Query(query): Query<LogsQuery>) -> impl IntoResponse {
    let s = ctx.state.read().await;
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);
    let logs: Vec<_> = s.trade_logs.iter().rev().skip(offset).take(limit).cloned().collect();
    Json(logs)
}

#[derive(Deserialize)]
pub struct WatchAddressBody {
    pub address: String,
}

async fn put_watch_address(State(ctx): State<ApiContext>, Json(body): Json<WatchAddressBody>) -> impl IntoResponse {
    match Pubkey::from_str(&body.address) {
        Ok(pubkey) => {
            let mut s = ctx.state.write().await;
            s.watch_address = pubkey;
            (StatusCode::OK, Json(serde_json::json!({ "watch_address": pubkey.to_string() })))
        }
        Err(_) => {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("Invalid address: {}", body.address) })))
        }
    }
}

async fn get_wallet(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let pubkey = {
        let s = ctx.state.read().await;
        s.wallet.as_ref().map(|kp| kp.pubkey())
    };

    match pubkey {
        Some(pk) => {
            let balance = ctx.rpc_client.get_balance(&pk).await.ok().map(|b| b as f64 / 1e9);
            let message = if balance.unwrap_or(0.0) == 0.0 {
                format!("Wallet has no funds. Send SOL to {} to start trading.", pk)
            } else {
                "Wallet loaded.".to_string()
            };
            Json(serde_json::json!({
                "pubkey": pk.to_string(),
                "balance_sol": balance,
                "message": message,
            }))
        }
        None => Json(serde_json::json!({
            "pubkey": null,
            "balance_sol": null,
            "message": "No wallet found.",
        })),
    }
}
