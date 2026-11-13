//! Shreds as Agave 4.4.1 puts them on the wire: decoding, the checks Agave's `sanitize`
//! makes, the Merkle root, the leader's signature and deshredding, without
//! `solana-ledger`.
//!
//! Each piece follows `solana-ledger` 4.4.1 and names the function it follows:
//! `src/shred.rs` (variants, headers, [`Shred`]), `src/shred/merkle.rs` (payload layout,
//! decoding, Merkle root), `src/shred/shred_data.rs` and `src/shred/shred_code.rs`
//! (`sanitize`), `src/shred/merkle_tree.rs` (the tree), `src/shred/wire.rs` ([`wire`]),
//! `src/sigverify_shreds.rs` ([`wire::verify_shred`]) and `src/shredder.rs` ([`deshred`]).
//! The SDK's differential tests run both on the same bytes, corrupted ones included.
//!
//! Agave 4.4.1 produces and accepts only Merkle shreds whose erasure batches chain to
//! the previous batch's Merkle root, optionally resigned by a retransmitter. Legacy
//! shreds and unchained Merkle shreds were removed from Agave before 4.4, and their
//! variant bytes are rejected here as they are there.
//!
//! ```
//! use solana_stream_sdk::shred::{wire, Shred};
//!
//! # fn leader_of(_slot: u64) -> Option<solana_pubkey::Pubkey> { None }
//! fn accept(datagram: &[u8]) -> Option<Shred> {
//!     // Find the slot's leader, check its signature, then decode.
//!     let leader = leader_of(wire::get_slot(datagram)?)?;
//!     if !wire::verify_shred(datagram, &leader) {
//!         return None;
//!     }
//!     Shred::new_from_serialized_shred(datagram).ok()
//! }
//! # assert!(accept(&[0u8; 1203]).is_none());
//! ```

use std::ops::Deref;

use bytes::Bytes;
use solana_hash::Hash;
use solana_pubkey::Pubkey;
use solana_signature::{Signature, SIGNATURE_BYTES};
use thiserror::Error;

mod merkle;
pub mod wire;

/// Agave's alias for [`wire`] (`shred::layout`). The readers it shares with
/// `solana_ledger::shred::layout` keep their names; `get_shred` takes the packet's bytes and
/// `get_flags` returns a `u8`, and the functions that write or take packets are absent
/// (`docs/migration-v5.md` in the repository lists them).
pub use self::wire as layout;

/// Bytes of the nonce a repair response appends to the shred (`SIZE_OF_NONCE`).
pub const SIZE_OF_NONCE: usize = 4;
/// Bytes of the leader's Ed25519 signature at the start of every shred.
pub const SIZE_OF_SIGNATURE: usize = SIGNATURE_BYTES;
/// Signature, variant, slot, index, version and FEC set index
/// (`SIZE_OF_COMMON_SHRED_HEADER` in `shred.rs`).
pub const SIZE_OF_COMMON_SHRED_HEADER: usize = 83;
/// Common header plus parent offset, flags and size (`SIZE_OF_DATA_SHRED_HEADERS`).
pub const SIZE_OF_DATA_SHRED_HEADERS: usize = 88;
/// Common header plus the coding header (`SIZE_OF_CODING_SHRED_HEADERS`).
pub const SIZE_OF_CODING_SHRED_HEADERS: usize = 89;
/// Bytes of a Merkle root (`SIZE_OF_MERKLE_ROOT` in `merkle_tree.rs`).
pub const SIZE_OF_MERKLE_ROOT: usize = 32;
/// Bytes of one Merkle proof entry (`SIZE_OF_MERKLE_PROOF_ENTRY` in `merkle_tree.rs`).
pub const SIZE_OF_MERKLE_PROOF_ENTRY: usize = 20;
/// Target data shreds per erasure batch (`DATA_SHREDS_PER_FEC_BLOCK`).
pub const DATA_SHREDS_PER_FEC_BLOCK: usize = 32;
/// Coding shreds per erasure batch (`CODING_SHREDS_PER_FEC_BLOCK`).
pub const CODING_SHREDS_PER_FEC_BLOCK: usize = 32;
/// Proof entries of a 32:32 batch, the only batch Agave 4.4.1 makes
/// (`PROOF_ENTRIES_FOR_32_32_BATCH` in `merkle_tree.rs`).
pub const PROOF_ENTRIES_FOR_32_32_BATCH: u8 = 6;
/// Data shreds a slot can hold (`MAX_DATA_SHREDS_PER_SLOT`, from
/// `solana_cost_model::shred_limit::DEFAULT_MAX_DATA_SHREDS_PER_SLOT`).
pub const MAX_DATA_SHREDS_PER_SLOT: usize = 32_768;
/// Coding shreds a slot can hold (`MAX_CODE_SHREDS_PER_SLOT`).
pub const MAX_CODE_SHREDS_PER_SLOT: usize = MAX_DATA_SHREDS_PER_SLOT;
/// Payload of a coding shred (`merkle::ShredCode::SIZE_OF_PAYLOAD`, 1228).
pub const CODE_SHRED_PAYLOAD_SIZE: usize = solana_packet::PACKET_DATA_SIZE - SIZE_OF_NONCE;
/// Payload of a data shred (`merkle::ShredData::SIZE_OF_PAYLOAD`, 1203): the coding
/// payload's erasure-coded part plus the signature.
pub const DATA_SHRED_PAYLOAD_SIZE: usize =
    CODE_SHRED_PAYLOAD_SIZE - SIZE_OF_CODING_SHRED_HEADERS + SIZE_OF_SIGNATURE;

