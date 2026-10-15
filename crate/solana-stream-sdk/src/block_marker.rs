//! Alpenglow block markers inside a deshredded payload.
//!
//! A deshredded payload is either an entry batch or a block marker. Both start
//! with a little-endian `u64`: the entry count. A marker carries a count of zero
//! followed by a `VersionedBlockMarker`. The wire layout is
//! `BlockComponent` / `VersionedBlockMarker` / `BlockMarkerV1` in
//! `solana-entry` (`block_component.rs`).

use crate::{decode_entries, Result};
use solana_entry::{block_component::VersionedBlockMarker, entry::Entry};

/// Width of the leading entry count (`u64` LE); a block marker carries zero here.
const ENTRY_COUNT_LEN: usize = 8;
/// `VersionedBlockMarker` tag (`u16` LE) for `BlockMarkerV1`.
const BLOCK_MARKER_VERSION_V1: u16 = 1;
/// Marker header after the entry count: version (`u16`) + variant (`u8`) + length (`u16`).
const BLOCK_MARKER_HEADER_LEN: usize = 13;

/// Variant of an Alpenglow block marker, numbered as `BlockMarkerV1` in `solana-entry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockMarkerKind {
    BlockFooter,
    BlockHeader,
    UpdateParent,
    GenesisCertificate,
    /// A zero entry count followed by bytes that are not a well-formed V1 marker,
    /// or a V1 marker with a variant this SDK does not know.
    Unknown,
}

/// Decoded content of one deshredded payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockComponentPayload {
    /// An entry batch. An empty vector is the empty batch agave sends to close
    /// an aborted slot.
    Entries(Vec<Entry>),
    /// A block marker. `marker` is `None` when the marker body does not decode,
    /// and always for [`BlockMarkerKind::Unknown`].
    BlockMarker {
        kind: BlockMarkerKind,
        marker: Option<VersionedBlockMarker>,
    },
}

/// Classify a deshredded payload without decoding it.
///
/// `None` is an entry batch, including the all-zero empty batch (`[0u8; 8]`,
/// optionally zero-padded). `Some` is a block marker. `Unknown` means a zero
/// entry count followed by bytes that are not a V1 marker; a caller that guessed
/// where a segment starts (a mid-batch slice can begin with zero bytes) must
/// treat `Unknown` as undecodable, not as a marker. `SlotDataBatch` never
/// guesses: a segment starts at index 0 or right after an observed
/// DATA_COMPLETE shred.
pub fn classify_block_marker(payload: &[u8]) -> Option<BlockMarkerKind> {
    if payload.len() < ENTRY_COUNT_LEN || payload[..ENTRY_COUNT_LEN] != [0u8; ENTRY_COUNT_LEN] {
        return None;
    }
    if payload[ENTRY_COUNT_LEN..].iter().all(|byte| *byte == 0) {
        return None;
    }

    let well_formed = payload.len() >= BLOCK_MARKER_HEADER_LEN
        && payload[8..10] == BLOCK_MARKER_VERSION_V1.to_le_bytes()
        && BLOCK_MARKER_HEADER_LEN + u16::from_le_bytes([payload[11], payload[12]]) as usize
            == payload.len();
    if !well_formed {
        return Some(BlockMarkerKind::Unknown);
    }
    Some(match payload[10] {
        0 => BlockMarkerKind::BlockFooter,
        1 => BlockMarkerKind::BlockHeader,
        2 => BlockMarkerKind::UpdateParent,
        3 => BlockMarkerKind::GenesisCertificate,
        _ => BlockMarkerKind::Unknown,
    })
}

/// Decode a deshredded payload into an entry batch or a block marker.
///
/// The kind comes from [`classify_block_marker`]; a marker is never decoded as
/// entries. `Unknown` means a zero entry count followed by bytes that are not a
/// V1 marker; a caller that guessed where a segment starts (a mid-batch slice
/// can begin with zero bytes) must treat `Unknown` as undecodable, not as a
/// marker. `SlotDataBatch` never guesses: a segment starts at index 0 or right
/// after an observed DATA_COMPLETE shred.
pub fn decode_block_component(payload: &[u8]) -> Result<BlockComponentPayload> {
    if let Some(kind) = classify_block_marker(payload) {
        let marker = if kind == BlockMarkerKind::Unknown {
            None
        } else {
            wincode::deserialize::<VersionedBlockMarker>(&payload[ENTRY_COUNT_LEN..]).ok()
        };
        return Ok(BlockComponentPayload::BlockMarker { kind, marker });
    }
    if payload.len() >= ENTRY_COUNT_LEN && payload.iter().all(|byte| *byte == 0) {
        return Ok(BlockComponentPayload::Entries(Vec::new()));
    }
    Ok(BlockComponentPayload::Entries(decode_entries(payload)?))
}
