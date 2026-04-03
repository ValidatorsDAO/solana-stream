use anyhow::{Context, Result};
use log::{info, warn};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use std::path::Path;

const WALLET_FILE: &str = "wallet.json";

/// Load keypair from wallet.json if it exists, otherwise generate a new one and save it.
pub fn load_or_create_wallet() -> Result<Keypair> {
    let path = Path::new(WALLET_FILE);
    if path.exists() {
        let bytes_str = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", WALLET_FILE))?;
        let bytes: Vec<u8> = serde_json::from_str(&bytes_str)
            .with_context(|| format!("Failed to parse {} as JSON array", WALLET_FILE))?;
        if bytes.len() != 64 {
            return Err(anyhow::anyhow!("wallet.json must contain exactly 64 bytes, got {}", bytes.len()));
        }
        let secret: [u8; 32] = bytes[..32].try_into().unwrap();
        let keypair = Keypair::new_from_array(secret);
        info!("Loaded wallet: {}", keypair.pubkey());
        Ok(keypair)
    } else {
        let keypair = Keypair::new();
        let bytes: Vec<u8> = keypair.to_bytes().to_vec();
        let json = serde_json::to_string(&bytes).context("Failed to serialize keypair")?;
        std::fs::write(path, json)
            .with_context(|| format!("Failed to write {}", WALLET_FILE))?;
        warn!(
            "Generated new wallet and saved to {}: {}",
            WALLET_FILE,
            keypair.pubkey()
        );
        Ok(keypair)
    }
}

/// Fetch the SOL balance (in lamports) for a given pubkey.
pub async fn get_balance(rpc_client: &RpcClient, pubkey: &Pubkey) -> Result<u64> {
    rpc_client
        .get_balance(pubkey)
        .await
        .context("Failed to fetch wallet balance")
}
