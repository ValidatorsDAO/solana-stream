use crate::state::{AppState, Position, PositionStatus, TradeAction, TradeLog};
use crate::wallet::keypair_from_bytes;
use crate::webhook::notify_discord;
use chrono::Utc;
use log::{error, info, warn};
use rand::Rng;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use std::{sync::Arc, time::Duration};
use tokio::{sync::RwLock, time::sleep};
use ultima_swap_pumpfun::{self as pumpswap, Pool};
use uuid::Uuid;

const FEE_RECIPIENT_COUNT: usize = 8;
const TX_FEE_RESERVE: u64 = 10_000_000; // 0.01 SOL
const POOL_FETCH_RETRIES: usize = 6;
const POOL_FETCH_RETRY_DELAY_MS: u64 = 500;

/// Process a detected create_pool event. Performs a buy if conditions are met.
pub async fn handle_new_pool(
    pool_address: Pubkey,
    base_mint: Pubkey,
    state: Arc<RwLock<AppState>>,
    rpc_client: Arc<RpcClient>,
) {
    // Read config first (needed for min_pool_sol check before notification).
    let (
        running,
        max_positions,
        buy_amount_lamports,
        slippage_bps,
        min_pool_sol,
        webhook_url,
        wallet_bytes,
    ) = {
        let s = state.read().await;
        let wallet_bytes = s.wallet.as_ref().map(|kp| kp.to_bytes().to_vec());
        (
            s.running,
            s.config.max_positions,
            s.config.buy_amount_lamports,
            s.config.slippage_bps,
            s.config.min_pool_sol_lamports,
            s.webhook_url.clone(),
            wallet_bytes,
        )
    };

    if !running {
        return;
    }

    {
        let s = state.read().await;
        if s.active_position_count() >= max_positions {
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

    let keypair = match keypair_from_bytes(&wallet_bytes) {
        Ok(kp) => kp,
        Err(e) => {
            error!("Keypair error: {:?}", e);
            return;
        }
    };

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
                "Insufficient balance: {} lamports (need {})",
                balance,
                buy_amount_lamports + TX_FEE_RESERVE
            ),
        )
        .await;
        return;
    }

    // Fetch pool account data with retries because Geyser often sees create_pool
    // before the RPC node can serve the newly created pool account.
    let pool_account = match fetch_pool_account_with_retry(&rpc_client, pool_address).await {
        Ok(a) => a,
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!(
                    "Fetch pool failed after {} retries ({} ms delay): {:?}",
                    POOL_FETCH_RETRIES, POOL_FETCH_RETRY_DELAY_MS, e,
                ),
            )
            .await;
            return;
        }
    };

    let pool_data = match Pool::try_from_slice(&pool_account.data) {
        Ok(p) => p,
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Deserialize pool failed: {:?}", e),
            )
            .await;
            return;
        }
    };

    let quote_vault_balance = match rpc_client
        .get_token_account_balance(&pool_data.pool_quote_token_account)
        .await
    {
        Ok(b) => b.amount.parse::<u64>().unwrap_or(0),
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Fetch quote vault failed: {:?}", e),
            )
            .await;
            return;
        }
    };

    // Check minimum pool SOL liquidity.
    if quote_vault_balance < min_pool_sol {
        info!(
            "Pool {} skipped: quote reserves {} < min_pool_sol {}",
            pool_address, quote_vault_balance, min_pool_sol
        );
        return;
    }

    // Notify via webhook + record in logs (only after min_pool_sol filter passes).
    {
        let mut s = state.write().await;
        let msg = format!(
            "🆕 Pool qualified — Pool: {} | Base Mint: {} | SOL reserves: {:.4} | Time: {}",
            pool_address,
            base_mint,
            quote_vault_balance as f64 / 1e9,
            Utc::now().to_rfc3339()
        );
        s.push_notification(pool_address, base_mint, msg.clone());
        if let Some(url) = &webhook_url {
            let discord_msg = format!(
                "🆕 **Pool Qualified for Trade**\n\
                 Pool: `{}`\n\
                 Base Mint: `{}`\n\
                 SOL Reserves: `{:.4} SOL`\n\
                 Timestamp: {}",
                pool_address,
                base_mint,
                quote_vault_balance as f64 / 1e9,
                Utc::now().to_rfc3339()
            );
            let url = url.clone();
            tokio::spawn(async move {
                notify_discord(&url, &discord_msg).await;
            });
        }
    }

    let base_vault_balance = match rpc_client
        .get_token_account_balance(&pool_data.pool_base_token_account)
        .await
    {
        Ok(b) => b.amount.parse::<u64>().unwrap_or(0),
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Fetch base vault failed: {:?}", e),
            )
            .await;
            return;
        }
    };

    let base_amount_out = match pumpswap::base_out_for_exact_quote_in(
        base_vault_balance,
        quote_vault_balance,
        buy_amount_lamports,
        pumpswap::DEFAULT_FEE_BPS,
    ) {
        Ok(a) => a,
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("AMM math error: {:?}", e),
            )
            .await;
            return;
        }
    };

    let max_quote_in = match pumpswap::with_slippage_max(buy_amount_lamports, slippage_bps) {
        Ok(v) => v,
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Slippage error: {:?}", e),
            )
            .await;
            return;
        }
    };

    let fee_idx = rand::thread_rng().gen_range(0..FEE_RECIPIENT_COUNT);

    info!(
        "Buying {} base atoms for max {} lamports on pool {}",
        base_amount_out, max_quote_in, pool_address
    );

    let mut instructions = vec![pumpswap::create_base_ata_if_needed(
        &keypair.pubkey(),
        &base_mint,
    )];

    let buy_ix = match pumpswap::build_buy(pumpswap::BuyParams {
        pool: pool_address,
        pool_data,
        user: keypair.pubkey(),
        base_amount_out,
        max_quote_amount_in: max_quote_in,
        fee_recipient_index: fee_idx,
    }) {
        Ok(ix) => ix,
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Build buy ix failed: {:?}", e),
            )
            .await;
            return;
        }
    };
    instructions.push(buy_ix);

    let recent_blockhash = match rpc_client.get_latest_blockhash().await {
        Ok(bh) => bh,
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Blockhash failed: {:?}", e),
            )
            .await;
            return;
        }
    };

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[&keypair],
        recent_blockhash,
    );

    // Use send_transaction (non-blocking, N2 fix) instead of send_and_confirm.
    match rpc_client.send_transaction(&tx).await {
        Ok(sig) => {
            let sig_str = sig.to_string();
            info!("Buy tx sent: sig={} tokens={}", sig_str, base_amount_out);

            let position_id = Uuid::new_v4().to_string();
            let position = Position {
                id: position_id.clone(),
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
                message: None,
            };
            let mut s = state.write().await;
            s.positions.insert(position_id, position);
            s.push_log(log);
        }
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Buy tx failed: {:?}", e),
            )
            .await;
        }
    }
}