/// `ShredFlags::SHRED_TICK_REFERENCE_MASK` in `shred.rs`.
pub const FLAG_SHRED_TICK_REFERENCE_MASK: u8 = 0b0011_1111;
/// `ShredFlags::DATA_COMPLETE_SHRED`: the last data shred of an entry batch.
pub const FLAG_DATA_COMPLETE_SHRED: u8 = 0b0100_0000;
/// `ShredFlags::LAST_SHRED_IN_SLOT`, which includes [`FLAG_DATA_COMPLETE_SHRED`].
pub const FLAG_LAST_SHRED_IN_SLOT: u8 = 0b1100_0000;

const _: () = assert!(CODE_SHRED_PAYLOAD_SIZE == 1228);
const _: () = assert!(DATA_SHRED_PAYLOAD_SIZE == 1203);

/// Why a shred was rejected. The message of each variant is the one `solana-ledger`
/// 4.4.1 gives for the same input (`shred::Error`), except [`TruncatedHeaders`] (Agave
/// reports the header deserializer's `WincodeRead` error) and [`InvalidErasureShardIndex`]
/// (Agave prints the headers).
///
/// [`TruncatedHeaders`]: ShredError::TruncatedHeaders
/// [`InvalidErasureShardIndex`]: ShredError::InvalidErasureShardIndex
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum ShredError {
    #[error("Invalid data size: {size}, payload: {payload}")]
    InvalidDataSize { size: u16, payload: usize },
    #[error("Invalid deshred set")]
    InvalidDeshredSet,
    /// The shred's place in its erasure batch is outside the batch or the slot.
    #[error("Invalid erasure shard index")]
    InvalidErasureShardIndex,
    #[error("Invalid merkle proof")]
    InvalidMerkleProof,
    #[error("Invalid num coding shreds: {0}")]
    InvalidNumCodingShreds(u16),
    #[error("Invalid parent_offset: {parent_offset}, slot: {slot}")]
    InvalidParentOffset { slot: u64, parent_offset: u16 },
    #[error("Invalid payload size: {0}")]
    InvalidPayloadSize(usize),
    #[error("Invalid proof size: {0}")]
    InvalidProofSize(u8),
    #[error("Invalid shred flags: {0}")]
    InvalidShredFlags(u8),
    #[error("Invalid {0:?} shred index: {1}")]
    InvalidShredIndex(ShredType, u32),
    #[error("Invalid shred type")]
    InvalidShredType,
    #[error("Invalid shred variant")]
    InvalidShredVariant,
    /// Deshredding: a gap in the indices, or no data-complete flag on the last shred
    /// (Agave's `reed_solomon_erasure::Error::TooFewDataShards`).
    #[error("The number of provided data shards is smaller than the one in codec")]
    TooFewDataShards,
    /// Fewer bytes than the coding shred headers (Agave: `Error::WincodeRead`).
    #[error("Truncated shred headers: {0} bytes")]
    TruncatedHeaders(usize),
}

/// Data or coding shred, with Agave's byte values (`ShredType` in `shred.rs`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum ShredType {
    Data = 0b1010_0101,
    Code = 0b0101_1010,
}

/// The variant byte: data or code, the number of Merkle proof entries (low four bits)
/// and whether a retransmitter signature follows the proof (`ShredVariant` in
/// `shred.rs`):
///
/// | byte          | variant                 |
/// |---------------|-------------------------|
/// | `0b0110_????` | Merkle code             |
/// | `0b0111_????` | Merkle code, resigned   |
/// | `0b1001_????` | Merkle data             |
/// | `0b1011_????` | Merkle data, resigned   |
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShredVariant {
    MerkleCode { proof_size: u8, resigned: bool },
    MerkleData { proof_size: u8, resigned: bool },
}

impl ShredVariant {
    /// Number of Merkle proof entries.
    pub fn proof_size(self) -> u8 {
        match self {
            Self::MerkleCode { proof_size, .. } | Self::MerkleData { proof_size, .. } => proof_size,
        }
    }

