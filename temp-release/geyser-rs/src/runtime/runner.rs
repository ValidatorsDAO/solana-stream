use backoff::backoff::Backoff;
use backoff::{future::retry, ExponentialBackoff};
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use solana_stream_sdk::yellowstone_grpc_proto::geyser::SubscribeRequestPing;
use solana_stream_sdk::{
    GeyserGrpcClient, GeyserGrpcClientError, GeyserSubscribeRequest, GeyserSubscribeUpdate,
    GeyserUpdateOneof,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::transport::ClientTlsConfig;
use tonic::{Code, Status};

/// Streams updates into `updates_tx`, reconnecting with `from_slot` after every
/// disconnect. Returns only when the server rejects the subscription in a way a
/// reconnect cannot fix (see [`non_retryable_status`]); the error carries the
/// server's message.
pub async fn run_geyser_stream(
    grpc_endpoint: String,
    x_token: Option<String>,
    request: GeyserSubscribeRequest,
    tracked_slot: Arc<AtomicU64>,
    updates_tx: mpsc::Sender<GeyserSubscribeUpdate>,
) -> anyhow::Result<()> {
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

        match stream_result {
            Ok(true) => reconnect_backoff.reset(),
            Ok(false) => {}
            Err(e) => {
                if let Some(status) = non_retryable_status(&e) {
                    return Err(anyhow::anyhow!(
                        "the gRPC server ended the subscription with {:?}: {}. Not reconnecting: \
                         the server rejects the same request again. Send a smaller request \
                         (fewer filters, accounts or owners), or split it across connections.",
                        status.code(),
                        status.message()
                    ));
                }
                error!("Failed to handle stream: {:?}", e);
            }
        }

        if let Some(delay) = reconnect_backoff.next_backoff() {
            tokio::time::sleep(delay).await;
        } else {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

/// The status that ends [`run_geyser_stream`] instead of a reconnect.
///
/// Yellowstone gRPC 17 ends a `Subscribe` stream whose request is larger than the
/// server's `max_decoding_message_size` with `RESOURCE_EXHAUSTED` and a message that
/// names the limit. Reconnecting sends the same request and gets the same answer, so
/// retrying it would loop forever.
fn non_retryable_status(error: &anyhow::Error) -> Option<&Status> {
    error
        .chain()
        .find_map(|cause| {
            cause.downcast_ref::<Status>().or_else(|| {
                match cause.downcast_ref::<GeyserGrpcClientError>() {
                    Some(GeyserGrpcClientError::TonicStatus(status)) => Some(status),
                    _ => None,
                }
            })
        })
        .filter(|status| status.code() == Code::ResourceExhausted)
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

#[cfg(test)]
mod tests {
    use super::run_geyser_stream;
    use futures::stream::{self, BoxStream};
    use futures::StreamExt;
    use solana_stream_sdk::yellowstone_grpc_proto::geyser::geyser_server::{Geyser, GeyserServer};
    use solana_stream_sdk::yellowstone_grpc_proto::geyser::{
        GetBlockHeightRequest, GetBlockHeightResponse, GetLatestBlockhashRequest,
        GetLatestBlockhashResponse, GetSlotRequest, GetSlotResponse, GetVersionRequest,
        GetVersionResponse, IsBlockhashValidRequest, IsBlockhashValidResponse, PingRequest,
        PongResponse, SubscribeDeshredRequest, SubscribeGossipRequest, SubscribeReplayInfoRequest,
        SubscribeReplayInfoResponse, SubscribeRequest, SubscribeUpdate, SubscribeUpdateDeshred,
        SubscribeUpdateGossip,
    };
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tonic::transport::server::TcpIncoming;
    use tonic::transport::Server;
    use tonic::{Request, Response, Status, Streaming};

    /// The message yellowstone-grpc-geyser 17 sends with the status, in its shape.
    const SERVER_MESSAGE: &str =
        "request too large: 70000 bytes exceeds max_decoding_message_size of 65536 bytes";

    /// Ends a `Subscribe` stream with RESOURCE_EXHAUSTED when the first request that asks
    /// for something arrives, as a 17.0.0 server does for a request over its
    /// `max_decoding_message_size`. The empty request `subscribe()` sends first passes.
    struct ExhaustedGeyser {
        subscribe_calls: Arc<AtomicUsize>,
    }

    fn unimplemented<T>() -> Result<Response<T>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    #[tonic::async_trait]
    impl Geyser for ExhaustedGeyser {
        type SubscribeStream = BoxStream<'static, Result<SubscribeUpdate, Status>>;
        type SubscribeDeshredStream = BoxStream<'static, Result<SubscribeUpdateDeshred, Status>>;
        type SubscribeGossipStream = BoxStream<'static, Result<SubscribeUpdateGossip, Status>>;

        async fn subscribe(
            &self,
            request: Request<Streaming<SubscribeRequest>>,
        ) -> Result<Response<Self::SubscribeStream>, Status> {
            self.subscribe_calls.fetch_add(1, Ordering::SeqCst);
            let requests = request.into_inner();
            let rejection = stream::once(async move {
                let mut requests = requests;
                while let Some(Ok(request)) = requests.next().await {
                    if request != SubscribeRequest::default() {
                        break;
                    }
                }
                Err(Status::resource_exhausted(SERVER_MESSAGE))
            });
            Ok(Response::new(Box::pin(rejection)))
        }

        async fn subscribe_deshred(
            &self,
            _request: Request<Streaming<SubscribeDeshredRequest>>,
        ) -> Result<Response<Self::SubscribeDeshredStream>, Status> {
            unimplemented()
        }

        async fn subscribe_gossip(
            &self,
            _request: Request<SubscribeGossipRequest>,
        ) -> Result<Response<Self::SubscribeGossipStream>, Status> {
            unimplemented()
        }

        async fn subscribe_replay_info(
            &self,
            _request: Request<SubscribeReplayInfoRequest>,
        ) -> Result<Response<SubscribeReplayInfoResponse>, Status> {
            unimplemented()
        }

        async fn ping(
            &self,
            _request: Request<PingRequest>,
        ) -> Result<Response<PongResponse>, Status> {
            unimplemented()
        }

        async fn get_latest_blockhash(
            &self,
            _request: Request<GetLatestBlockhashRequest>,
        ) -> Result<Response<GetLatestBlockhashResponse>, Status> {
            unimplemented()
        }

        async fn get_block_height(
            &self,
            _request: Request<GetBlockHeightRequest>,
        ) -> Result<Response<GetBlockHeightResponse>, Status> {
            unimplemented()
        }

        async fn get_slot(
            &self,
            _request: Request<GetSlotRequest>,
        ) -> Result<Response<GetSlotResponse>, Status> {
            unimplemented()
        }

        async fn is_blockhash_valid(
            &self,
            _request: Request<IsBlockhashValidRequest>,
        ) -> Result<Response<IsBlockhashValidResponse>, Status> {
            unimplemented()
        }

        async fn get_version(
            &self,
            _request: Request<GetVersionRequest>,
        ) -> Result<Response<GetVersionResponse>, Status> {
            unimplemented()
        }
    }

    /// `{:?}` of whatever the runner returns, so this test reads the same before and
    /// after the runner learned to return an error.
    fn describe<T: std::fmt::Debug>(value: T) -> String {
        format!("{value:?}")
    }

    #[tokio::test]
    async fn resource_exhausted_stops_the_runner_with_the_server_message() {
        let subscribe_calls = Arc::new(AtomicUsize::new(0));
        let incoming = TcpIncoming::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = incoming.local_addr().unwrap();
        let server = tokio::spawn(
            Server::builder()
                .add_service(GeyserServer::new(ExhaustedGeyser {
                    subscribe_calls: subscribe_calls.clone(),
                }))
                .serve_with_incoming(incoming),
        );

        let (updates_tx, _updates_rx) = mpsc::channel(8);
        let request = SubscribeRequest {
            slots: [("slots".to_string(), Default::default())].into(),
            ..Default::default()
        };
        let runner = tokio::spawn(run_geyser_stream(
            format!("http://{addr}"),
            None,
            request,
            Arc::new(AtomicU64::new(0)),
            updates_tx,
        ));

        // A runner that retries reconnects after its first backoff (0.25 s to 0.75 s).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !runner.is_finished() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let finished = runner.is_finished();
        let calls = subscribe_calls.load(Ordering::SeqCst);
        if !finished {
            runner.abort();
        }
        server.abort();

        assert!(
            finished,
            "the runner kept reconnecting after RESOURCE_EXHAUSTED ({calls} subscribe calls in 3 s)"
        );
        assert_eq!(
            calls, 1,
            "the runner subscribed again after RESOURCE_EXHAUSTED"
        );
        let outcome = describe(runner.await.unwrap());
        assert!(
            outcome.contains(SERVER_MESSAGE) && outcome.contains("ResourceExhausted"),
            "the runner did not report the server's status and message: {outcome}"
        );
    }
}
