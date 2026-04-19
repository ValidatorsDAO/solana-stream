use crate::state::{persist_trade_log_to_redis, AppState, Position, PositionStatus, TradeAction, TradeLog};
use crate::wallet::keypair_from_bytes;
use crate::i18n;
use crate::webhook::notify_discord;
use chrono::Utc;
use log::{error, info, warn};
use rand::Rng;
use solana_commitment_config::CommitmentConfig;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
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

/// Confirm a transaction on-chain. Returns:
/// - Ok(true) if confirmed and successful
/// - Ok(false) if confirmed but failed (on-chain error)
/// - Err(msg) if timeout or RPC error
async fn confirm_transaction(rpc_client: &RpcClient, sig: &Signature, max_polls: u32) -> Result<bool, String> {
    for _ in 0..max_polls {
        sleep(Duration::from_millis(500)).await;
        match rpc_client.get_signature_statuses(&[*sig]).await {
            Ok(resp) => {
                if let Some(Some(status)) = resp.value.first() {
                    if status.satisfies_commitment(CommitmentConfig::confirmed()) {
                        return Ok(status.err.is_none());
                    }
                }
            }
            Err(e) => {
                warn!("getSignatureStatuses error for {}: {:?}", sig, e);
            }
        }
    }
    Err(format!("Tx {} not confirmed after {} polls", sig, max_polls))
}

