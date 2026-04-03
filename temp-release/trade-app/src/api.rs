use crate::state::{AppState, TradeAction, TradeLog};
use crate::webhook::notify_discord;
use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use log::warn;
use scalar_api_reference::axum::router as scalar_router;
use serde::{Deserialize, Serialize};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use utoipa::OpenApi;

pub type SharedState = Arc<RwLock<AppState>>;

#[derive(Clone)]
pub struct ApiContext {
    pub state: SharedState,
    pub rpc_client: Arc<RpcClient>,
    pub api_token: Option<String>,
}

// ─── OpenAPI spec ────────────────────────────────────────────────────────────

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Trade App — PumpSwap Auto-Trading Bot",
        version = "1.0.0",
        description = "API for controlling the PumpSwap auto-trading bot via Geyser gRPC."
    ),
    paths(
        get_config,
        put_config,
        post_trade_start,
        post_trade_stop,
        get_trade_status,
        get_logs,
        get_trade_history,
        get_trade_by_id,
        put_watch_address,
        get_wallet,
        post_grpc_start,
        post_grpc_stop,
    ),
    components(schemas(
        PartialTradeConfig,
        StartResponse,
        StatusResponse,
        WatchAddressBody,
        WalletResponse,
        TradeHistoryQuery,
        TradeHistoryResponse,
    ))
)]
struct ApiDoc;

// ─── Router ──────────────────────────────────────────────────────────────────

