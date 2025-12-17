use crate::{
    txn::{
        default_token_program_ids, detect_program_hit, first_signatures, parse_pubkeys, MintDetail,
        ProgramHit, ProgramWatchConfig,
    },
    Result, SolanaStreamError,
};
use chrono::{DateTime, LocalResult, TimeZone, Utc};
use futures::future::join_all;
use log::{error, info, warn};
use serde::Deserialize;
use solana_ledger::shred::{
    Shred, Shredder, MAX_CODE_SHREDS_PER_SLOT, MAX_DATA_SHREDS_PER_SLOT, SIZE_OF_NONCE,
};
use solana_packet::PACKET_DATA_SIZE;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, transaction::VersionedTransaction};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::UdpSocket, sync::Mutex};

const COMMON_HEADER_LEN: usize = 83;
const CODING_HEADER_LEN: usize = 6;
const SHRED_PAYLOAD_SIZE: usize = PACKET_DATA_SIZE - SIZE_OF_NONCE;
// Matches `solana_ledger::shred::merkle::ShredData::SIZE_OF_PAYLOAD` (Agave merkle data shreds).
const MERKLE_DATA_SHRED_PAYLOAD_SIZE: usize = 1203;
const MERKLE_DATA_SHRED_PAYLOAD_WITH_NONCE: usize = MERKLE_DATA_SHRED_PAYLOAD_SIZE + SIZE_OF_NONCE;
// Maximum UDP payload we expect to receive (legacy/repair shreds with nonce).
const MAX_UDP_PAYLOAD_SIZE: usize = PACKET_DATA_SIZE;

pub const DEFAULT_BIND_ADDR: &str = "0.0.0.0:10001";
pub const DEFAULT_RPC_ENDPOINT: &str = "https://api.mainnet-beta.solana.com";
pub const DEFAULT_WATCH_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"; // pump.fun program (sample)
pub const DEFAULT_WATCH_AUTHORITY: &str = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"; // pump.fun mint authority (sample)
pub const DEFAULT_COMPLETED_TTL: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_FUTURE_SLOT: u64 = 512;
pub const DEFAULT_STRICT_NUM_DATA: u16 = 32;
pub const DEFAULT_STRICT_NUM_CODING: u16 = 32;
pub const DEFAULT_EVICT_COOLDOWN: Duration = Duration::from_millis(300);

const DEFAULT_MAX_DATAGRAM_SIZE: usize = 65_536;

/// A raw UDP datagram without any decoding assumptions.
#[derive(Debug, Clone)]
pub struct UdpDatagram {
    /// Raw payload bytes.
    pub payload: Vec<u8>,
    /// Timestamp when the packet was received locally.
    pub received_at: Instant,
    /// Source address of the packet.
    pub from: std::net::SocketAddr,
}

/// Minimal UDP receiver for Shredstream traffic.
pub struct UdpShredReceiver {
    socket: UdpSocket,
    buffer: Vec<u8>,
}

impl UdpShredReceiver {
    /// Bind to a local address and prepare a UDP receiver.
    ///
    /// `bind_addr` is the local `ip:port` where shreds will be sent.
    /// `max_datagram_size` allows overriding the receive buffer size; defaults to 64 KiB.
    pub async fn bind(
        bind_addr: impl AsRef<str>,
        max_datagram_size: Option<usize>,
    ) -> Result<Self> {
        let socket = UdpSocket::bind(bind_addr.as_ref()).await?;
        let size = max_datagram_size
            .unwrap_or(DEFAULT_MAX_DATAGRAM_SIZE)
            .max(2048);

        Ok(Self {
            socket,
            buffer: vec![0u8; size],
        })
    }

    /// Return the bound local address.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr> {
        Ok(self.socket.local_addr()?)
    }

    /// Receive a single UDP datagram without decoding.
    pub async fn recv_raw(&mut self) -> Result<UdpDatagram> {
        let (len, from) = self.socket.recv_from(&mut self.buffer).await?;
        let received_at = Instant::now();
        let payload = self.buffer[..len].to_vec();

        Ok(UdpDatagram {
            payload,
            received_at,
            from,
        })
    }
}

/// Attempt to deshred a batch of shreds into `Entry` items.
pub fn deshred_shreds_to_entries(
    shreds: &[solana_ledger::shred::Shred],
) -> Result<Vec<solana_entry::entry::Entry>> {
    let payloads: Vec<&[u8]> = shreds.iter().map(|s| s.payload().as_ref()).collect();
    let data = Shredder::deshred(payloads)
        .map_err(|e| SolanaStreamError::Serialization(format!("deshred failed: {e}")))?;

    bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&data)
        .map_err(|e| SolanaStreamError::Serialization(format!("entry decode failed: {e}")))
}

#[derive(Clone)]
pub struct ShredsUdpConfig {
    pub bind_addr: String,
    pub rpc_endpoint: String,
    pub log_raw: bool,
    pub log_shreds: bool,
    pub log_entries: bool,
    pub log_deshred_attempts: bool,
    pub log_deshred_errors: bool,
    pub require_code_match: bool,
    pub skip_vote_sigs: bool,
    pub log_watch_hits: bool,
    pub log_deferred: bool,
    pub watch_program_ids: Vec<Pubkey>,
    pub watch_authorities: Vec<Pubkey>,
    pub token_program_ids: Vec<Pubkey>,
    pub completed_ttl: Duration,
    pub enable_latency_monitor: bool,
    pub strict_fec: bool,
    pub strict_num_data: u16,
    pub strict_num_coding: u16,
    pub slot_window_root: Option<u64>,
    pub slot_window_max_future: u64,
    pub evict_cooldown: Duration,
    pub warn_once_per_fec: bool,
    pub pump_min_lamports: u64,
}

#[derive(Clone)]
pub struct ShredsUdpState {
    transactions_by_slot: Option<Arc<Mutex<HashMap<u64, Vec<(String, DateTime<Utc>)>>>>>,
    shred_buffer: Arc<Mutex<HashMap<FecKey, ShredBatch>>>,
    completed: Arc<Mutex<HashMap<FecKey, Instant>>>,
    suppressed: Arc<Mutex<HashMap<FecKey, Instant>>>,
    block_time_cache: Option<BlockTimeCache>,
    completed_ttl: Duration,
    suppressed_ttl: Duration,
    warnings: Arc<Mutex<HashMap<FecKey, Instant>>>,
    metrics: Arc<ShredMetrics>,
}

#[derive(Default)]
pub struct ShredMetrics {
    payload_size_mismatch: AtomicU64,
    payload_trailing: AtomicU64,
    payload_len_merkle_data: AtomicU64,
    payload_len_legacy: AtomicU64,
    payload_len_with_nonce: AtomicU64,
    payload_len_other: AtomicU64,
    sanitize_fail_data: AtomicU64,
    sanitize_fail_code: AtomicU64,
    slot_window_future: AtomicU64,
    slot_window_past: AtomicU64,
    fec_mismatch: AtomicU64,
    index_oob: AtomicU64,
    duplicate_conflict: AtomicU64,
    entry_decode_failed: AtomicU64,
    fec_set_evicted_on_decode: AtomicU64,
}

