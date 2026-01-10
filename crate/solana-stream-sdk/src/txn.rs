use std::str::FromStr;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use solana_sdk::{
    message::VersionedMessage, pubkey::Pubkey, signature::Signature,
    transaction::VersionedTransaction,
};
use solana_vote_program::id as vote_program_id;

const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqPjhAG8cHpQdV3ESy1dpeBeXcAD9fQg";
const DEFAULT_PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMPFUN_CREATE_DISC: [u8; 8] = [0x18, 0x1e, 0xc8, 0x28, 0x05, 0x1c, 0x07, 0x77];
const PUMPFUN_CREATE_V2_DISC: [u8; 8] = [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];
const PUMPFUN_BUY_DISC: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
const PUMPFUN_BUY_EXACT_SOL_IN_DISC: [u8; 8] =
    [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
const PUMPFUN_SELL_DISC: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];

#[derive(Clone)]
pub struct ProgramWatchConfig {
    pub program_ids: Vec<Pubkey>,
    pub authorities: Vec<Pubkey>,
    pub token_program_ids: Vec<Pubkey>,
    pub skip_vote_txs: bool,
    pub mint_finder: Arc<dyn MintFinder + Send + Sync>,
    pub detailers: Vec<Arc<dyn MintDetailer + Send + Sync>>,
}

impl ProgramWatchConfig {
    pub fn new(program_ids: Vec<Pubkey>, authorities: Vec<Pubkey>) -> Self {
        let mf = Arc::new(default_mint_finder(&program_ids));
        Self {
            program_ids: program_ids.clone(),
            authorities,
            token_program_ids: default_token_program_ids(),
            skip_vote_txs: true,
            mint_finder: mf.clone(),
            detailers: default_detailers_from_programs(&program_ids),
        }
    }

    pub fn with_token_program_ids(mut self, token_program_ids: Vec<Pubkey>) -> Self {
        self.token_program_ids = token_program_ids;
        self
    }

    pub fn with_skip_vote_txs(mut self, skip_vote_txs: bool) -> Self {
        self.skip_vote_txs = skip_vote_txs;
        self
    }

    pub fn with_mint_finder(mut self, mint_finder: Arc<dyn MintFinder + Send + Sync>) -> Self {
        self.mint_finder = mint_finder;
        self
    }

