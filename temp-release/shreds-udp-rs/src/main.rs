use dotenvy::dotenv;
use env_logger;
use log::{error, info};
use solana_stream_sdk::{
    shreds_udp::{
        collect_watch_events, decode_udp_datagram, deshred_shreds_to_entries, insert_shred,
        latency_monitor_task, log_watch_events, DeshredPolicy, ShredInsertOutcome,
        ShredReadyBatch, ShredSource, ShredsUdpConfig, ShredsUdpState, WatchEvent,
    },
    UdpShredReceiver,
};
use std::sync::Arc;
use tokio::signal;

const EMBEDDED_CONFIG: &str = include_str!("../settings.jsonc");

async fn handle_ready_batch(
    ready: ShredReadyBatch,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
    watch_cfg: &Arc<solana_stream_sdk::txn::ProgramWatchConfig>,
) {
    let key = ready.key;
    match deshred_shreds_to_entries(&ready.shreds) {
        Ok(entries) => {
            let txs: Vec<&solana_sdk::transaction::VersionedTransaction> =
                entries.iter().flat_map(|e| e.transactions.iter()).collect();
            info!(
                "deshred slot={} entries={} txs={}",
                key.slot,
                entries.len(),
                txs.len()
            );

            // Default logging (honors pump_min_lamports). This is the first sink; swap or extend with
            // custom logic below if you need additional actions.
            log_watch_events(
                key.slot,
                &txs,
                watch_cfg.as_ref(),
                cfg.log_watch_hits,
                cfg.pump_min_lamports,
            );
            // Structured hits for custom hooks; use this to attach your own side-effects.
            let events =
                collect_watch_events(key.slot, &txs, watch_cfg.as_ref(), cfg.pump_min_lamports);
            maybe_custom_watch_hook(&events, cfg.pump_min_lamports);

            if cfg.log_entries {
                let sigs: Vec<String> = solana_stream_sdk::txn::first_signatures(
                    txs.iter().copied(),
                    usize::MAX, // include all non-vote sigs in preview
                    watch_cfg.skip_vote_txs,
                )
                .into_iter()
                .map(|s| s.to_string())
                .collect();
                info!(
                    "entries preview slot={} fec_set={} sigs_first_non_vote={:?}",
                    key.slot, key.fec_set, sigs
                );
            }

            state.remove_batch(&key).await;
            if matches!(ready.source, ShredSource::Data) {
                state.mark_completed(key).await;
            }
        }
        Err(e) => {
            if cfg.log_deshred_errors {
                error!(
                    "deshred failed slot={} fec_set={} err={}",
                    key.slot, key.fec_set, e
                );
            }
            state.remove_batch(&key).await;
            state.mark_suppressed(key).await;
        }
    }
}

/// Optional sample hook to show where custom actions can be triggered after detection.
/// Enable by setting `SHREDS_UDP_CUSTOM_HOOK=1`. Replace the body with your own sink.
fn maybe_custom_watch_hook(events: &[WatchEvent], pump_min_lamports: u64) {
    if std::env::var("SHREDS_UDP_CUSTOM_HOOK").is_err() {
        return;
    }
    // This is the second “sink” point: structured hits. Use it to send alerts/txs/etc.
    // It already respects pump_min_lamports (only logs hits at/above the threshold if amounts exist).
    for event in events.iter() {
        for detail in event.details.iter().filter(|d| {
            pump_min_lamports == 0
                || d.sol_amount
                    .map(|l| l >= pump_min_lamports)
                    .unwrap_or(true)
        }) {
            info!(
                "[custom hook] slot={} sig={} mint={} action={:?} sol_amount={:?} token_amount={:?}",
                event.slot,
                event.hit.signature,
                detail.mint,
                detail.action,
                detail.sol_amount,
                detail.token_amount
            );
            // Place your own side-effects here (queue, RPC call, on-chain tx, etc.).
        }
    }
}