impl ShredMetrics {
    fn inc_payload_size_mismatch(&self) {
        self.payload_size_mismatch.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_payload_trailing(&self) -> u64 {
        self.payload_trailing.fetch_add(1, Ordering::Relaxed)
    }
    fn inc_payload_len_merkle_data(&self) -> u64 {
        self.payload_len_merkle_data.fetch_add(1, Ordering::Relaxed)
    }
    fn inc_payload_len_legacy(&self) -> u64 {
        self.payload_len_legacy.fetch_add(1, Ordering::Relaxed)
    }
    fn inc_payload_len_with_nonce(&self) -> u64 {
        self.payload_len_with_nonce.fetch_add(1, Ordering::Relaxed)
    }
    fn inc_payload_len_other(&self) -> u64 {
        self.payload_len_other.fetch_add(1, Ordering::Relaxed)
    }
    fn inc_sanitize_fail_data(&self) {
        self.sanitize_fail_data.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_sanitize_fail_code(&self) {
        self.sanitize_fail_code.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_slot_window_future(&self) {
        self.slot_window_future.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_slot_window_past(&self) {
        self.slot_window_past.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_fec_mismatch(&self) {
        self.fec_mismatch.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_index_oob(&self) {
        self.index_oob.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_duplicate_conflict(&self) {
        self.duplicate_conflict.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_entry_decode_failed(&self) {
        self.entry_decode_failed.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_fec_set_evicted_on_decode(&self) {
        self.fec_set_evicted_on_decode
            .fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for ShredsUdpConfig {
    fn default() -> Self {
        Self {
            bind_addr: DEFAULT_BIND_ADDR.to_string(),
            rpc_endpoint: DEFAULT_RPC_ENDPOINT.to_string(),
            log_raw: false,
            log_shreds: false,
            log_entries: false,
            log_deshred_attempts: false,
            log_deshred_errors: false,
            require_code_match: false,
            skip_vote_sigs: true,
            log_watch_hits: true,
            log_deferred: false,
            watch_program_ids: Vec::new(),
            watch_authorities: Vec::new(),
            token_program_ids: default_token_program_ids(),
            completed_ttl: DEFAULT_COMPLETED_TTL,
            enable_latency_monitor: false,
            strict_fec: true,
            strict_num_data: DEFAULT_STRICT_NUM_DATA,
            strict_num_coding: DEFAULT_STRICT_NUM_CODING,
            slot_window_root: None,
            slot_window_max_future: DEFAULT_MAX_FUTURE_SLOT,
            evict_cooldown: DEFAULT_EVICT_COOLDOWN,
            warn_once_per_fec: true,
            pump_min_lamports: 0,
        }
    }
}

impl ShredsUdpConfig {
    pub fn defaults() -> Self {
        Self::default()
    }

    pub fn from_embedded(raw: &str) -> Self {
        let mut cfg = Self::defaults();

        if let Some(file_cfg) = load_config_str(raw) {
            info!("Loaded embedded SHREDS_UDP_CONFIG");
            cfg = cfg.apply_file(file_cfg);
        } else {
            warn!("Failed to parse embedded SHREDS_UDP_CONFIG");
        }

        cfg = apply_env_overrides(cfg);
        info!("ShredsUdpConfig {}", cfg.describe());
        cfg
    }

    fn apply_file(mut self, file: ShredsUdpConfigFile) -> Self {
        if let Some(v) = file.bind_addr {
            self.bind_addr = v;
        }
        if let Some(v) = file.rpc_endpoint {
            self.rpc_endpoint = v;
        }
        if let Some(v) = file.log_raw {
            self.log_raw = v;
        }
        if let Some(v) = file.log_shreds {
            self.log_shreds = v;
        }
        if let Some(v) = file.log_entries {
            self.log_entries = v;
        }
        if let Some(v) = file.log_deshred_attempts {
            self.log_deshred_attempts = v;
        }
        if let Some(v) = file.log_deshred_errors {
            self.log_deshred_errors = v;
        }
        if let Some(v) = file.require_code_match {
            self.require_code_match = v;
        }
        if let Some(v) = file.skip_vote_sigs {
            self.skip_vote_sigs = v;
        }
        if let Some(v) = file.log_watch_hits {
            self.log_watch_hits = v;
        }
        if let Some(v) = file.log_deferred {
            self.log_deferred = v;
        }
        if let Some(v) = file.watch_program_ids {
            self.watch_program_ids = parse_pubkeys(Some(v.as_str()), &[]);
        }
        if let Some(v) = file.watch_authorities {
            self.watch_authorities = parse_pubkeys(Some(v.as_str()), &[]);
        }
        if let Some(v) = file.token_program_ids {
            self.token_program_ids = parse_pubkeys(Some(v.as_str()), &[]);
        }
        if let Some(ms) = file.completed_ttl_ms {
            self.completed_ttl = Duration::from_millis(ms);
        }
        if let Some(v) = file.enable_latency_monitor {
            self.enable_latency_monitor = v;
        }
        if let Some(v) = file.strict_fec {
            self.strict_fec = v;
        }
        if let Some(v) = file.strict_num_data {
            self.strict_num_data = v;
        }
        if let Some(v) = file.strict_num_coding {
            self.strict_num_coding = v;
        }
        if let Some(v) = file.slot_window_root {
            self.slot_window_root = Some(v);
        }
        if let Some(v) = file.slot_window_max_future {
            self.slot_window_max_future = v;
        }
        if let Some(ms) = file.evict_cooldown_ms {
            self.evict_cooldown = Duration::from_millis(ms);
        }
        if let Some(v) = file.warn_once_per_fec {
            self.warn_once_per_fec = v;
        }
        if let Some(v) = file.pump_min_lamports {
            self.pump_min_lamports = v;
        }
        self
    }

    fn env_override_pubkeys(primary_var: &str, legacy_var: &str) -> Option<Vec<Pubkey>> {
        if let Ok(raw) = env::var(primary_var) {
            return Some(parse_pubkeys(Some(raw.as_str()), &[]));
        }
        if let Ok(raw) = env::var(legacy_var) {
            return Some(parse_pubkeys(Some(raw.as_str()), &[]));
        }
        None
    }

    pub fn from_env() -> Self {
        let mut cfg = Self::defaults();

        let cwd = env::current_dir().ok();
        let exe_dir = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));
        let cwd_display = cwd
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let exe_display = exe_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let env_config = env::var("SHREDS_UDP_CONFIG").ok();
        let (config_path, searched_paths) =
            resolve_config_path(env_config.as_deref(), cwd.as_deref(), exe_dir.as_deref());

        if let Some(raw) = env_config.as_ref() {
            if !Path::new(raw).exists() {
                warn!(
                    "SHREDS_UDP_CONFIG={} not found; falling back to search paths",
                    raw
                );
            }
        }

        if let Some(path) = config_path {
            if let Some(file_cfg) = load_config_file(&path) {
                info!(
                    "Loaded SHREDS_UDP_CONFIG from {} (cwd={}, exe_dir={})",
                    path.display(),
                    cwd_display,
                    exe_display
                );
                cfg = cfg.apply_file(file_cfg);
            } else {
                warn!("Failed to load SHREDS_UDP_CONFIG from {}", path.display());
            }
        } else {
            warn!(
                "No SHREDS_UDP_CONFIG found (cwd={}, exe_dir={}, searched={})",
                cwd_display,
                exe_display,
                searched_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        cfg = apply_env_overrides(cfg);
        info!("ShredsUdpConfig {}", cfg.describe());
        cfg
    }

    pub fn watch_config(&self) -> ProgramWatchConfig {
        ProgramWatchConfig::new(
            if self.watch_program_ids.is_empty() {
                parse_pubkeys(None, &[DEFAULT_WATCH_PROGRAM_ID])
            } else {
                self.watch_program_ids.clone()
            },
            if self.watch_authorities.is_empty() {
                parse_pubkeys(None, &[DEFAULT_WATCH_AUTHORITY])
            } else {
                self.watch_authorities.clone()
            },
        )
        .with_token_program_ids(if self.token_program_ids.is_empty() {
            default_token_program_ids()
        } else {
            self.token_program_ids.clone()
        })
        .with_skip_vote_txs(self.skip_vote_sigs)
    }

    /// Build a watch config without populating pump.fun defaults when the lists are empty.
    pub fn watch_config_no_defaults(&self) -> ProgramWatchConfig {
        ProgramWatchConfig::new(self.watch_program_ids.clone(), self.watch_authorities.clone())
            .with_token_program_ids(if self.token_program_ids.is_empty() {
                default_token_program_ids()
            } else {
                self.token_program_ids.clone()
            })
            .with_skip_vote_txs(self.skip_vote_sigs)
    }

    pub fn describe(&self) -> String {
        format!(
            "bind_addr={} rpc={} slot_window_root={:?} max_future={} strict_fec={} num_data={} num_coding={} require_code_match={} log_raw={} log_shreds={} log_entries={} log_deshred_attempts={} evict_cooldown_ms={} completed_ttl_ms={} warn_once_per_fec={} pump_min_lamports={}",
            self.bind_addr,
            self.rpc_endpoint,
            self.slot_window_root,
            self.slot_window_max_future,
            self.strict_fec,
            self.strict_num_data,
            self.strict_num_coding,
            self.require_code_match,
            self.log_raw,
            self.log_shreds,
            self.log_entries,
            self.log_deshred_attempts,
            self.evict_cooldown.as_millis(),
            self.completed_ttl.as_millis(),
            self.warn_once_per_fec,
            self.pump_min_lamports,
        )
    }
}

impl ShredsUdpState {
    pub fn new(cfg: &ShredsUdpConfig) -> Self {
        Self {
            transactions_by_slot: cfg
                .enable_latency_monitor
                .then(|| Arc::new(Mutex::new(HashMap::new()))),
            shred_buffer: Arc::new(Mutex::new(HashMap::new())),
            completed: Arc::new(Mutex::new(HashMap::new())),
            suppressed: Arc::new(Mutex::new(HashMap::new())),
            block_time_cache: cfg
                .enable_latency_monitor
                .then(|| BlockTimeCache::new(&cfg.rpc_endpoint)),
            completed_ttl: cfg.completed_ttl,
            suppressed_ttl: cfg.evict_cooldown,
            warnings: Arc::new(Mutex::new(HashMap::new())),
            metrics: Arc::new(ShredMetrics::default()),
        }
    }

    pub fn block_time_cache(&self) -> Option<BlockTimeCache> {
        self.block_time_cache.clone()
    }

    pub fn transactions_by_slot(
        &self,
    ) -> Option<Arc<Mutex<HashMap<u64, Vec<(String, DateTime<Utc>)>>>>> {
        self.transactions_by_slot.clone()
    }

    pub fn completed_ttl(&self) -> Duration {
        self.completed_ttl
    }

    pub fn suppressed_ttl(&self) -> Duration {
        self.suppressed_ttl
    }

    pub fn metrics(&self) -> Arc<ShredMetrics> {
        self.metrics.clone()
    }

    pub async fn remove_batch(&self, key: &FecKey) {
        self.shred_buffer.lock().await.remove(key);
    }

    pub async fn mark_completed(&self, key: FecKey) {
        self.completed.lock().await.insert(key, Instant::now());
    }

    pub async fn mark_suppressed(&self, key: FecKey) {
        self.suppressed.lock().await.insert(key, Instant::now());
    }
}

pub async fn run_shreds_udp(
    cfg: ShredsUdpConfig,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut receiver = UdpShredReceiver::bind(&cfg.bind_addr, None).await?;
    let local_addr = receiver.local_addr()?;
    info!("Listening for UDP shreds on {}", local_addr);
    info!("Ensure the sender targets this ip:port.");

    let watch_cfg = Arc::new(cfg.watch_config());
    let policy = DeshredPolicy {
        require_code_match: cfg.require_code_match,
    };
    let state = ShredsUdpState::new(&cfg);

    let latency_handle = if cfg.enable_latency_monitor {
        if let (Some(cache), Some(txs)) = (state.block_time_cache(), state.transactions_by_slot()) {
            Some(tokio::spawn(async move {
                latency_monitor_task(cache, txs).await;
            }))
        } else {
            None
        }
    } else {
        None
    };

    let receive_handle = {
        let watch_cfg = watch_cfg.clone();
        let cfg = cfg.clone();
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = handle_pumpfun_watcher(
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
        tokio::try_join!(latency_handle, receive_handle)
            .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))?;
    } else {
        receive_handle
            .await
            .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))?;
    }
    Ok(())
}

#[derive(Clone)]
struct ShredBatch {
    data_shreds: HashMap<u32, Shred>,
    code_shreds: HashMap<u32, Shred>,
    required_data: Option<usize>,
    required_data_from_data: Option<usize>,
    required_data_from_code: Option<usize>,
    data_complete_seen: bool,
    expected_first_coding_index: Option<u32>,
    expected_num_data: Option<u16>,
    expected_num_coding: Option<u16>,
    last_attempted_count: usize,
    dup_data: usize,
    dup_code: usize,
}

#[derive(Debug)]
pub struct BatchStatus {
    pub data_len: usize,
    pub code_len: usize,
    pub required_data: Option<usize>,
    pub required_data_from_data: Option<usize>,
    pub required_data_from_code: Option<usize>,
    pub data_complete_seen: bool,
    pub missing: Vec<u32>,
    pub missing_ranges: Vec<(u32, u32)>,
    pub dup_data: usize,
    pub dup_code: usize,
    pub expected_first_coding_index: Option<u32>,
    pub expected_num_data: Option<u16>,
    pub expected_num_coding: Option<u16>,
    pub coding_summary: CodingHeaderSummary,
}

#[derive(Clone, Debug)]
struct CodingHeaderInfo {
    num_data_shreds: u16,
    num_coding_shreds: u16,
    position: u16,
    first_coding_index: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FecKey {
    pub slot: u64,
    pub version: u16,
    pub fec_set: u32,
}

#[derive(Clone, Debug, Default)]
pub struct CodingHeaderSummary {
    pub parsed: usize,
    pub invalid: usize,
    pub num_data_shreds: Vec<usize>,
    pub num_coding_shreds: Vec<usize>,
    pub first_coding_indices: Vec<u32>,
    pub positions_preview: Vec<u16>,
}

impl CodingHeaderSummary {
    fn consistent_num_data(&self) -> Option<usize> {
        (self.num_data_shreds.len() == 1).then(|| self.num_data_shreds[0])
    }

    fn consistent_first_index(&self) -> Option<u32> {
        (self.first_coding_indices.len() == 1).then(|| self.first_coding_indices[0])
    }

    fn describe(&self) -> String {
        format!(
            "parsed={} invalid={} num_data={:?} num_coding={:?} first_coding_index={:?} pos_sample={:?}",
            self.parsed,
            self.invalid,
            self.num_data_shreds,
            self.num_coding_shreds,
            self.first_coding_indices,
            self.positions_preview
        )
    }
}

#[derive(Clone, Copy)]
pub struct DeshredPolicy {
    pub require_code_match: bool,
}

enum ReadyToDeshred {
    Ready(Vec<Shred>),
    Gated(String),
    NotReady,
}

#[derive(Clone, Copy, Debug)]
pub enum ShredSource {
    Data,
    Coding,
}

#[derive(Debug)]
pub struct ShredReadyBatch {
    pub key: FecKey,
    pub shreds: Vec<Shred>,
    pub status: Option<BatchStatus>,
    pub source: ShredSource,
}

#[derive(Debug)]
pub enum ShredInsertOutcome {
    Ready(ShredReadyBatch),
    Deferred {
        key: FecKey,
        source: ShredSource,
        reason: String,
        status: Option<BatchStatus>,
    },
    Buffered {
        key: FecKey,
        source: ShredSource,
    },
    Skipped,
}

#[derive(Debug)]
pub struct WatchEvent {
    pub slot: u64,
    pub hit: ProgramHit,
    pub details: Vec<MintDetail>,
}

#[derive(Clone)]
pub struct BlockTimeCache {
    rpc_client: Arc<RpcClient>,
    cache: Arc<Mutex<HashMap<u64, i64>>>,
    fetching: Arc<Mutex<HashSet<u64>>>,
}

impl BlockTimeCache {
    pub fn new(rpc_endpoint: &str) -> Self {
        Self {
            rpc_client: Arc::new(RpcClient::new(rpc_endpoint.to_string())),
            cache: Arc::new(Mutex::new(HashMap::new())),
            fetching: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    async fn get_block_time(&self, slot: u64) -> Option<i64> {
        {
            let cache = self.cache.lock().await;
            if let Some(time) = cache.get(&slot) {
                return Some(*time);
            }
        }

        {
            let mut fetching = self.fetching.lock().await;
            if fetching.contains(&slot) {
                return None;
            }
            fetching.insert(slot);
        }

        let block_time_result = self.rpc_client.get_block_time(slot).await;

        let block_time = match block_time_result {
            Ok(time) => Some(time),
            Err(err) => {
                if format!("{:?}", err).contains("Block not available") {
                    None
                } else {
                    error!("Error fetching block time for slot {}: {:?}", slot, err);
                    None
                }
            }
        };

        {
            let mut cache = self.cache.lock().await;
            if let Some(time) = block_time {
                cache.insert(slot, time);
                if cache.len() > 20 {
                    if let Some(oldest_slot) = cache.keys().next().cloned() {
                        cache.remove(&oldest_slot);
                    }
                }
            }
        }

        let mut fetching = self.fetching.lock().await;
        fetching.remove(&slot);

        block_time
    }
}

fn resolve_config_path(
    env_override: Option<&str>,
    cwd: Option<&Path>,
    exe_dir: Option<&Path>,
) -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut searched = Vec::new();

    if let Some(raw) = env_override {
        let path = PathBuf::from(raw);
        searched.push(path.clone());
        if path.exists() {
            return (Some(path), searched);
        }
    }

    let mut candidates = Vec::new();
    if let Some(dir) = cwd {
        candidates.push(dir.join("settings.jsonc"));
        candidates.push(dir.join("settings.json"));
    }
    if let Some(dir) = exe_dir {
        candidates.push(dir.join("settings.jsonc"));
        candidates.push(dir.join("settings.json"));
    }
    candidates.push(PathBuf::from("client/shreds-udp-rs/settings.jsonc"));
    candidates.push(PathBuf::from("client/shreds-udp-rs/settings.json"));
    candidates.push(PathBuf::from("src/settings.jsonc"));
    candidates.push(PathBuf::from("src/settings.json"));

    let mut seen = HashSet::new();
    for path in candidates {
        if !seen.insert(path.clone()) {
            continue;
        }
        searched.push(path.clone());
        if path.exists() {
            return (Some(path), searched);
        }
    }

    (None, searched)
}

fn env_bool_opt(name: &str) -> Option<bool> {
    env::var(name).ok().and_then(|v| {
        let v = v.to_ascii_lowercase();
        match v.as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

fn env_parse_u64(name: &str) -> Option<u64> {
    env::var(name).ok().and_then(|v| v.parse::<u64>().ok())
}

fn env_parse_u16(name: &str) -> Option<u16> {
    env::var(name).ok().and_then(|v| v.parse::<u16>().ok())
}

#[derive(Debug, Deserialize, Default)]
struct ShredsUdpConfigFile {
    bind_addr: Option<String>,
    rpc_endpoint: Option<String>,
    log_raw: Option<bool>,
    log_shreds: Option<bool>,
    log_entries: Option<bool>,
    log_deshred_attempts: Option<bool>,
    log_deshred_errors: Option<bool>,
    require_code_match: Option<bool>,
    skip_vote_sigs: Option<bool>,
    log_watch_hits: Option<bool>,
    log_deferred: Option<bool>,
    watch_program_ids: Option<String>,
    watch_authorities: Option<String>,
    token_program_ids: Option<String>,
    completed_ttl_ms: Option<u64>,
    enable_latency_monitor: Option<bool>,
    strict_fec: Option<bool>,
    strict_num_data: Option<u16>,
    strict_num_coding: Option<u16>,
    slot_window_root: Option<u64>,
    slot_window_max_future: Option<u64>,
    evict_cooldown_ms: Option<u64>,
    warn_once_per_fec: Option<bool>,
    pump_min_lamports: Option<u64>,
}

fn load_config_file(path: &Path) -> Option<ShredsUdpConfigFile> {
    let raw = fs::read_to_string(path).ok()?;
    load_config_str(&raw)
}

fn load_config_str(raw: &str) -> Option<ShredsUdpConfigFile> {
    serde_jsonc::from_str(raw)
        .or_else(|_| toml::from_str(raw))
        .ok()
}

fn parse_watch_list(primary_var: &str, legacy_var: &str, defaults: &[&str]) -> Vec<Pubkey> {
    if let Ok(raw) = env::var(primary_var) {
        let parsed = parse_pubkeys(Some(raw.as_str()), &[]);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    if let Ok(raw) = env::var(legacy_var) {
        let parsed = parse_pubkeys(Some(raw.as_str()), defaults);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    parse_pubkeys(None, defaults)
}

fn apply_env_overrides(mut cfg: ShredsUdpConfig) -> ShredsUdpConfig {
    let log_raw = env_bool_opt("SHREDS_UDP_LOG_RAW");
    let log_shreds = env_bool_opt("SHREDS_UDP_LOG_SHREDS");
    let log_entries = env_bool_opt("SHREDS_UDP_LOG_ENTRIES");
    let log_deshred_attempts = env_bool_opt("SHREDS_UDP_LOG_DESHRED_ATTEMPTS");
    let log_deshred_errors = env_bool_opt("SHREDS_UDP_LOG_DESHRED_ERRORS");
    let require_code_match = env_bool_opt("SHREDS_UDP_REQUIRE_CODE_MATCH");
    let skip_vote_sigs = env_bool_opt("SHREDS_UDP_SKIP_VOTE_SIGS");
    let log_watch_hits = env_bool_opt("SHREDS_UDP_LOG_WATCH_HITS");
    let log_deferred = env_bool_opt("SHREDS_UDP_LOG_DEFER");
    let watch_program_ids = {
        let primary = parse_watch_list(
            "SHREDS_UDP_WATCH_PROGRAM_IDS",
            "SHREDS_UDP_PUMPFUN_PROGRAM_IDS",
            &[],
        );
        if primary.is_empty() {
            parse_watch_list(
                "SHREDS_UDP_WATCH_PROGRAM_IDS",
                "SHREDS_UDP_PUMPFUN_PROGRAM_IDS",
                &[DEFAULT_WATCH_PROGRAM_ID],
            )
        } else {
            primary
        }
    };
    let watch_authorities = {
        let primary = parse_watch_list(
            "SHREDS_UDP_WATCH_AUTHORITIES",
            "SHREDS_UDP_PUMPFUN_AUTHORITIES",
            &[],
        );
        if primary.is_empty() {
            parse_watch_list(
                "SHREDS_UDP_WATCH_AUTHORITIES",
                "SHREDS_UDP_PUMPFUN_AUTHORITIES",
                &[DEFAULT_WATCH_AUTHORITY],
            )
        } else {
            primary
        }
    };
    let strict_fec = env_bool_opt("SHREDS_UDP_STRICT_FEC").unwrap_or(cfg.strict_fec);
    let strict_num_data = env_parse_u16("SHREDS_UDP_STRICT_NUM_DATA").unwrap_or(cfg.strict_num_data);
    let strict_num_coding =
        env_parse_u16("SHREDS_UDP_STRICT_NUM_CODING").unwrap_or(cfg.strict_num_coding);
    let slot_window_root = env_parse_u64("SHREDS_UDP_ROOT_SLOT");
    let slot_window_max_future =
        env_parse_u64("SHREDS_UDP_MAX_FUTURE").unwrap_or(cfg.slot_window_max_future);
    let evict_cooldown = env_parse_u64("SHREDS_UDP_EVICT_COOLDOWN_MS").map(Duration::from_millis);
    let warn_once_per_fec = env_bool_opt("SHREDS_UDP_WARN_ONCE");
    let pump_min_lamports = env_parse_u64("SHREDS_UDP_PUMP_MIN_LAMPORTS");

    cfg.rpc_endpoint = env::var("SOLANA_RPC_ENDPOINT").unwrap_or(cfg.rpc_endpoint);
    if let Some(v) = log_raw {
        cfg.log_raw = v;
    }
    if let Some(v) = log_shreds {
        cfg.log_shreds = v;
    }
    if let Some(v) = log_entries {
        cfg.log_entries = v;
    }
    if let Some(v) = log_deshred_attempts {
        cfg.log_deshred_attempts = v;
    }
    if let Some(v) = log_deshred_errors {
        cfg.log_deshred_errors = v;
    }
    if let Some(v) = require_code_match {
        cfg.require_code_match = v;
    }
    if let Some(v) = skip_vote_sigs {
        cfg.skip_vote_sigs = v;
    }
    if let Some(v) = log_watch_hits {
        cfg.log_watch_hits = v;
    }
    if let Some(v) = log_deferred {
        cfg.log_deferred = v;
    }
    if let Some(list) = ShredsUdpConfig::env_override_pubkeys(
        "SHREDS_UDP_WATCH_PROGRAM_IDS",
        "SHREDS_UDP_PUMPFUN_PROGRAM_IDS",
    ) {
        if !list.is_empty() {
            cfg.watch_program_ids = list;
        }
    } else {
        cfg.watch_program_ids = watch_program_ids;
    }
    if let Some(list) = ShredsUdpConfig::env_override_pubkeys(
        "SHREDS_UDP_WATCH_AUTHORITIES",
        "SHREDS_UDP_PUMPFUN_AUTHORITIES",
    ) {
        if !list.is_empty() {
            cfg.watch_authorities = list;
        }
    } else {
        cfg.watch_authorities = watch_authorities;
    }
    cfg.token_program_ids = default_token_program_ids();
    if let Some(ms) = env_parse_u64("SHREDS_UDP_COMPLETED_TTL_MS") {
        cfg.completed_ttl = Duration::from_millis(ms);
    }
    cfg.enable_latency_monitor =
        env_bool_opt("SHREDS_UDP_ENABLE_LATENCY").unwrap_or(cfg.enable_latency_monitor);
    cfg.strict_fec = strict_fec;
    cfg.strict_num_data = strict_num_data;
    cfg.strict_num_coding = strict_num_coding;
    cfg.slot_window_root = slot_window_root.or(cfg.slot_window_root);
    cfg.slot_window_max_future = slot_window_max_future;
    if let Some(ms) = evict_cooldown {
        cfg.evict_cooldown = ms;
    }
    if let Some(v) = warn_once_per_fec {
        cfg.warn_once_per_fec = v;
    }
    if let Some(v) = pump_min_lamports {
        cfg.pump_min_lamports = v;
    }

    cfg
}

async fn prepare_log_message(
    slot: u64,
    transactions_by_slot: &Arc<Mutex<HashMap<u64, Vec<(String, DateTime<Utc>)>>>>,
) {
    let received_time = Utc::now();
    transactions_by_slot
        .lock()
        .await
        .entry(slot)
        .or_default()
        .push(("dummy_signature".to_string(), received_time));
}

pub async fn latency_monitor_task(
    block_time_cache: BlockTimeCache,
    transactions_by_slot: Arc<Mutex<HashMap<u64, Vec<(String, DateTime<Utc>)>>>>,
) {
    const MAX_LATENCIES: usize = 420;
    let mut latency_buffer = Vec::new();

    loop {
        tokio::time::sleep(Duration::from_millis(420)).await;

        let slots: Vec<u64> = transactions_by_slot.lock().await.keys().cloned().collect();

        let block_time_futures = slots.iter().map(|&slot| {
            let value = block_time_cache.clone();
            async move {
                let block_time = value.get_block_time(slot).await;
                (slot, block_time)
            }
        });

        let slot_block_times = join_all(block_time_futures).await;

        for (slot, block_time_unix_opt) in slot_block_times {
            if let Some(block_time_unix) = block_time_unix_opt {
                let block_time = match Utc.timestamp_opt(block_time_unix, 0) {
                    LocalResult::Single(t) => t,
                    LocalResult::None => {
                        error!("Invalid block time for slot {}", slot);
                        continue;
                    }
                    _ => {
                        error!("Unexpected error fetching block time for slot {}", slot);
                        continue;
                    }
                };

                let txs = transactions_by_slot
                    .lock()
                    .await
                    .remove(&slot)
                    .unwrap_or_default();

                for (_, recv_time) in txs {
                    let latency = recv_time
                        .signed_duration_since(block_time)
                        .num_milliseconds()
                        .saturating_sub(500)
                        .max(0);
                    latency_buffer.push(latency);
                    if latency_buffer.len() > MAX_LATENCIES {
                        latency_buffer.remove(0);
                    }

                    let avg_latency =
                        latency_buffer.iter().sum::<i64>() as f64 / latency_buffer.len() as f64;

                    info!(
                        "Slot: {}\n⏰ BlockTime: {}\n📥 ReceivedAt: {}\n🚀 Adjusted Latency: {} ms\n📊 Average Latency (latest {}): {:.2} ms\n",
                        slot,
                        block_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        recv_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        latency,
                        latency_buffer.len(),
                        avg_latency
                    );
                }
            }
        }
    }
}

pub async fn handle_pumpfun_watcher(
    receiver: &mut UdpShredReceiver,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
    policy: DeshredPolicy,
    watch_cfg: Arc<ProgramWatchConfig>,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let datagram = receiver.recv_raw().await?;
    let payload_len = datagram.payload.len();
    if cfg.log_raw {
        let recv_ts = Utc::now();
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

    if let Some(shred_info) = decode_udp_datagram(&datagram, state, cfg).await {
        match insert_shred(
            shred_info,
            &datagram,
            state,
            cfg,
            &policy,
        )
        .await
        {
            ShredInsertOutcome::Ready(ready) => {
                if cfg.log_deshred_attempts {
                    if let Some(st) = &ready.status {
                        info!(
                            "deshred attempt {}",
                            format_status(ready.key.slot, ready.key.version, ready.key.fec_set, st)
                        );
                    }
                }
                process_ready_batch(ready, state, cfg, watch_cfg.clone()).await;
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
                            format_status(key.slot, key.version, key.fec_set, &st)
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
        return Ok(());
    }

    if cfg.log_raw {
        info!("payload not recognized as Shred; raw size {}", payload_len);
    }

    Ok(())
}

/// Decode and prefilter a UDP datagram into a sanitized shred.
pub async fn decode_udp_datagram(
    datagram: &UdpDatagram,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
) -> Option<DecodedShred> {
    prefilter_shred(datagram, state, cfg).await
}

/// Insert a decoded shred into the in-memory FEC buffer and report readiness.
pub async fn insert_shred(
    decoded: DecodedShred,
    datagram: &UdpDatagram,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
    policy: &DeshredPolicy,
) -> ShredInsertOutcome {
    let slot = decoded.shred.slot();
    let version = decoded.shred.version();
    let fec_set = decoded.shred.fec_set_index();
    let key = FecKey {
        slot,
        version,
        fec_set,
    };
    let metrics = state.metrics();

    {
        let mut done = state.completed.lock().await;
        done.retain(|_, until| until.elapsed() < state.completed_ttl());
        if done.contains_key(&key) {
            return ShredInsertOutcome::Skipped;
        }
    }
    {
        let mut sup = state.suppressed.lock().await;
        sup.retain(|_, until| until.elapsed() < state.suppressed_ttl());
        if sup.contains_key(&key) {
            return ShredInsertOutcome::Skipped;
        }
    }

    match &decoded.shred {
        Shred::ShredData(_) => {
            process_data_shred(decoded, datagram, state, cfg, policy, key, metrics).await
        }
        Shred::ShredCode(_) => {
            process_code_shred(decoded, datagram, state, cfg, policy, key, metrics).await
        }
    }
}

async fn process_data_shred(
    decoded: DecodedShred,
    datagram: &UdpDatagram,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
    policy: &DeshredPolicy,
    key: FecKey,
    metrics: Arc<ShredMetrics>,
) -> ShredInsertOutcome {
    if let Some(txs) = state.transactions_by_slot() {
        prepare_log_message(key.slot, &txs).await;
    }
    let last = decoded.shred.last_in_slot();
    let complete = decoded.shred.data_complete();
    if cfg.log_shreds {
        info!(
            "shred DATA slot={} idx={} ver={} fec_set={} last={} complete={} from={} bytes={} canonical={}",
            key.slot,
            decoded.shred.index(),
            key.version,
            key.fec_set,
            last,
            complete,
            datagram.from,
            decoded.received_len,
            decoded.canonical_payload_len(),
        );
    }
    let ready = {
        let mut buf = state.shred_buffer.lock().await;
        let entry = buf.entry(key).or_insert_with(ShredBatch::new);

        if last || complete {
            let required = (decoded.shred.index().saturating_sub(key.fec_set) as usize) + 1;
            entry.update_required_data_from_data(required);
        }

        entry.insert_data_shred(decoded.shred.clone(), metrics.as_ref());
        entry.ready_to_deshred(policy)
    };

    match ready {
        ReadyToDeshred::Ready(shreds) => {
            let status = {
                let buf = state.shred_buffer.lock().await;
                buf.get(&key).map(|b| b.status(key.fec_set))
            };
            ShredInsertOutcome::Ready(ShredReadyBatch {
                key,
                shreds,
                status,
                source: ShredSource::Data,
            })
        }
        ReadyToDeshred::Gated(reason) => {
            let status = {
                let buf = state.shred_buffer.lock().await;
                buf.get(&key).map(|b| b.status(key.fec_set))
            };
            ShredInsertOutcome::Deferred {
                key,
                source: ShredSource::Data,
                reason,
                status,
            }
        }
        ReadyToDeshred::NotReady => ShredInsertOutcome::Buffered {
            key,
            source: ShredSource::Data,
        },
    }
}

async fn process_code_shred(
    decoded: DecodedShred,
    datagram: &UdpDatagram,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
    policy: &DeshredPolicy,
    key: FecKey,
    metrics: Arc<ShredMetrics>,
) -> ShredInsertOutcome {
    let ready = {
        let mut buf = state.shred_buffer.lock().await;
        let entry = buf.entry(key).or_insert_with(ShredBatch::new);

        if let Some(header) = decode_coding_header(&decoded.shred) {
            entry.update_required_data_from_code(header.num_data_shreds as usize);
        }

        entry.insert_code_shred(decoded.shred.clone(), metrics.as_ref());
        entry.ready_to_deshred(policy)
    };

    let outcome = match ready {
        ReadyToDeshred::Ready(shreds) => {
            let status = {
                let buf = state.shred_buffer.lock().await;
                buf.get(&key).map(|b| b.status(key.fec_set))
            };
            ShredInsertOutcome::Ready(ShredReadyBatch {
                key,
                shreds,
                status,
                source: ShredSource::Coding,
            })
        }
        ReadyToDeshred::Gated(reason) => {
            let status = {
                let buf = state.shred_buffer.lock().await;
                buf.get(&key).map(|b| b.status(key.fec_set))
            };
            ShredInsertOutcome::Deferred {
                key,
                source: ShredSource::Coding,
                reason,
                status,
            }
        }
        ReadyToDeshred::NotReady => ShredInsertOutcome::Buffered {
            key,
            source: ShredSource::Coding,
        },
    };

    if cfg.log_shreds {
        info!(
            "shred CODE slot={} idx={} ver={} fec_set={} from={} bytes={} canonical={}",
            key.slot,
            decoded.shred.index(),
            key.version,
            key.fec_set,
            datagram.from,
            decoded.received_len,
            decoded.canonical_payload_len(),
        );
    }

    outcome
}

async fn process_ready_batch(
    ready: ShredReadyBatch,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
    watch_cfg: Arc<ProgramWatchConfig>,
) {
    let metrics = state.metrics();
    let ShredReadyBatch {
        key,
        shreds,
        status,
        source,
    } = ready;

    match deshred_shreds_to_entries(&shreds) {
        Ok(entries) => {
            let txs: Vec<&VersionedTransaction> =
                entries.iter().flat_map(|e| e.transactions.iter()).collect();
            info!(
                "deshred slot={} entries={} txs={}",
                key.slot,
                entries.len(),
                txs.len()
            );

            log_watch_events(
                key.slot,
                &txs,
                watch_cfg.as_ref(),
                cfg.log_watch_hits,
                cfg.pump_min_lamports,
            );

            if cfg.log_entries {
                let sigs: Vec<String> = first_signatures(
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
            if matches!(source, ShredSource::Data) {
                state.mark_completed(key).await;
            }
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("entry decode failed")
                || err_str.contains("invalid transaction message version")
            {
                metrics.inc_entry_decode_failed();
                metrics.inc_fec_set_evicted_on_decode();
            }
            if cfg.log_deshred_errors {
                error!(
                    "deshred failed for slot {} fec_set {}: {}",
                    key.slot, key.fec_set, e
                );
                if let Some(st) = status {
                    error!(
                        "deshred context {}",
                        format_status(key.slot, key.version, key.fec_set, &st)
                    );
                }
            }
            state.remove_batch(&key).await;
            state.mark_suppressed(key).await;
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedShred {
    pub shred: Shred,
    pub received_len: usize,
    pub canonical_len: usize,
}

impl DecodedShred {
    pub fn canonical_payload_len(&self) -> usize {
        self.canonical_len
    }
}

fn decode_shred(payload: &[u8]) -> Option<DecodedShred> {
    match Shred::new_from_serialized_shred(payload.to_vec()) {
        Ok(shred) => {
            let canonical_len = shred.payload().len();
            Some(DecodedShred {
                received_len: payload.len(),
                canonical_len,
                shred,
            })
        }
        Err(_) => None,
    }
}

async fn prefilter_shred(
    datagram: &UdpDatagram,
    state: &ShredsUdpState,
    cfg: &ShredsUdpConfig,
) -> Option<DecodedShred> {
    let metrics = state.metrics();
    let payload_len = datagram.payload.len();

    match payload_len {
        MERKLE_DATA_SHRED_PAYLOAD_SIZE => {
            if metrics.inc_payload_len_merkle_data() == 0 {
                info!(
                    "observed merkle data shred payload len={} from {} (Agave merkle sizing)",
                    payload_len, datagram.from
                );
            }
        }
        SHRED_PAYLOAD_SIZE => {
            metrics.inc_payload_len_legacy();
        }
        PACKET_DATA_SIZE | MERKLE_DATA_SHRED_PAYLOAD_WITH_NONCE => {
            metrics.inc_payload_len_with_nonce();
        }
        _ => {
            let n = metrics.inc_payload_len_other();
            if n % 50 == 0 {
                warn!(
                    "unexpected UDP payload len={} from {} (merkle={} legacy={} with_nonce={} max={})",
                    payload_len,
                    datagram.from,
                    MERKLE_DATA_SHRED_PAYLOAD_SIZE,
                    SHRED_PAYLOAD_SIZE,
                    PACKET_DATA_SIZE,
                    MAX_UDP_PAYLOAD_SIZE
                );
            }
        }
    }

    if payload_len > MAX_UDP_PAYLOAD_SIZE {
        metrics.inc_payload_size_mismatch();
        warn!(
            "drop packet too large len={} (max {}); from {}",
            payload_len, MAX_UDP_PAYLOAD_SIZE, datagram.from
        );
        return None;
    }

    if payload_len < COMMON_HEADER_LEN {
        metrics.inc_payload_size_mismatch();
        warn!(
            "drop packet too small len={} (need at least {}) from {}",
            payload_len, COMMON_HEADER_LEN, datagram.from
        );
        return None;
    }

    let decoded = match decode_shred(datagram.payload.as_slice()) {
        Some(s) => s,
        None => {
            let n = metrics
                .payload_size_mismatch
                .fetch_add(1, Ordering::Relaxed);
            if n % 100 == 0 {
                warn!(
                    "drop packet: not a valid shred len={} from {} (merkle={} legacy={} with_nonce={} max={})",
                    payload_len,
                    datagram.from,
                    MERKLE_DATA_SHRED_PAYLOAD_SIZE,
                    SHRED_PAYLOAD_SIZE,
                    PACKET_DATA_SIZE,
                    MAX_UDP_PAYLOAD_SIZE
                );
            }
            return None;
        }
    };
    let key = FecKey {
        slot: decoded.shred.slot(),
        version: decoded.shred.version(),
        fec_set: decoded.shred.fec_set_index(),
    };

    if decoded.received_len > decoded.canonical_payload_len() {
        let extra = decoded.received_len - decoded.canonical_payload_len();
        let n = metrics.inc_payload_trailing();
        if n % 100 == 0 {
            info!(
                "shred received with trailing bytes slot={} ver={} fec_set={} recv_len={} canonical={} extra={} from={}",
                key.slot,
                key.version,
                key.fec_set,
                decoded.received_len,
                decoded.canonical_payload_len(),
                extra,
                datagram.from
            );
        }
    }

    if let Some(root) = cfg.slot_window_root {
        if decoded.shred.slot() > root.saturating_add(cfg.slot_window_max_future) {
            metrics.inc_slot_window_future();
            warn_once(
                state,
                key,
                "drop shred beyond slot window (future)",
                cfg.warn_once_per_fec,
            )
            .await;
            return None;
        }
        if decoded.shred.slot() < root {
            metrics.inc_slot_window_past();
            warn_once(
                state,
                key,
                "drop shred outside slot window (past)",
                cfg.warn_once_per_fec,
            )
            .await;
            return None;
        }
    }

    if let Err(err) = decoded.shred.sanitize() {
        match decoded.shred {
            Shred::ShredData(_) => metrics.inc_sanitize_fail_data(),
            Shred::ShredCode(_) => metrics.inc_sanitize_fail_code(),
        }
        warn_once(
            state,
            key,
            &format!("sanitize failed: {err:?}"),
            cfg.warn_once_per_fec,
        )
        .await;
        return None;
    }

    match &decoded.shred {
        Shred::ShredData(_data) => {
            if decoded.shred.index() as usize >= MAX_DATA_SHREDS_PER_SLOT {
                metrics.inc_index_oob();
                warn_once(state, key, "data shred index oob", cfg.warn_once_per_fec).await;
                return None;
            }
        }
        Shred::ShredCode(_code) => {
            let Some(header) = decode_coding_header(&decoded.shred) else {
                metrics.inc_sanitize_fail_code();
                warn_once(
                    state,
                    key,
                    "coding header parse failed",
                    cfg.warn_once_per_fec,
                )
                .await;
                return None;
            };

            if decoded.shred.index() as usize >= MAX_CODE_SHREDS_PER_SLOT {
                metrics.inc_index_oob();
                warn_once(state, key, "coding shred index oob", cfg.warn_once_per_fec).await;
                return None;
            }

            if cfg.strict_fec
                && (header.num_data_shreds > cfg.strict_num_data
                    || header.num_coding_shreds > cfg.strict_num_coding)
            {
                metrics.inc_fec_mismatch();
                warn_once(
                    state,
                    key,
                    "coding shred exceeds strict fec sizing",
                    cfg.warn_once_per_fec,
                )
                .await;
                return None;
            }

            if header.first_coding_index != decoded.shred.fec_set_index() {
                metrics.inc_fec_mismatch();
                warn_once(
                    state,
                    key,
                    "coding first index does not match fec_set_index",
                    cfg.warn_once_per_fec,
                )
                .await;
                return None;
            }

            let fec_size = u32::from(header.num_data_shreds)
                .saturating_add(u32::from(header.num_coding_shreds));
            if decoded.shred.index() >= header.first_coding_index.saturating_add(fec_size) {
                metrics.inc_index_oob();
                warn_once(
                    state,
                    key,
                    "coding shred index outside fec set",
                    cfg.warn_once_per_fec,
                )
                .await;
                return None;
            }
        }
    }

    Some(decoded)
}

async fn warn_once(state: &ShredsUdpState, key: FecKey, msg: &str, warn_once: bool) {
    if !warn_once {
        warn!(
            "slot={} ver={} fec_set={} {}",
            key.slot, key.version, key.fec_set, msg
        );
        return;
    }
    let mut warnings = state.warnings.lock().await;
    warnings.retain(|_, ts| ts.elapsed() < state.completed_ttl());
    if warnings.insert(key, Instant::now()).is_none() {
        warn!(
            "slot={} ver={} fec_set={} {}",
            key.slot, key.version, key.fec_set, msg
        );
    }
}

fn decode_coding_header(shred: &Shred) -> Option<CodingHeaderInfo> {
    if !matches!(shred, Shred::ShredCode(_)) {
        return None;
    }
    let bytes = shred.payload().as_ref();
    if bytes.len() < COMMON_HEADER_LEN + CODING_HEADER_LEN {
        return None;
    }
    let num_data_shreds =
        u16::from_le_bytes([bytes[COMMON_HEADER_LEN], bytes[COMMON_HEADER_LEN + 1]]);
    let num_coding_shreds =
        u16::from_le_bytes([bytes[COMMON_HEADER_LEN + 2], bytes[COMMON_HEADER_LEN + 3]]);
    let position = u16::from_le_bytes([bytes[COMMON_HEADER_LEN + 4], bytes[COMMON_HEADER_LEN + 5]]);
    if num_data_shreds == 0 {
        return None;
    }
    let first_coding_index = shred.index().checked_sub(position as u32)?;

    Some(CodingHeaderInfo {
        num_data_shreds,
        num_coding_shreds,
        position,
        first_coding_index,
    })
}

fn summarize_coding_headers(code_shreds: &HashMap<u32, Shred>) -> CodingHeaderSummary {
    let mut summary = CodingHeaderSummary::default();
    let mut num_data = BTreeSet::new();
    let mut num_coding = BTreeSet::new();
    let mut first_indices = BTreeSet::new();

    for shred in code_shreds.values() {
        if let Some(header) = decode_coding_header(shred) {
            summary.parsed += 1;
            num_data.insert(header.num_data_shreds as usize);
            num_coding.insert(header.num_coding_shreds as usize);
            first_indices.insert(header.first_coding_index);
            if summary.positions_preview.len() < 8 {
                summary.positions_preview.push(header.position);
            }
        } else {
            summary.invalid += 1;
        }
    }

    summary.num_data_shreds = num_data.into_iter().collect();
    summary.num_coding_shreds = num_coding.into_iter().collect();
    summary.first_coding_indices = first_indices.into_iter().collect();
    summary
}

fn detail_action_priority(action: Option<&str>) -> u8 {
    match action {
        Some("buy") | Some("sell") => 2,
        Some("create") => 1,
        _ => 0,
    }
}

fn merge_mint_detail(current: &mut MintDetail, incoming: &MintDetail) {
    let current_priority = detail_action_priority(current.action);
    let incoming_priority = detail_action_priority(incoming.action);
    let current_is_create = matches!(current.action, Some("create"));
    let incoming_is_trade = matches!(incoming.action, Some("buy") | Some("sell"));
    let incoming_is_create = matches!(incoming.action, Some("create"));

    if incoming_is_create {
        // Prefer create over later trade classification; keep existing amounts.
        current.action = Some("create");
        if let Some(label) = incoming.label {
            current.label = Some(label);
        }
        // Keep amounts/names if already present; otherwise let the generic merge below fill them.
        if current.sol_amount.is_none() {
            current.sol_amount = incoming.sol_amount;
        }
        if current.token_amount.is_none() {
            current.token_amount = incoming.token_amount;
        }
        if current.name.is_none() {
            current.name = incoming.name.clone();
        }
        if current.symbol.is_none() {
            current.symbol = incoming.symbol.clone();
        }
        if current.uri.is_none() {
            current.uri = incoming.uri.clone();
        }
        return;
    } else if current_is_create && incoming_is_trade {

        // Keep create, but backfill amounts from the trade.
        if current.sol_amount.is_none() {
            current.sol_amount = incoming.sol_amount;
        }
        if current.token_amount.is_none() {
            current.token_amount = incoming.token_amount;
        }
        return;
    } else {
        if let Some(action) = incoming.action {
            if incoming_priority > current_priority || current.action.is_none() {
                current.action = Some(action);
            }
        }
        if let Some(label) = incoming.label {
            if current.label.is_none() || incoming_priority > current_priority {
                current.label = Some(label);
            }
        }
        if let Some(sol) = incoming.sol_amount {
            if current.sol_amount.is_none() || incoming_priority >= current_priority {
                current.sol_amount = Some(sol);
            }
        }
        if let Some(token) = incoming.token_amount {
            if current.token_amount.is_none() || incoming_priority >= current_priority {
                current.token_amount = Some(token);
            }
        }
    }

    if current.name.is_none() {
        current.name = incoming.name.clone();
    }
    if current.symbol.is_none() {
        current.symbol = incoming.symbol.clone();
    }
    if current.uri.is_none() {
        current.uri = incoming.uri.clone();
    }
}

fn filter_pump_details(details: &mut Vec<MintDetail>, pump_min_lamports: u64) {
    details.retain(|d| matches!(d.action, Some("buy") | Some("sell") | Some("create")));
    if pump_min_lamports == 0 {
        return;
    }
    details.retain(|d| match d.action {
        Some("buy") | Some("sell") => d
            .sol_amount
            .map(|amt| amt >= pump_min_lamports)
            .unwrap_or(false),
        // If create already carries the initial buy amounts (create/buy), apply the threshold too.
        Some("create") => d
            .sol_amount
            .map(|amt| amt >= pump_min_lamports)
            .unwrap_or(true),
        _ => false,
    });
}

pub fn collect_watch_events(
    slot: u64,
    txs: &[&VersionedTransaction],
    watch_cfg: &ProgramWatchConfig,
    pump_min_lamports: u64,
) -> Vec<WatchEvent> {
    let _ = pump_min_lamports;
    let mut events = Vec::new();
    for tx in txs {
        if let Some(hit) = detect_program_hit(tx, watch_cfg) {
            let mut detail_map: BTreeMap<Pubkey, MintDetail> = hit
                .mints
                .iter()
                .map(|m| {
                    let action = match m.label {
                        Some("pump:create") => Some("create"),
                        Some("pump:buy") => Some("buy"),
                        Some("pump:sell") => Some("sell"),
                        Some("pump:trade") => Some("trade"),
                        _ => None,
                    };
                    (
                        m.mint,
                        MintDetail {
                            mint: m.mint,
                            label: m.label,
                            action,
                            token_amount: None,
                            sol_amount: None,
                            name: None,
                            symbol: None,
                            uri: None,
                        },
                    )
                })
                .collect();
            for d in &watch_cfg.detailers {
                for det in d.detail(tx, watch_cfg, &hit.mints) {
                    detail_map
                        .entry(det.mint)
                        .and_modify(|curr| merge_mint_detail(curr, &det))
                        .or_insert(det);
                }
            }
            if detail_map.is_empty() {
                continue;
            }
            let mut details: Vec<MintDetail> = detail_map.values().cloned().collect();
            details.sort_by(|a, b| a.mint.cmp(&b.mint));
            details.dedup_by(|a, b| a.mint == b.mint);
            events.push(WatchEvent {
                slot,
                hit,
                details,
            });
        }
    }
    events
}

pub fn log_watch_events(
    slot: u64,
    txs: &[&VersionedTransaction],
    watch_cfg: &ProgramWatchConfig,
    log_watch_hits: bool,
    pump_min_lamports: u64,
) {
    if !log_watch_hits {
        return;
    }
    fn mint_priority(detail: &MintDetail) -> u8 {
        if let Some(action) = detail.action {
            if action == "create" {
                return 0;
            }
            if action == "trade" {
                return 1;
            }
        }
        if let Some(label) = detail.label {
            if label.starts_with("pump:") {
                return 2;
            }
        }
        10
    }

    for event in collect_watch_events(slot, txs, watch_cfg, pump_min_lamports) {
        let prefix = match (event.hit.program_hit, event.hit.authority_hit) {
            (true, true) => "🎯🐣",
            (true, false) => "🎯",
            (false, true) => "🐣",
            _ => "👀",
        };
        let mut details = event.details;
        filter_pump_details(&mut details, pump_min_lamports);
        details.sort_by(|a, b| {
            mint_priority(a)
                .cmp(&mint_priority(b))
                .then_with(|| a.mint.cmp(&b.mint))
        });
        details.dedup_by(|a, b| a.mint == b.mint);
        if details.is_empty() {
            continue;
        }
        if let Some(primary) = details.first() {
                let is_create = primary.action == Some("create")
                    || primary.label == Some("pump:create");
                let base_kind = primary.action.or(primary.label).unwrap_or("unknown");
                let kind =
                    if is_create && (primary.sol_amount.is_some() || primary.token_amount.is_some()) {
                        "create/buy"
                    } else if is_create {
                        "create"
                    } else {
                        base_kind
                    };
                let missing_amounts = primary.sol_amount.is_none() && primary.token_amount.is_none();
                // Pump.fun buy/create logs carry the max SOL cap, not the actual filled amount.
                let is_pump_buy_cap = matches!(primary.action, Some("buy") | Some("create"))
                    && primary
                        .label
                        .map(|l| l.starts_with("pump:"))
                        .unwrap_or(false)
                    && primary.sol_amount.is_some();
                let icon = if missing_amounts {
                    "❓"
                } else if is_create {
                    "🐣"
                } else {
                match primary.action {
                    Some("buy") => "🟢",
                    Some("sell") => "🔻",
                    _ => "🪙",
                }
                };
                let lamports_display = primary
                    .sol_amount
                    .map(|l| {
                        if is_pump_buy_cap {
                            format!("{} (max)", l)
                        } else {
                            l.to_string()
                        }
                    })
                    .unwrap_or_else(|| "-".to_string());
                let sol_display = primary
                    .sol_amount
                    .map(|lamports| {
                        let base = format!("{:.9}", lamports as f64 / 1_000_000_000_f64);
                        if is_pump_buy_cap {
                            format!("{} (max)", base)
                        } else {
                            base
                        }
                    })
                    .unwrap_or_else(|| "-".to_string());
            let token_amount_display = primary
                .token_amount
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".to_string());
            info!(
                "{} {}\n  slot: {}\n  sig: {}\n  mint: {}\n  kind: {}\n  lamports: {}\n  sol: {}\n  token_amount: {}",
                icon,
                prefix,
                slot,
                event.hit.signature,
                primary.mint,
                kind,
                lamports_display,
                sol_display,
                token_amount_display
            );
        } else {
            let mint = event
                .hit
                .mints
                .get(0)
                .map(|m| m.mint.to_string())
                .unwrap_or_else(|| "<unknown>".to_string());
            info!(
                "❓ {}\n  slot: {}\n  sig: {}\n  mint: {}\n  kind: unknown\n  lamports: -\n  sol: -\n  token_amount: -",
                prefix, slot, event.hit.signature, mint
            );
        }
    }
}

impl ShredBatch {
    fn new() -> Self {
        Self {
            data_shreds: HashMap::new(),
            code_shreds: HashMap::new(),
            required_data: None,
            required_data_from_data: None,
            required_data_from_code: None,
            data_complete_seen: false,
            expected_first_coding_index: None,
            expected_num_data: None,
            expected_num_coding: None,
            last_attempted_count: 0,
            dup_data: 0,
            dup_code: 0,
        }
    }

    fn update_required_data(&mut self, required: usize) {
        self.required_data = Some(self.required_data.map_or(required, |cur| cur.max(required)));
    }

    fn update_required_data_from_data(&mut self, required: usize) {
        self.required_data_from_data = Some(
            self.required_data_from_data
                .map_or(required, |cur| cur.max(required)),
        );
        self.update_required_data(required);
    }

    fn update_required_data_from_code(&mut self, required: usize) {
        self.required_data_from_code = Some(
            self.required_data_from_code
                .map_or(required, |cur| cur.max(required)),
        );
        self.update_required_data(required);
    }

    fn insert_data_shred(&mut self, shred: Shred, metrics: &ShredMetrics) {
        if shred.data_complete() {
            self.data_complete_seen = true;
        }
        if let Some(existing) = self.data_shreds.get(&shred.index()) {
            if existing.payload() != shred.payload() {
                metrics.inc_duplicate_conflict();
                return;
            }
            self.dup_data += 1;
            return;
        }
        if shred.index() as usize >= MAX_DATA_SHREDS_PER_SLOT {
            metrics.inc_index_oob();
            return;
        }
        self.data_shreds.insert(shred.index(), shred);
    }

    fn insert_code_shred(&mut self, shred: Shred, metrics: &ShredMetrics) {
        if let Some(header) = decode_coding_header(&shred) {
            if header.first_coding_index != shred.fec_set_index() {
                metrics.inc_fec_mismatch();
                return;
            }
            match self.expected_first_coding_index {
                Some(first) if first != header.first_coding_index => {
                    metrics.inc_fec_mismatch();
                    return;
                }
                None => self.expected_first_coding_index = Some(header.first_coding_index),
                _ => {}
            }
            match self.expected_num_data {
                Some(num) if num != header.num_data_shreds => {
                    metrics.inc_fec_mismatch();
                    return;
                }
                None => self.expected_num_data = Some(header.num_data_shreds),
                _ => {}
            }
            match self.expected_num_coding {
                Some(num) if num != header.num_coding_shreds => {
                    metrics.inc_fec_mismatch();
                    return;
                }
                None => self.expected_num_coding = Some(header.num_coding_shreds),
                _ => {}
            }
            let fec_size = u32::from(header.num_data_shreds)
                .saturating_add(u32::from(header.num_coding_shreds));
            if shred.index() >= header.first_coding_index.saturating_add(fec_size) {
                metrics.inc_index_oob();
                return;
            }
        }
        if shred.index() as usize >= MAX_CODE_SHREDS_PER_SLOT {
            metrics.inc_index_oob();
            return;
        }
        if let Some(existing) = self.code_shreds.get(&shred.index()) {
            if existing.payload() != shred.payload() {
                metrics.inc_duplicate_conflict();
                return;
            }
            self.dup_code += 1;
            return;
        }
        self.code_shreds.insert(shred.index(), shred);
    }

    fn ready_to_deshred(&mut self, policy: &DeshredPolicy) -> ReadyToDeshred {
        let data_len = self.data_shreds.len();
        let Some(required) = self.required_data else {
            return ReadyToDeshred::NotReady;
        };
        if data_len < required || data_len <= self.last_attempted_count {
            return ReadyToDeshred::NotReady;
        }
        if !self.data_complete_seen {
            return ReadyToDeshred::Gated("waiting for data-complete shred".to_string());
        }

        if policy.require_code_match {
            if self.code_shreds.is_empty() {
                return ReadyToDeshred::Gated("waiting for coding shred".to_string());
            }
            let summary = summarize_coding_headers(&self.code_shreds);
            if summary.parsed == 0 {
                return ReadyToDeshred::Gated("no parseable coding headers yet".to_string());
            }
            if summary.consistent_first_index().is_none() {
                return ReadyToDeshred::Gated(
                    "coding headers disagree on first_coding_index".to_string(),
                );
            }
            let Some(code_required) = summary.consistent_num_data() else {
                return ReadyToDeshred::Gated(
                    "coding headers disagree on num_data_shreds".to_string(),
                );
            };
            let data_required = self.required_data_from_data.unwrap_or(required);
            if code_required != data_required {
                return ReadyToDeshred::Gated(format!(
                    "coding num_data_shreds {} mismatches data {}",
                    code_required, data_required
                ));
            }
        }

        self.last_attempted_count = data_len;
        let mut shreds: Vec<Shred> = self.data_shreds.values().cloned().collect();
        shreds.sort_by_key(|s| s.index());
        ReadyToDeshred::Ready(shreds)
    }

    fn status(&self, fec_set: u32) -> BatchStatus {
        let required = self.required_data;
        let mut missing = Vec::new();
        if let Some(req) = required {
            for idx in fec_set..(fec_set + req as u32) {
                if !self.data_shreds.contains_key(&idx) {
                    missing.push(idx);
                    if missing.len() >= 12 {
                        break;
                    }
                }
            }
        }
        let ranges = missing_ranges(fec_set, required, &self.data_shreds);
        BatchStatus {
            data_len: self.data_shreds.len(),
            code_len: self.code_shreds.len(),
            required_data: required,
            required_data_from_data: self.required_data_from_data,
            required_data_from_code: self.required_data_from_code,
            data_complete_seen: self.data_complete_seen,
            missing,
            missing_ranges: ranges,
            dup_data: self.dup_data,
            dup_code: self.dup_code,
            expected_first_coding_index: self.expected_first_coding_index,
            expected_num_data: self.expected_num_data,
            expected_num_coding: self.expected_num_coding,
            coding_summary: summarize_coding_headers(&self.code_shreds),
        }
    }
}

fn format_status(slot: u64, version: u16, fec_set: u32, st: &BatchStatus) -> String {
    format!(
        "slot={} ver={} fec_set={} have_data={} code={} required_data={:?} required_data_from_data={:?} required_data_from_code={:?} data_complete_seen={} missing_preview={:?} missing_ranges={:?} dup_data={} dup_code={} expected_first_coding_index={:?} expected_num_data={:?} expected_num_coding={:?} coding_summary={}",
        slot,
        version,
        fec_set,
        st.data_len,
        st.code_len,
        st.required_data,
        st.required_data_from_data,
        st.required_data_from_code,
        st.data_complete_seen,
        st.missing,
        st.missing_ranges,
        st.dup_data,
        st.dup_code,
        st.expected_first_coding_index,
        st.expected_num_data,
        st.expected_num_coding,
        st.coding_summary.describe(),
    )
}

fn missing_ranges(
    fec_set: u32,
    required: Option<usize>,
    data_shreds: &HashMap<u32, Shred>,
) -> Vec<(u32, u32)> {
    let Some(req) = required else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    let mut start: Option<u32> = None;
    let mut last_missing: Option<u32> = None;
    for idx in fec_set..(fec_set + req as u32) {
        if data_shreds.contains_key(&idx) {
            if let (Some(s), Some(e)) = (start.take(), last_missing.take()) {
                ranges.push((s, e));
                if ranges.len() >= 6 {
                    return ranges;
                }
            }
        } else {
            if start.is_none() {
                start = Some(idx);
            }
            last_missing = Some(idx);
        }
    }
    if let (Some(s), Some(e)) = (start, last_missing) {
        ranges.push((s, e));
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn make_detail(
        mint: Pubkey,
        action: Option<&'static str>,
        label: Option<&'static str>,
        sol_amount: Option<u64>,
    ) -> MintDetail {
        MintDetail {
            mint,
            label,
            action,
            sol_amount,
            token_amount: None,
            name: None,
            symbol: None,
            uri: None,
        }
    }

    #[test]
    fn merge_prefers_buy_over_create() {
        let mint = Pubkey::new_from_array([1u8; 32]);
        let mut current = make_detail(mint, Some("create"), Some("pump:create"), None);
        let incoming = MintDetail {
            token_amount: Some(42),
            ..make_detail(mint, Some("buy"), Some("pump:buy"), Some(200))
        };

        merge_mint_detail(&mut current, &incoming);

        assert_eq!(current.action, Some("buy"));
        assert_eq!(current.label, Some("pump:buy"));
        assert_eq!(current.sol_amount, Some(200));
        assert_eq!(current.token_amount, Some(42));
    }

    #[test]
    fn filter_drops_trade_and_small_buys() {
        let mint_buy_small = Pubkey::new_from_array([2u8; 32]);
        let mint_buy_large = Pubkey::new_from_array([3u8; 32]);
        let mint_create = Pubkey::new_from_array([4u8; 32]);
        let mint_trade = Pubkey::new_from_array([5u8; 32]);

        let mut details = vec![
            make_detail(mint_buy_small, Some("buy"), Some("pump:buy"), Some(50)),
            make_detail(mint_buy_large, Some("buy"), Some("pump:buy"), Some(200)),
            make_detail(mint_create, Some("create"), Some("pump:create"), None),
            make_detail(mint_trade, Some("trade"), Some("pump:trade"), None),
        ];

        filter_pump_details(&mut details, 100);

        assert!(details
            .iter()
            .all(|d| matches!(d.action, Some("buy") | Some("sell") | Some("create"))));
        assert!(details.iter().any(|d| d.mint == mint_create));
        assert!(details.iter().any(|d| d.mint == mint_buy_large));
        assert!(!details.iter().any(|d| d.mint == mint_buy_small));
        assert!(!details.iter().any(|d| d.mint == mint_trade));
    }
}
