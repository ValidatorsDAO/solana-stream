use crate::state::{persist_trade_log_to_redis, AppState, Position, PositionStatus, TradeAction, TradeLog};
use crate::wallet::keypair_from_bytes;
use crate::webhook::notify_discord;
use chrono::Utc;
use log::{error, info, warn};
use rand::Rng;
use solana_commitment_config::CommitmentConfig;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use std::{sync::Arc, time::Duration};
use tokio::{sync::RwLock, time::sleep};
use ultima_swap_pumpfun::{self as pumpswap, Pool};

/// Anchor discriminator for Pool: sha256("account:Pool")[..8]
const POOL_DISCRIMINATOR: [u8; 8] = [241, 154, 109, 4, 17, 177, 109, 188];
use uuid::Uuid;

const FEE_RECIPIENT_COUNT: usize = 8;
const TX_FEE_RESERVE: u64 = 10_000_000; // 0.01 SOL
const POOL_FETCH_RETRIES: usize = 10;
const POOL_FETCH_RETRY_DELAY_MS: u64 = 80;

/// Process a detected create_pool event. Performs a buy if conditions are met.
pub async fn handle_new_pool(
    pool_address: Pubkey,
    base_mint: Pubkey,
    state: Arc<RwLock<AppState>>,
    rpc_client: Arc<RpcClient>,
    send_rpc_client: Arc<RpcClient>,
) {
    // Read config first (needed for min_pool_sol check before notification).
    let (
        running,
        max_positions,
        buy_amount_lamports,
        slippage_bps,
        sell_multiplier,
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
            s.config.sell_multiplier,
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

    // Fetch pool account data with retries at confirmed commitment.
    // Geyser delivers create_pool before finalized confirmation, so we use confirmed.
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

    let pool_data = match deserialize_pool_lenient(&pool_account.data) {
        Ok(p) => p,
        Err(e) => {
            push_error_log_with_webhook(
                &state,
                &webhook_url,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Deserialize pool failed: {:?}", e),
            )
            .await;
            return;
        }
    };

    let quote_vault_balance = match fetch_token_balance_with_retry(
        &rpc_client, &pool_data.pool_quote_token_account,
    ).await {
        Ok(b) => b,
        Err(e) => {
            push_error_log_with_webhook(
                &state, &webhook_url, pool_address, base_mint, buy_amount_lamports,
                format!("Fetch quote vault failed after retries: {:?}", e),
            ).await;
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

    let base_vault_balance = match fetch_token_balance_with_retry(
        &rpc_client, &pool_data.pool_base_token_account,
    ).await {
        Ok(b) => b,
        Err(e) => {
            push_error_log_with_webhook(
                &state, &webhook_url, pool_address, base_mint, buy_amount_lamports,
                format!("Fetch base vault failed after retries: {:?}", e),
            ).await;
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

    // Determine graduated token's token program (SPL Token or Token-2022).
    // PumpSwap calls graduated token "quote_mint"; we call it "base_mint" in engine.
    let quote_token_program = match rpc_client.get_account(&base_mint).await {
        Ok(acct) => acct.owner,
        Err(e) => {
            push_error_log(
                &state, pool_address, base_mint, buy_amount_lamports,
                format!("Failed to fetch mint account for token program detection: {:?}", e),
            ).await;
            return;
        }
    };
    info!(
        "Graduated token {} uses token program {}",
        base_mint, quote_token_program
    );

    info!(
        "Buying {} base atoms for max {} lamports on pool {}",
        base_amount_out, max_quote_in, pool_address
    );

    // 1) Create graduated token ATA (PumpSwap's quote_mint)
    let mut instructions = vec![pumpswap::create_ata_if_needed(
        &keypair.pubkey(),
        &base_mint,
        &quote_token_program,
    )];

    // 2) Create WSOL ATA + wrap SOL for the buy
    let wsol_mint = pumpswap::WSOL_MINT;
    let wsol_ata = Pubkey::find_program_address(
        &[
            keypair.pubkey().as_ref(),
            pumpswap::TOKEN_PROGRAM.as_ref(),
            wsol_mint.as_ref(),
        ],
        &pumpswap::ASSOCIATED_TOKEN_PROGRAM,
    ).0;
    // CreateIdempotent for WSOL ATA
    instructions.push(pumpswap::create_ata_if_needed(
        &keypair.pubkey(),
        &wsol_mint,
        &pumpswap::TOKEN_PROGRAM,
    ));
    // Transfer SOL to WSOL ATA (SystemProgram.Transfer = instruction index 2, 12-byte data)
    {
        let transfer_data = {
            let mut d = vec![2, 0, 0, 0]; // instruction index 2 = Transfer (little-endian u32)
            d.extend_from_slice(&max_quote_in.to_le_bytes());
            d
        };
        instructions.push(Instruction {
            program_id: pumpswap::SYSTEM_PROGRAM,
            accounts: vec![
                AccountMeta::new(keypair.pubkey(), true),  // from
                AccountMeta::new(wsol_ata, false),         // to
            ],
            data: transfer_data,
        });
    }
    // SyncNative to update WSOL balance
    instructions.push(Instruction {
        program_id: pumpswap::TOKEN_PROGRAM,
        accounts: vec![AccountMeta::new(wsol_ata, false)],
        data: vec![17], // SyncNative instruction discriminator
    });

    // PumpSwap naming: "sell" = spend base(WSOL), receive quote(graduated)
    // This is what we want when "buying" the graduated token.
    let sell_ix = match pumpswap::build_sell(pumpswap::SellParams {
        pool: pool_address,
        pool_data,
        user: keypair.pubkey(),
        base_amount_in: buy_amount_lamports,
        min_quote_amount_out: base_amount_out, // min graduated tokens to receive
        fee_recipient_index: fee_idx,
        quote_token_program,
    }) {
        Ok(ix) => ix,
        Err(e) => {
            push_error_log(
                &state,
                pool_address,
                base_mint,
                buy_amount_lamports,
                format!("Build sell(buy graduated) ix failed: {:?}", e),
            )
            .await;
            return;
        }
    };
    instructions.push(sell_ix);

    // Close WSOL ATA after buy to reclaim rent + leftover SOL
    instructions.push(Instruction {
        program_id: pumpswap::TOKEN_PROGRAM,
        accounts: vec![
            AccountMeta::new(wsol_ata, false),                          // account to close
            AccountMeta::new(keypair.pubkey(), false),                  // destination
            AccountMeta::new_readonly(keypair.pubkey(), true),          // authority
        ],
        data: vec![9], // CloseAccount instruction discriminator
    });

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

    // Skip preflight simulation — the pool exists at confirmed level but
    // preflight runs at finalized, causing AccountNotInitialized (0xbc4).
    let send_cfg = RpcSendTransactionConfig {
        skip_preflight: true,
        ..Default::default()
    };
    match send_rpc_client.send_transaction_with_config(&tx, send_cfg).await {
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
                quote_token_program,
            };
            let log = TradeLog {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                action: TradeAction::Buy,
                pool: pool_address,
                base_mint,
                amount_sol: buy_amount_lamports as f64 / 1e9,
                amount_tokens: base_amount_out,
                tx_signature: Some(sig_str.clone()),
                error: None,
                message: None,
            };
            let mut s = state.write().await;
            s.positions.insert(position_id, position);
            s.push_log(log.clone());

            // Persist buy log to Redis
            if let Some(ref client) = s.redis_client {
                persist_trade_log_to_redis(client, &log);
            }

            // Discord notification for buy
            if let Some(url) = &webhook_url {
                let discord_msg = format!(
                    "💰 **Buy Executed**\n\
                     Pool: `{}`\n\
                     Base Mint: `{}`\n\
                     Amount: `{:.4} SOL`\n\
                     Tokens: `{}`\n\
                     Tx: `{}`",
                    pool_address, base_mint,
                    buy_amount_lamports as f64 / 1e9,
                    base_amount_out, sig_str,
                );
                let url = url.clone();
                tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
            }

            // Immediately spawn sell monitoring loop for this position.
            // Polls pool reserves every 500ms and sells as soon as target is hit.
            drop(s); // release write lock before spawning
            let state_sell = state.clone();
            let rpc_sell = rpc_client.clone();
            let send_rpc_sell = send_rpc_client.clone();
            tokio::spawn(async move {
                info!("Sell monitor started for pool {} (target {}x)", pool_address, sell_multiplier);
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                    // Check if position is still active
                    let still_active = {
                        let s = state_sell.read().await;
                        s.positions.values().any(|p| {
                            p.pool == pool_address && p.status == PositionStatus::Active
                        })
                    };
                    if !still_active {
                        info!("Sell monitor exiting for pool {} (position no longer active)", pool_address);
                        break;
                    }

                    // Fetch current pool reserves
                    let pool_account = match rpc_sell
                        .get_account_with_commitment(
                            &pool_address,
                            solana_commitment_config::CommitmentConfig::confirmed(),
                        )
                        .await
                    {
                        Ok(resp) => match resp.value {
                            Some(a) => a,
                            None => continue,
                        },
                        Err(_) => continue,
                    };
                    let pool_data = match deserialize_pool_lenient(&pool_account.data) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };

                    // Fetch reserves from vault accounts
                    let quote_reserves = match fetch_token_balance_with_retry(
                        &rpc_sell, &pool_data.pool_quote_token_account,
                    ).await {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let base_reserves = match fetch_token_balance_with_retry(
                        &rpc_sell, &pool_data.pool_base_token_account,
                    ).await {
                        Ok(b) => b,
                        Err(_) => continue,
                    };

                    // Run sell check with current reserves
                    check_and_sell_positions(
                        state_sell.clone(),
                        rpc_sell.clone(),
                        send_rpc_sell.clone(),
                        pool_address,
                        quote_reserves,
                        base_reserves,
                    ).await;
                }
            });
        }
        Err(e) => {
            push_error_log_with_webhook(
                &state,
                &webhook_url,
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
    send_rpc_client: Arc<RpcClient>,
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

        // PumpSwap naming: "buy" = spend quote(graduated), receive base(WSOL)
        // This is what we want when selling our graduated tokens for SOL.
        let sell_ix = match pumpswap::build_buy(pumpswap::BuyParams {
            pool: pool_address,
            pool_data,
            user: keypair.pubkey(),
            base_amount_out: min_quote_out,  // min WSOL to receive
            max_quote_amount_in: position.base_amount, // graduated tokens to spend
            fee_recipient_index: fee_idx,
            quote_token_program: position.quote_token_program,
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

        let sell_cfg = RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        };
        match send_rpc_client.send_transaction_with_config(&tx, sell_cfg).await {
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
                    tx_signature: Some(sig_str.clone()),
                    error: None,
                    message: None,
                };
                let mut s = state.write().await;
                if let Some(p) = s.positions.get_mut(&id) {
                    p.status = PositionStatus::Sold;
                }
                s.push_log(log.clone());

                // Persist sell log to Redis
                if let Some(ref client) = s.redis_client {
                    persist_trade_log_to_redis(client, &log);
                }

                // Discord notification for sell
                if let Some(url) = &s.webhook_url {
                    let discord_msg = format!(
                        "🔔 **Sell Executed**\n\
                         Pool: `{}`\n\
                         Base Mint: `{}`\n\
                         SOL out: `{:.4}`\n\
                         Tokens sold: `{}`\n\
                         Tx: `{}`",
                        position.pool, position.base_mint,
                        min_quote_out as f64 / 1e9,
                        position.base_amount, sig_str,
                    );
                    let url = url.clone();
                    tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
                }
            }
            Err(e) => {
                let err_msg = format!("Sell tx failed for position {}: {:?}", id, e);
                error!("{}", err_msg);
                let log = TradeLog {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    action: TradeAction::Error,
                    pool: position.pool,
                    base_mint: position.base_mint,
                    amount_sol: 0.0,
                    amount_tokens: position.base_amount,
                    tx_signature: None,
                    error: Some(err_msg.clone()),
                    message: None,
                };
                let mut s = state.write().await;
                if let Some(p) = s.positions.get_mut(&id) {
                    p.status = PositionStatus::Failed;
                }
                s.push_log(log);

                // Discord notification for sell error
                if let Some(url) = &s.webhook_url {
                    let discord_msg = format!("❌ **Sell Failed**\nPool: `{}`\n{}", position.pool, err_msg);
                    let url = url.clone();
                    tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
                }
            }
        }
    }
}

async fn fetch_pool_account_with_retry(
    rpc_client: &RpcClient,
    pool_address: Pubkey,
) -> anyhow::Result<solana_sdk::account::Account> {
    let mut last_error = None;
    let commitment = CommitmentConfig::confirmed();

    for attempt in 0..POOL_FETCH_RETRIES {
        match rpc_client
            .get_account_with_commitment(&pool_address, commitment)
            .await
        {
            Ok(response) => {
                if let Some(account) = response.value {
                    if attempt > 0 {
                        info!(
                            "Pool account {} available after {} retries (~{} ms)",
                            pool_address,
                            attempt,
                            attempt as u64 * POOL_FETCH_RETRY_DELAY_MS,
                        );
                    }
                    return Ok(account);
                }
                // RPC returned null value — not yet visible at confirmed.
                last_error = Some(anyhow::anyhow!("AccountNotFound at confirmed: {}", pool_address));
            }
            Err(error) => {
                let is_not_found = format!("{error:?}").contains("AccountNotFound");
                last_error = Some(anyhow::anyhow!(error));
                if !is_not_found {
                    break; // Non-retryable error
                }
            }
        }
        if attempt + 1 < POOL_FETCH_RETRIES {
            sleep(Duration::from_millis(POOL_FETCH_RETRY_DELAY_MS)).await;
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
    push_error_log_with_webhook(state, &None, pool, base_mint, amount_lamports, error_msg).await;
}

async fn push_error_log_with_webhook(
    state: &Arc<RwLock<AppState>>,
    webhook_url: &Option<String>,
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
        error: Some(error_msg.clone()),
        message: None,
    };
    let mut s = state.write().await;
    s.push_log(log);

    // Determine webhook: prefer explicit param, fall back to state.
    let url = webhook_url.as_ref().or(s.webhook_url.as_ref());
    if let Some(url) = url {
        let discord_msg = format!(
            "❌ **Error**\nPool: `{}`\nBase Mint: `{}`\n```{}```",
            pool, base_mint, error_msg
        );
        let url = url.clone();
        tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
    }
}

/// Fetch token account balance with retries at confirmed commitment.
async fn fetch_token_balance_with_retry(
    rpc_client: &RpcClient,
    token_account: &Pubkey,
) -> anyhow::Result<u64> {
    let mut last_error = None;
    let commitment = CommitmentConfig::confirmed();
    for attempt in 0..POOL_FETCH_RETRIES {
        match rpc_client
            .get_token_account_balance_with_commitment(token_account, commitment)
            .await
        {
            Ok(response) => return Ok(response.value.amount.parse::<u64>().unwrap_or(0)),
            Err(e) => {
                let msg = format!("{e:?}");
                let is_not_found = msg.contains("could not find account") || msg.contains("AccountNotFound");
                last_error = Some(anyhow::anyhow!(e));
                if !is_not_found {
                    break;
                }
            }
        }
        if attempt + 1 < POOL_FETCH_RETRIES {
            sleep(Duration::from_millis(POOL_FETCH_RETRY_DELAY_MS)).await;
        }
    }
    Err(last_error.expect("retry loop must record last error"))
}

/// Lenient Pool deserialization that tolerates trailing bytes.
/// PumpSwap may add new fields; we only need the known prefix.
pub fn deserialize_pool_lenient(data: &[u8]) -> Result<Pool, std::io::Error> {
    if data.len() < 8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Account data too short",
        ));
    }
    if data[..8] != POOL_DISCRIMINATOR {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid Pool discriminator",
        ));
    }
    solana_sdk::borsh1::try_from_slice_unchecked::<Pool>(&data[8..]).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })
}

