//! The Merkle tree over an erasure batch, as `solana-ledger` 4.4.1
//! `src/shred/merkle_tree.rs` builds it: leaves and inner nodes are SHA-256 hashes with
//! a domain prefix, and a proof entry keeps the first 20 bytes of a sibling.

use solana_hash::Hash;
use solana_sha256_hasher::hashv;

use super::{ShredError, SIZE_OF_MERKLE_PROOF_ENTRY};

/// `MERKLE_HASH_PREFIX_LEAF` in `merkle_tree.rs`.
const MERKLE_HASH_PREFIX_LEAF: &[u8] = b"\x00SOLANA_MERKLE_SHREDS_LEAF";
/// `MERKLE_HASH_PREFIX_NODE` in `merkle_tree.rs`.
const MERKLE_HASH_PREFIX_NODE: &[u8] = b"\x01SOLANA_MERKLE_SHREDS_NODE";

/// One entry of a Merkle proof: the first 20 bytes of a sibling node
/// (`MerkleProofEntry` in `merkle_tree.rs`).
pub(crate) type MerkleProofEntry = [u8; SIZE_OF_MERKLE_PROOF_ENTRY];

/// The leaf of a shred: the hash of the bytes between the signature and the proof
/// (`get_merkle_node` in `merkle.rs`).
pub(crate) fn leaf(node: &[u8]) -> Hash {
    hashv(&[MERKLE_HASH_PREFIX_LEAF, node])
}

/// `join_nodes` in `merkle_tree.rs`: the parent of two siblings, each cut to 20 bytes.
fn join_nodes(node: &[u8], other: &[u8]) -> Hash {
    let node = &node[..SIZE_OF_MERKLE_PROOF_ENTRY];
    let other = &other[..SIZE_OF_MERKLE_PROOF_ENTRY];
    hashv(&[MERKLE_HASH_PREFIX_NODE, node, other])
}

/// `get_merkle_root` in `merkle_tree.rs`: folds the proof into the leaf at `index`.
/// The proof must lead to the root exactly, so `index` has to be below `2^proof.len()`.
pub(crate) fn get_merkle_root<'a, I>(index: usize, node: Hash, proof: I) -> Result<Hash, ShredError>
where
    I: IntoIterator<Item = &'a MerkleProofEntry>,
{
    let (index, root) = proof
        .into_iter()
        .fold((index, node), |(index, node), other| {
            let parent = if index % 2 == 0 {
                join_nodes(node.as_ref(), other)
            } else {
                join_nodes(other, node.as_ref())
            };
            (index >> 1, parent)
        });
    (index == 0)
        .then_some(root)
        .ok_or(ShredError::InvalidMerkleProof)
}

/// `get_merkle_proof` in `merkle.rs`: the `proof_size` entries from `proof_offset`.
pub(crate) fn get_merkle_proof(
    shred: &[u8],
    proof_offset: usize,
    proof_size: u8,
) -> Result<impl Iterator<Item = &MerkleProofEntry>, ShredError> {
    let proof_size = usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY;
    let proof = shred
        .get(proof_offset..proof_offset + proof_size)
        .ok_or(ShredError::InvalidPayloadSize(shred.len()))?;
    // The slice is a whole number of entries, so nothing is left over.
    let (entries, _) = proof.as_chunks::<SIZE_OF_MERKLE_PROOF_ENTRY>();
    Ok(entries.iter())
}

/// `get_merkle_node` in `merkle.rs`: the leaf over `shred[start..end]`.
pub(crate) fn get_merkle_node(shred: &[u8], start: usize, end: usize) -> Result<Hash, ShredError> {
    shred
        .get(start..end)
        .map(leaf)
        .ok_or(ShredError::InvalidPayloadSize(shred.len()))
}
