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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv().ok();
    env_logger::init();

    let cfg = ShredsUdpConfig::from_env();
    let mut receiver = UdpShredReceiver::bind(&cfg.bind_addr, None).await?;
    let local_addr = receiver.local_addr()?;
    info!("Listening for UDP shreds on {}", local_addr);
    info!("Ensure the sender targets this ip:port.");

    let policy = DeshredPolicy {
        require_code_match: cfg.require_code_match,
    };
    let watch_cfg = Arc::new(cfg.watch_config());
    let state = ShredsUdpState::new(&cfg);

    let latency_handle = if let (true, Some(cache), Some(txs)) = (
        cfg.enable_latency_monitor,
        state.block_time_cache(),
        state.transactions_by_slot(),
    ) {
        Some(tokio::spawn(async move {
            latency_monitor_task(cache, txs).await;
        }))
    } else {
        None
    };

    let recv_handle = {
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
    };

    if let Some(latency_handle) = latency_handle {
        tokio::try_join!(latency_handle, recv_handle)?;
    } else {
        recv_handle.await?;
    }
    Ok(())
}
