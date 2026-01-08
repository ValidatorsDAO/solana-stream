use anyhow::Context;
use std::env;

use crate::utils::fallback::{DEFAULT_CONFIG_PATH, DEFAULT_RPC_ENDPOINT};

#[derive(Clone)]
pub struct Settings {
    pub config_path: String,
    pub grpc_endpoint: String,
    pub x_token: Option<String>,
    pub rpc_endpoint: String,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let config_path =
            env::var("CONFIG_PATH").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
        let grpc_endpoint = env::var("GRPC_ENDPOINT").context("GRPC_ENDPOINT is missing")?;
        let x_token = env::var("X_TOKEN").ok();
        let rpc_endpoint =
            env::var("SOLANA_RPC_ENDPOINT").unwrap_or_else(|_| DEFAULT_RPC_ENDPOINT.to_string());

        Ok(Self {
            config_path,
            grpc_endpoint,
            x_token,
            rpc_endpoint,
        })
    }
}