/// Fetch the actual SOL change for a wallet from a confirmed transaction.
/// Returns the lamport difference (post - pre) for account index 0 (fee payer).
async fn fetch_actual_sol_change(rpc_client: &RpcClient, sig: &Signature) -> Option<i64> {
    match rpc_client.get_transaction_with_config(
        sig,
        solana_rpc_client_api::config::RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        },
    ).await {
        Ok(tx) => {
            let meta = tx.transaction.meta?;
            // Account index 0 = fee payer (our wallet). This holds for
            // self-signed transactions; multi-signer TXs would need
            // explicit index lookup.
            let pre = *meta.pre_balances.first()?;
            let post = *meta.post_balances.first()?;
            Some(post as i64 - pre as i64)
        }
        Err(e) => {
            warn!("Failed to fetch tx details for {}: {:?}", sig, e);
            None
        }
    }
}

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
        let needed = buy_amount_lamports + TX_FEE_RESERVE;
        let wallet_pubkey = keypair.pubkey();
        let error_msg = format!(
            "Insufficient balance: {} lamports (need {})",
            balance, needed
        );
        error!("{}", error_msg);
        {
            let mut s = state.write().await;
            s.push_log(TradeLog {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                action: TradeAction::Error,
                pool: pool_address,
                base_mint,
                amount_sol: buy_amount_lamports as f64 / 1e9,
                amount_tokens: 0,
                tx_signature: None,
                error: Some(error_msg.clone()),
                message: None,
            });
        }
        if let Some(url) = webhook_url.clone() {
            let discord_msg = i18n::insufficient_balance(
                &pool_address.to_string(),
                &base_mint.to_string(),
                &wallet_pubkey.to_string(),
                balance as f64 / 1e9,
                needed as f64 / 1e9,
            );
            tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
        }
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

    // Fetch WSOL (base) vault first — needed for min_pool_sol check
    let base_vault_balance = match fetch_token_balance_with_retry(
        &rpc_client, &pool_data.pool_base_token_account,
    ).await {
        Ok(b) => b,
        Err(e) => {
            push_error_log_with_webhook(
                &state, &webhook_url, pool_address, base_mint, buy_amount_lamports,
                format!("Fetch base vault (WSOL) failed after retries: {:?}", e),
            ).await;
            return;
        }
    };

    // Check minimum pool SOL liquidity (base vault = WSOL)
    if base_vault_balance < min_pool_sol {
        info!(
            "Pool {} skipped: WSOL reserves {} < min_pool_sol {}",
            pool_address, base_vault_balance, min_pool_sol
        );
        return;
    }

    let quote_vault_balance = match fetch_token_balance_with_retry(
        &rpc_client, &pool_data.pool_quote_token_account,
    ).await {
        Ok(b) => b,
        Err(e) => {
            push_error_log_with_webhook(
                &state, &webhook_url, pool_address, base_mint, buy_amount_lamports,
                format!("Fetch quote vault (graduated) failed after retries: {:?}", e),
            ).await;
            return;
        }
    };

    // Notify via webhook + record in logs
    {
        let mut s = state.write().await;
        let pool_str = pool_address.to_string();
        let mint_str = base_mint.to_string();
        let sol = base_vault_balance as f64 / 1e9;
        let ts = Utc::now().to_rfc3339();
        let msg = i18n::pool_qualified_log(&pool_str, &mint_str, sol, &ts);
        s.push_notification(pool_address, base_mint, msg);
        if let Some(url) = &webhook_url {
            let discord_msg = i18n::pool_qualified_webhook(&pool_str, &mint_str, sol, &ts);
            let url = url.clone();
            tokio::spawn(async move {
                notify_discord(&url, &discord_msg).await;
            });
        }
    }

    // PumpSwap: base=WSOL, quote=graduated. We spend WSOL (base_in) to get graduated (quote_out).
    let base_amount_out = match pumpswap::quote_out_for_exact_base_in(
        base_vault_balance,    // WSOL reserves (base)
        quote_vault_balance,   // graduated reserves (quote)
        buy_amount_lamports,   // WSOL to spend (base_in)
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
        min_quote_amount_out: pumpswap::with_slippage_min(base_amount_out, slippage_bps).unwrap_or(0), // min graduated tokens (with slippage)
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

            match confirm_transaction(&rpc_client, &sig, 30).await {
                Ok(true) => {
                    info!("Buy tx confirmed: sig={}", sig_str);

                    // Fetch actual token balance from wallet (AMM may give more than min estimate)
                    let actual_tokens = check_remaining_token_balance(
                        &rpc_client, &keypair.pubkey(), &base_mint, &quote_token_program,
                    ).await;
                    let final_amount = if actual_tokens > 0 { actual_tokens } else { base_amount_out };
                    if actual_tokens != base_amount_out {
                        info!("Buy actual tokens: {} (estimated: {})", actual_tokens, base_amount_out);
                    }

                    let position_id = Uuid::new_v4().to_string();
                    let position = Position {
                        id: position_id.clone(),
                        pool: pool_address,
                        base_mint,
                        buy_price_lamports: buy_amount_lamports,
                        base_amount: final_amount,
                        bought_at: Utc::now(),
                        status: PositionStatus::Active,
                        total_sell_lamports: 0,
                        quote_token_program,
                    };
                    let log = TradeLog {
                        id: Uuid::new_v4().to_string(),
                        timestamp: Utc::now(),
                        action: TradeAction::Buy,
                        pool: pool_address,
                        base_mint,
                        amount_sol: buy_amount_lamports as f64 / 1e9,
                        amount_tokens: final_amount,
                        tx_signature: Some(sig_str.clone()),
                        error: None,
                        message: None,
                    };
                    let mut s = state.write().await;
                    s.positions.insert(position_id, position);
                    s.push_log(log.clone());
                    if let Some(ref client) = s.redis_client {
                        persist_trade_log_to_redis(client, &log);
                    }
                    // Discord: Buy Confirmed
                    if let Some(url) = &webhook_url {
                        let discord_msg = i18n::buy_confirmed(
                            &pool_address.to_string(),
                            &base_mint.to_string(),
                            buy_amount_lamports as f64 / 1e9,
                            final_amount,
                            &sig_str,
                        );
                        let url = url.clone();
                        tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
                    }
                    // Spawn sell monitoring loop for this position.
                    drop(s);
                    let state_sell = state.clone();
                    let rpc_sell = rpc_client.clone();
                    let send_rpc_sell = send_rpc_client.clone();
                    tokio::spawn(async move {
                        info!("Sell monitor started for pool {} (target {}x)", pool_address, sell_multiplier);
                        loop {
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
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
                }
                Ok(false) => {
                    // Buy failed on-chain (slippage, etc). Only tx fee lost (~0.000005 SOL).
                    // Log internally but don't spam Discord.
                    push_error_log(
                        &state, pool_address, base_mint, buy_amount_lamports,
                        format!("Buy tx confirmed but failed on-chain: {}", sig_str),
                    ).await;
                }
                Err(timeout_msg) => {
                    warn!("{}", timeout_msg);
                    push_error_log_with_webhook(
                        &state, &webhook_url, pool_address, base_mint, buy_amount_lamports,
                        format!("Buy tx confirmation timeout (unknown status): {}", sig_str),
                    ).await;
                }
            }
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
    let (sell_multiplier, slippage_bps, sell_timeout_secs, exit_pool_sol_lamports, wallet_bytes) = {
        let s = state.read().await;
        let wb = s.wallet.as_ref().map(|kp| kp.to_bytes().to_vec());
        (
            s.config.sell_multiplier,
            s.config.slippage_bps,
            s.config.sell_timeout_secs,
            s.config.exit_pool_sol_lamports,
            wb,
        )
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
                let elapsed_secs = (Utc::now() - p.bought_at).num_seconds().max(0) as u64;
                let timed_out = sell_timeout_secs > 0 && elapsed_secs >= sell_timeout_secs;
                let low_liquidity = exit_pool_sol_lamports > 0 && current_base_reserves <= exit_pool_sol_lamports;

                // If already partially sold, always sell remaining (drain mode)
                if p.total_sell_lamports > 0 {
                    info!("Drain mode: position {} total_sell={}", p.id, p.total_sell_lamports);
                    return true;
                }
                // Forced retreat modes
                if timed_out || low_liquidity {
                    info!(
                        "Forced exit: pos={} timed_out={} elapsed={}s low_liquidity={} base_r={} threshold={} amount={}",
                        p.id, timed_out, elapsed_secs, low_liquidity, current_base_reserves, exit_pool_sol_lamports, p.base_amount
                    );
                    return true;
                }
                // PumpSwap: base=WSOL, quote=graduated. We sell graduated (quote_in) for WSOL (base_out).
                let current_value = pumpswap::base_out_for_exact_quote_in(
                    current_base_reserves,   // WSOL reserves
                    current_quote_reserves,  // graduated token reserves
                    p.base_amount,           // our graduated tokens (quote_in)
                    pumpswap::DEFAULT_FEE_BPS,
                )
                .unwrap_or(0);
                let target = (p.buy_price_lamports as f64 * sell_multiplier) as u64;
                let should_sell = current_value >= target;
                if !should_sell {
                    info!("Sell check: pos={} val={} target={} base_r={} quote_r={} amount={}",
                        p.id, current_value, target, current_base_reserves, current_quote_reserves, p.base_amount);
                }
                should_sell
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

        let pool_data = match deserialize_pool_lenient(&pool_account.data) {
            Ok(p) => p,
            Err(e) => {
                error!("Deserialize pool for sell failed: {:?}", e);
                mark_position_status(&state, &id, PositionStatus::Failed).await;
                continue;
            }
        };

        let elapsed_secs = (Utc::now() - position.bought_at).num_seconds().max(0) as u64;
        let timed_out = sell_timeout_secs > 0 && elapsed_secs >= sell_timeout_secs;
        let low_liquidity = exit_pool_sol_lamports > 0 && current_base_reserves <= exit_pool_sol_lamports;
        let forced_reason = if timed_out && low_liquidity {
            Some(format!("timeout {}s + low liquidity {:.6} SOL", elapsed_secs, current_base_reserves as f64 / 1e9))
        } else if timed_out {
            Some(format!("timeout {}s", elapsed_secs))
        } else if low_liquidity {
            Some(format!("low liquidity {:.6} SOL", current_base_reserves as f64 / 1e9))
        } else {
            None
        };

        // base_out_for_exact_quote_in: how much WSOL we get for our graduated tokens
        let min_quote_out = match pumpswap::base_out_for_exact_quote_in(
            current_base_reserves,   // WSOL reserves
            current_quote_reserves,  // graduated token reserves
            position.base_amount,    // our graduated tokens (quote_in)
            pumpswap::DEFAULT_FEE_BPS,
        ) {
            Ok(v) => pumpswap::with_slippage_min(v, slippage_bps).unwrap_or(0),
            Err(_) => continue,
        };

        // Skip dust / no-liquidity: if expected output is 0 lamports, burn + close ATA
        if min_quote_out == 0 {
            let retreat_reason = forced_reason.clone().unwrap_or_else(|| "dust / zero output".to_string());
            info!("Position {} retreat burn: {} — closing ATA", id, retreat_reason);
            let (close_ok, burn_sig) = burn_and_close_ata(
                &rpc_client, &send_rpc_client, &keypair,
                &position.base_mint, &position.quote_token_program,
            ).await;

            let buy_sol = position.buy_price_lamports as f64 / 1e9;
            let (total_sell, webhook_url_close, redis_client_opt) = {
                let mut s = state.write().await;
                let ts = s.positions.get(&id).map(|p| p.total_sell_lamports).unwrap_or(0);
                if let Some(p) = s.positions.get_mut(&id) {
                    p.status = if close_ok { PositionStatus::Closed } else { PositionStatus::Sold };
                }
                let note = TradeLog {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    action: TradeAction::Notification,
                    pool: position.pool,
                    base_mint: position.base_mint,
                    amount_sol: 0.0,
                    amount_tokens: position.base_amount,
                    tx_signature: None,
                    error: None,
                    message: Some(format!("Retreat burn: {} | ATA {}", retreat_reason, if close_ok { "closed" } else { "close failed" })),
                };
                s.push_log(note.clone());
                if let Some(ref client) = s.redis_client {
                    persist_trade_log_to_redis(client, &note);
                }
                (ts, s.webhook_url.clone(), s.redis_client.clone())
            };
            let _ = redis_client_opt; // keep shape explicit

            let sell_sol = total_sell as f64 / 1e9;
            let profit_sol = sell_sol - buy_sol;
            let profit_pct = if buy_sol > 0.0 { (profit_sol / buy_sol) * 100.0 } else { 0.0 };
            let profit_sign = if profit_sol >= 0.0 { "+" } else { "" };
            let emoji = if profit_sol >= 0.0 { "🟢" } else { "🔴" };
            if let Some(url) = webhook_url_close {
                let discord_msg = i18n::retreat_burn(
                    &position.pool.to_string(),
                    &position.base_mint.to_string(),
                    &retreat_reason,
                    emoji,
                    profit_sign,
                    profit_sol,
                    profit_pct,
                    buy_sol,
                    sell_sol,
                    close_ok,
                    burn_sig.as_deref(),
                );
                tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
            }
            maybe_auto_stop_after_cycle(&state).await;
            continue;
        }

        // PumpSwap "Buy" = buy base(WSOL) by spending quote(graduated tokens)
        // This is what we want: sell graduated tokens → receive SOL.
        let sell_ix = match pumpswap::build_buy(pumpswap::BuyParams {
            pool: pool_address,
            pool_data,
            user: keypair.pubkey(),
            base_amount_out: min_quote_out,            // min WSOL (lamports) to receive
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

        // Build WSOL ATA CreateIdempotent instruction (needed to receive SOL from swap)
        let wsol_mint = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
        let token_program = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        let ata_program = Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
        let system_program = Pubkey::from_str_const("11111111111111111111111111111111");
        let (wsol_ata, _) = Pubkey::find_program_address(
            &[keypair.pubkey().as_ref(), token_program.as_ref(), wsol_mint.as_ref()],
            &ata_program,
        );
        // CreateIdempotent (discriminator = 1)
        let create_wsol_ata_ix = Instruction {
            program_id: ata_program,
            accounts: vec![
                AccountMeta::new(keypair.pubkey(), true),   // funding
                AccountMeta::new(wsol_ata, false),          // ata
                AccountMeta::new_readonly(keypair.pubkey(), false), // wallet
                AccountMeta::new_readonly(wsol_mint, false),       // mint
                AccountMeta::new_readonly(system_program, false),
                AccountMeta::new_readonly(token_program, false),
            ],
            data: vec![1], // CreateIdempotent
        };

        // Close WSOL ATA after swap to recover SOL
        let close_wsol_ix = Instruction {
            program_id: token_program,
            accounts: vec![
                AccountMeta::new(wsol_ata, false),
                AccountMeta::new(keypair.pubkey(), false),
                AccountMeta::new_readonly(keypair.pubkey(), true),
            ],
            data: vec![9], // CloseAccount
        };

        let tx = Transaction::new_signed_with_payer(
            &[create_wsol_ata_ix, sell_ix, close_wsol_ix],
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

                match confirm_transaction(&rpc_client, &sig, 30).await {
                    Ok(true) => {
                        info!("Sell tx confirmed for position {}: sig={}", id, sig_str);
                        // Fetch actual SOL received (lamport delta for payer account[0])
                        let actual_lamports = match fetch_actual_sol_change(&rpc_client, &sig).await {
                            Some(delta) if delta > 0 => delta as u64,
                            _ => min_quote_out,
                        };

                        let log = TradeLog {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Utc::now(),
                            action: TradeAction::Sell,
                            pool: position.pool,
                            base_mint: position.base_mint,
                            amount_sol: actual_lamports as f64 / 1e9,
                            amount_tokens: position.base_amount,
                            tx_signature: Some(sig_str.clone()),
                            error: None,
                            message: None,
                        };
                        {
                            let mut s = state.write().await;
                            if let Some(p) = s.positions.get_mut(&id) {
                                p.total_sell_lamports += actual_lamports;
                            }
                            s.push_log(log.clone());
                            if let Some(ref client) = s.redis_client {
                                persist_trade_log_to_redis(client, &log);
                            }
                        }

                        // Check if tokens remain in wallet
                        const DUST_THRESHOLD: u64 = 10_000;
                        let remaining = check_remaining_token_balance(
                            &rpc_client, &keypair.pubkey(), &position.base_mint, &position.quote_token_program,
                        ).await;
                        if remaining > DUST_THRESHOLD {
                            // More tokens to sell — reset to Active for next sell cycle
                            info!("Position {} has {} remaining tokens after sell, resetting to Active", id, remaining);
                            let mut s = state.write().await;
                            if let Some(p) = s.positions.get_mut(&id) {
                                p.base_amount = remaining;
                                p.status = PositionStatus::Active;
                            }
                        } else {
                            // All sold (or dust). Burn dust + close ATA + recover rent.
                            info!("Position {} sell complete, closing ATA (remaining dust: {})", id, remaining);
                            let (close_ok, _burn_sig) = burn_and_close_ata(
                                &rpc_client, &send_rpc_client, &keypair,
                                &position.base_mint, &position.quote_token_program,
                            ).await;

                            // Read cumulative totals for final notification
                            let (total_sell, webhook_url_final) = {
                                let mut s = state.write().await;
                                let ts = s.positions.get(&id).map(|p| p.total_sell_lamports).unwrap_or(0);
                                if let Some(p) = s.positions.get_mut(&id) {
                                    p.status = if close_ok { PositionStatus::Closed } else { PositionStatus::Sold };
                                }
                                (ts, s.webhook_url.clone())
                            };

                            let buy_sol = position.buy_price_lamports as f64 / 1e9;
                            let sell_sol = total_sell as f64 / 1e9;
                            let profit_sol = sell_sol - buy_sol;
                            let profit_pct = if buy_sol > 0.0 { (profit_sol / buy_sol) * 100.0 } else { 0.0 };
                            let profit_sign = if profit_sol >= 0.0 { "+" } else { "" };
                            let emoji = if profit_sol >= 0.0 { "🟢" } else { "🔴" };

                            info!(
                                "Position {} CLOSED: buy={:.6} SOL, total_sell={:.6} SOL, profit={}{:.6} SOL ({}{:.1}%)",
                                id, buy_sol, sell_sol, profit_sign, profit_sol, profit_sign, profit_pct
                            );

                            if let Some(url) = webhook_url_final {
                                let discord_msg = i18n::trade_complete(
                                    emoji,
                                    &position.pool.to_string(),
                                    &position.base_mint.to_string(),
                                    profit_sign,
                                    profit_sol,
                                    profit_pct,
                                    buy_sol,
                                    sell_sol,
                                    &sig_str,
                                    close_ok,
                                );
                                tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
                            }
                            maybe_auto_stop_after_cycle(&state).await;
                        }
                    }
                    Ok(false) => {
                        warn!("Sell tx failed on-chain for position {}: sig={}", id, sig_str);
                        let webhook_url_sell = {
                            let mut s = state.write().await;
                            // Reset to Active so sell monitor can retry
                            if let Some(p) = s.positions.get_mut(&id) {
                                p.status = PositionStatus::Active;
                            }
                            let log = TradeLog {
                                id: Uuid::new_v4().to_string(),
                                timestamp: Utc::now(),
                                action: TradeAction::Error,
                                pool: position.pool,
                                base_mint: position.base_mint,
                                amount_sol: 0.0,
                                amount_tokens: position.base_amount,
                                tx_signature: Some(sig_str.clone()),
                                error: Some(format!("Sell tx failed on-chain: {}", sig_str)),
                                message: None,
                            };
                            s.push_log(log);
                            s.webhook_url.clone()
                        };
                        if let Some(url) = webhook_url_sell {
                            let discord_msg = i18n::sell_failed_onchain(&position.pool.to_string(), &sig_str);
                            tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
                        }
                    }
                    Err(timeout_msg) => {
                        warn!("{}", timeout_msg);
                        let webhook_url_sell = {
                            let mut s = state.write().await;
                            // Reset to Active so sell monitor can retry
                            if let Some(p) = s.positions.get_mut(&id) {
                                p.status = PositionStatus::Active;
                            }
                            s.webhook_url.clone()
                        };
                        if let Some(url) = webhook_url_sell {
                            let discord_msg = i18n::sell_tx_unknown(&position.pool.to_string(), &sig_str);
                            tokio::spawn(async move { notify_discord(&url, &discord_msg).await });
                        }
                    }
                }
            }
            Err(e) => {
                let err_msg = format!("Sell tx send failed for position {}: {:?}", id, e);
                error!("{}", err_msg);
                let webhook_url_sell = {
                    let mut s = state.write().await;
                    // Reset to Active so sell monitor can retry
                    if let Some(p) = s.positions.get_mut(&id) {
                        p.status = PositionStatus::Active;
                    }
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
                    s.push_log(log);
                    s.webhook_url.clone()
                };
                if let Some(url) = webhook_url_sell {
                    let discord_msg = i18n::sell_send_failed(&position.pool.to_string(), &err_msg);
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
        let discord_msg = i18n::error_notify(&pool.to_string(), &base_mint.to_string(), &error_msg);
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

/// Burn any remaining dust tokens and close the ATA to recover rent.
/// Returns (success, Option<tx_signature>).
async fn burn_and_close_ata(
    rpc_client: &RpcClient,
    send_rpc_client: &RpcClient,
    keypair: &solana_sdk::signer::keypair::Keypair,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> (bool, Option<String>) {
    let ata_program = Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    let (ata, _) = Pubkey::find_program_address(
        &[keypair.pubkey().as_ref(), token_program.as_ref(), mint.as_ref()],
        &ata_program,
    );

    // Check current balance
    let balance = match rpc_client
        .get_token_account_balance_with_commitment(&ata, CommitmentConfig::confirmed())
        .await
    {
        Ok(resp) => resp.value.amount.parse::<u64>().unwrap_or(0),
        Err(e) => {
            // ATA might not exist (already closed)
            info!("ATA {} not found or error, may already be closed: {:?}", ata, e);
            return (true, None);
        }
    };

    let mut instructions = Vec::new();

    // Burn remaining dust if any
    if balance > 0 {
        // SPL Token Burn instruction (index 8): amount as u64 LE
        let mut burn_data = vec![8];
        burn_data.extend_from_slice(&balance.to_le_bytes());
        instructions.push(Instruction {
            program_id: *token_program,
            accounts: vec![
                AccountMeta::new(ata, false),                          // account
                AccountMeta::new(*mint, false),                        // mint
                AccountMeta::new_readonly(keypair.pubkey(), true),     // authority
            ],
            data: burn_data,
        });
    }

    // CloseAccount instruction (index 9)
    instructions.push(Instruction {
        program_id: *token_program,
        accounts: vec![
            AccountMeta::new(ata, false),                          // account to close
            AccountMeta::new(keypair.pubkey(), false),             // destination for rent
            AccountMeta::new_readonly(keypair.pubkey(), true),     // authority
        ],
        data: vec![9],
    });

    let bh = match rpc_client.get_latest_blockhash().await {
        Ok(bh) => bh,
        Err(e) => {
            error!("Blockhash failed for ATA close: {:?}", e);
            return (false, None);
        }
    };

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[keypair],
        bh,
    );

    let cfg = RpcSendTransactionConfig { skip_preflight: true, ..Default::default() };
    match send_rpc_client.send_transaction_with_config(&tx, cfg).await {
        Ok(sig) => {
            let sig_str = sig.to_string();
            info!("Burn+Close ATA tx sent for {}: sig={}", ata, sig_str);
            match confirm_transaction(rpc_client, &sig, 30).await {
                Ok(true) => {
                    info!("ATA {} closed successfully, rent recovered", ata);
                    (true, Some(sig_str))
                }
                Ok(false) => {
                    warn!("Burn+Close ATA tx failed on-chain: {}", sig_str);
                    (false, Some(sig_str))
                }
                Err(e) => {
                    warn!("Burn+Close ATA tx confirmation timeout: {}", e);
                    (false, Some(sig_str))
                }
            }
        }
        Err(e) => {
            error!("Failed to send burn+close ATA tx for {}: {:?}", ata, e);
            (false, None)
        }
    }
}

/// Check remaining token balance for a mint in a wallet.
/// Returns 0 if no balance or on error.
async fn check_remaining_token_balance(
    rpc_client: &RpcClient,
    wallet: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> u64 {
    // Derive ATA address
    let ata_program = Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    let (ata, _) = Pubkey::find_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ata_program,
    );
    match rpc_client
        .get_token_account_balance_with_commitment(&ata, CommitmentConfig::confirmed())
        .await
    {
        Ok(resp) => resp.value.amount.parse::<u64>().unwrap_or(0),
        Err(_) => 0,
    }
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

/// In single-shot mode (`config.auto_loop == false`), set `running = false`
/// once a buy/sell cycle has fully wound down (no active or selling positions).
/// Has no effect when `auto_loop` is true or when the bot was already stopped.
async fn maybe_auto_stop_after_cycle(state: &Arc<RwLock<AppState>>) {
    let msg = i18n::auto_loop_disabled_stopped();
    let webhook_url = {
        let mut s = state.write().await;
        if !s.running || s.config.auto_loop {
            return;
        }
        if s.active_position_count() != 0 {
            return;
        }
        s.running = false;
        s.push_notification(
            solana_sdk::pubkey::Pubkey::default(),
            solana_sdk::pubkey::Pubkey::default(),
            msg.clone(),
        );
        info!("AUTO_LOOP=false → trading stopped after one buy/sell cycle");
        s.webhook_url.clone()
    };
    if let Some(url) = webhook_url {
        tokio::spawn(async move { notify_discord(&url, &msg).await });
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

    // For restored positions we don't know the original buy price.
    // Set buy_price = 1 so sell triggers quickly on the next sell check.
    // Note: extremely high sell_multiplier values can still delay exit after
    // restore, because current_value must satisfy buy_price * multiplier.
    let current_value: u64 = 1;

    let position_id = Uuid::new_v4().to_string();
    let position = Position {
        id: position_id.clone(),
        pool: pool_address,
        base_mint: token_mint,
        buy_price_lamports: current_value, // current value as baseline
        base_amount: token_amount,
        bought_at: Utc::now(),
        status: PositionStatus::Active,
        total_sell_lamports: 0,
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
            i18n::position_restored(token_amount, current_value),
        );
        s.config.sell_multiplier
    };

    // Spawn sell monitor loop (same as post-buy)
    let state_sell = state.clone();
    let rpc_sell = rpc_client.clone();
    let send_rpc_sell = send_rpc_client.clone();
    tokio::spawn(async move {
        info!("Sell monitor started for restored position pool {} (target {}x)", pool_address, sell_multiplier);
        let mut poll_count: u64 = 0;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            poll_count += 1;
            if poll_count <= 3 || poll_count % 60 == 0 {
                info!("Sell monitor poll #{} for pool {}", poll_count, pool_address);
            }

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
                    None => { warn!("Sell monitor: pool account not found for {}", pool_address); continue; },
                },
                Err(e) => { warn!("Sell monitor: RPC error fetching pool {}: {:?}", pool_address, e); continue; },
            };
            let pool_data = match deserialize_pool_lenient(&pool_account.data) {
                Ok(p) => p,
                Err(e) => { warn!("Sell monitor: pool deserialize failed for {}: {:?}", pool_address, e); continue; },
            };

            let quote_reserves = match fetch_token_balance_with_retry(
                &rpc_sell, &pool_data.pool_quote_token_account,
            ).await {
                Ok(b) => b,
                Err(e) => { warn!("Sell monitor: quote reserves fetch failed: {:?}", e); continue; },
            };
            let base_reserves = match fetch_token_balance_with_retry(
                &rpc_sell, &pool_data.pool_base_token_account,
            ).await {
                Ok(b) => b,
                Err(e) => { warn!("Sell monitor: base reserves fetch failed: {:?}", e); continue; },
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

#[cfg(test)]
mod auto_loop_tests {
    use super::*;
    use crate::state::{AppState, Position, PositionStatus};
    use chrono::Utc;
    use solana_sdk::pubkey::Pubkey;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn make_state(auto_loop: bool, running: bool) -> Arc<RwLock<AppState>> {
        let mut s = AppState::new(None);
        s.config.auto_loop = auto_loop;
        s.running = running;
        Arc::new(RwLock::new(s))
    }

    fn insert_active_position(s: &mut AppState) {
        let id = "test-pos".to_string();
        s.positions.insert(
            id.clone(),
            Position {
                id,
                pool: Pubkey::new_unique(),
                base_mint: Pubkey::new_unique(),
                buy_price_lamports: 100_000,
                base_amount: 1,
                bought_at: Utc::now(),
                status: PositionStatus::Active,
                total_sell_lamports: 0,
                quote_token_program: Pubkey::default(),
            },
        );
    }

    #[tokio::test]
    async fn stops_when_auto_loop_off_and_no_positions() {
        let state = make_state(false, true);
        maybe_auto_stop_after_cycle(&state).await;
        assert!(!state.read().await.running, "should auto-stop");
    }

    #[tokio::test]
    async fn keeps_running_when_auto_loop_on() {
        let state = make_state(true, true);
        maybe_auto_stop_after_cycle(&state).await;
        assert!(state.read().await.running, "auto_loop=true must keep running");
    }

    #[tokio::test]
    async fn keeps_running_when_active_position_remains() {
        let state = make_state(false, true);
        {
            let mut s = state.write().await;
            insert_active_position(&mut s);
        }
        maybe_auto_stop_after_cycle(&state).await;
        assert!(
            state.read().await.running,
            "must wait for all positions to close"
        );
    }

    #[tokio::test]
    async fn noop_when_already_stopped() {
        let state = make_state(false, false);
        maybe_auto_stop_after_cycle(&state).await;
        assert!(!state.read().await.running);
    }
}