    /// Whether a retransmitter signature follows the Merkle proof.
    pub fn resigned(self) -> bool {
        match self {
            Self::MerkleCode { resigned, .. } | Self::MerkleData { resigned, .. } => resigned,
        }
    }
}

impl From<ShredVariant> for ShredType {
    fn from(variant: ShredVariant) -> Self {
        match variant {
            ShredVariant::MerkleCode { .. } => ShredType::Code,
            ShredVariant::MerkleData { .. } => ShredType::Data,
        }
    }
}

impl From<ShredVariant> for u8 {
    fn from(variant: ShredVariant) -> u8 {
        match variant {
            ShredVariant::MerkleCode {
                proof_size,
                resigned: false,
            } => proof_size | 0x60,
            ShredVariant::MerkleCode {
                proof_size,
                resigned: true,
            } => proof_size | 0x70,
            ShredVariant::MerkleData {
                proof_size,
                resigned: false,
            } => proof_size | 0x90,
            ShredVariant::MerkleData {
                proof_size,
                resigned: true,
            } => proof_size | 0xb0,
        }
    }
}

/// `TryFrom<u8> for ShredVariant` in `shred.rs`. The legacy bytes Agave names first
/// (`ShredType::Code`, `0x5a`, and `ShredType::Data`, `0xa5`) have high nibbles `0x5`
/// and `0xa`, so they fail the match below as they fail Agave's explicit check.
impl TryFrom<u8> for ShredVariant {
    type Error = ShredError;

    fn try_from(byte: u8) -> Result<Self, ShredError> {
        let proof_size = byte & 0x0F;
        match byte & 0xF0 {
            0x60 => Ok(Self::MerkleCode {
                proof_size,
                resigned: false,
            }),
            0x70 => Ok(Self::MerkleCode {
                proof_size,
                resigned: true,
            }),
            0x90 => Ok(Self::MerkleData {
                proof_size,
                resigned: false,
            }),
            0xb0 => Ok(Self::MerkleData {
                proof_size,
                resigned: true,
            }),
            _ => Err(ShredError::InvalidShredVariant),
        }
    }
}

/// `ShredCommonHeader` in `shred.rs`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CommonHeader {
    signature: Signature,
    variant: ShredVariant,
    slot: u64,
    index: u32,
    version: u16,
    fec_set_index: u32,
}

impl CommonHeader {
    /// Reads the header from a payload of at least [`SIZE_OF_COMMON_SHRED_HEADER`] bytes.
    fn read(payload: &[u8]) -> Result<Self, ShredError> {
        let truncated = || ShredError::TruncatedHeaders(payload.len());
        Ok(Self {
            signature: wire::get_signature(payload).ok_or_else(truncated)?,
            variant: wire::get_shred_variant(payload)?,
            slot: wire::get_slot(payload).ok_or_else(truncated)?,
            index: wire::get_index(payload).ok_or_else(truncated)?,
            version: wire::get_version(payload).ok_or_else(truncated)?,
            fec_set_index: wire::get_fec_set_index(payload).ok_or_else(truncated)?,
        })
    }
}

/// `DataShredHeader` in `shred.rs`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DataHeader {
    parent_offset: u16,
    flags: u8,
    /// Headers plus data.
    size: u16,
}

/// `CodingShredHeader` in `shred.rs`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CodingHeader {
    num_data_shreds: u16,
    num_coding_shreds: u16,
    /// In `0..num_coding_shreds`.
    position: u16,
}

/// The capacity of a shred's erasure-coded buffer after its headers: for a data shred
/// the most entry bytes it can carry (`capacity` in `merkle.rs`'s `impl_merkle_shred!`).
fn capacity(
    size_of_payload: usize,
    size_of_headers: usize,
    proof_size: u8,
    resigned: bool,
) -> Result<usize, ShredError> {
    size_of_payload
        .checked_sub(
            size_of_headers
                + SIZE_OF_MERKLE_ROOT
                + usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY
                + if resigned { SIZE_OF_SIGNATURE } else { 0 },
        )
        .ok_or(ShredError::InvalidProofSize(proof_size))
}

/// `get_chained_merkle_root_offset` in `merkle.rs`: right after the buffer.
fn get_chained_merkle_root_offset(
    size_of_payload: usize,
    size_of_headers: usize,
    proof_size: u8,
    resigned: bool,
) -> Result<usize, ShredError> {
    Ok(size_of_headers + capacity(size_of_payload, size_of_headers, proof_size, resigned)?)
}

/// `get_proof_offset` in `merkle.rs`: after the chained Merkle root.
fn get_proof_offset(
    size_of_payload: usize,
    size_of_headers: usize,
    proof_size: u8,
    resigned: bool,
) -> Result<usize, ShredError> {
    Ok(
        get_chained_merkle_root_offset(size_of_payload, size_of_headers, proof_size, resigned)?
            + SIZE_OF_MERKLE_ROOT,
    )
}

