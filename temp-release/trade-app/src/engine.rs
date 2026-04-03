use crate::state::{AppState, Position, PositionStatus, TradeAction, TradeLog};
use chrono::Utc;
use log::{error, info, warn};
use rand::Rng;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use std::sync::Arc;
use tokio::sync::RwLock;
use ultima_swap_pumpfun::{self as pumpswap, Pool};
use uuid::Uuid;

/// Number of fee recipient slots to rotate over.
const FEE_RECIPIENT_COUNT: usize = 8;

/// Minimum lamports reserved for transaction fees on top of buy amount.
const TX_FEE_RESERVE: u64 = 10_000_000; // 0.01 SOL

/// Process a detected create_pool event. Performs a buy if conditions are met.
pub async fn handle_new_pool(
    pool_address: Pubkey,
    base_mint: Pubkey,
    state: Arc<RwLock<AppState>>,
    rpc_client: Arc<RpcClient>,
) {
    // Check if trading is active and under position limit.
    let (running, max_positions, buy_amount_lamports, slippage_bps, wallet_bytes) = {
        let s = state.read().await;
        let wallet_bytes = s.wallet.as_ref().map(|kp| kp.to_bytes().to_vec());
        (
            s.running,
            s.config.max_positions,
            s.config.buy_amount_lamports,
            s.config.slippage_bps,
            wallet_bytes,
        )
    };

    if !running {
        return;
    }

    {
        let s = state.read().await;
        let active = s
            .positions
            .iter()
            .filter(|p| p.status == PositionStatus::Active || p.status == PositionStatus::Selling)
            .count();
        if active >= max_positions {
            warn!(
                "Max positions ({}) reached, skipping pool {}",
                max_positions, pool_address
            );
            return;
        }
    }

    let wallet_bytes = match wallet_bytes {
        Some(b) => b,
        None => {
            error!("No wallet loaded, cannot buy");
            return;
        }
    };

    if wallet_bytes.len() != 64 {
        error!("Invalid wallet bytes length: {}", wallet_bytes.len());
        return;
    }
    let secret: [u8; 32] = wallet_bytes[..32].try_into().unwrap();
    let keypair = Keypair::new_from_array(secret);

    // Check balance.
    let balance = match rpc_client.get_balance(&keypair.pubkey()).await {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to fetch balance: {:?}", e);
            return;
        }
    };

    if balance < buy_amount_lamports + TX_FEE_RESERVE {
        push_error_log(
            &state,
            pool_address,
            base_mint,
            buy_amount_lamports,
            format!(
                "Insufficient balance: {} lamports (need {} + {} fee reserve)",
                balance, buy_amount_lamports, TX_FEE_RESERVE
            ),
        )
        .await;
        return;
    }

    // Fetch pool account data.
    let pool_account = match rpc_client.get_account(&pool_address).await {
        Ok(a) => a,
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("Failed to fetch pool account: {:?}", e)).await;
            return;
        }
    };

    let pool_data = match Pool::try_from_slice(&pool_account.data) {
        Ok(p) => p,
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("Failed to deserialize pool: {:?}", e)).await;
            return;
        }
    };

    // Fetch pool vault balances for quote calculation.
    let quote_vault_balance = match rpc_client.get_token_account_balance(&pool_data.pool_quote_token_account).await {
        Ok(b) => b.amount.parse::<u64>().unwrap_or(0),
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("Failed to fetch quote vault: {:?}", e)).await;
            return;
        }
    };

    let base_vault_balance = match rpc_client.get_token_account_balance(&pool_data.pool_base_token_account).await {
        Ok(b) => b.amount.parse::<u64>().unwrap_or(0),
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("Failed to fetch base vault: {:?}", e)).await;
            return;
        }
    };

    // Calculate how many base tokens we get for our SOL budget.
    let base_amount_out = match pumpswap::base_out_for_exact_quote_in(
        base_vault_balance,
        quote_vault_balance,
        buy_amount_lamports,
        pumpswap::DEFAULT_FEE_BPS,
    ) {
        Ok(a) => a,
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("AMM math error: {:?}", e)).await;
            return;
        }
    };

    let max_quote_in = match pumpswap::with_slippage_max(buy_amount_lamports, slippage_bps) {
        Ok(v) => v,
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("Slippage calc error: {:?}", e)).await;
            return;
        }
    };

    let fee_recipient_index = rand::thread_rng().gen_range(0..FEE_RECIPIENT_COUNT);

    info!(
        "New PumpSwap pool detected: {} base_mint: {} — buying {} base atoms for max {} lamports",
        pool_address, base_mint, base_amount_out, max_quote_in
    );

    // Build instructions.
    let mut instructions = vec![];

    // Create base ATA if needed.
    instructions.push(pumpswap::create_base_ata_if_needed(
        &keypair.pubkey(),
        &base_mint,
    ));

    // Build buy instruction.
    let buy_ix = match pumpswap::build_buy(pumpswap::BuyParams {
        pool: pool_address,
        pool_data: pool_data.clone(),
        user: keypair.pubkey(),
        base_amount_out,
        max_quote_amount_in: max_quote_in,
        fee_recipient_index,
    }) {
        Ok(ix) => ix,
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("Build buy ix failed: {:?}", e)).await;
            return;
        }
    };
    instructions.push(buy_ix);

    // Sign and send.
    let recent_blockhash = match rpc_client.get_latest_blockhash().await {
        Ok(bh) => bh,
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("Failed to get blockhash: {:?}", e)).await;
            return;
        }
    };

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[&keypair],
        recent_blockhash,
    );

    match rpc_client.send_and_confirm_transaction(&tx).await {
        Ok(sig) => {
            let sig_str = sig.to_string();
            info!("Buy successful: sig={} tokens={}", sig_str, base_amount_out);

            let position = Position {
                id: Uuid::new_v4().to_string(),
                pool: pool_address,
                base_mint,
                buy_price_lamports: buy_amount_lamports,
                base_amount: base_amount_out,
                bought_at: Utc::now(),
                status: PositionStatus::Active,
            };
            let log = TradeLog {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                action: TradeAction::Buy,
                pool: pool_address,
                base_mint,
                amount_sol: buy_amount_lamports as f64 / 1e9,
                amount_tokens: base_amount_out,
                tx_signature: Some(sig_str),
                error: None,
            };
            let mut s = state.write().await;
            s.positions.push(position);
            s.push_log(log);
        }
        Err(e) => {
            push_error_log(&state, pool_address, base_mint, buy_amount_lamports, format!("Buy tx failed: {:?}", e)).await;
        }
    }
}

