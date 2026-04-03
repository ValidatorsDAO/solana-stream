use anyhow::Context;
use std::env;
use std::path::Path;

use crate::utils::fallback::{DEFAULT_API_PORT, DEFAULT_CONFIG_PATH};

const DEFAULT_ERPC_BASE: &str = "https://edge.erpc.global";

#[derive(Clone)]
pub struct Settings {
    pub config_path: String,
    pub grpc_endpoint: String,
    pub x_token: Option<String>,
    pub rpc_endpoint: String,
    pub api_port: u16,
    /// Optional Discord webhook URL for notifications.
    pub webhook_url: Option<String>,
    /// Optional Bearer token for API authentication.
    pub api_token: Option<String>,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        // Load .env.erpc if it exists (contains ERPC_API_KEY).
        let erpc_env_path = Path::new(".env.erpc");
        if erpc_env_path.exists() {
            dotenv::from_path(erpc_env_path).ok();
        }

        let config_path =
            env::var("CONFIG_PATH").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
        let grpc_endpoint = env::var("GRPC_ENDPOINT").context("GRPC_ENDPOINT is missing")?;
        let x_token = env::var("X_TOKEN").ok();

        // Build RPC endpoint: prefer explicit SOLANA_RPC_ENDPOINT, then ERPC_API_KEY.
        let rpc_endpoint = if let Ok(explicit) = env::var("SOLANA_RPC_ENDPOINT") {
            explicit
        } else if let Ok(api_key) = env::var("ERPC_API_KEY") {
            format!("{}?api-key={}", DEFAULT_ERPC_BASE, api_key)
        } else {
            "https://api.mainnet-beta.solana.com".to_string()
        };

        let api_port = env::var("API_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_API_PORT);
        let webhook_url = env::var("WEBHOOK_URL").ok();
        let api_token = env::var("API_TOKEN").ok();

        Ok(Self {
            config_path,
            grpc_endpoint,
            x_token,
            rpc_endpoint,
            api_port,
            webhook_url,
            api_token,
        })
    }
}