/// Check all active positions for sell targets.
pub async fn check_and_sell_positions(
    state: Arc<RwLock<AppState>>,
    rpc_client: Arc<RpcClient>,
    pool_address: Pubkey,
    current_quote_reserves: u64,
    current_base_reserves: u64,
) {
    let (sell_multiplier, slippage_bps, wallet_bytes) = {
        let s = state.read().await;
        let wb = s.wallet.as_ref().map(|kp| kp.to_bytes().to_vec());
        (s.config.sell_multiplier, s.config.slippage_bps, wb)
    };

    let wallet_bytes = match wallet_bytes {
        Some(b) => b,
        None => return,
    };

    // B2 fix: collect IDs, not indices.
    let ids_to_sell: Vec<String> = {
        let s = state.read().await;
        s.positions
            .values()
            .filter(|p| {
                if p.pool != pool_address || p.status != PositionStatus::Active {
                    return false;
                }
                let current_value = pumpswap::quote_out_for_exact_base_in(
                    current_base_reserves,
                    current_quote_reserves,
                    p.base_amount,
                    pumpswap::DEFAULT_FEE_BPS,
                )
                .unwrap_or(0);
                current_value >= (p.buy_price_lamports as f64 * sell_multiplier) as u64
            })
            .map(|p| p.id.clone())
            .collect()
    };

    for id in ids_to_sell {
        // Mark as Selling.
        let position = {
            let mut s = state.write().await;
            match s.positions.get_mut(&id) {
                Some(p) => {
                    p.status = PositionStatus::Selling;
                    p.clone()
                }
                None => continue,
            }
        };

        let keypair = match keypair_from_bytes(&wallet_bytes) {
            Ok(kp) => kp,
            Err(e) => {
                error!("Keypair error: {:?}", e);
                continue;
            }
        };

        let fee_idx = rand::thread_rng().gen_range(0..FEE_RECIPIENT_COUNT);

        let pool_account = match rpc_client.get_account(&pool_address).await {
            Ok(a) => a,
            Err(e) => {
                error!("Fetch pool for sell failed: {:?}", e);
                mark_position_status(&state, &id, PositionStatus::Failed).await;
                continue;
            }
        };

        let pool_data = match Pool::try_from_slice(&pool_account.data) {
            Ok(p) => p,
            Err(e) => {
                error!("Deserialize pool for sell failed: {:?}", e);
                mark_position_status(&state, &id, PositionStatus::Failed).await;
                continue;
            }
        };

        let min_quote_out = match pumpswap::quote_out_for_exact_base_in(
            current_base_reserves,
            current_quote_reserves,
            position.base_amount,
            pumpswap::DEFAULT_FEE_BPS,
        ) {
            Ok(v) => pumpswap::with_slippage_min(v, slippage_bps).unwrap_or(0),
            Err(_) => continue,
        };

        let sell_ix = match pumpswap::build_sell(pumpswap::SellParams {
            pool: pool_address,
            pool_data,
            user: keypair.pubkey(),
            base_amount_in: position.base_amount,
            min_quote_amount_out: min_quote_out,
            fee_recipient_index: fee_idx,
        }) {
            Ok(ix) => ix,
            Err(e) => {
                error!("Build sell ix failed: {:?}", e);
                mark_position_status(&state, &id, PositionStatus::Failed).await;
                continue;
            }
        };

        let bh = match rpc_client.get_latest_blockhash().await {
            Ok(bh) => bh,
            Err(e) => {
                error!("Blockhash for sell failed: {:?}", e);
                mark_position_status(&state, &id, PositionStatus::Failed).await;
                continue;
            }
        };

        let tx = Transaction::new_signed_with_payer(
            &[sell_ix],
            Some(&keypair.pubkey()),
            &[&keypair],
            bh,
        );

        match rpc_client.send_transaction(&tx).await {
            Ok(sig) => {
                let sig_str = sig.to_string();
                info!("Sell tx sent for position {}: sig={}", id, sig_str);
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
                    message: None,
                };
                let mut s = state.write().await;
                if let Some(p) = s.positions.get_mut(&id) {
                    p.status = PositionStatus::Sold;
                }
                s.push_log(log);
            }
            Err(e) => {
                error!("Sell tx failed for position {}: {:?}", id, e);
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
                    message: None,
                };
                let mut s = state.write().await;
                if let Some(p) = s.positions.get_mut(&id) {
                    p.status = PositionStatus::Failed;
                }
                s.push_log(log);
            }
        }
    }
}

async fn fetch_pool_account_with_retry(
    rpc_client: &RpcClient,
    pool_address: Pubkey,
) -> anyhow::Result<solana_sdk::account::Account> {
    let mut last_error = None;

    for attempt in 0..POOL_FETCH_RETRIES {
        match rpc_client.get_account(&pool_address).await {
            Ok(account) => {
                if attempt > 0 {
                    info!(
                        "Pool account {} became available after {} retries",
                        pool_address, attempt
                    );
                }
                return Ok(account);
            }
            Err(error) => {
                let should_retry = format!("{error:?}").contains("AccountNotFound");
                last_error = Some(anyhow::anyhow!(error));
                if !should_retry || attempt + 1 == POOL_FETCH_RETRIES {
                    break;
                }
                sleep(Duration::from_millis(POOL_FETCH_RETRY_DELAY_MS)).await;
            }
        }
    }

    Err(last_error.expect("retry loop must record last error"))
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
        message: None,
    };
    let mut s = state.write().await;
    s.push_log(log);
}

async fn mark_position_status(state: &Arc<RwLock<AppState>>, id: &str, status: PositionStatus) {
    let mut s = state.write().await;
    if let Some(p) = s.positions.get_mut(id) {
        p.status = status;
    }
}
