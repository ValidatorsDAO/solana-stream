use chrono::{DateTime, Utc};
use log::warn;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use std::collections::{HashMap, VecDeque};
use std::str::FromStr;

pub const PUMPSWAP_AMM: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub const TRADE_LOG_CAP: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeConfig {
    pub buy_amount_lamports: u64,
    pub sell_multiplier: f64,
    pub slippage_bps: u64,
    pub max_positions: usize,
    /// Minimum pool SOL liquidity (in lamports) to trigger a buy.
    /// Pools with less quote reserves are skipped.
    pub min_pool_sol_lamports: u64,
}

impl Default for TradeConfig {
    fn default() -> Self {
        Self {
            buy_amount_lamports: 100_000, // 0.0001 SOL
            sell_multiplier: 1.1,
            slippage_bps: 500, // 5% — reasonable for small positions
            max_positions: 1,
            min_pool_sol_lamports: 100_000, // 0.0001 SOL
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PositionStatus {
    Active,
    Selling,
    Sold,
    /// ATA burned + closed, rent recovered. Terminal state.
    Closed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub id: String,
    #[serde(serialize_with = "pubkey_str")]
    pub pool: Pubkey,
    #[serde(serialize_with = "pubkey_str")]
    pub base_mint: Pubkey,
    pub buy_price_lamports: u64,
    pub base_amount: u64,
    pub bought_at: DateTime<Utc>,
    pub status: PositionStatus,
    /// Cumulative SOL received across all partial sells (lamports).
    pub total_sell_lamports: u64,
    /// Token program that owns the graduated (quote) mint.
    /// TOKEN_PROGRAM for legacy SPL tokens, TOKEN_2022 for Token Extensions.
    #[serde(serialize_with = "pubkey_str")]
    pub quote_token_program: Pubkey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TradeAction {
    Buy,
    Sell,
    Error,
    Notification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeLog {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub action: TradeAction,
    #[serde(serialize_with = "pubkey_str", deserialize_with = "pubkey_from_str")]
    pub pool: Pubkey,
    #[serde(serialize_with = "pubkey_str", deserialize_with = "pubkey_from_str")]
    pub base_mint: Pubkey,
    pub amount_sol: f64,
    pub amount_tokens: u64,
    pub tx_signature: Option<String>,
    pub error: Option<String>,
    /// Human-readable message for notification events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub struct AppState {
    pub config: TradeConfig,
    pub running: bool,
    /// Whether gRPC streaming is active.
    pub grpc_streaming: bool,
    pub wallet: Option<Keypair>,
    /// Positions keyed by UUID string for safe concurrent access.
    pub positions: HashMap<String, Position>,
    pub trade_logs: VecDeque<TradeLog>,
    pub watch_address: Pubkey,
    /// Discord webhook URL for notifications (from env).
    pub webhook_url: Option<String>,
    /// Optional Redis client for persistent trade log storage.
    pub redis_client: Option<redis::Client>,
}

impl AppState {
    pub fn new(webhook_url: Option<String>) -> Self {
        Self {
            config: TradeConfig::default(),
            running: false,
            grpc_streaming: true,
            wallet: None,
            positions: HashMap::new(),
            trade_logs: VecDeque::new(),
            watch_address: Pubkey::from_str(PUMPSWAP_AMM).expect("valid pubkey"),
            webhook_url,
            redis_client: None,
        }
    }

    pub fn push_log(&mut self, log: TradeLog) {
        if self.trade_logs.len() >= TRADE_LOG_CAP {
            self.trade_logs.pop_front();
        }
        self.trade_logs.push_back(log);
    }

    /// Push a notification event into the trade log.
    pub fn push_notification(&mut self, pool: Pubkey, base_mint: Pubkey, message: String) {
        let log = TradeLog {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            action: TradeAction::Notification,
            pool,
            base_mint,
            amount_sol: 0.0,
            amount_tokens: 0,
            tx_signature: None,
            error: None,
            message: Some(message),
        };
        self.push_log(log);
    }

    pub fn active_position_count(&self) -> usize {
        self.positions
            .values()
            .filter(|p| p.status == PositionStatus::Active || p.status == PositionStatus::Selling)
            .count()
    }
}

fn pubkey_str<S: serde::Serializer>(pubkey: &Pubkey, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&pubkey.to_string())
}

fn pubkey_from_str<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Pubkey, D::Error> {
    let s = String::deserialize(deserializer)?;
    Pubkey::from_str(&s).map_err(serde::de::Error::custom)
}

/// Persist a trade log entry to Redis (fire-and-forget).
/// Stores the JSON under `trade:logs:{id}`, and adds the id to sorted sets
/// `trade:logs:timeline` and `trade:logs:pool:{pool}` scored by unix-millis.
pub fn persist_trade_log_to_redis(client: &redis::Client, log: &TradeLog) {
    let client = client.clone();
    let log_id = log.id.clone();
    let pool_key = format!("trade:logs:pool:{}", log.pool);
    let score = log.timestamp.timestamp_millis() as f64;
    let json = match serde_json::to_string(log) {
        Ok(j) => j,
        Err(e) => {
            warn!("Failed to serialize TradeLog for Redis: {:?}", e);
            return;
        }
    };
    tokio::spawn(async move {
        let result: Result<(), redis::RedisError> = async {
            let mut con = client.get_multiplexed_async_connection().await?;
            redis::pipe()
                .atomic()
                .cmd("SET")
                .arg(format!("trade:logs:{}", &log_id))
                .arg(&json)
                .ignore()
                .cmd("ZADD")
                .arg("trade:logs:timeline")
                .arg(score)
                .arg(&log_id)
                .ignore()
                .cmd("ZADD")
                .arg(&pool_key)
                .arg(score)
                .arg(&log_id)
                .ignore()
                .query_async::<()>(&mut con)
                .await?;
            Ok(())
        }
        .await;
        if let Err(e) = result {
            warn!("Redis write failed for trade log {}: {:?}", log_id, e);
        }
    });
}