/// `get_retransmitter_signature_offset` in `merkle.rs`: after the proof, and only for a
/// resigned variant.
fn get_retransmitter_signature_offset(
    size_of_payload: usize,
    size_of_headers: usize,
    proof_size: u8,
    resigned: bool,
) -> Result<usize, ShredError> {
    if !resigned {
        return Err(ShredError::InvalidShredVariant);
    }
    let proof_offset = get_proof_offset(size_of_payload, size_of_headers, proof_size, resigned)?;
    Ok(proof_offset + usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY)
}

/// A shred's bytes (`shred::Payload` in Agave): read it as `&[u8]` (it derefs to one, and
/// `as_ref()` gives one), and clone it without copying.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Payload(Bytes);

impl Deref for Payload {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Payload {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Payload> for Vec<u8> {
    fn from(payload: Payload) -> Self {
        payload.0.to_vec()
    }
}

/// A shred decoded and checked as `solana_ledger::shred::Shred::new_from_serialized_shred`
/// 4.4.1 does it, with the same variants (`ShredCode`, `ShredData`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Shred {
    ShredCode(ShredCode),
    ShredData(ShredData),
}

/// A data shred: headers, entry bytes, the chained Merkle root, the Merkle proof and,
/// when resigned, the retransmitter's signature (`merkle::ShredData`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShredData {
    common_header: CommonHeader,
    data_header: DataHeader,
    payload: Payload,
}

/// A coding shred: headers, an erasure-coded shard, the chained Merkle root, the Merkle
/// proof and, when resigned, the retransmitter's signature (`merkle::ShredCode`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShredCode {
    common_header: CommonHeader,
    coding_header: CodingHeader,
    payload: Payload,
}

/// What the two shred types share (`impl_merkle_shred!` and `impl_shred_common!` in
/// `merkle.rs` and `common.rs`).
macro_rules! impl_merkle_shred {
    () => {
        /// The canonical payload: the received bytes cut to the shred's size.
        pub fn payload(&self) -> &Payload {
            &self.payload
        }

        pub fn slot(&self) -> u64 {
            self.common_header.slot
        }

        pub fn index(&self) -> u32 {
            self.common_header.index
        }

        pub fn version(&self) -> u16 {
            self.common_header.version
        }

        pub fn fec_set_index(&self) -> u32 {
            self.common_header.fec_set_index
        }

        /// The leader's signature over the Merkle root.
        pub fn signature(&self) -> &Signature {
            &self.common_header.signature
        }

        pub fn variant(&self) -> ShredVariant {
            self.common_header.variant
        }

        fn capacity(&self) -> Result<usize, ShredError> {
            capacity(
                Self::SIZE_OF_PAYLOAD,
                Self::SIZE_OF_HEADERS,
                self.common_header.variant.proof_size(),
                self.common_header.variant.resigned(),
            )
        }

        fn proof_offset(&self) -> Result<usize, ShredError> {
            get_proof_offset(
                Self::SIZE_OF_PAYLOAD,
                Self::SIZE_OF_HEADERS,
                self.common_header.variant.proof_size(),
                self.common_header.variant.resigned(),
            )
        }

        /// `merkle_proof` in `merkle.rs`.
        fn merkle_proof(
            &self,
        ) -> Result<impl Iterator<Item = &merkle::MerkleProofEntry>, ShredError> {
            merkle::get_merkle_proof(
                &self.payload,
                self.proof_offset()?,
                self.common_header.variant.proof_size(),
            )
        }

        /// The Merkle root of the erasure batch, computed from this shred and its proof
        /// (`merkle_root` in `merkle.rs`); the leader's signature is over it.
        pub fn merkle_root(&self) -> Result<Hash, ShredError> {
            let index = self.erasure_shard_index()?;
            let proof_offset = self.proof_offset()?;
            let proof = self.merkle_proof()?;
            let node = merkle::get_merkle_node(&self.payload, SIZE_OF_SIGNATURE, proof_offset)?;
            merkle::get_merkle_root(index, node, proof)
        }

        /// The Merkle root of the previous erasure batch (`chained_merkle_root`).
        pub fn chained_merkle_root(&self) -> Result<Hash, ShredError> {
            let offset = get_chained_merkle_root_offset(
                Self::SIZE_OF_PAYLOAD,
                Self::SIZE_OF_HEADERS,
                self.common_header.variant.proof_size(),
                self.common_header.variant.resigned(),
            )?;
            self.payload
                .get(offset..offset + SIZE_OF_MERKLE_ROOT)
                .and_then(|root| <[u8; SIZE_OF_MERKLE_ROOT]>::try_from(root).ok())
                .map(Hash::new_from_array)
                .ok_or(ShredError::InvalidPayloadSize(self.payload.len()))
        }

        /// The retransmitter's signature; [`ShredError::InvalidShredVariant`] when the
        /// variant is not resigned (`retransmitter_signature`).
        pub fn retransmitter_signature(&self) -> Result<Signature, ShredError> {
            let offset = get_retransmitter_signature_offset(
                Self::SIZE_OF_PAYLOAD,
                Self::SIZE_OF_HEADERS,
                self.common_header.variant.proof_size(),
                self.common_header.variant.resigned(),
            )?;
            self.payload
                .get(offset..offset + SIZE_OF_SIGNATURE)
                .and_then(|bytes| <[u8; SIZE_OF_SIGNATURE]>::try_from(bytes).ok())
                .map(Signature::from)
                .ok_or(ShredError::InvalidPayloadSize(self.payload.len()))
        }

        /// The erasure-coded part of the payload (`erasure_shard_offsets` and
        /// `erasure_shard` in `merkle.rs`).
        fn erasure_shard(&self) -> Result<&[u8], ShredError> {
            if self.payload.len() != Self::SIZE_OF_PAYLOAD {
                return Err(ShredError::InvalidPayloadSize(self.payload.len()));
            }
            let end = Self::SIZE_OF_HEADERS + self.capacity()?;
            self.payload
                .get(Self::ERASURE_SHARD_START_OFFSET..end)
                .ok_or(ShredError::InvalidPayloadSize(self.payload.len()))
        }
    };
}

