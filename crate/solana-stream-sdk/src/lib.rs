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
pub use shredstream_proto::{
    CommitmentLevel, SubscribeEntriesRequest, SubscribeRequestFilterAccounts,
    SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterAccountsFilterLamports,
    SubscribeRequestFilterAccountsFilterMemcmp, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions,
};

pub use yellowstone_grpc_client::{GeyserGrpcClient, GeyserGrpcClientError, Interceptor};

pub use yellowstone_grpc_proto::{
    geyser::{
        subscribe_update::UpdateOneof as GeyserUpdateOneof, SlotStatus,
        SubscribeUpdateBlock as GeyserUpdateBlock,
        SubscribeUpdateBlockMeta as GeyserUpdateBlockMeta, SubscribeUpdateSlot as GeyserUpdateSlot,
    },
    plugin::{
        filter::message::FilteredUpdate,
        message::{
            MessageAccount, MessageBlock, MessageBlockMeta, MessageEntry, MessageSlot,
            MessageTransaction,
        },
    },
    prelude::{
        CommitmentLevel as GeyserCommitmentLevel, SubscribeRequest as GeyserSubscribeRequest,
        SubscribeRequestAccountsDataSlice as GeyserAccountsDataSlice,
        SubscribeRequestFilterAccounts as GeyserFilterAccounts,
        SubscribeRequestFilterAccountsFilter as GeyserFilterAccountsFilter,
        SubscribeRequestFilterBlocks as GeyserFilterBlocks,
        SubscribeRequestFilterBlocksMeta as GeyserFilterBlocksMeta,
        SubscribeRequestFilterEntry as GeyserFilterEntry,
        SubscribeRequestFilterSlots as GeyserFilterSlots,
        SubscribeRequestFilterTransactions as GeyserFilterTransactions,
        SubscribeUpdate as GeyserSubscribeUpdate,
        SubscribeUpdateAccountInfo as GeyserUpdateAccountInfo,
        SubscribeUpdateEntry as GeyserUpdateEntry,
        SubscribeUpdateTransactionInfo as GeyserUpdateTransactionInfo,
    },
    prost::Message,
};

pub type Result<T> = std::result::Result<T, SolanaStreamError>;