fn describe_status(st: &solana_stream_sdk::shreds_udp::BatchStatus) -> String {
    format!(
        "have_data={} code={} required_data={:?} missing_preview={:?}",
        st.data_len, st.code_len, st.required_data, st.missing
    )
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("create SIGTERM listener");
        let mut hup = signal(SignalKind::hangup()).expect("create SIGHUP listener");
        tokio::select! {
            _ = signal::ctrl_c() => {},
            _ = term.recv() => {},
            _ = hup.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C handler");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv().ok();
    env_logger::init();

    // Use the embedded settings.jsonc (non-secret), allow env to override RPC etc.
    let cfg = ShredsUdpConfig::from_embedded(EMBEDDED_CONFIG);
    let receiver = UdpShredReceiver::bind(&cfg.bind_addr, None).await?;
    let local_addr = receiver.local_addr()?;
    info!("Listening for UDP shreds on {}", local_addr);
    info!("Ensure the sender targets this ip:port.");

    let policy = DeshredPolicy {
        require_code_match: cfg.require_code_match,
    };
    let watch_cfg = Arc::new(cfg.watch_config());
    let state = ShredsUdpState::new(&cfg);

    let mut latency_handle = if let (true, Some(cache), Some(txs)) = (
        cfg.enable_latency_monitor,
        state.block_time_cache(),
        state.transactions_by_slot(),
    ) {
        Some(tokio::spawn(async move { latency_monitor_task(cache, txs).await }))
    } else {
        None
    };

    let mut recv_handle = Some({
        let state = state.clone();
        let watch_cfg = watch_cfg.clone();
        let cfg = cfg.clone();
        let mut receiver = receiver;
        tokio::spawn(async move {
            loop {
                let datagram = match receiver.recv_raw().await {
                    Ok(d) => d,
                    Err(e) => {
                        error!("recv_raw error: {:?}", e);
                        continue;
                    }
                };

                let payload_len = datagram.payload.len();
                if cfg.log_raw {
                    let recv_ts = chrono::Utc::now();
                    let preview: String = datagram
                        .payload
                        .iter()
                        .take(48)
                        .map(|b| format!("{:02x}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    info!(
                        "recv {} bytes from {} at {} | preview: {}{}",
                        payload_len,
                        datagram.from,
                        recv_ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        preview,
                        if payload_len > 48 { " ..." } else { "" }
                    );
                }

                if let Some(decoded) = decode_udp_datagram(&datagram, &state, &cfg).await {
                    match insert_shred(decoded, &datagram, &state, &cfg, &policy).await {
                        ShredInsertOutcome::Ready(ready) => {
                            if cfg.log_deshred_attempts {
                                if let Some(st) = &ready.status {
                                    info!(
                                        "deshred attempt slot={} ver={} fec_set={} {}",
                                        ready.key.slot,
                                        ready.key.version,
                                        ready.key.fec_set,
                                        describe_status(st)
                                    );
                                }
                            }
                            handle_ready_batch(ready, &state, &cfg, &watch_cfg).await;
                        }
                        ShredInsertOutcome::Deferred {
                            key,
                            reason,
                            status,
                            ..
                        } => {
                            if cfg.log_deferred {
                                if let Some(st) = status {
                                    info!(
                                        "deshred deferred reason={} {}",
                                        reason,
                                        describe_status(&st)
                                    );
                                } else {
                                    info!(
                                        "deshred deferred reason={} slot={} ver={} fec_set={}",
                                        reason, key.slot, key.version, key.fec_set
                                    );
                                }
                            }
                        }
                        ShredInsertOutcome::Buffered { .. } | ShredInsertOutcome::Skipped => {}
                    }
                } else if cfg.log_raw {
                    info!("payload not recognized as Shred; raw size {}", payload_len);
                }
            }
        })
    });

    tokio::select! {
        _ = shutdown_signal() => {
            info!("Shutdown signal received, stopping tasks...");
            if let Some(handle) = recv_handle.take() { handle.abort(); }
            if let Some(handle) = latency_handle.take() { handle.abort(); }
        }
        res = async {
            match (latency_handle.take(), recv_handle.take()) {
                (Some(latency_handle), Some(recv_handle)) => {
                    tokio::try_join!(latency_handle, recv_handle)?;
                }
                (Some(latency_handle), None) => {
                    latency_handle.await?;
                }
                (None, Some(recv_handle)) => {
                    recv_handle.await?;
                }
                (None, None) => {}
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        } => {
            res?;
        }
    }
    Ok(())
}