async fn mark_position_status(state: &Arc<RwLock<AppState>>, id: &str, status: PositionStatus) {
    let mut s = state.write().await;
    if let Some(p) = s.positions.get_mut(id) {
        p.status = status;
    }
}

/// WSOL mint address.
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

/// Restore positions from wallet token holdings on startup.
///
/// Scans both Token and Token-2022 accounts for non-zero balances,
/// finds the corresponding PumpSwap pool via `getProgramAccounts`,
/// and creates Active positions with sell monitors.
pub async fn restore_positions_from_wallet(
    state: Arc<RwLock<AppState>>,
    rpc_client: Arc<RpcClient>,
    send_rpc_client: Arc<RpcClient>,
) {
    let wallet_pubkey = {
        let s = state.read().await;
        match &s.wallet {
            Some(kp) => kp.pubkey(),
            None => {
                warn!("No wallet loaded, skipping position restore");
                return;
            }
        }
    };

    info!("Restoring positions from wallet {} ...", wallet_pubkey);

    let token_programs = [
        Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
        Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
    ];

    let mut restored = 0u32;

    for token_program in &token_programs {
        let accounts = match rpc_client
            .get_token_accounts_by_owner(
                &wallet_pubkey,
                solana_rpc_client_api::request::TokenAccountsFilter::ProgramId(*token_program),
            )
            .await
        {
            Ok(a) => a,
            Err(e) => {
                warn!("Failed to fetch token accounts for {:?}: {:?}", token_program, e);
                continue;
            }
        };

        for keyed in &accounts {
            // Parse the account data to get mint + amount
            let data = match &keyed.account.data {
                solana_account_decoder::UiAccountData::Json(parsed) => parsed,
                _ => continue,
            };
            let info = match data.parsed.get("info") {
                Some(v) => v,
                None => continue,
            };
            let mint_str = match info.get("mint").and_then(|v| v.as_str()) {
                Some(m) => m,
                None => continue,
            };
            // Skip WSOL
            if mint_str == WSOL_MINT {
                continue;
            }
            let amount_str = match info
                .get("tokenAmount")
                .and_then(|v| v.get("amount"))
                .and_then(|v| v.as_str())
            {
                Some(a) => a,
                None => continue,
            };
            let amount: u64 = match amount_str.parse() {
                Ok(a) if a > 0 => a,
                _ => continue,
            };

            let token_mint = match Pubkey::try_from(mint_str) {
                Ok(p) => p,
                Err(_) => continue,
            };

            info!("Found token {} amount={} — searching for PumpSwap pool...", mint_str, amount);

            // Find PumpSwap pool for this token.
            // PumpSwap pools have base_mint (offset 11) or quote_mint (offset 43).
            // On-chain: base_mint = WSOL, quote_mint = graduated token.
            // quote_mint is at discriminator(8) + pool_bump(1) + index(2) + creator(32) + base_mint(32) = offset 75
            let wsol_mint = Pubkey::from_str_const(WSOL_MINT);
            let pump_amm = Pubkey::from_str_const("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");

            // Filter: quote_mint = token_mint (at offset 75)
            use solana_rpc_client_api::filter::{Memcmp, RpcFilterType};
            use solana_rpc_client_api::config::RpcProgramAccountsConfig;
            use solana_account_decoder::UiAccountEncoding;

            let filters = vec![
                RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, POOL_DISCRIMINATOR.to_vec())),
                RpcFilterType::Memcmp(Memcmp::new_raw_bytes(75, token_mint.to_bytes().to_vec())),
            ];

            let config = RpcProgramAccountsConfig {
                filters: Some(filters),
                account_config: solana_rpc_client_api::config::RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64),
                    commitment: Some(CommitmentConfig::confirmed()),
                    ..Default::default()
                },
                ..Default::default()
            };

            let pool_accounts = match rpc_client
                .get_program_accounts_with_config(&pump_amm, config)
                .await
            {
                Ok(a) => a,
                Err(e) => {
                    warn!("getProgramAccounts failed for mint {}: {:?}", mint_str, e);
                    continue;
                }
            };

            if pool_accounts.is_empty() {
                // Try base_mint = token_mint (offset 43)
                let filters2 = vec![
                    RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, POOL_DISCRIMINATOR.to_vec())),
                    RpcFilterType::Memcmp(Memcmp::new_raw_bytes(43, token_mint.to_bytes().to_vec())),
                ];
                let config2 = RpcProgramAccountsConfig {
                    filters: Some(filters2),
                    account_config: solana_rpc_client_api::config::RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64),
                        commitment: Some(CommitmentConfig::confirmed()),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let pool_accounts2 = match rpc_client
                    .get_program_accounts_with_config(&pump_amm, config2)
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        warn!("getProgramAccounts (base_mint) failed for mint {}: {:?}", mint_str, e);
                        continue;
                    }
                };
                if pool_accounts2.is_empty() {
                    warn!("No PumpSwap pool found for mint {}", mint_str);
                    continue;
                }
                // Use the first pool found
                let (pool_address, pool_account) = &pool_accounts2[0];
                if let Err(e) = restore_single_position(
                    &state, &rpc_client, &send_rpc_client,
                    *pool_address, &pool_account.data, token_mint, amount, *token_program,
                ).await {
                    warn!("Failed to restore position for mint {} (base): {:?}", mint_str, e);
                } else {
                    restored += 1;
                }
            } else {
                let (pool_address, pool_account) = &pool_accounts[0];
                if let Err(e) = restore_single_position(
                    &state, &rpc_client, &send_rpc_client,
                    *pool_address, &pool_account.data, token_mint, amount, *token_program,
                ).await {
                    warn!("Failed to restore position for mint {} (quote): {:?}", mint_str, e);
                } else {
                    restored += 1;
                }
            }
        }
    }

    info!("Position restore complete: {} positions restored", restored);
}