    pub fn with_detailers(mut self, detailers: Vec<Arc<dyn MintDetailer + Send + Sync>>) -> Self {
        self.detailers = detailers;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ProgramHit {
    pub signature: Signature,
    pub program_hit: bool,
    pub authority_hit: bool,
    pub mints: Vec<MintInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MintInfo {
    pub mint: Pubkey,
    pub label: Option<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintDetail {
    pub mint: Pubkey,
    pub label: Option<&'static str>,
    pub action: Option<&'static str>,
    pub sol_amount: Option<u64>,
    pub token_amount: Option<u64>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub uri: Option<String>,
}

pub fn parse_pubkeys_env(var: &str, defaults: &[&str]) -> Vec<Pubkey> {
    let raw = std::env::var(var).ok();
    parse_pubkeys(raw.as_deref(), defaults)
}

pub fn parse_pubkeys(raw: Option<&str>, defaults: &[&str]) -> Vec<Pubkey> {
    let mut set = BTreeSet::new();

    match raw {
        Some(value) => {
            for part in value.split(',') {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(pk) = Pubkey::from_str(trimmed) {
                    set.insert(pk);
                }
            }
        }
        None => {
            for def in defaults {
                if let Ok(pk) = Pubkey::from_str(def) {
                    set.insert(pk);
                }
            }
        }
    }

    set.into_iter().collect()
}

pub fn default_token_program_ids() -> Vec<Pubkey> {
    parse_pubkeys(
        None,
        &[
            TOKEN_PROGRAM_ID,
            TOKEN_2022_PROGRAM_ID, //
        ],
    )
}

pub fn is_vote_transaction(tx: &VersionedTransaction) -> bool {
    let keys = tx.message.static_account_keys();
    tx.message.instructions().iter().any(|ix| {
        keys.get(ix.program_id_index as usize)
            .map(|pid| pid == &vote_program_id())
            .unwrap_or(false)
    })
}

pub fn detect_program_hit(
    tx: &VersionedTransaction,
    cfg: &ProgramWatchConfig,
) -> Option<ProgramHit> {
    if cfg.program_ids.is_empty() && cfg.authorities.is_empty() {
        return None;
    }
    let keys = tx.message.static_account_keys();
    let authority_hit = !cfg.authorities.is_empty()
        && keys
            .iter()
            .any(|key| cfg.authorities.iter().any(|auth| auth == key));
    let mut program_hit = false;

    for ix in tx.message.instructions() {
        let Some(program_id) = keys.get(ix.program_id_index as usize) else {
            continue;
        };
        if cfg.skip_vote_txs && *program_id == vote_program_id() {
            return None;
        }
        if !program_hit && cfg.program_ids.iter().any(|want| want == program_id) {
            program_hit = true;
        }
    }

    let mint_accounts: Vec<MintInfo> = cfg.mint_finder.find_mints(tx, cfg);
    if !program_hit && !authority_hit {
        return None;
    }

    Some(ProgramHit {
        signature: tx.signatures.get(0).cloned().unwrap_or_default(),
        program_hit,
        authority_hit,
        mints: mint_accounts,
    })
}

pub fn extract_mint_accounts(
    message: &VersionedMessage,
    token_program_ids: &[Pubkey],
) -> Vec<Pubkey> {
    let mut mints = BTreeSet::new();
    let keys = message.static_account_keys();
    for ix in message.instructions() {
        let Some(program_id) = keys.get(ix.program_id_index as usize) else {
            continue;
        };
        if !token_program_ids.iter().any(|id| id == program_id) {
            continue;
        }
        let Some(tag) = ix.data.first() else {
            continue;
        };
        // Token program: 0 InitializeMint, 7 MintTo, 14 InitializeMint2, 20 InitializeMintCloseAuthority
        if matches!(tag, 0 | 7 | 14 | 20) {
            if let Some(mint_idx) = ix.accounts.get(0) {
                if let Some(mint) = keys.get(*mint_idx as usize) {
                    mints.insert(*mint);
                }
            }
        }
    }
    mints.into_iter().collect()
}

pub fn first_signatures<'a, I>(txs: I, limit: usize, skip_vote_txs: bool) -> Vec<Signature>
where
    I: IntoIterator<Item = &'a VersionedTransaction>,
{
    txs.into_iter()
        .filter(|tx| !(skip_vote_txs && is_vote_transaction(tx)))
        .filter_map(|tx| tx.signatures.get(0))
        .take(limit)
        .cloned()
        .collect()
}

pub fn fmt_pubkeys(pubkeys: &[Pubkey]) -> Vec<String> {
    pubkeys.iter().map(|p| p.to_string()).collect()
}

pub trait MintFinder {
    fn find_mints(&self, tx: &VersionedTransaction, cfg: &ProgramWatchConfig) -> Vec<MintInfo>;
}

pub trait MintDetailer {
    fn detail(
        &self,
        tx: &VersionedTransaction,
        cfg: &ProgramWatchConfig,
        mints: &[MintInfo],
    ) -> Vec<MintDetail>;
}

fn insert_mint(map: &mut BTreeMap<Pubkey, MintInfo>, mint: Pubkey, label: Option<&'static str>) {
    map.entry(mint)
        .and_modify(|info| {
            if info.label.is_none() && label.is_some() {
                info.label = label;
            }
        })
        .or_insert(MintInfo { mint, label });
}

/// Default SPL Token/Token-2022 finder: looks for Initialize/MintTo tags at accounts[0].
pub struct SplTokenMintFinder;

impl MintFinder for SplTokenMintFinder {
    fn find_mints(&self, tx: &VersionedTransaction, cfg: &ProgramWatchConfig) -> Vec<MintInfo> {
        let mut mint_accounts = BTreeMap::new();
        let keys = tx.message.static_account_keys();
        for ix in tx.message.instructions() {
            let Some(program_id) = keys.get(ix.program_id_index as usize) else {
                continue;
            };
            if !cfg.token_program_ids.iter().any(|id| id == program_id) {
                continue;
            }
            let Some(tag) = ix.data.first() else {
                continue;
            };
            if matches!(tag, 0 | 7 | 14 | 20) {
                if let Some(mint_idx) = ix.accounts.get(0) {
                    if let Some(mint) = keys.get(*mint_idx as usize) {
                        insert_mint(&mut mint_accounts, *mint, Some("spl-token"));
                    }
                }
            }
        }
        mint_accounts.into_values().collect()
    }
}

/// Pump.fun finder: picks mint from pump.fun instructions by account position (create_v2/buy/sell).
pub struct PumpfunAccountMintFinder {
    pumpfun_ids: Vec<Pubkey>,
}

impl PumpfunAccountMintFinder {
    pub fn new(pumpfun_ids: Vec<Pubkey>) -> Self {
        Self { pumpfun_ids }
    }
}

fn is_system_id(pk: &Pubkey) -> bool {
    // Avoid depending on solana_sdk::system_program in this build profile.
    pk.to_string() == "11111111111111111111111111111111"
}

impl MintFinder for PumpfunAccountMintFinder {
    fn find_mints(&self, tx: &VersionedTransaction, _cfg: &ProgramWatchConfig) -> Vec<MintInfo> {
        let keys = tx.message.static_account_keys();
        let mut mints = BTreeMap::new();
        for ix in tx.message.instructions() {
            let Some(program_id) = keys.get(ix.program_id_index as usize) else {
                continue;
            };
            if !self.pumpfun_ids.iter().any(|id| id == program_id) {
                continue;
            }
            let kind = match ix.data.get(0..8) {
                Some(bytes)
                    if bytes == PUMPFUN_CREATE_V2_DISC || bytes == PUMPFUN_CREATE_DISC =>
                {
                    Some("pump:create")
                }
                Some(bytes) if bytes == PUMPFUN_BUY_DISC => Some("pump:buy"),
                Some(bytes) if bytes == PUMPFUN_BUY_EXACT_SOL_IN_DISC => Some("pump:buy_exact"),
                Some(bytes) if bytes == PUMPFUN_SELL_DISC => Some("pump:sell"),
                _ => None,
            };

            match kind {
                Some("pump:create") => {
                    if let Some(mint_idx) = ix.accounts.get(0) {
                        if let Some(mint) = keys.get(*mint_idx as usize) {
                            if !is_system_id(mint) {
                                insert_mint(&mut mints, *mint, Some("pump:create"));
                            }
                        }
                    }
                }
                Some("pump:buy") | Some("pump:buy_exact") | Some("pump:sell") => {
                    if ix.accounts.len() > 2 {
                        if let Some(mint_idx) = ix.accounts.get(2) {
                            if let Some(mint) = keys.get(*mint_idx as usize) {
                                if !is_system_id(mint) {
                                    insert_mint(&mut mints, *mint, kind);
                                }
                            }
                        }
                    }
                }
                _ => {
                    // Fallback: keep a generic trade tag if we can't classify (e.g., new ix name)
                    if ix.accounts.len() > 2 {
                        if let Some(mint_idx) = ix.accounts.get(2) {
                            if let Some(mint) = keys.get(*mint_idx as usize) {
                                if !is_system_id(mint) {
                                    insert_mint(&mut mints, *mint, Some("pump:trade"));
                                }
                            }
                        }
                    }
                }
            }
        }
        mints.into_values().collect()
    }
}

/// Pump.fun detailer: adds action metadata based on label.
pub struct PumpfunDetailer {
    pumpfun_ids: Vec<Pubkey>,
}

impl PumpfunDetailer {
    pub fn new(pumpfun_ids: Vec<Pubkey>) -> Self {
        Self { pumpfun_ids }
    }
}

impl MintDetailer for PumpfunDetailer {
    fn detail(
        &self,
        tx: &VersionedTransaction,
        _cfg: &ProgramWatchConfig,
        mints: &[MintInfo],
    ) -> Vec<MintDetail> {
        let keys = tx.message.static_account_keys();
        let mut out = BTreeMap::new();
        for ix in tx.message.instructions() {
            let Some(program_id) = keys.get(ix.program_id_index as usize) else {
                continue;
            };
            if !self.pumpfun_ids.iter().any(|id| id == program_id) {
                continue;
            }
            let disc = ix.data.get(0..8);
            let kind = match disc {
                Some(bytes)
                    if bytes == PUMPFUN_CREATE_V2_DISC || bytes == PUMPFUN_CREATE_DISC =>
                {
                    Some("create")
                }
                Some(bytes) if bytes == PUMPFUN_BUY_DISC => Some("buy"),
                Some(bytes) if bytes == PUMPFUN_BUY_EXACT_SOL_IN_DISC => Some("buy_exact"),
                Some(bytes) if bytes == PUMPFUN_SELL_DISC => Some("sell"),
                _ => None,
            };
            let mint_idx = match kind {
                Some("create") => ix.accounts.get(0),
                _ => ix.accounts.get(2),
            };
            let Some(mint_idx) = mint_idx else {
                continue;
            };
            let Some(mint) = keys.get(*mint_idx as usize) else {
                continue;
            };
            if is_system_id(mint) {
                continue;
            }
            if kind.is_none() {
                // Unknown discriminator: skip to avoid bogus values.
                continue;
            }
            let (token_amount, sol_amount) = match kind {
                Some("create") => (None, None),
                Some("buy_exact") => (
                    None,
                    ix.data
                        .get(8..16)
                        .and_then(|b| b.try_into().ok())
                        .map(u64::from_le_bytes),
                ),
                _ => (
                    ix.data
                        .get(8..16)
                        .and_then(|b| b.try_into().ok())
                        .map(u64::from_le_bytes),
                    ix.data
                        .get(16..24)
                        .and_then(|b| b.try_into().ok())
                        .map(u64::from_le_bytes),
                ),
            };
            let (action, label) = match kind {
                Some("create") => (Some("create"), Some("pump:create")),
                Some("buy") => (Some("buy"), Some("pump:buy")),
                Some("buy_exact") => (Some("buy"), Some("pump:buy_exact")),
                Some("sell") => (Some("sell"), Some("pump:sell")),
                _ => (None, None),
            };
            let entry = out.entry(*mint).or_insert(MintDetail {
                mint: *mint,
                label,
                action,
                sol_amount,
                token_amount,
                name: None,
                symbol: None,
                uri: None,
            });
            if let Some(k) = action {
                if entry.action.is_none() || entry.action == Some("create") {
                    entry.action = Some(k);
                    entry.label = label;
                }
            }
            if sol_amount.is_some() {
                entry.sol_amount = sol_amount;
            }
            if token_amount.is_some() {
                entry.token_amount = token_amount;
            }
        }
        for m in mints {
            if !out.contains_key(&m.mint) {
                let action = m.label.map(|l| {
                    let trimmed = l.trim_start_matches("pump:");
                    match trimmed {
                        "trade" => "sell",
                        "buy_exact" => "buy",
                        other => other,
                    }
                });
                out.insert(m.mint, MintDetail {
                    mint: m.mint,
                    label: m.label,
                    action,
                    sol_amount: None,
                    token_amount: None,
                    name: None,
                    symbol: None,
                    uri: None,
                });
            }
        }
        out.into_values().collect()
    }
}

/// Composite finder: aggregates multiple strategies.
pub struct CompositeMintFinder {
    finders: Vec<Arc<dyn MintFinder + Send + Sync>>,
}

impl CompositeMintFinder {
    pub fn new(finders: Vec<Arc<dyn MintFinder + Send + Sync>>) -> Self {
        Self { finders }
    }
}

impl MintFinder for CompositeMintFinder {
    fn find_mints(&self, tx: &VersionedTransaction, cfg: &ProgramWatchConfig) -> Vec<MintInfo> {
        let mut out = BTreeMap::new();
        for f in &self.finders {
            for m in f.find_mints(tx, cfg) {
                insert_mint(&mut out, m.mint, m.label);
            }
        }
        out.into_values().collect()
    }
}

fn default_mint_finder(program_ids: &[Pubkey]) -> CompositeMintFinder {
    let mut pumpfun_ids: Vec<Pubkey> = program_ids.to_vec();
    if pumpfun_ids.is_empty() {
        if let Ok(id) = Pubkey::from_str(DEFAULT_PUMPFUN_PROGRAM_ID) {
            pumpfun_ids.push(id);
        }
    }
    CompositeMintFinder::new(vec![
        Arc::new(PumpfunAccountMintFinder::new(pumpfun_ids.clone())),
        Arc::new(SplTokenMintFinder),
    ])
}

pub fn default_detailers_from_programs(
    program_ids: &[Pubkey],
) -> Vec<Arc<dyn MintDetailer + Send + Sync>> {
    let mut pumpfun_ids: Vec<Pubkey> = program_ids.to_vec();
    if pumpfun_ids.is_empty() {
        if let Ok(id) = Pubkey::from_str(DEFAULT_PUMPFUN_PROGRAM_ID) {
            pumpfun_ids.push(id);
        }
    }
    vec![Arc::new(PumpfunDetailer::new(pumpfun_ids))]
}
