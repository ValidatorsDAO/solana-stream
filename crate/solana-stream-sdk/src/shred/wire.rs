//! Reads pieces of a serialized shred without decoding all of it, as `solana-ledger`
//! 4.4.1 `src/shred/wire.rs` (also exported as `layout`) does, and checks the leader's
//! signature the way `src/sigverify_shreds.rs` does.
//!
//! These functions work on the bytes as received. They make only the checks the named
//! Agave function makes, so they accept some shreds [`Shred::new_from_serialized_shred`]
//! rejects; Agave's signature check runs on this path, before a shred is decoded.
//!
//! [`Shred::new_from_serialized_shred`]: super::Shred::new_from_serialized_shred

use solana_hash::Hash;
use solana_pubkey::Pubkey;
use solana_signature::{Signature, SIGNATURE_BYTES};

use super::{
    merkle, ShredData, ShredError, ShredType, ShredVariant, CODE_SHRED_PAYLOAD_SIZE,
    DATA_SHRED_PAYLOAD_SIZE, FLAG_DATA_COMPLETE_SHRED, SIZE_OF_CODING_SHRED_HEADERS,
    SIZE_OF_DATA_SHRED_HEADERS, SIZE_OF_MERKLE_ROOT, SIZE_OF_SIGNATURE,
};

const OFFSET_OF_SHRED_VARIANT: usize = SIZE_OF_SIGNATURE;
const OFFSET_OF_SLOT: usize = 65;
const OFFSET_OF_INDEX: usize = 73;
const OFFSET_OF_VERSION: usize = 77;
const OFFSET_OF_FEC_SET_INDEX: usize = 79;
// Data shred header.
pub(crate) const OFFSET_OF_PARENT_OFFSET: usize = 83;
pub(crate) const OFFSET_OF_FLAGS: usize = 85;
pub(crate) const OFFSET_OF_DATA_SIZE: usize = 86;
// Coding shred header.
pub(crate) const OFFSET_OF_NUM_DATA_SHREDS: usize = 83;
pub(crate) const OFFSET_OF_NUM_CODING_SHREDS: usize = 85;
pub(crate) const OFFSET_OF_POSITION: usize = 87;

fn read<const N: usize>(shred: &[u8], offset: usize) -> Option<[u8; N]> {
    shred.get(offset..offset + N)?.try_into().ok()
}

pub(crate) fn read_u16(shred: &[u8], offset: usize) -> Option<u16> {
    read(shred, offset).map(u16::from_le_bytes)
}

fn read_u32(shred: &[u8], offset: usize) -> Option<u32> {
    read(shred, offset).map(u32::from_le_bytes)
}

/// `get_shred_size` in `wire.rs`: the canonical payload size for the shred's variant.
fn get_shred_size(shred: &[u8]) -> Option<usize> {
    Some(match get_shred_variant(shred).ok()? {
        ShredVariant::MerkleCode { .. } => CODE_SHRED_PAYLOAD_SIZE,
        ShredVariant::MerkleData { .. } => DATA_SHRED_PAYLOAD_SIZE,
    })
}

/// The shred in a received packet: its first [`DATA_SHRED_PAYLOAD_SIZE`] or
/// [`CODE_SHRED_PAYLOAD_SIZE`] bytes, by variant, or `None` when the packet is shorter or
/// its variant is unknown (`get_shred` in `wire.rs`). Trailing bytes, such as a repair
/// nonce, are left out.
pub fn get_shred(packet: &[u8]) -> Option<&[u8]> {
    packet.get(..get_shred_size(packet)?)
}

/// The leader's signature, the first 64 bytes (`get_signature` in `wire.rs`).
pub fn get_signature(shred: &[u8]) -> Option<Signature> {
    read::<SIGNATURE_BYTES>(shred, 0).map(Signature::from)
}

/// The shred variant byte (`get_shred_variant` in `wire.rs`).
pub fn get_shred_variant(shred: &[u8]) -> Result<ShredVariant, ShredError> {
    let Some(&shred_variant) = shred.get(OFFSET_OF_SHRED_VARIANT) else {
        return Err(ShredError::InvalidPayloadSize(shred.len()));
    };
    ShredVariant::try_from(shred_variant)
}

/// Data or code (`get_shred_type` in `wire.rs`).
pub fn get_shred_type(shred: &[u8]) -> Result<ShredType, ShredError> {
    get_shred_variant(shred).map(ShredType::from)
}