async fn restore_single_position(
    state: &Arc<RwLock<AppState>>,
    rpc_client: &Arc<RpcClient>,
    send_rpc_client: &Arc<RpcClient>,
    pool_address: Pubkey,
    pool_data_raw: &[u8],
    token_mint: Pubkey,
    token_amount: u64,
    quote_token_program: Pubkey,
) -> Result<(), String> {
    let pool_data = deserialize_pool_lenient(pool_data_raw)
        .map_err(|e| format!("Pool deserialize: {:?}", e))?;

    // Fetch current reserves to estimate current value as buy_price
    let quote_reserves = fetch_token_balance_with_retry(rpc_client, &pool_data.pool_quote_token_account)
        .await
        .map_err(|e| format!("Fetch quote reserves: {:?}", e))?;
    let base_reserves = fetch_token_balance_with_retry(rpc_client, &pool_data.pool_base_token_account)
        .await
        .map_err(|e| format!("Fetch base reserves: {:?}", e))?;

    // Estimate the current SOL value of our token holdings.
    // Use this as buy_price so sell_multiplier applies from current price.
    let current_value = pumpswap::quote_out_for_exact_base_in(
        base_reserves,
        quote_reserves,
        token_amount,
        pumpswap::DEFAULT_FEE_BPS,
    )
    .unwrap_or(0);

    let position_id = Uuid::new_v4().to_string();
    let position = Position {
        id: position_id.clone(),
        pool: pool_address,
        base_mint: token_mint,
        buy_price_lamports: current_value, // current value as baseline
        base_amount: token_amount,
        bought_at: Utc::now(),
        status: PositionStatus::Active,
        quote_token_program,
    };

    info!(
        "Restored position: pool={} mint={} amount={} est_value={} lamports",
        pool_address, token_mint, token_amount, current_value
    );

    let sell_multiplier = {
        let mut s = state.write().await;
        s.positions.insert(position_id.clone(), position);
        s.push_notification(
            pool_address,
            token_mint,
            format!("Position restored from wallet: {} tokens, est. value {} lamports", token_amount, current_value),
        );
        s.config.sell_multiplier
    };

    // Spawn sell monitor loop (same as post-buy)
    let state_sell = state.clone();
    let rpc_sell = rpc_client.clone();
    let send_rpc_sell = send_rpc_client.clone();
    tokio::spawn(async move {
        info!("Sell monitor started for restored position pool {} (target {}x)", pool_address, sell_multiplier);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let still_active = {
                let s = state_sell.read().await;
                s.positions.values().any(|p| {
                    p.pool == pool_address && p.status == PositionStatus::Active
                })
            };
            if !still_active {
                info!("Sell monitor exiting for restored pool {} (position no longer active)", pool_address);
                break;
            }

            let pool_account = match rpc_sell
                .get_account_with_commitment(
                    &pool_address,
                    CommitmentConfig::confirmed(),
                )
                .await
            {
                Ok(resp) => match resp.value {
                    Some(a) => a,
                    None => continue,
                },
                Err(_) => continue,
            };
            let pool_data = match deserialize_pool_lenient(&pool_account.data) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let quote_reserves = match fetch_token_balance_with_retry(
                &rpc_sell, &pool_data.pool_quote_token_account,
            ).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            let base_reserves = match fetch_token_balance_with_retry(
                &rpc_sell, &pool_data.pool_base_token_account,
            ).await {
                Ok(b) => b,
                Err(_) => continue,
            };

            check_and_sell_positions(
                state_sell.clone(),
                rpc_sell.clone(),
                send_rpc_sell.clone(),
                pool_address,
                quote_reserves,
                base_reserves,
            ).await;
        }
    });

    Ok(())
}