pub fn build_router(
    state: SharedState,
    rpc_client: Arc<RpcClient>,
    api_token: Option<String>,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let ctx = ApiContext {
        state,
        rpc_client,
        api_token,
    };

    // OpenAPI JSON endpoint + Scalar docs at /docs
    let openapi_json = ApiDoc::openapi()
        .to_pretty_json()
        .expect("valid openapi json");
    let scalar_config = serde_json::json!({
        "spec": { "content": openapi_json },
    });

    let api_router = Router::new()
        .route("/api/config", get(get_config))
        .route("/api/config", put(put_config))
        .route("/api/trade/start", post(post_trade_start))
        .route("/api/trade/stop", post(post_trade_stop))
        .route("/api/trade/status", get(get_trade_status))
        .route("/api/logs", get(get_logs))
        .route("/api/trades/history", get(get_trade_history))
        .route("/api/trades/{id}", get(get_trade_by_id))
        .route("/api/watch-address", put(put_watch_address))
        .route("/api/wallet", get(get_wallet))
        .route("/api/grpc/start", post(post_grpc_start))
        .route("/api/grpc/stop", post(post_grpc_stop))
        .route(
            "/openapi.json",
            get(move || {
                let json = ApiDoc::openapi()
                    .to_pretty_json()
                    .expect("valid openapi json");
                async move { (StatusCode::OK, [("content-type", "application/json")], json) }
            }),
        )
        .route_layer(middleware::from_fn_with_state(ctx.clone(), auth_middleware))
        .with_state(ctx);

    Router::new()
        .merge(scalar_router("/docs", &scalar_config))
        .merge(api_router)
        .layer(cors)
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

// ─── Schemas ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PartialTradeConfig {
    pub buy_amount_lamports: Option<u64>,
    pub sell_multiplier: Option<f64>,
    pub slippage_bps: Option<u64>,
    pub max_positions: Option<usize>,
    pub min_pool_sol_lamports: Option<u64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StartResponse {
    started: bool,
    already_running: bool,
    message: String,
    wallet_pubkey: Option<String>,
    balance_sol: Option<f64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StatusResponse {
    running: bool,
    grpc_streaming: bool,
    active_positions: usize,
    wallet_balance: Option<f64>,
    phase: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct WatchAddressBody {
    pub address: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct WalletResponse {
    pub pubkey: Option<String>,
    pub balance_sol: Option<f64>,
    pub message: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TradeHistoryQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    /// Filter by action: "Buy" or "Sell"
    pub action: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TradeHistoryResponse {
    pub trades: Vec<serde_json::Value>,
    pub total: usize,
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// Get the current trade configuration.
#[utoipa::path(get, path = "/api/config", responses((status = 200, description = "Current trade configuration")), tag = "Config")]
async fn get_config(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let s = ctx.state.read().await;
    Json(s.config.clone())
}

/// Update trade configuration (partial update).
#[utoipa::path(put, path = "/api/config", request_body = PartialTradeConfig, responses((status = 200, description = "Updated trade configuration")), tag = "Config")]
async fn put_config(
    State(ctx): State<ApiContext>,
    Json(body): Json<PartialTradeConfig>,
) -> impl IntoResponse {
    let mut s = ctx.state.write().await;
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
    if let Some(v) = body.min_pool_sol_lamports {
        s.config.min_pool_sol_lamports = v;
    }
    Json(s.config.clone())
}

/// Start auto-trading (requires sufficient wallet balance).
#[utoipa::path(post, path = "/api/trade/start", responses((status = 200, body = StartResponse)), tag = "Trade")]
async fn post_trade_start(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let (has_wallet, pubkey, buy_amount_lamports) = {
        let s = ctx.state.read().await;
        let pk = s.wallet.as_ref().map(|kp| kp.pubkey());
        (pk.is_some(), pk, s.config.buy_amount_lamports)
    };

    if !has_wallet {
        return (
            StatusCode::BAD_REQUEST,
            Json(StartResponse {
                started: false,
                already_running: false,
                message: "No wallet loaded.".to_string(),
                wallet_pubkey: None,
                balance_sol: None,
            }),
        );
    }

    let pubkey = pubkey.unwrap();
    let balance = match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        ctx.rpc_client.get_balance(&pubkey),
    )
    .await
    {
        Ok(Ok(b)) => b,
        _ => 0,
    };
    let balance_sol = balance as f64 / 1e9;

    if balance < buy_amount_lamports + 10_000_000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(StartResponse {
                started: false,
                already_running: false,
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

    let mut s = ctx.state.write().await;
    if s.running {
        return (
            StatusCode::OK,
            Json(StartResponse {
                started: true,
                already_running: true,
                message: "Trading already running.".to_string(),
                wallet_pubkey: Some(pubkey.to_string()),
                balance_sol: Some(balance_sol),
            }),
        );
    }

    s.running = true;
    let msg = format!(
        "▶️ Trading started — wallet: {} | balance: {:.4} SOL | buy: {:.4} SOL | sell: {}x",
        pubkey,
        balance_sol,
        buy_amount_lamports as f64 / 1e9,
        s.config.sell_multiplier
    );
    fire_notification(&mut s, &msg);
    (
        StatusCode::OK,
        Json(StartResponse {
            started: true,
            already_running: false,
            message: "Trading started.".to_string(),
            wallet_pubkey: Some(pubkey.to_string()),
            balance_sol: Some(balance_sol),
        }),
    )
}

/// Stop auto-trading.
#[utoipa::path(post, path = "/api/trade/stop", responses((status = 200)), tag = "Trade")]
async fn post_trade_stop(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let mut s = ctx.state.write().await;
    s.running = false;
    let positions = s.active_position_count();
    let msg = format!("⏹️ Trading stopped — active positions: {}", positions);
    fire_notification(&mut s, &msg);
    Json(serde_json::json!({ "stopped": true }))
}

/// Get trade status (running, gRPC streaming, positions, balance).
#[utoipa::path(get, path = "/api/trade/status", responses((status = 200, body = StatusResponse)), tag = "Trade")]
async fn get_trade_status(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let (running, grpc_streaming, active, pubkey) = {
        let s = ctx.state.read().await;
        let pk = s.wallet.as_ref().map(|kp| kp.pubkey());
        (s.running, s.grpc_streaming, s.active_position_count(), pk)
    };

    let wallet_balance = if let Some(pk) = pubkey {
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            ctx.rpc_client.get_balance(&pk),
        )
        .await
        {
            Ok(Ok(b)) => Some(b as f64 / 1e9),
            _ => None,
        }
    } else {
        None
    };

    let phase = if !grpc_streaming {
        "grpc_paused"
    } else if !running {
        "stopped"
    } else if active > 0 {
        "position_open"
    } else {
        "waiting_for_buy"
    };

    Json(StatusResponse {
        running,
        grpc_streaming,
        active_positions: active,
        wallet_balance,
        phase: phase.to_string(),
    })
}

/// Get trade logs (newest first).
#[utoipa::path(get, path = "/api/logs", params(("limit" = Option<usize>, Query, description = "Max logs"), ("offset" = Option<usize>, Query, description = "Skip")), responses((status = 200, description = "Recent logs")), tag = "Logs")]
async fn get_logs(
    State(ctx): State<ApiContext>,
    Query(query): Query<LogsQuery>,
) -> impl IntoResponse {
    let s = ctx.state.read().await;
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);
    let logs: Vec<_> = s
        .trade_logs
        .iter()
        .rev()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect();
    Json(logs)
}

/// Update the watch address (AMM program to monitor).
#[utoipa::path(put, path = "/api/watch-address", request_body = WatchAddressBody, responses((status = 200)), tag = "Config")]
async fn put_watch_address(
    State(ctx): State<ApiContext>,
    Json(body): Json<WatchAddressBody>,
) -> impl IntoResponse {
    match Pubkey::from_str(&body.address) {
        Ok(pubkey) => {
            let mut s = ctx.state.write().await;
            s.watch_address = pubkey;
            (
                StatusCode::OK,
                Json(serde_json::json!({ "watch_address": pubkey.to_string() })),
            )
        }
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid address: {}", body.address) })),
        ),
    }
}

/// Get wallet info and balance.
#[utoipa::path(get, path = "/api/wallet", responses((status = 200, body = WalletResponse)), tag = "Wallet")]
async fn get_wallet(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let pubkey = {
        let s = ctx.state.read().await;
        s.wallet.as_ref().map(|kp| kp.pubkey())
    };

    match pubkey {
        Some(pk) => {
            let balance = match tokio::time::timeout(
                std::time::Duration::from_secs(3),
                ctx.rpc_client.get_balance(&pk),
            )
            .await
            {
                Ok(Ok(b)) => Some(b as f64 / 1e9),
                _ => None,
            };
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

/// Start gRPC streaming (pool detection & notifications).
#[utoipa::path(post, path = "/api/grpc/start", responses((status = 200)), tag = "gRPC")]
async fn post_grpc_start(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let mut s = ctx.state.write().await;
    s.grpc_streaming = true;
    let msg = "📡 gRPC streaming started — pool detection & notifications active".to_string();
    fire_notification(&mut s, &msg);
    Json(serde_json::json!({ "grpc_streaming": true }))
}

/// Stop gRPC streaming (pauses pool detection & notifications).
#[utoipa::path(post, path = "/api/grpc/stop", responses((status = 200)), tag = "gRPC")]
async fn post_grpc_stop(State(ctx): State<ApiContext>) -> impl IntoResponse {
    let mut s = ctx.state.write().await;
    s.grpc_streaming = false;
    let msg = "⏸️ gRPC streaming stopped — pool detection paused".to_string();
    fire_notification(&mut s, &msg);
    Json(serde_json::json!({ "grpc_streaming": false }))
}

/// Get trade history (Buy/Sell only, newest first). Uses Redis if available, otherwise in-memory.
#[utoipa::path(get, path = "/api/trades/history", params(
    ("limit" = Option<usize>, Query, description = "Max trades (default 50)"),
    ("offset" = Option<usize>, Query, description = "Skip"),
    ("action" = Option<String>, Query, description = "Filter: Buy or Sell"),
), responses((status = 200, body = TradeHistoryResponse)), tag = "Trades")]
async fn get_trade_history(
    State(ctx): State<ApiContext>,
    Query(query): Query<TradeHistoryQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(50).min(1000);
    let offset = query.offset.unwrap_or(0);
    let action_filter: Option<TradeAction> = query.action.as_deref().and_then(|a| match a {
        "Buy" => Some(TradeAction::Buy),
        "Sell" => Some(TradeAction::Sell),
        _ => None,
    });

    let s = ctx.state.read().await;

    // Try Redis first
    if let Some(ref client) = s.redis_client {
        if let Ok(result) = fetch_trade_history_from_redis(client, limit, offset, &action_filter).await {
            return Json(result);
        }
        // Redis failed, fall through to in-memory
    }

    // In-memory fallback
    let filtered: Vec<&TradeLog> = s
        .trade_logs
        .iter()
        .rev()
        .filter(|log| {
            if log.action != TradeAction::Buy && log.action != TradeAction::Sell {
                return false;
            }
            if let Some(ref af) = action_filter {
                return &log.action == af;
            }
            true
        })
        .collect();
    let total = filtered.len();
    let trades: Vec<serde_json::Value> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|log| serde_json::to_value(log).unwrap_or_default())
        .collect();
    Json(TradeHistoryResponse { trades, total })
}

/// Get a specific trade log by id. Uses Redis if available, otherwise in-memory.
#[utoipa::path(get, path = "/api/trades/{id}", params(
    ("id" = String, Path, description = "Trade log ID"),
), responses((status = 200, description = "Trade log detail"), (status = 404, description = "Not found")), tag = "Trades")]
async fn get_trade_by_id(
    State(ctx): State<ApiContext>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let s = ctx.state.read().await;

    // Try Redis first
    if let Some(ref client) = s.redis_client {
        if let Ok(Some(json_val)) = fetch_trade_by_id_from_redis(client, &id).await {
            return (StatusCode::OK, Json(json_val));
        }
    }

    // In-memory fallback
    if let Some(log) = s.trade_logs.iter().find(|l| l.id == id) {
        let val = serde_json::to_value(log).unwrap_or_default();
        return (StatusCode::OK, Json(val));
    }

    (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Trade not found" })))
}

async fn fetch_trade_history_from_redis(
    client: &redis::Client,
    limit: usize,
    offset: usize,
    action_filter: &Option<TradeAction>,
) -> Result<TradeHistoryResponse, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;

    // Get total count
    let total: usize = redis::cmd("ZCARD")
        .arg("trade:logs:timeline")
        .query_async(&mut con)
        .await?;

    // Fetch ids from sorted set (newest first = ZREVRANGE)
    // Over-fetch if we need to filter by action
    let fetch_count = if action_filter.is_some() {
        (limit + offset) * 3 // fetch extra to account for filtering
    } else {
        limit + offset
    };
    let ids: Vec<String> = redis::cmd("ZREVRANGE")
        .arg("trade:logs:timeline")
        .arg(0i64)
        .arg((fetch_count - 1) as i64)
        .query_async(&mut con)
        .await?;

    if ids.is_empty() {
        return Ok(TradeHistoryResponse {
            trades: vec![],
            total: 0,
        });
    }

    // MGET all trade log JSONs
    let keys: Vec<String> = ids.iter().map(|id| format!("trade:logs:{}", id)).collect();
    let jsons: Vec<Option<String>> = redis::cmd("MGET")
        .arg(&keys)
        .query_async(&mut con)
        .await?;

    let mut trades: Vec<serde_json::Value> = Vec::new();
    let mut filtered_total = 0usize;
    for json_opt in jsons {
        if let Some(json_str) = json_opt {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                // Filter: only Buy/Sell
                let action_str = val.get("action").and_then(|a| a.as_str()).unwrap_or("");
                if action_str != "Buy" && action_str != "Sell" {
                    continue;
                }
                if let Some(ref af) = action_filter {
                    let expected = match af {
                        TradeAction::Buy => "Buy",
                        TradeAction::Sell => "Sell",
                        _ => "",
                    };
                    if action_str != expected {
                        continue;
                    }
                }
                filtered_total += 1;
                if filtered_total > offset && trades.len() < limit {
                    trades.push(val);
                }
            }
        }
    }

    Ok(TradeHistoryResponse {
        trades,
        total: filtered_total,
    })
}

async fn fetch_trade_by_id_from_redis(
    client: &redis::Client,
    id: &str,
) -> Result<Option<serde_json::Value>, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;
    let result: Option<String> = redis::cmd("GET")
        .arg(format!("trade:logs:{}", id))
        .query_async(&mut con)
        .await?;
    match result {
        Some(json_str) => match serde_json::from_str::<serde_json::Value>(&json_str) {
            Ok(val) => Ok(Some(val)),
            Err(e) => {
                warn!("Failed to parse trade log JSON from Redis: {:?}", e);
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Push a notification to logs + webhook (fire-and-forget).
fn fire_notification(state: &mut AppState, msg: &str) {
    use solana_sdk::pubkey::Pubkey;
    state.push_notification(Pubkey::default(), Pubkey::default(), msg.to_string());
    if let Some(url) = &state.webhook_url {
        let url = url.clone();
        let discord_msg = msg.to_string();
        tokio::spawn(async move {
            notify_discord(&url, &discord_msg).await;
        });
    }
}