/// `get_slot` in `wire.rs`; use it to find the slot's leader before [`verify_shred`].
pub fn get_slot(shred: &[u8]) -> Option<u64> {
    read(shred, OFFSET_OF_SLOT).map(u64::from_le_bytes)
}

/// `get_index` in `wire.rs`.
pub fn get_index(shred: &[u8]) -> Option<u32> {
    read_u32(shred, OFFSET_OF_INDEX)
}

/// `get_version` in `wire.rs`.
pub fn get_version(shred: &[u8]) -> Option<u16> {
    read_u16(shred, OFFSET_OF_VERSION)
}

/// `get_fec_set_index` in `wire.rs`.
pub fn get_fec_set_index(shred: &[u8]) -> Option<u32> {
    read_u32(shred, OFFSET_OF_FEC_SET_INDEX)
}

/// The data shred header's parent offset; `None` for a coding shred.
pub fn get_parent_offset(shred: &[u8]) -> Option<u16> {
    match get_shred_type(shred).ok()? {
        ShredType::Data => read_u16(shred, OFFSET_OF_PARENT_OFFSET),
        ShredType::Code => None,
    }
}

/// The data shred header's flags (`get_flags` in `wire.rs`): [`ShredError::InvalidShredType`]
/// for a coding shred. Every bit of the byte is defined, so any value is returned.
pub fn get_flags(shred: &[u8]) -> Result<u8, ShredError> {
    match get_shred_type(shred)? {
        ShredType::Code => Err(ShredError::InvalidShredType),
        ShredType::Data => shred
            .get(OFFSET_OF_FLAGS)
            .copied()
            .ok_or(ShredError::InvalidPayloadSize(shred.len())),
    }
}

/// `get_data_size` in `wire.rs`; the caller has checked the shred is data.
fn get_data_size(shred: &[u8]) -> Result<u16, ShredError> {
    read_u16(shred, OFFSET_OF_DATA_SIZE).ok_or(ShredError::InvalidPayloadSize(shred.len()))
}

/// The entry bytes a data shred carries (`get_data` in `wire.rs`).
pub fn get_data(shred: &[u8]) -> Result<&[u8], ShredError> {
    match get_shred_variant(shred)? {
        ShredVariant::MerkleCode { .. } => Err(ShredError::InvalidShredType),
        ShredVariant::MerkleData {
            proof_size,
            resigned,
        } => ShredData::get_data(shred, proof_size, resigned, get_data_size(shred)?),
    }
}

/// The coding header: `(num_data_shreds, num_coding_shreds, position)`; `None` for a
/// data shred.
pub fn get_coding_header(shred: &[u8]) -> Option<(u16, u16, u16)> {
    match get_shred_type(shred).ok()? {
        ShredType::Code => Some((
            read_u16(shred, OFFSET_OF_NUM_DATA_SHREDS)?,
            read_u16(shred, OFFSET_OF_NUM_CODING_SHREDS)?,
            read_u16(shred, OFFSET_OF_POSITION)?,
        )),
        ShredType::Data => None,
    }
}

/// The Merkle root of the shred's erasure batch, computed from the shred and its proof
/// (`get_merkle_root` in `wire.rs`, with `ShredData::get_merkle_root` and
/// `ShredCode::get_merkle_root` in `merkle.rs`). This is what the leader signs.
pub fn get_merkle_root(shred: &[u8]) -> Option<Hash> {
    let (index, proof_size, resigned, size_of_headers, size_of_payload) =
        match get_shred_variant(shred).ok()? {
            ShredVariant::MerkleCode {
                proof_size,
                resigned,
            } => {
                // num_data_shreds + position
                let num_data_shreds = usize::from(read_u16(shred, OFFSET_OF_NUM_DATA_SHREDS)?);
                let position = usize::from(read_u16(shred, OFFSET_OF_POSITION)?);
                (
                    num_data_shreds.checked_add(position)?,
                    proof_size,
                    resigned,
                    SIZE_OF_CODING_SHRED_HEADERS,
                    CODE_SHRED_PAYLOAD_SIZE,
                )
            }
            ShredVariant::MerkleData {
                proof_size,
                resigned,
            } => {
                // index - fec_set_index
                let fec_set_index = get_fec_set_index(shred)?;
                let index = get_index(shred)?.checked_sub(fec_set_index)?;
                (
                    usize::try_from(index).ok()?,
                    proof_size,
                    resigned,
                    SIZE_OF_DATA_SHRED_HEADERS,
                    DATA_SHRED_PAYLOAD_SIZE,
                )
            }
        };
    let proof_offset =
        super::get_proof_offset(size_of_payload, size_of_headers, proof_size, resigned).ok()?;
    let proof = merkle::get_merkle_proof(shred, proof_offset, proof_size).ok()?;
    let node = merkle::get_merkle_node(shred, SIZE_OF_SIGNATURE, proof_offset).ok()?;
    merkle::get_merkle_root(index, node, proof).ok()
}

