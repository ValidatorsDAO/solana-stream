//! # Solana Stream SDK
//!
//! A Rust SDK for streaming Solana data using Jito protocols.
//! This crate provides convenient wrappers around the Jito protobuf definitions
//! for easier integration with Solana streaming services.

pub mod error;
pub mod shredstream;
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

// Shredstream protobuf exports
pub use shredstream_proto::{
    CommitmentLevel, SubscribeEntriesRequest, SubscribeRequestFilterAccounts,
    SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterAccountsFilterLamports,
    SubscribeRequestFilterAccountsFilterMemcmp, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions,
};

pub use yellowstone_grpc_client::{GeyserGrpcClient, GeyserGrpcClientError, Interceptor};

// Geyser protobuf exports with clear prefixes
pub use yellowstone_grpc_proto::{
    geyser::{
        subscribe_update::UpdateOneof as GeyserUpdateOneof, SlotStatus as GeyserSlotStatus,
        SubscribeUpdateBlock as GeyserUpdateBlock,
        SubscribeUpdateBlockMeta as GeyserUpdateBlockMeta, SubscribeUpdateSlot as GeyserUpdateSlot,
    },
    plugin::{
        filter::message::FilteredUpdate as GeyserFilteredUpdate,
        message::{
            MessageAccount as GeyserMessageAccount, MessageBlock as GeyserMessageBlock,
            MessageBlockMeta as GeyserMessageBlockMeta, MessageEntry as GeyserMessageEntry,
            MessageSlot as GeyserMessageSlot, MessageTransaction as GeyserMessageTransaction,
        },
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
        SubscribeUpdate as GeyserSubscribeUpdate,
        SubscribeUpdateAccountInfo as GeyserSubscribeUpdateAccountInfo,
        SubscribeUpdateEntry as GeyserSubscribeUpdateEntry,
        SubscribeUpdateTransactionInfo as GeyserSubscribeUpdateTransactionInfo,
    },
    prost::Message as GeyserMessage,
};

pub type Result<T> = std::result::Result<T, SolanaStreamError>;