impl ShredData {
    const SIZE_OF_PAYLOAD: usize = DATA_SHRED_PAYLOAD_SIZE;
    const SIZE_OF_HEADERS: usize = SIZE_OF_DATA_SHRED_HEADERS;
    // Everything after the signature is erasure coded (`ERASURE_SHARD_START_OFFSET`).
    const ERASURE_SHARD_START_OFFSET: usize = SIZE_OF_SIGNATURE;

    impl_merkle_shred!();

    /// `ShredData::from_payload` in `merkle.rs`: at least [`DATA_SHRED_PAYLOAD_SIZE`]
    /// bytes, cut to that size, then [`Self::sanitize`].
    fn from_payload(mut payload: Bytes) -> Result<Self, ShredError> {
        if payload.len() < Self::SIZE_OF_PAYLOAD {
            return Err(ShredError::InvalidPayloadSize(payload.len()));
        }
        payload.truncate(Self::SIZE_OF_PAYLOAD);
        let truncated = || ShredError::TruncatedHeaders(payload.len());
        let common_header = CommonHeader::read(&payload)?;
        let data_header = DataHeader {
            parent_offset: wire::read_u16(&payload, wire::OFFSET_OF_PARENT_OFFSET)
                .ok_or_else(truncated)?,
            flags: *payload.get(wire::OFFSET_OF_FLAGS).ok_or_else(truncated)?,
            size: wire::read_u16(&payload, wire::OFFSET_OF_DATA_SIZE).ok_or_else(truncated)?,
        };
        if !matches!(common_header.variant, ShredVariant::MerkleData { .. }) {
            return Err(ShredError::InvalidShredVariant);
        }
        let shred = Self {
            common_header,
            data_header,
            payload: Payload(payload),
        };
        shred.sanitize()?;
        Ok(shred)
    }

    /// `ShredData::get_data` in `merkle.rs`: `shred[88..size]`, where `size` (the data
    /// header's size field, headers included) is at least the headers and at most the
    /// headers plus the capacity.
    fn get_data(
        shred: &[u8],
        proof_size: u8,
        resigned: bool,
        size: u16,
    ) -> Result<&[u8], ShredError> {
        let size = usize::from(size);
        let data_buffer_size = capacity(
            Self::SIZE_OF_PAYLOAD,
            Self::SIZE_OF_HEADERS,
            proof_size,
            resigned,
        )?;
        (Self::SIZE_OF_HEADERS..=Self::SIZE_OF_HEADERS + data_buffer_size)
            .contains(&size)
            .then(|| shred.get(Self::SIZE_OF_HEADERS..size))
            .flatten()
            .ok_or(ShredError::InvalidDataSize {
                size: size as u16,
                payload: shred.len(),
            })
    }

    /// The entry bytes this shred carries (`data`).
    pub fn data(&self) -> Result<&[u8], ShredError> {
        Self::get_data(
            &self.payload,
            self.common_header.variant.proof_size(),
            self.common_header.variant.resigned(),
            self.data_header.size,
        )
    }

    /// The parent slot: `slot - parent_offset`; a zero offset is valid only in slot 0
    /// (`ShredData::parent` in `traits.rs`).
    pub fn parent(&self) -> Result<u64, ShredError> {
        let slot = self.common_header.slot;
        let parent_offset = self.data_header.parent_offset;
        if parent_offset == 0 && slot != 0 {
            return Err(ShredError::InvalidParentOffset {
                slot,
                parent_offset,
            });
        }
        slot.checked_sub(u64::from(parent_offset))
            .ok_or(ShredError::InvalidParentOffset {
                slot,
                parent_offset,
            })
    }

