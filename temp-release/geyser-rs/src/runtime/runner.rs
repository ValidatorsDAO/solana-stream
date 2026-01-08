use backoff::backoff::Backoff;
use backoff::{future::retry, ExponentialBackoff};
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use solana_stream_sdk::yellowstone_grpc_proto::geyser::SubscribeRequestPing;
use solana_stream_sdk::{
    GeyserGrpcClient, GeyserSubscribeRequest, GeyserSubscribeUpdate, GeyserUpdateOneof,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::transport::ClientTlsConfig;

pub async fn run_geyser_stream(
    grpc_endpoint: String,
    x_token: Option<String>,
    request: GeyserSubscribeRequest,
    tracked_slot: Arc<AtomicU64>,
    updates_tx: mpsc::Sender<GeyserSubscribeUpdate>,
) {
    let mut reconnect_backoff = ExponentialBackoff {
        max_elapsed_time: None,
        ..ExponentialBackoff::default()
    };

    loop {
        let stream_result = {
            let grpc_endpoint = grpc_endpoint.clone();
            let x_token = x_token.clone();
            let request = request.clone();
            let tracked_slot = tracked_slot.clone();
            let updates_tx = updates_tx.clone();

            async move {
                let mut client = retry(
                    ExponentialBackoff {
                        max_elapsed_time: None,
                        ..ExponentialBackoff::default()
                    },
                    || {
                        let grpc_endpoint = grpc_endpoint.clone();
                        let x_token = x_token.clone();
                        async move {
                            let mut builder =
                                GeyserGrpcClient::build_from_shared(grpc_endpoint.clone())?;
                            if let Some(token) = x_token {
                                builder = builder.x_token(Some(token))?;
                            }
                            if grpc_endpoint.starts_with("https://") {
                                builder =
                                    builder.tls_config(ClientTlsConfig::new().with_native_roots())?;
                            }
                            builder.connect().await.map_err(backoff::Error::transient)
                        }
                    },
                )
                .await?;

                let mut subscribe_request = request.clone();
                let resume_from_slot = tracked_slot.load(Ordering::Relaxed);
                subscribe_request.from_slot = if resume_from_slot > 0 {
                    Some(resume_from_slot.saturating_sub(1))
                } else {
                    None
                };

                let (mut sink, mut stream) = client.subscribe().await?;
                sink.send(subscribe_request).await?;

                let mut saw_non_heartbeat = false;

                while let Some(message) = stream.next().await {
                    match message {
                        Ok(update) => {
                            if matches!(update.update_oneof, Some(GeyserUpdateOneof::Ping(_))) {
                                if let Err(err) = sink
                                    .send(GeyserSubscribeRequest {
                                        ping: Some(SubscribeRequestPing { id: 1 }),
                                        ..Default::default()
                                    })
                                    .await
                                {
                                    warn!("Failed to respond to ping: {:?}", err);
                                    break;
                                }
                                continue;
                            }

                            if matches!(update.update_oneof, Some(GeyserUpdateOneof::Pong(_))) {
                                continue;
                            }

                            update_tracked_slot(&update, &tracked_slot);
                            saw_non_heartbeat = true;

                            if let Err(err) = updates_tx.try_send(update) {
                                warn!("Dropping update due to full channel: {:?}", err);
                            }
                        }
                        Err(e) => {
                            return Err(anyhow::Error::from(e));
                        }
                    }
                }

                info!("Stream ended, reconnecting...");
                Ok::<bool, anyhow::Error>(saw_non_heartbeat)
            }
        }
        .await;

        if let Ok(true) = stream_result {
            reconnect_backoff.reset();
        }

        if let Err(e) = stream_result {
            error!("Failed to handle stream: {:?}", e);
        }

        if let Some(delay) = reconnect_backoff.next_backoff() {
            tokio::time::sleep(delay).await;
        } else {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

fn update_tracked_slot(update: &GeyserSubscribeUpdate, tracked_slot: &AtomicU64) {
    let maybe_slot = match &update.update_oneof {
        Some(GeyserUpdateOneof::Transaction(tx)) => Some(tx.slot),
        Some(GeyserUpdateOneof::Slot(slot)) => Some(slot.slot),
        Some(GeyserUpdateOneof::Block(block)) => Some(block.slot),
        Some(GeyserUpdateOneof::BlockMeta(block_meta)) => Some(block_meta.slot),
        Some(GeyserUpdateOneof::Account(account)) => Some(account.slot),
        Some(GeyserUpdateOneof::Entry(entry)) => Some(entry.slot),
        _ => None,
    };

    if let Some(slot) = maybe_slot {
        tracked_slot.fetch_max(slot, Ordering::Relaxed);
    }
}
