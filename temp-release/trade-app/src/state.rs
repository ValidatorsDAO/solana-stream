use chrono::{DateTime, Utc};
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
}

impl Default for TradeConfig {
    fn default() -> Self {
        Self {
            buy_amount_lamports: 100_000_000,
            sell_multiplier: 1.5,
            slippage_bps: 300,
            max_positions: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PositionStatus {
    Active,
    Selling,
    Sold,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TradeAction {
    Buy,
    Sell,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeLog {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub action: TradeAction,
    #[serde(serialize_with = "pubkey_str")]
    pub pool: Pubkey,
    #[serde(serialize_with = "pubkey_str")]
    pub base_mint: Pubkey,
    pub amount_sol: f64,
    pub amount_tokens: u64,
    pub tx_signature: Option<String>,
    pub error: Option<String>,
}

pub struct AppState {
    pub config: TradeConfig,
    pub running: bool,
    pub wallet: Option<Keypair>,
    /// Positions keyed by UUID string for safe concurrent access.
    pub positions: HashMap<String, Position>,
    pub trade_logs: VecDeque<TradeLog>,
    pub watch_address: Pubkey,
    /// Discord webhook URL for notifications (from env).
    pub webhook_url: Option<String>,
}

impl AppState {
    pub fn new(webhook_url: Option<String>) -> Self {
        Self {
            config: TradeConfig::default(),
            running: false,
            wallet: None,
            positions: HashMap::new(),
            trade_logs: VecDeque::new(),
            watch_address: Pubkey::from_str(PUMPSWAP_AMM).expect("valid pubkey"),
            webhook_url,
        }
    }

    pub fn push_log(&mut self, log: TradeLog) {
        if self.trade_logs.len() >= TRADE_LOG_CAP {
            self.trade_logs.pop_front();
        }
        self.trade_logs.push_back(log);
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