    /// The flags byte: the reference tick and the data-complete and last-in-slot bits.
    pub fn flags(&self) -> u8 {
        self.data_header.flags
    }

    /// The tick of the slot this shred's entries were made at, saturated at 63.
    pub fn reference_tick(&self) -> u8 {
        self.data_header.flags & FLAG_SHRED_TICK_REFERENCE_MASK
    }

    pub fn last_in_slot(&self) -> bool {
        self.data_header.flags & FLAG_LAST_SHRED_IN_SLOT == FLAG_LAST_SHRED_IN_SLOT
    }

    pub fn data_complete(&self) -> bool {
        wire::is_data_complete(self.data_header.flags)
    }

    /// `index - fec_set_index` (`shred_data::erasure_shard_index`).
    fn erasure_shard_index(&self) -> Result<usize, ShredError> {
        self.common_header
            .index
            .checked_sub(self.common_header.fec_set_index)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or(ShredError::InvalidErasureShardIndex)
    }

    /// `ShredData::sanitize` in `merkle.rs`, then `shred_data::sanitize`, in their order.
    pub fn sanitize(&self) -> Result<(), ShredError> {
        if !matches!(self.common_header.variant, ShredVariant::MerkleData { .. }) {
            return Err(ShredError::InvalidShredVariant);
        }
        let _ = self.merkle_proof()?;
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(ShredError::InvalidPayloadSize(self.payload.len()));
        }
        let index = self.common_header.index;
        if index as usize >= MAX_DATA_SHREDS_PER_SLOT {
            return Err(ShredError::InvalidShredIndex(ShredType::Data, index));
        }
        // LAST_SHRED_IN_SLOT includes DATA_COMPLETE_SHRED.
        let flags = self.data_header.flags;
        if flags & FLAG_LAST_SHRED_IN_SLOT != 0 && !wire::is_data_complete(flags) {
            return Err(ShredError::InvalidShredFlags(flags));
        }
        let _data = self.data()?;
        let _parent = self.parent()?;
        let _shard_index = self.erasure_shard_index()?;
        let _erasure_shard = self.erasure_shard()?;
        Ok(())
    }
}

impl ShredCode {
    const SIZE_OF_PAYLOAD: usize = CODE_SHRED_PAYLOAD_SIZE;
    const SIZE_OF_HEADERS: usize = SIZE_OF_CODING_SHRED_HEADERS;
    // The headers are not erasure coded (`ERASURE_SHARD_START_OFFSET`).
    const ERASURE_SHARD_START_OFFSET: usize = Self::SIZE_OF_HEADERS;

    impl_merkle_shred!();

    /// `ShredCode::from_payload` in `merkle.rs`: the headers are read first (fewer bytes
    /// than the headers is [`ShredError::TruncatedHeaders`]), then at least
    /// [`CODE_SHRED_PAYLOAD_SIZE`] bytes, cut to that size, then [`Self::sanitize`].
    fn from_payload(mut payload: Bytes) -> Result<Self, ShredError> {
        let truncated = || ShredError::TruncatedHeaders(payload.len());
        if payload.len() < Self::SIZE_OF_HEADERS {
            return Err(truncated());
        }
        let common_header = CommonHeader::read(&payload)?;
        let coding_header = CodingHeader {
            num_data_shreds: wire::read_u16(&payload, wire::OFFSET_OF_NUM_DATA_SHREDS)
                .ok_or_else(truncated)?,
            num_coding_shreds: wire::read_u16(&payload, wire::OFFSET_OF_NUM_CODING_SHREDS)
                .ok_or_else(truncated)?,
            position: wire::read_u16(&payload, wire::OFFSET_OF_POSITION).ok_or_else(truncated)?,
        };
        if !matches!(common_header.variant, ShredVariant::MerkleCode { .. }) {
            return Err(ShredError::InvalidShredVariant);
        }
        if payload.len() < Self::SIZE_OF_PAYLOAD {
            return Err(ShredError::InvalidPayloadSize(payload.len()));
        }
        payload.truncate(Self::SIZE_OF_PAYLOAD);
        let shred = Self {
            common_header,
            coding_header,
            payload: Payload(payload),
        };
        shred.sanitize()?;
        Ok(shred)
    }

    /// Data shreds in the erasure batch.
    pub fn num_data_shreds(&self) -> u16 {
        self.coding_header.num_data_shreds
    }

    /// Coding shreds in the erasure batch.
    pub fn num_coding_shreds(&self) -> u16 {
        self.coding_header.num_coding_shreds
    }

    /// This shred's place among the batch's coding shreds.
    pub fn position(&self) -> u16 {
        self.coding_header.position
    }