/// The Merkle root of the previous erasure batch, which this batch chains to
/// (`get_chained_merkle_root` in `wire.rs`).
pub fn get_chained_merkle_root(shred: &[u8]) -> Option<Hash> {
    let (proof_size, resigned, size_of_headers, size_of_payload) =
        match get_shred_variant(shred).ok()? {
            ShredVariant::MerkleCode {
                proof_size,
                resigned,
            } => (
                proof_size,
                resigned,
                SIZE_OF_CODING_SHRED_HEADERS,
                CODE_SHRED_PAYLOAD_SIZE,
            ),
            ShredVariant::MerkleData {
                proof_size,
                resigned,
            } => (
                proof_size,
                resigned,
                SIZE_OF_DATA_SHRED_HEADERS,
                DATA_SHRED_PAYLOAD_SIZE,
            ),
        };
    let offset = super::get_chained_merkle_root_offset(
        size_of_payload,
        size_of_headers,
        proof_size,
        resigned,
    )
    .ok()?;
    read::<SIZE_OF_MERKLE_ROOT>(shred, offset).map(Hash::new_from_array)
}

/// Whether the variant carries a retransmitter signature (`is_retransmitter_signed_variant`
/// in `wire.rs`).
pub fn is_retransmitter_signed_variant(shred: &[u8]) -> Result<bool, ShredError> {
    Ok(match get_shred_variant(shred)? {
        ShredVariant::MerkleCode { resigned, .. } | ShredVariant::MerkleData { resigned, .. } => {
            resigned
        }
    })
}

/// The retransmitter's signature of a resigned shred (`get_retransmitter_signature` in
/// `wire.rs`): [`ShredError::InvalidShredVariant`] when the variant is not resigned.
pub fn get_retransmitter_signature(shred: &[u8]) -> Result<Signature, ShredError> {
    let offset = match get_shred_variant(shred)? {
        ShredVariant::MerkleCode {
            proof_size,
            resigned,
        } => super::get_retransmitter_signature_offset(
            CODE_SHRED_PAYLOAD_SIZE,
            SIZE_OF_CODING_SHRED_HEADERS,
            proof_size,
            resigned,
        ),
        ShredVariant::MerkleData {
            proof_size,
            resigned,
        } => super::get_retransmitter_signature_offset(
            DATA_SHRED_PAYLOAD_SIZE,
            SIZE_OF_DATA_SHRED_HEADERS,
            proof_size,
            resigned,
        ),
    }?;
    read::<SIGNATURE_BYTES>(shred, offset)
        .map(Signature::from)
        .ok_or(ShredError::InvalidPayloadSize(shred.len()))
}

/// Whether `leader` signed the Merkle root of the shred in `packet`, as Agave's
/// `sigverify_shreds::verify_shred_cpu` decides for a received packet: the shred is the
/// packet cut to its canonical size ([`get_shred`]), and the signature in its first 64
/// bytes has to verify over the root [`get_merkle_root`] computes.
///
/// `verify_shred_cpu` looks the leader up by [`get_slot`] in the leader schedule; here the
/// caller does that and passes the leader of `get_slot(packet)`. Agave keeps a cache of
/// verified `(signature, leader, root)` triples, which changes no outcome.
///
/// The decoded counterpart is [`Shred::verify`](super::Shred::verify), which also needs
/// the shred to pass the decoder's checks.
#[must_use]
pub fn verify_shred(packet: &[u8], leader: &Pubkey) -> bool {
    let Some(shred) = get_shred(packet) else {
        return false;
    };
    let Some(signature) = get_signature(shred) else {
        return false;
    };
    let Some(merkle_root) = get_merkle_root(shred) else {
        return false;
    };
    signature.verify(leader.as_ref(), merkle_root.as_ref())
}

/// `true` when the data shred's flags mark the end of an entry batch.
pub(crate) fn is_data_complete(flags: u8) -> bool {
    flags & FLAG_DATA_COMPLETE_SHRED == FLAG_DATA_COMPLETE_SHRED
}
