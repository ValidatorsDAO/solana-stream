use anyhow::{Context, Result};
use log::{info, warn};
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

const WALLET_FILE: &str = "wallet.json";
const VANITY_SUFFIX: &str = "SLV";

/// Reconstruct a Keypair from 64-byte (secret+pubkey) array, verifying that
/// the derived public key matches the stored one (B1 fix).
pub fn keypair_from_bytes(bytes: &[u8]) -> Result<Keypair> {
    if bytes.len() != 64 {
        return Err(anyhow::anyhow!(
            "wallet.json must contain exactly 64 bytes, got {}",
            bytes.len()
        ));
    }
    let secret: [u8; 32] = bytes[..32].try_into().unwrap();
    let keypair = Keypair::new_from_array(secret);
    // Verify pubkey matches stored bytes (B1: detect corrupted wallet.json).
    let derived_pubkey = keypair.pubkey();
    let stored_pubkey_bytes: [u8; 32] = bytes[32..64].try_into().unwrap();
    let stored_pubkey = solana_sdk::pubkey::Pubkey::new_from_array(stored_pubkey_bytes);
    if derived_pubkey != stored_pubkey {
        return Err(anyhow::anyhow!(
            "wallet.json corrupted: derived pubkey {} != stored pubkey {}",
            derived_pubkey,
            stored_pubkey
        ));
    }
    Ok(keypair)
}

/// Load keypair from wallet.json if it exists, otherwise generate a new one and save it.
pub fn load_or_create_wallet() -> Result<Keypair> {
    let path = Path::new(WALLET_FILE);
    if path.exists() {
        let bytes_str = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", WALLET_FILE))?;
        let bytes: Vec<u8> = serde_json::from_str(&bytes_str)
            .with_context(|| format!("Failed to parse {} as JSON array", WALLET_FILE))?;
        let keypair = keypair_from_bytes(&bytes)?;
        info!("Loaded wallet: {}", keypair.pubkey());
        Ok(keypair)
    } else {
        let keypair = grind_keypair_with_suffix(VANITY_SUFFIX)?;
        let bytes: Vec<u8> = keypair.to_bytes().to_vec();
        let json = serde_json::to_string(&bytes).context("Failed to serialize keypair")?;
        std::fs::write(path, json).with_context(|| format!("Failed to write {}", WALLET_FILE))?;
        warn!(
            "Generated new vanity wallet (suffix='{}') and saved to {}: {}",
            VANITY_SUFFIX,
            WALLET_FILE,
            keypair.pubkey()
        );
        Ok(keypair)
    }
}

/// Grind a keypair whose base58 pubkey ends with `suffix`.
/// Parallelized across available CPU cores; first match wins.
fn grind_keypair_with_suffix(suffix: &str) -> Result<Keypair> {
    let num_threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    info!(
        "Grinding vanity wallet with suffix '{}' across {} threads...",
        suffix, num_threads
    );

    let found = Arc::new(AtomicBool::new(false));
    let attempts = Arc::new(AtomicU64::new(0));
    let (tx, rx) = mpsc::channel::<Keypair>();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(num_threads);
    for _ in 0..num_threads {
        let suffix = suffix.to_string();
        let found = Arc::clone(&found);
        let attempts = Arc::clone(&attempts);
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            let mut local = 0u64;
            while !found.load(Ordering::Relaxed) {
                for _ in 0..1024 {
                    let kp = Keypair::new();
                    if kp.pubkey().to_string().ends_with(&suffix) {
                        found.store(true, Ordering::Relaxed);
                        let _ = tx.send(kp);
                        attempts.fetch_add(local, Ordering::Relaxed);
                        return;
                    }
                    local += 1;
                }
            }
            attempts.fetch_add(local, Ordering::Relaxed);
        }));
    }
    drop(tx);

    let keypair = rx
        .recv()
        .context("vanity grinder produced no keypair (all workers exited)")?;
    for h in handles {
        let _ = h.join();
    }
    info!(
        "Vanity grind complete: {} attempts in {:.2}s",
        attempts.load(Ordering::Relaxed),
        start.elapsed().as_secs_f64()
    );
    Ok(keypair)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grinder_produces_pubkey_with_requested_suffix() {
        let kp = grind_keypair_with_suffix("SLV").expect("grinder returned a keypair");
        let pubkey = kp.pubkey().to_string();
        assert!(
            pubkey.ends_with("SLV"),
            "pubkey {} does not end with SLV",
            pubkey
        );

        // Round-trip: persisted 64 bytes must reconstruct the same pubkey.
        let bytes = kp.to_bytes().to_vec();
        let restored = keypair_from_bytes(&bytes).expect("round-trip reload");
        assert_eq!(restored.pubkey(), kp.pubkey());
    }
}