/// Check all active positions and sell those that have hit the target multiplier.
pub async fn check_and_sell_positions(
    state: Arc<RwLock<AppState>>,
    rpc_client: Arc<RpcClient>,
    pool_address: Pubkey,
    current_quote_reserves: u64,
    current_base_reserves: u64,
) {
    let (sell_multiplier, slippage_bps, wallet_bytes) = {
        let s = state.read().await;
        let wallet_bytes = s.wallet.as_ref().map(|kp| kp.to_bytes().to_vec());
        (s.config.sell_multiplier, s.config.slippage_bps, wallet_bytes)
    };

    let wallet_bytes = match wallet_bytes {
        Some(b) => b,
        None => return,
    };

    // Collect positions eligible for selling.
    let positions_to_sell: Vec<(usize, Position)> = {
        let s = state.read().await;
        s.positions
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                if p.pool != pool_address || p.status != PositionStatus::Active {
                    return false;
                }
                // Check if current value >= buy_price * sell_multiplier
                let current_value = pumpswap::quote_out_for_exact_base_in(
                    current_base_reserves,
                    current_quote_reserves,
                    p.base_amount,
                    pumpswap::DEFAULT_FEE_BPS,
                )
                .unwrap_or(0);
                current_value >= (p.buy_price_lamports as f64 * sell_multiplier) as u64
            })
            .map(|(i, p)| (i, p.clone()))
            .collect()
    };

    for (idx, position) in positions_to_sell {
        // Mark as Selling.
        {
            let mut s = state.write().await;
            if let Some(p) = s.positions.get_mut(idx) {
                p.status = PositionStatus::Selling;
            }
        }

        if wallet_bytes.len() != 64 {
            error!("Invalid wallet bytes length");
            continue;
        }
        let secret: [u8; 32] = wallet_bytes[..32].try_into().unwrap();
        let keypair = Keypair::new_from_array(secret);

        let fee_recipient_index = rand::thread_rng().gen_range(0..FEE_RECIPIENT_COUNT);

        // Fetch pool data for sell.
        let pool_account = match rpc_client.get_account(&pool_address).await {
            Ok(a) => a,
            Err(e) => {
                error!("Failed to fetch pool for sell: {:?}", e);
                mark_position_failed(&state, idx).await;
                continue;
            }
        };

        let pool_data = match Pool::try_from_slice(&pool_account.data) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to deserialize pool for sell: {:?}", e);
                mark_position_failed(&state, idx).await;
                continue;
            }
        };

        let min_quote_out = match pumpswap::quote_out_for_exact_base_in(
            current_base_reserves,
            current_quote_reserves,
            position.base_amount,
            pumpswap::DEFAULT_FEE_BPS,
        ) {
            Ok(v) => match pumpswap::with_slippage_min(v, slippage_bps) {
                Ok(min) => min,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let sell_ix = match pumpswap::build_sell(pumpswap::SellParams {
            pool: pool_address,
            pool_data,
            user: keypair.pubkey(),
            base_amount_in: position.base_amount,
            min_quote_amount_out: min_quote_out,
            fee_recipient_index,
        }) {
            Ok(ix) => ix,
            Err(e) => {
                error!("Build sell ix failed: {:?}", e);
                mark_position_failed(&state, idx).await;
                continue;
            }
        };

        let recent_blockhash = match rpc_client.get_latest_blockhash().await {
            Ok(bh) => bh,
            Err(e) => {
                error!("Failed to get blockhash for sell: {:?}", e);
                mark_position_failed(&state, idx).await;
                continue;
            }
        };

        let tx = Transaction::new_signed_with_payer(
            &[sell_ix],
            Some(&keypair.pubkey()),
            &[&keypair],
            recent_blockhash,
        );

        match rpc_client.send_and_confirm_transaction(&tx).await {
            Ok(sig) => {
                let sig_str = sig.to_string();
                info!("Sell successful for position {}: sig={}", position.id, sig_str);
                let log = TradeLog {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    action: TradeAction::Sell,
                    pool: position.pool,
                    base_mint: position.base_mint,
                    amount_sol: min_quote_out as f64 / 1e9,
                    amount_tokens: position.base_amount,
                    tx_signature: Some(sig_str),
                    error: None,
                };
                let mut s = state.write().await;
                if let Some(p) = s.positions.get_mut(idx) {
                    p.status = PositionStatus::Sold;
                }
                s.push_log(log);
            }
            Err(e) => {
                error!("Sell tx failed for position {}: {:?}", position.id, e);
                let log = TradeLog {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    action: TradeAction::Error,
                    pool: position.pool,
                    base_mint: position.base_mint,
                    amount_sol: 0.0,
                    amount_tokens: position.base_amount,
                    tx_signature: None,
                    error: Some(format!("{:?}", e)),
                };
                let mut s = state.write().await;
                if let Some(p) = s.positions.get_mut(idx) {
                    p.status = PositionStatus::Failed;
                }
                s.push_log(log);
            }
        }
    }
}

async fn push_error_log(
    state: &Arc<RwLock<AppState>>,
    pool: Pubkey,
    base_mint: Pubkey,
    amount_lamports: u64,
    error_msg: String,
) {
    error!("{}", error_msg);
    let log = TradeLog {
        id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        action: TradeAction::Error,
        pool,
        base_mint,
        amount_sol: amount_lamports as f64 / 1e9,
        amount_tokens: 0,
        tx_signature: None,
        error: Some(error_msg),
    };
    let mut s = state.write().await;
    s.push_log(log);
}

async fn mark_position_failed(state: &Arc<RwLock<AppState>>, idx: usize) {
    let mut s = state.write().await;
    if let Some(p) = s.positions.get_mut(idx) {
        p.status = PositionStatus::Failed;
    }
}
