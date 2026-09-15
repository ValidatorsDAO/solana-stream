//! Decode the Solana ledger wire format shared by UDP and gRPC entry streams.
//!
//! Transaction v1 is not compatible with a bincode/serde transaction decoder.
//! Use the current Agave Entry schema for legacy, v0 and v1 transactions alike.

use crate::{Result, SolanaStreamError};
use solana_entry::entry::Entry;

/// Decode serialized entries, including mixed legacy, v0 and v1 transactions.
///
/// This decodes the wire format; it does not verify transaction signatures or
/// establish that the entries belong to a confirmed or finalized block.
pub fn decode_entries(data: &[u8]) -> Result<Vec<Entry>> {
    wincode::deserialize(data)
        .map_err(|e| SolanaStreamError::Serialization(format!("entry decode failed: {e}")))
}
