use dotenvy::dotenv;
use env_logger;
use log::{error, info};
use solana_stream_sdk::{
    shreds_udp::{
        collect_watch_events, decode_udp_datagram, deshred_shreds_to_entries, insert_shred,
        DeshredPolicy, ShredInsertOutcome, ShredReadyBatch, ShredSource, ShredsUdpConfig,
        ShredsUdpState,
    },
    txn::{first_signatures, parse_pubkeys, ProgramWatchConfig, SplTokenMintFinder},
    UdpShredReceiver,
};
use std::sync::Arc;
use tokio::signal;

const EMBEDDED_CONFIG: &str = include_str!("../../settings.jsonc");

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

async fn handle_ready_batch(
    ready: ShredReadyBatch,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
    watch_cfg: &Arc<ProgramWatchConfig>,
) {
    let key = ready.key;
    let source = ready.source;

    match deshred_shreds_to_entries(&ready.shreds) {
        Ok(entries) => {
            let txs: Vec<&solana_sdk::transaction::VersionedTransaction> =
                entries.iter().flat_map(|e| e.transactions.iter()).collect();
            info!(
                "slot={} entries={} txs={} (generic logger)",
                key.slot,
                entries.len(),
                txs.len()
            );

            if cfg.log_entries {
                let sigs: Vec<String> = first_signatures(
                    txs.iter().copied(),
                    12,
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

            if !(watch_cfg.program_ids.is_empty() && watch_cfg.authorities.is_empty()) {
                for event in collect_watch_events(key.slot, &txs, watch_cfg.as_ref(), 0) {
                    for detail in &event.details {
                        info!(
                            "hit slot={} sig={} mint={} label={:?} action={:?} lamports={:?} token_amount={:?}",
                            event.slot,
                            event.hit.signature,
                            detail.mint,
                            detail.label,
                            detail.action,
                            detail.sol_amount,
                            detail.token_amount,
                        );
                    }
                }
            }

            state.remove_batch(&key).await;
            if matches!(source, ShredSource::Data) {
                state.mark_completed(key).await;
            }
        }
        Err(e) => {
            error!(
                "deshred failed slot={} fec_set={} err={}",
                key.slot, key.fec_set, e
            );
            state.remove_batch(&key).await;
            state.mark_suppressed(key).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv().ok();
    env_logger::init();

    // Read shared settings but avoid pump.fun-specific defaults.
    let cfg = ShredsUdpConfig::from_embedded(EMBEDDED_CONFIG);
    let mut receiver = UdpShredReceiver::bind(&cfg.bind_addr, None).await?;
    let local_addr = receiver.local_addr()?;
    info!("Generic UDP logger listening on {}", local_addr);
    info!("Set GENERIC_WATCH_PROGRAM_IDS / GENERIC_WATCH_AUTHORITIES to watch specific programs; defaults to none.");

    let policy = DeshredPolicy {
        require_code_match: cfg.require_code_match,
    };
    let state = ShredsUdpState::new(&cfg);
    let watch_program_ids =
        parse_pubkeys(std::env::var("GENERIC_WATCH_PROGRAM_IDS").ok().as_deref(), &[]);
    let watch_authorities =
        parse_pubkeys(std::env::var("GENERIC_WATCH_AUTHORITIES").ok().as_deref(), &[]);
    let watch_cfg = Arc::new(
        ProgramWatchConfig::new(watch_program_ids, watch_authorities)
            .with_token_program_ids(cfg.token_program_ids.clone())
            .with_skip_vote_txs(cfg.skip_vote_sigs)
            .with_mint_finder(Arc::new(SplTokenMintFinder))
            .with_detailers(Vec::new()),
    );

    tokio::select! {
        _ = shutdown_signal() => {
            info!("Shutdown signal received, stopping tasks...");
        }
        res = async {
            loop {
                let datagram = receiver.recv_raw().await?;
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
                            handle_ready_batch(ready, &state, &cfg, &watch_cfg).await;
                        }
                        ShredInsertOutcome::Deferred { key, reason, .. } => {
                            if cfg.log_deferred {
                                info!(
                                    "deshred deferred (generic) slot={} fec_set={} reason={}",
                                    key.slot, key.fec_set, reason
                                );
                            }
                        }
                        ShredInsertOutcome::Buffered { .. } | ShredInsertOutcome::Skipped => {}
                    }
                } else if cfg.log_raw {
                    info!("payload not recognized as Shred; raw size {}", payload_len);
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        } => {
            res?;
        }
    }
    Ok(())
}
