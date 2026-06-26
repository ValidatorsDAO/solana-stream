//! # Solana Stream SDK
//!
//! A Rust SDK for streaming Solana data using Shreds and gRPC.
//! This crate provides convenient wrappers around the Shreds protobuf definitions
//! for easier integration with Solana streaming services.

pub mod error;
pub mod shreds_udp;
pub mod shredstream;
pub mod txn;
pub use yellowstone_grpc_client;
pub use yellowstone_grpc_proto;

// Internal protobuf modules
pub mod shared {
    tonic::include_proto!("shared");
}

pub mod shredstream_proto {
    tonic::include_proto!("shredstream");
}

// Re-export commonly used types for convenience
// Re-export error types
pub use error::SolanaStreamError;
// Re-export shredstream client
pub use shredstream::ShredstreamClient;
// Re-export UDP receiver
pub use shreds_udp::{deshred_shreds_to_entries, UdpDatagram, UdpShredReceiver};

// Shredstream protobuf exports
pub use shredstream_proto::{
    CommitmentLevel, SubscribeEntriesRequest, SubscribeRequestFilterAccounts,
    SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterAccountsFilterLamports,
    SubscribeRequestFilterAccountsFilterMemcmp, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions,
};

pub use yellowstone_grpc_client::{GeyserGrpcClient, GeyserGrpcClientError, Interceptor};

// Geyser protobuf exports with clear prefixes
pub use yellowstone_grpc_proto::cuckoo::{
    CompressedAccountFilterSet as GeyserCompressedAccountFilterSet,
    CuckooBuildError as GeyserCuckooBuildError, CuckooFilter as GeyserCuckooFilter,
    TableFullError as GeyserCuckooTableFullError,
    YellowstoneHasherBuilder as GeyserYellowstoneHasherBuilder,
};
pub use yellowstone_grpc_proto::{
    geyser::{
        subscribe_update::UpdateOneof as GeyserUpdateOneof,
        CuckooFilter as GeyserCuckooFilterMessage,
        CuckooHashAlgorithm as GeyserCuckooHashAlgorithm, SlotStatus as GeyserSlotStatus,
        SubscribeDeshredRequest as GeyserSubscribeDeshredRequest,
        SubscribeRequestFilterDeshredTransactions as GeyserSubscribeRequestFilterDeshredTransactions,
        SubscribeUpdateBlock as GeyserUpdateBlock,
        SubscribeUpdateBlockMeta as GeyserUpdateBlockMeta,
        SubscribeUpdateDeshred as GeyserSubscribeUpdateDeshred,
        SubscribeUpdateDeshredTransaction as GeyserSubscribeUpdateDeshredTransaction,
        SubscribeUpdateDeshredTransactionInfo as GeyserSubscribeUpdateDeshredTransactionInfo,
        SubscribeUpdateSlot as GeyserUpdateSlot,
        TokenAccountExpansionControlFlag as GeyserTokenAccountExpansionControlFlag,
    },
    prelude::{
        geyser_client::GeyserClient as GeyserGrpcInnerClient,
        subscribe_request_filter_accounts_filter::Filter as GeyserAccountsFilterEnum,
        subscribe_request_filter_accounts_filter_lamports::Cmp as GeyserLamportsCmp,
        subscribe_request_filter_accounts_filter_memcmp::Data as GeyserMemcmpData,
        CommitmentLevel as GeyserCommitmentLevel, SubscribeRequest as GeyserSubscribeRequest,
        SubscribeRequestAccountsDataSlice as GeyserAccountsDataSlice,
        SubscribeRequestFilterAccounts as GeyserSubscribeRequestFilterAccounts,
        SubscribeRequestFilterAccountsFilter as GeyserSubscribeRequestFilterAccountsFilter,
        SubscribeRequestFilterAccountsFilterLamports as GeyserSubscribeRequestFilterAccountsFilterLamports,
        SubscribeRequestFilterAccountsFilterMemcmp as GeyserSubscribeRequestFilterAccountsFilterMemcmp,
        SubscribeRequestFilterBlocks as GeyserSubscribeRequestFilterBlocks,
        SubscribeRequestFilterBlocksMeta as GeyserSubscribeRequestFilterBlocksMeta,
        SubscribeRequestFilterEntry as GeyserSubscribeRequestFilterEntry,
        SubscribeRequestFilterSlots as GeyserSubscribeRequestFilterSlots,
        SubscribeRequestFilterTransactions as GeyserSubscribeRequestFilterTransactions,
        SubscribeRequestPing as GeyserSubscribeRequestPing,
        SubscribeUpdate as GeyserSubscribeUpdate,
        SubscribeUpdateAccountInfo as GeyserSubscribeUpdateAccountInfo,
        SubscribeUpdateEntry as GeyserSubscribeUpdateEntry,
        SubscribeUpdateTransactionInfo as GeyserSubscribeUpdateTransactionInfo,
    },
    prost::Message as GeyserMessage,
};

pub type Result<T> = std::result::Result<T, SolanaStreamError>;