    /// The index of the batch's first coding shred: `index - position`
    /// (`ShredCode::first_coding_index` in `traits.rs`).
    pub fn first_coding_index(&self) -> Option<u32> {
        self.common_header
            .index
            .checked_sub(u32::from(self.coding_header.position))
    }

    /// `num_data_shreds + position`, when the batch fits the slot and the position the
    /// batch (`shred_code::erasure_shard_index`).
    fn erasure_shard_index(&self) -> Result<usize, ShredError> {
        self.checked_erasure_shard_index()
            .ok_or(ShredError::InvalidErasureShardIndex)
    }

    fn checked_erasure_shard_index(&self) -> Option<usize> {
        let CodingHeader {
            num_data_shreds,
            num_coding_shreds,
            position,
        } = self.coding_header;
        // The batch's last data and last coding shred stay below the slot's limits.
        if self
            .common_header
            .fec_set_index
            .checked_add(u32::from(num_data_shreds.checked_sub(1)?))? as usize
            >= MAX_DATA_SHREDS_PER_SLOT
        {
            return None;
        }
        if self
            .first_coding_index()?
            .checked_add(u32::from(num_coding_shreds.checked_sub(1)?))? as usize
            >= MAX_CODE_SHREDS_PER_SLOT
        {
            return None;
        }
        let num_data_shreds = usize::from(num_data_shreds);
        let num_coding_shreds = usize::from(num_coding_shreds);
        let fec_set_size = num_data_shreds.checked_add(num_coding_shreds)?;
        let index = usize::from(position).checked_add(num_data_shreds)?;
        (index < fec_set_size).then_some(index)
    }

    /// `ShredCode::sanitize` in `merkle.rs`, then `shred_code::sanitize`, in their order.
    pub fn sanitize(&self) -> Result<(), ShredError> {
        if !matches!(self.common_header.variant, ShredVariant::MerkleCode { .. }) {
            return Err(ShredError::InvalidShredVariant);
        }
        let _ = self.merkle_proof()?;
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(ShredError::InvalidPayloadSize(self.payload.len()));
        }
        let index = self.common_header.index;
        if index as usize >= MAX_CODE_SHREDS_PER_SLOT {
            return Err(ShredError::InvalidShredIndex(ShredType::Code, index));
        }
        let num_coding_shreds = self.coding_header.num_coding_shreds;
        if usize::from(num_coding_shreds) > 8 * DATA_SHREDS_PER_FEC_BLOCK {
            return Err(ShredError::InvalidNumCodingShreds(num_coding_shreds));
        }
        let _shard_index = self.erasure_shard_index()?;
        let _erasure_shard = self.erasure_shard()?;
        Ok(())
    }
}

