use serde::Deserialize;
use solana_stream_sdk::{
    GeyserAccountsFilterEnum, GeyserLamportsCmp, GeyserMemcmpData,
    GeyserSubscribeRequestFilterAccounts, GeyserSubscribeRequestFilterAccountsFilter,
    GeyserSubscribeRequestFilterAccountsFilterLamports,
    GeyserSubscribeRequestFilterAccountsFilterMemcmp, GeyserSubscribeRequestFilterBlocks,
    GeyserSubscribeRequestFilterBlocksMeta, GeyserSubscribeRequestFilterEntry,
    GeyserSubscribeRequestFilterSlots, GeyserSubscribeRequestFilterTransactions,
};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub commitment: Option<String>,
    pub transactions: HashMap<String, TransactionFilter>,
    pub accounts: HashMap<String, AccountFilter>,
    pub slots: HashMap<String, SlotFilter>,
    pub blocks: HashMap<String, BlockFilter>,
    pub blocks_meta: HashMap<String, BlockMetaFilter>,
    pub entry: HashMap<String, EntryFilter>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionFilter {
    pub account_include: Option<Vec<String>>,
    pub account_exclude: Option<Vec<String>>,
    pub account_required: Option<Vec<String>>,
    pub vote: Option<bool>,
    pub failed: Option<bool>,
    pub signature: Option<String>,
}

impl From<&TransactionFilter> for GeyserSubscribeRequestFilterTransactions {
    fn from(filter: &TransactionFilter) -> Self {
        Self {
            account_include: filter.account_include.clone().unwrap_or_default(),
            account_exclude: filter.account_exclude.clone().unwrap_or_default(),
            account_required: filter.account_required.clone().unwrap_or_default(),
            vote: filter.vote,
            failed: filter.failed,
            signature: filter.signature.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountFilter {
    pub account: Option<Vec<String>>,
    pub owner: Option<Vec<String>>,
    pub filters: Option<Vec<AccountSubFilter>>,
}

impl From<&AccountFilter> for GeyserSubscribeRequestFilterAccounts {
    fn from(filter: &AccountFilter) -> Self {
        Self {
            nonempty_txn_signature: None,
            account: filter.account.clone().unwrap_or_default(),
            owner: filter.owner.clone().unwrap_or_default(),
            filters: filter.filters.as_ref().map_or(vec![], |fs| {
                fs.iter()
                    .filter_map(|f| {
                        if let Some(memcmp) = &f.memcmp {
                            Some(GeyserSubscribeRequestFilterAccountsFilter {
                                filter: Some(GeyserAccountsFilterEnum::Memcmp(
                                    GeyserSubscribeRequestFilterAccountsFilterMemcmp {
                                        offset: memcmp.offset as u64,
                                        data: Some(GeyserMemcmpData::Base58(memcmp.data.clone())),
                                    },
                                )),
                            })
                        } else if let Some(datasize) = f.datasize {
                            Some(GeyserSubscribeRequestFilterAccountsFilter {
                                filter: Some(GeyserAccountsFilterEnum::Datasize(datasize)),
                            })
                        } else if let Some(token_account_state) = f.token_account_state {
                            Some(GeyserSubscribeRequestFilterAccountsFilter {
                                filter: Some(GeyserAccountsFilterEnum::TokenAccountState(
                                    token_account_state,
                                )),
                            })
                        } else if let Some(lamports) = &f.lamports {
                            let cmp_enum = match lamports.cmp.as_str() {
                                "eq" => GeyserLamportsCmp::Eq(lamports.value),
                                "ne" => GeyserLamportsCmp::Ne(lamports.value),
                                "lt" => GeyserLamportsCmp::Lt(lamports.value),
                                "gt" => GeyserLamportsCmp::Gt(lamports.value),
                                _ => return None,
                            };
                            Some(GeyserSubscribeRequestFilterAccountsFilter {
                                filter: Some(GeyserAccountsFilterEnum::Lamports(
                                    GeyserSubscribeRequestFilterAccountsFilterLamports {
                                        cmp: Some(cmp_enum),
                                    },
                                )),
                            })
                        } else {
                            None
                        }
                    })
                    .collect()
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountSubFilter {
    pub memcmp: Option<Memcmp>,
    pub datasize: Option<u64>,
    pub token_account_state: Option<bool>,
    pub lamports: Option<Lamports>,
}

#[derive(Debug, Deserialize)]
pub struct Memcmp {
    pub offset: usize,
    pub data: String,
}

#[derive(Debug, Deserialize)]
pub struct Lamports {
    pub cmp: String,
    pub value: u64,
}

#[derive(Debug, Deserialize)]
pub struct SlotFilter {
    pub filter_by_commitment: Option<bool>,
    pub interslot_updates: Option<bool>,
}

impl From<&SlotFilter> for GeyserSubscribeRequestFilterSlots {
    fn from(filter: &SlotFilter) -> Self {
        Self {
            filter_by_commitment: filter.filter_by_commitment,
            interslot_updates: filter.interslot_updates,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct BlockFilter {
    pub account_include: Option<Vec<String>>,
    pub include_transactions: Option<bool>,
    pub include_accounts: Option<bool>,
    pub include_entries: Option<bool>,
}

impl From<&BlockFilter> for GeyserSubscribeRequestFilterBlocks {
    fn from(filter: &BlockFilter) -> Self {
        Self {
            account_include: filter.account_include.clone().unwrap_or_default(),
            include_transactions: filter.include_transactions,
            include_accounts: filter.include_accounts,
            include_entries: filter.include_entries,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct BlockMetaFilter {}

impl From<&BlockMetaFilter> for GeyserSubscribeRequestFilterBlocksMeta {
    fn from(_: &BlockMetaFilter) -> Self {
        Self {}
    }
}

#[derive(Debug, Deserialize)]
pub struct EntryFilter {}

impl From<&EntryFilter> for GeyserSubscribeRequestFilterEntry {
    fn from(_: &EntryFilter) -> Self {
        Self {}
    }
}
