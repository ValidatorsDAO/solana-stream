//! The Geyser stream runner.
//!
//! It subscribes with `subscribe_with_reconnect` (yellowstone-grpc-client 17), the
//! recommended path: the stream reconnects by itself, replays from the last confirmed slot
//! and reports each dropped connection as `ReconnectEvent::Disconnected`, listing the banks
//! it delivered only in part. When the stream gives up (its retry budget is spent), the
//! runner subscribes again with `from_slot` set from the last slot it saw.
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use solana_stream_sdk::yellowstone_grpc_proto::geyser::SubscribeRequestPing;
use solana_stream_sdk::{
    GeyserBackoff, GeyserGrpcClient, GeyserGrpcClientError, GeyserReconnectConfig,
    GeyserReconnectEvent, GeyserReconnectPolicy, GeyserSubscribeRequest, GeyserSubscribeUpdate,
    GeyserUpdateOneof,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::transport::ClientTlsConfig;
use tonic::{Code, Status};

/// Reconnects the stream makes by itself before it ends with the last error: after 100 ms,
/// 200 ms, … 3.2 s, about 6 s in all. A connection that delivers a block refills it.
pub const RECONNECT_BACKOFF: GeyserBackoff = GeyserBackoff::new(Duration::from_millis(100), 2.0, 6);

/// Delay before the runner subscribes again after the stream gave up; it doubles up to
/// [`RESUBSCRIBE_MAX_DELAY`] and starts over once a stream delivered updates.
const RESUBSCRIBE_DELAY: Duration = Duration::from_secs(1);
const RESUBSCRIBE_MAX_DELAY: Duration = Duration::from_secs(60);

/// Streams events into `events_tx` until the server rejects the subscription in a way no
/// reconnect can fix (see [`non_retryable_status`]); the error carries the server's message.
///
/// Updates are dropped with a warning when `events_tx` is full. `Disconnected` events are
/// never dropped: the receiver must discard what it derived from the banks they list.
pub async fn run_geyser_stream(
    grpc_endpoint: String,
    x_token: Option<String>,
    request: GeyserSubscribeRequest,
    tracked_slot: Arc<AtomicU64>,
    events_tx: mpsc::Sender<GeyserReconnectEvent>,
) -> anyhow::Result<()> {
    run_with_backoff(
        grpc_endpoint,
        x_token,
        request,
        tracked_slot,
        events_tx,
        RECONNECT_BACKOFF,
    )
    .await
}

async fn run_with_backoff(
    grpc_endpoint: String,
    x_token: Option<String>,
    request: GeyserSubscribeRequest,
    tracked_slot: Arc<AtomicU64>,
    events_tx: mpsc::Sender<GeyserReconnectEvent>,
    reconnect_backoff: GeyserBackoff,
) -> anyhow::Result<()> {
    let mut delay = RESUBSCRIBE_DELAY;
    loop {
        let mut saw_updates = false;
        let result = stream_once(
            &grpc_endpoint,
            x_token.as_deref(),
            &request,
            &tracked_slot,
            &events_tx,
            reconnect_backoff.clone(),
            &mut saw_updates,
        )
        .await;

        if let Err(error) = &result {
            if let Some(status) = non_retryable_status(error) {
                return Err(anyhow::anyhow!(
                    "the gRPC server ended the subscription with {:?}: {}. Not reconnecting: \
                     the server rejects the same request again. Send a smaller request \
                     (fewer filters, accounts or owners), or split it across connections.",
                    status.code(),
                    status.message()
                ));
            }
            if events_tx.is_closed() {
                return Err(anyhow::anyhow!("the update receiver was dropped"));
            }
            error!("Geyser stream failed: {error:#}");
        } else {
            info!("Geyser stream ended, subscribing again");
        }

        if saw_updates {
            delay = RESUBSCRIBE_DELAY;
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RESUBSCRIBE_MAX_DELAY);
    }
}

async fn stream_once(
    grpc_endpoint: &str,
    x_token: Option<&str>,
    request: &GeyserSubscribeRequest,
    tracked_slot: &AtomicU64,
    events_tx: &mpsc::Sender<GeyserReconnectEvent>,
    reconnect_backoff: GeyserBackoff,
    saw_updates: &mut bool,
) -> anyhow::Result<()> {
    // This starter builds against the published SDK 4.0.0, which does not yet
    // export tune_geyser_grpc_builder. Apply the same windows directly.
    const WINDOW: u32 = 16 * 1024 * 1024;
    let mut builder = GeyserGrpcClient::build_from_shared(grpc_endpoint.to_string())?
        .initial_stream_window_size(WINDOW)
        .initial_connection_window_size(WINDOW)
        .http2_adaptive_window(false)
        .set_reconnect_config(
            GeyserReconnectConfig::default()
                .with_backoff(reconnect_backoff)
                .with_policy(GeyserReconnectPolicy::Replay { slots_behind: 0 }),
        );
    if let Some(token) = x_token {
        builder = builder.x_token(Some(token.to_string()))?;
    }
    if grpc_endpoint.starts_with("https://") {
        builder = builder.tls_config(ClientTlsConfig::new().with_native_roots())?;
    }
    let mut client = builder.connect().await?;

    // The server replays only from the first request of a stream (Yellowstone gRPC 17), so
    // the resume point goes into this one. The stream keeps track of it from here on.
    let mut request = request.clone();
    let resume_from_slot = tracked_slot.load(Ordering::Relaxed);
    if resume_from_slot > 0 {
        request.from_slot = Some(resume_from_slot.saturating_sub(1));
    }
    let (mut sink, mut stream) = client.subscribe_with_reconnect(Some(request)).await?;

    while let Some(event) = stream.next().await {
        match event? {
            GeyserReconnectEvent::Update(update) => match &update.update_oneof {
                Some(GeyserUpdateOneof::Ping(_)) => {
                    let ping = GeyserSubscribeRequest {
                        ping: Some(SubscribeRequestPing { id: 1 }),
                        ..Default::default()
                    };
                    if let Err(err) = sink.send(ping).await {
                        warn!("Failed to respond to ping: {err:?}");
                    }
                }
                Some(GeyserUpdateOneof::Pong(_)) => {}
                _ => {
                    update_tracked_slot(&update, tracked_slot);
                    *saw_updates = true;
                    if let Err(err) = events_tx.try_send(GeyserReconnectEvent::Update(update)) {
                        match err {
                            mpsc::error::TrySendError::Full(_) => {
                                warn!("Dropping update: the update channel is full")
                            }
                            mpsc::error::TrySendError::Closed(_) => {
                                anyhow::bail!("the update receiver was dropped")
                            }
                        }
                    }
                }
            },
            GeyserReconnectEvent::Disconnected(disconnected) => {
                info!(
                    "Geyser connection dropped; reconnecting from slot {:?}, {} partial bank(s) to discard",
                    disconnected.replay_from_slot,
                    disconnected.partial_banks.len()
                );
                events_tx
                    .send(GeyserReconnectEvent::Disconnected(disconnected))
                    .await
                    .map_err(|_| anyhow::anyhow!("the update receiver was dropped"))?;
            }
        }
    }
    Ok(())
}

/// The status that ends [`run_geyser_stream`] instead of a new subscription.
///
/// Yellowstone gRPC 17 ends a `Subscribe` stream whose request is larger than the server's
/// `max_decoding_message_size` with `RESOURCE_EXHAUSTED` and a message that names the limit.
/// The reconnecting stream retries it within its budget (it also treats the status as
/// transient) and then ends with it; subscribing again would get the same answer.
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
    use super::run_with_backoff;
    use futures::stream::{self, BoxStream};
    use futures::StreamExt;
    use solana_stream_sdk::yellowstone_grpc_proto::geyser::geyser_server::{Geyser, GeyserServer};
    use solana_stream_sdk::yellowstone_grpc_proto::geyser::subscribe_update::UpdateOneof;
    use solana_stream_sdk::yellowstone_grpc_proto::geyser::{
        GetBlockHeightRequest, GetBlockHeightResponse, GetLatestBlockhashRequest,
        GetLatestBlockhashResponse, GetSlotRequest, GetSlotResponse, GetVersionRequest,
        GetVersionResponse, IsBlockhashValidRequest, IsBlockhashValidResponse, PingRequest,
        PongResponse, SubscribeDeshredRequest, SubscribeGossipRequest, SubscribeReplayInfoRequest,
        SubscribeReplayInfoResponse, SubscribeRequest, SubscribeUpdate, SubscribeUpdateDeshred,
        SubscribeUpdateGossip,
    };
    use solana_stream_sdk::yellowstone_grpc_proto::geyser::{
        SubscribeUpdateAccount, SubscribeUpdateAccountInfo,
    };
    use solana_stream_sdk::{GeyserBackoff, GeyserBankRef, GeyserReconnectEvent};
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

    type UpdateStream = BoxStream<'static, Result<SubscribeUpdate, Status>>;
    type Script = Arc<dyn Fn(usize, Streaming<SubscribeRequest>) -> UpdateStream + Send + Sync>;

    /// A Geyser server whose `Subscribe` streams come from `script`, called with the
    /// 0-based number of the call and the client's request stream.
    struct MockGeyser {
        subscribe_calls: Arc<AtomicUsize>,
        script: Script,
    }

    /// Ends a `Subscribe` stream with RESOURCE_EXHAUSTED when the first request that asks
    /// for something arrives, as a 17.0.0 server does for a request over its
    /// `max_decoding_message_size`. The empty request `subscribe()` sends first passes.
    fn reject_with_resource_exhausted(
        _call: usize,
        requests: Streaming<SubscribeRequest>,
    ) -> UpdateStream {
        Box::pin(stream::once(async move {
            let mut requests = requests;
            while let Some(Ok(request)) = requests.next().await {
                if request != SubscribeRequest::default() {
                    break;
                }
            }
            Err(Status::resource_exhausted(SERVER_MESSAGE))
        }))
    }

    async fn serve(script: Script) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let subscribe_calls = Arc::new(AtomicUsize::new(0));
        let incoming = TcpIncoming::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = incoming.local_addr().unwrap();
        let geyser = MockGeyser {
            subscribe_calls: subscribe_calls.clone(),
            script,
        };
        let server = tokio::spawn(async move {
            let _ = Server::builder()
                .add_service(GeyserServer::new(geyser))
                .serve_with_incoming(incoming)
                .await;
        });
        (format!("http://{addr}"), subscribe_calls, server)
    }

    fn unimplemented<T>() -> Result<Response<T>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    #[tonic::async_trait]
    impl Geyser for MockGeyser {
        type SubscribeStream = BoxStream<'static, Result<SubscribeUpdate, Status>>;
        type SubscribeDeshredStream = BoxStream<'static, Result<SubscribeUpdateDeshred, Status>>;
        type SubscribeGossipStream = BoxStream<'static, Result<SubscribeUpdateGossip, Status>>;

        async fn subscribe(
            &self,
            request: Request<Streaming<SubscribeRequest>>,
        ) -> Result<Response<Self::SubscribeStream>, Status> {
            let call = self.subscribe_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Response::new((self.script)(call, request.into_inner())))
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
        let (endpoint, subscribe_calls, server) =
            serve(Arc::new(reject_with_resource_exhausted)).await;

        // The stream reconnects twice by itself before it ends with the status.
        const RETRIES: u32 = 2;
        let (events_tx, _events_rx) = mpsc::channel(8);
        let request = SubscribeRequest {
            slots: [("slots".to_string(), Default::default())].into(),
            ..Default::default()
        };
        let runner = tokio::spawn(run_with_backoff(
            endpoint,
            None,
            request,
            Arc::new(AtomicU64::new(0)),
            events_tx,
            GeyserBackoff::new(Duration::from_millis(1), 1.0, RETRIES),
        ));

        // A runner that subscribes again does so 1 s after the stream gave up.
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
            calls,
            1 + RETRIES as usize,
            "the runner subscribed again after the stream gave up on RESOURCE_EXHAUSTED"
        );
        let outcome = describe(runner.await.unwrap());
        assert!(
            outcome.contains(SERVER_MESSAGE) && outcome.contains("ResourceExhausted"),
            "the runner did not report the server's status and message: {outcome}"
        );
    }

    /// The first connection delivers one account write of bank (slot 42, bank id 7) and
    /// drops before the bank's block meta; later connections stay open and send nothing.
    fn drop_mid_bank(call: usize, _requests: Streaming<SubscribeRequest>) -> UpdateStream {
        if call > 0 {
            return Box::pin(stream::pending());
        }
        let account = SubscribeUpdate {
            filters: vec!["accounts".to_string()],
            update_oneof: Some(UpdateOneof::Account(SubscribeUpdateAccount {
                account: Some(SubscribeUpdateAccountInfo::default()),
                slot: 42,
                is_startup: false,
                bank_id: Some(7),
            })),
            created_at: None,
        };
        Box::pin(stream::iter(vec![
            Ok(account),
            Err(Status::unavailable("connection reset")),
        ]))
    }

    #[tokio::test]
    async fn a_connection_dropped_mid_bank_reaches_the_receiver_as_disconnected() {
        let (endpoint, _subscribe_calls, server) = serve(Arc::new(drop_mid_bank)).await;
        let request = SubscribeRequest {
            accounts: [("accounts".to_string(), Default::default())].into(),
            ..Default::default()
        };
        let (events_tx, mut events_rx) = mpsc::channel(8);
        let runner = tokio::spawn(run_with_backoff(
            endpoint,
            None,
            request,
            Arc::new(AtomicU64::new(0)),
            events_tx,
            GeyserBackoff::new(Duration::from_millis(1), 1.0, 2),
        ));

        let mut events = Vec::new();
        while events.len() < 2 {
            match tokio::time::timeout(Duration::from_secs(3), events_rx.recv()).await {
                Ok(Some(event)) => events.push(event),
                _ => break,
            }
        }
        runner.abort();
        server.abort();

        assert!(
            matches!(
                events.first(),
                Some(GeyserReconnectEvent::Update(update))
                    if matches!(update.update_oneof, Some(UpdateOneof::Account(_)))
            ),
            "expected the account write first, got {events:?}"
        );
        match events.get(1) {
            Some(GeyserReconnectEvent::Disconnected(disconnected)) => assert_eq!(
                disconnected.partial_banks,
                vec![GeyserBankRef {
                    slot: 42,
                    bank_id: 7
                }],
                "the bank delivered in part must be listed"
            ),
            other => panic!("expected Disconnected after the drop, got {other:?}"),
        }
    }

    /// After the client's first request that asks for something (as a server reads it), a
    /// slot update; then, on even calls, a dropped connection, and on odd calls an open
    /// stream that keeps reading requests.
    fn drop_after_one_slot(call: usize, requests: Streaming<SubscribeRequest>) -> UpdateStream {
        use solana_stream_sdk::yellowstone_grpc_proto::geyser::SubscribeUpdateSlot;
        let slot = SubscribeUpdate {
            filters: vec!["slots".to_string()],
            update_oneof: Some(UpdateOneof::Slot(SubscribeUpdateSlot {
                slot: call as u64 + 1,
                ..Default::default()
            })),
            created_at: None,
        };
        let first_request = stream::once(async move {
            let mut requests = requests;
            while let Some(Ok(request)) = requests.next().await {
                if request != SubscribeRequest::default() {
                    break;
                }
            }
            requests
        });
        Box::pin(first_request.flat_map(move |requests| {
            let tail: UpdateStream = if call % 2 == 0 {
                Box::pin(stream::iter(vec![Err(Status::unavailable(
                    "connection reset",
                ))]))
            } else {
                Box::pin(stream::once(async move {
                    let mut requests = requests;
                    while let Some(Ok(_)) = requests.next().await {}
                    futures::future::pending::<()>().await;
                    Err(Status::cancelled("unreachable"))
                }))
            };
            stream::iter(vec![Ok(slot.clone())]).chain(tail)
        }))
    }

    /// Reconnect latency: from the last update of a dropped connection to the first update
    /// of the next one, as the receiver sees them, with the runner's own settings.
    ///
    /// `cargo test -p geyser-rs --release -- --ignored --nocapture bench_reconnect_gap`
    #[tokio::test]
    #[ignore]
    async fn bench_reconnect_gap() {
        let mut gaps = Vec::new();
        for _ in 0..20 {
            let (endpoint, _calls, server) = serve(Arc::new(drop_after_one_slot)).await;
            let request = SubscribeRequest {
                slots: [("slots".to_string(), Default::default())].into(),
                ..Default::default()
            };
            let (events_tx, mut events_rx) = mpsc::channel(8);
            let runner = tokio::spawn(super::run_geyser_stream(
                endpoint,
                None,
                request,
                Arc::new(AtomicU64::new(0)),
                events_tx,
            ));
            let mut first = None;
            while let Ok(Some(event)) =
                tokio::time::timeout(Duration::from_secs(10), events_rx.recv()).await
            {
                if let GeyserReconnectEvent::Update(_) = event {
                    match first {
                        None => first = Some(tokio::time::Instant::now()),
                        Some(at) => {
                            gaps.push(at.elapsed());
                            break;
                        }
                    }
                }
            }
            runner.abort();
            server.abort();
        }
        gaps.sort();
        println!(
            "reconnect gap over {} runs: median {:?}, min {:?}, max {:?}",
            gaps.len(),
            gaps[gaps.len() / 2],
            gaps[0],
            gaps[gaps.len() - 1]
        );
    }
}
