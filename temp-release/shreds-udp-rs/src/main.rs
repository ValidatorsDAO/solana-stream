use dotenvy::dotenv;
use env_logger;
use log::{error, info};
use solana_stream_sdk::{
    shreds_udp::{
        handle_datagram, latency_monitor_task, DeshredPolicy, ShredsUdpConfig, ShredsUdpState,
    },
    UdpShredReceiver,
};
use std::sync::Arc;
use tokio::signal;

const EMBEDDED_CONFIG: &str = include_str!("../settings.jsonc");

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
    let mut receiver = UdpShredReceiver::bind(&cfg.bind_addr, None).await?;
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
        tokio::spawn(async move {
            loop {
                if let Err(e) = handle_datagram(
                    &mut receiver,
                    &state,
                    &cfg,
                    policy,
                    watch_cfg.clone(),
                )
                .await
                {
                    error!("UDP handling error: {:?}", e);
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