/// Forwards a method to the shred of either type, as `dispatch!` does in `shred.rs`.
macro_rules! dispatch {
    ($(#[$meta:meta])* pub fn $name:ident(&self) -> $out:ty) => {
        $(#[$meta])*
        pub fn $name(&self) -> $out {
            match self {
                Self::ShredCode(shred) => shred.$name(),
                Self::ShredData(shred) => shred.$name(),
            }
        }
    };
}

impl Shred {
    /// Decodes and checks a shred as `solana_ledger::shred::Shred::new_from_serialized_shred`
    /// 4.4.1 does: the variant byte picks the type, the bytes past the type's payload
    /// size (a repair nonce, padding) are dropped, and the shred has to pass
    /// [`Self::sanitize`]. The same bytes are accepted and rejected as there.
    ///
    /// The leader's signature is not checked: use [`Self::verify`] or
    /// [`wire::verify_shred`].
    pub fn new_from_serialized_shred(shred: impl Into<Vec<u8>>) -> Result<Self, ShredError> {
        let payload = Bytes::from(shred.into());
        match wire::get_shred_variant(&payload)? {
            ShredVariant::MerkleCode { .. } => {
                Ok(Self::ShredCode(ShredCode::from_payload(payload)?))
            }
            ShredVariant::MerkleData { .. } => {
                Ok(Self::ShredData(ShredData::from_payload(payload)?))
            }
        }
    }

    dispatch!(
        /// The checks Agave's `Shred::sanitize` makes. A decoded shred has passed them.
        pub fn sanitize(&self) -> Result<(), ShredError>
    );
    dispatch!(
        /// The canonical payload.
        pub fn payload(&self) -> &Payload
    );
    dispatch!(pub fn slot(&self) -> u64);
    dispatch!(pub fn index(&self) -> u32);
    dispatch!(pub fn version(&self) -> u16);
    dispatch!(pub fn fec_set_index(&self) -> u32);
    dispatch!(
        /// The leader's signature over [`Self::merkle_root`].
        pub fn signature(&self) -> &Signature
    );
    dispatch!(pub fn variant(&self) -> ShredVariant);
    dispatch!(
        /// The Merkle root of the erasure batch, computed from this shred and its proof.
        pub fn merkle_root(&self) -> Result<Hash, ShredError>
    );
    dispatch!(
        /// The Merkle root of the previous erasure batch.
        pub fn chained_merkle_root(&self) -> Result<Hash, ShredError>
    );
    dispatch!(
        /// The retransmitter's signature of a resigned shred.
        pub fn retransmitter_signature(&self) -> Result<Signature, ShredError>
    );

    pub fn shred_type(&self) -> ShredType {
        ShredType::from(self.variant())
    }

    pub fn is_data(&self) -> bool {
        self.shred_type() == ShredType::Data
    }

    pub fn is_code(&self) -> bool {
        self.shred_type() == ShredType::Code
    }

    /// Number of Merkle proof entries.
    pub fn proof_size(&self) -> u8 {
        self.variant().proof_size()
    }

    /// The parent slot; [`ShredError::InvalidShredType`] for a coding shred.
    pub fn parent(&self) -> Result<u64, ShredError> {
        match self {
            Self::ShredCode(_) => Err(ShredError::InvalidShredType),
            Self::ShredData(shred) => shred.parent(),
        }
    }

    /// The entry bytes of a data shred; [`ShredError::InvalidShredType`] for a coding shred.
    pub fn data(&self) -> Result<&[u8], ShredError> {
        match self {
            Self::ShredCode(_) => Err(ShredError::InvalidShredType),
            Self::ShredData(shred) => shred.data(),
        }
    }

    /// `false` for a coding shred.
    pub fn last_in_slot(&self) -> bool {
        match self {
            Self::ShredCode(_) => false,
            Self::ShredData(shred) => shred.last_in_slot(),
        }
    }

    /// `false` for a coding shred.
    pub fn data_complete(&self) -> bool {
        match self {
            Self::ShredCode(_) => false,
            Self::ShredData(shred) => shred.data_complete(),
        }
    }

    /// Whether `leader` signed this shred's Merkle root (`Shred::verify` in `shred.rs`).
    /// `false` when the root cannot be computed from the shred's proof.
    #[must_use]
    pub fn verify(&self, leader: &Pubkey) -> bool {
        match self.merkle_root() {
            Ok(merkle_root) => self
                .signature()
                .verify(leader.as_ref(), merkle_root.as_ref()),
            Err(_) => false,
        }
    }
}

/// Joins the entry bytes of consecutive data shreds, as `Shredder::deshred` in
/// `solana-ledger` 4.4.1 `src/shredder.rs` does, with the same accept and reject rules
/// and the same bytes out:
///
/// - the shreds' indices are consecutive ([`ShredError::TooFewDataShards`] otherwise);
/// - only the last shred carries the data-complete flag (a shred after one that does is
///   [`ShredError::InvalidDeshredSet`]; no flag on the last is `TooFewDataShards`, as is
///   an empty list);
/// - each one is a data shred whose size field fits its payload;
/// - when the shreds carry no data at all, the result is 1083 zero bytes (the data
///   capacity of a proofless shred), which decodes as an empty entry list.
///
/// The shreds are serialized payloads, for example [`Shred::payload`]. Only the fields
/// above are read; a shred's other checks are [`Shred::new_from_serialized_shred`]'s.
pub fn deshred<I, T>(shreds: I) -> Result<Vec<u8>, ShredError>
where
    I: IntoIterator<Item = T>,
    T: AsRef<[u8]>,
{
    let (data, _, data_complete) = shreds.into_iter().try_fold(
        (Vec::<u8>::new(), None::<u32>, false),
        |(mut data, prev, data_complete), shred| {
            // Nothing may follow a data-complete shred.
            if data_complete {
                return Err(ShredError::InvalidDeshredSet);
            }
            let shred = shred.as_ref();
            // Indices have to be consecutive.
            let index =
                Some(wire::get_index(shred).ok_or(ShredError::InvalidPayloadSize(shred.len()))?);
            if let Some(prev) = prev {
                if prev.checked_add(1) != index {
                    return Err(ShredError::TooFewDataShards);
                }
            }
            data.extend_from_slice(wire::get_data(shred)?);
            let flags = wire::get_flags(shred)?;
            Ok((data, index, wire::is_data_complete(flags)))
        },
    )?;
    // The last shred has to complete the data.
    if !data_complete {
        return Err(ShredError::TooFewDataShards);
    }
    if data.is_empty() {
        // Agave returns a zeroed buffer so that it deserializes as an empty Vec<Entry>.
        let data_buffer_size = capacity(
            DATA_SHRED_PAYLOAD_SIZE,
            SIZE_OF_DATA_SHRED_HEADERS,
            0,
            false,
        )?;
        Ok(vec![0u8; data_buffer_size])
    } else {
        Ok(data)
    }
}
