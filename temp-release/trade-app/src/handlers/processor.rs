use crate::engine::{check_and_sell_positions, handle_new_pool};
use crate::state::AppState;
use crate::utils::blocktime::{prepare_log_message, TransactionsBySlot};
use log::info;
use solana_commitment_config::CommitmentConfig;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_stream_sdk::{GeyserSubscribeUpdate, GeyserUpdateOneof};
use std::sync::Arc;
use tokio::sync::RwLock;
use ultima_swap_pumpfun as pumpswap;

// Debounce removed: every swap event triggers immediate sell check
// for minimal latency on price spikes.

pub async fn process_updates(
    mut updates_rx: tokio::sync::mpsc::Receiver<GeyserSubscribeUpdate>,
    transactions_by_slot: TransactionsBySlot,
    state: Arc<RwLock<AppState>>,
    rpc_client: Arc<RpcClient>,
    send_rpc_client: Arc<RpcClient>,
) {
    while let Some(update) = updates_rx.recv().await {
        handle_update(
            &update,
            &transactions_by_slot,
            state.clone(),
            rpc_client.clone(),
            send_rpc_client.clone(),
        )
        .await;
    }
}

async fn handle_update(
    update: &GeyserSubscribeUpdate,
    transactions_by_slot: &TransactionsBySlot,
    state: Arc<RwLock<AppState>>,
    rpc_client: Arc<RpcClient>,
    send_rpc_client: Arc<RpcClient>,
) {
    prepare_log_message(update, transactions_by_slot);

    let (watch_address, grpc_streaming) = {
        let s = state.read().await;
        (s.watch_address, s.grpc_streaming)
    };

    // Skip processing if gRPC streaming is paused.
    if !grpc_streaming {
        return;
    }

    if let Some(GeyserUpdateOneof::Transaction(tx_update)) = &update.update_oneof {
        if let Some(tx_info) = &tx_update.transaction {
            if let Some(tx) = &tx_info.transaction {
                // Parse account keys from the transaction message.
                if let Some(msg) = &tx.message {
                    let account_keys: Vec<Pubkey> = msg
                        .account_keys
                        .iter()
                        .filter_map(|k| {
                            if k.len() == 32 {
                                Some(Pubkey::new_from_array(k.as_slice().try_into().unwrap()))
                            } else {
                                None
                            }
                        })
                        .collect();

                    // Check each instruction in the tx.
                    for ix in &msg.instructions {
                        let program_idx = ix.program_id_index as usize;
                        if program_idx >= account_keys.len() {
                            continue;
                        }
                        let program_id = account_keys[program_idx];

                        // Only process instructions targeting the watched AMM program.
                        if program_id != watch_address {
                            continue;
                        }

                        // Resolve the instruction's account keys.
                        let ix_account_keys: Vec<Pubkey> = ix
                            .accounts
                            .iter()
                            .filter_map(|&idx| account_keys.get(idx as usize).copied())
                            .collect();

                        // ── create_pool detection ──
                        if let Some(detected) =
                            pumpswap::try_parse_create_pool(&ix.data, &ix_account_keys)
                        {
                            info!(
                                "Detected create_pool: pool={} base_mint={} creator={}",
                                detected.pool, detected.base_mint, detected.creator
                            );
                            let state_clone = state.clone();
                            let rpc_clone = rpc_client.clone();
                            let send_rpc_clone = send_rpc_client.clone();
                            let pool = detected.pool;
                            let base_mint = detected.base_mint;
                            tokio::spawn(async move {
                                handle_new_pool(pool, base_mint, state_clone, rpc_clone, send_rpc_clone).await;
                            });
                        }

                        // ── swap (buy/sell) detection → trigger sell check ──
                        if let Some(swap) = pumpswap::try_parse_swap(&ix.data, &ix_account_keys) {
                            // We only care about swaps on pools where we hold positions.
                            let has_position = {
                                let s = state.read().await;
                                s.positions.values().any(|p| {
                                    p.pool == swap.pool
                                        && (p.status == crate::state::PositionStatus::Active)
                                })
                            };
                            if has_position {
                                info!(
                                    "Detected swap on held pool {}: {:?} base={} quote={}",
                                    swap.pool, swap.direction, swap.base_amount, swap.quote_amount
                                );

                                // Fast path: extract post-swap reserves from geyser tx metadata
                                // (zero RPC latency — reserves come from the same geyser update)
                                let mut reserves_from_meta: Option<(u64, u64)> = None;
                                if let Some(meta) = &tx_info.meta {
                                    // Swap accounts layout:
                                    // [0]=pool [1]=user [2]=global_config [3]=base_mint [4]=quote_mint
                                    // [5]=user_base_ata [6]=user_quote_ata
                                    // [7]=pool_base_vault [8]=pool_quote_vault ...
                                    if ix_account_keys.len() >= 9 {
                                        let pool_base_vault = ix_account_keys[7];
                                        let pool_quote_vault = ix_account_keys[8];

                                        let mut base_r: Option<u64> = None;
                                        let mut quote_r: Option<u64> = None;

                                        for tb in &meta.post_token_balances {
                                            let acct_idx = tb.account_index as usize;
                                            if acct_idx < account_keys.len() {
                                                let acct = account_keys[acct_idx];
                                                if let Some(ref ui_amt) = tb.ui_token_amount {
                                                    if let Ok(amt) = ui_amt.amount.parse::<u64>() {
                                                        if acct == pool_base_vault {
                                                            base_r = Some(amt);
                                                        } else if acct == pool_quote_vault {
                                                            quote_r = Some(amt);
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        if let (Some(br), Some(qr)) = (base_r, quote_r) {
                                            reserves_from_meta = Some((qr, br));
                                        }
                                    }
                                }

                                let state_clone = state.clone();
                                let rpc_clone = rpc_client.clone();
                                let send_rpc_clone = send_rpc_client.clone();
                                let pool_addr = swap.pool;

                                if let Some((quote_reserves, base_reserves)) = reserves_from_meta {
                                    // Zero-latency path: reserves from geyser metadata
                                    tokio::spawn(async move {
                                        check_and_sell_positions(
                                            state_clone,
                                            rpc_clone,
                                            send_rpc_clone,
                                            pool_addr,
                                            quote_reserves,
                                            base_reserves,
                                        )
                                        .await;
                                    });
                                } else {
                                    // Fallback: fetch from RPC (processed commitment)
                                    tokio::spawn(async move {
                                        let pool_account = match rpc_clone
                                            .get_account_with_commitment(
                                                &pool_addr,
                                                CommitmentConfig::processed(),
                                            )
                                            .await
                                        {
                                            Ok(resp) => match resp.value {
                                                Some(a) => a,
                                                None => return,
                                            },
                                            Err(_) => return,
                                        };
                                        let pool_data = match crate::engine::deserialize_pool_lenient(
                                            &pool_account.data,
                                        ) {
                                            Ok(p) => p,
                                            Err(_) => return,
                                        };
                                        let quote_reserves = match rpc_clone
                                            .get_token_account_balance(&pool_data.pool_quote_token_account)
                                            .await
                                        {
                                            Ok(b) => b.amount.parse::<u64>().unwrap_or(0),
                                            Err(_) => return,
                                        };
                                        let base_reserves = match rpc_clone
                                            .get_token_account_balance(&pool_data.pool_base_token_account)
                                            .await
                                        {
                                            Ok(b) => b.amount.parse::<u64>().unwrap_or(0),
                                            Err(_) => return,
                                        };
                                        check_and_sell_positions(
                                            state_clone, rpc_clone, send_rpc_clone,
                                            pool_addr, quote_reserves, base_reserves,
                                        ).await;
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
