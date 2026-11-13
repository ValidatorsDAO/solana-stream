//! The SDK's shred decoder (`solana_stream_sdk::shred`) against `solana-ledger` 4.4.1,
//! the code an Agave validator runs, on the same bytes: shreds Agave's `Shredder` makes,
//! the same shreds corrupted byte by byte, and random bytes.
//!
//! For each input both sides must agree on: accept or reject (and why), every header
//! field (flags and variant byte included), `sanitize`, the Merkle root, the chained
//! Merkle root, the retransmitter signature, the leader signature check (decoded, and
//! Agave's `verify_shred_cpu` on the packet) with the right and a wrong key, every
//! packet-level reader, and `deshred`. Where solana-ledger keeps a reader private, the
//! test reads the bytes itself. Keys and inputs come from a seeded source, so counts and
//! failures repeat from run to run.
//!
//! `cargo test -p solana-stream-sdk --features udp --test shred_differential -- --nocapture`
//! prints how many inputs each test compared.

use std::{
    collections::{HashMap, HashSet},
    sync::{OnceLock, RwLock},
};

use solana_entry::entry::Entry;
use solana_hash::Hash;
use solana_keypair::Keypair;
use solana_ledger::{
    shred::{
        self as agave, layout as agave_layout, ProcessShredsStats, ReedSolomonCache, Shredder,
    },
    sigverify_shreds::{self, LruCache, SlotPubkeys},
};
use solana_perf::packet::{Packet, PacketRef};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_stream_sdk::shred::{self as sdk, wire, Shred, ShredError};

// ---------------------------------------------------------------------------------------
// What both decoders say about one input.

/// An error both sides can be compared on: its message, which the SDK keeps equal to
/// Agave's, except the two errors whose message differs by design (see `ShredError`).
fn agave_error(error: &agave::Error) -> String {
    match error {
        agave::Error::WincodeRead(_) => "TruncatedHeaders".to_string(),
        agave::Error::InvalidErasureShardIndex(_) => "InvalidErasureShardIndex".to_string(),
        error => error.to_string(),
    }
}

fn sdk_error(error: &ShredError) -> String {
    match error {
        ShredError::TruncatedHeaders(_) => "TruncatedHeaders".to_string(),
        ShredError::InvalidErasureShardIndex => "InvalidErasureShardIndex".to_string(),
        error => error.to_string(),
    }
}

/// A decoded shred, as far as either decoder exposes it.
#[derive(Debug, PartialEq)]
struct Decoded {
    slot: u64,
    index: u32,
    version: u16,
    fec_set_index: u32,
    signature: Vec<u8>,
    shred_type: u8,
    is_data: bool,
    is_code: bool,
    last_in_slot: bool,
    data_complete: bool,
    payload: Vec<u8>,
    parent: Result<u64, String>,
    data: Result<Vec<u8>, String>,
    proof_size: u8,
    /// The variant byte as the decoder re-encodes it, and its resigned bit.
    variant_byte: u8,
    resigned: bool,
    /// Data shreds only: the flags byte and its reference tick.
    flags: Option<u8>,
    reference_tick: Option<u8>,
    sanitize: Result<(), String>,
    merkle_root: Result<Hash, String>,
    chained_merkle_root: Result<Hash, String>,
    retransmitter_signature: Result<Vec<u8>, String>,
    verifies_leader: bool,
    verifies_other: bool,
}

fn agave_decoded(shred: &agave::Shred, leader: &Pubkey, other: &Pubkey) -> Decoded {
    Decoded {
        slot: shred.slot(),
        index: shred.index(),
        version: shred.version(),
        fec_set_index: shred.fec_set_index(),
        signature: shred.signature().as_ref().to_vec(),
        shred_type: u8::from(shred.shred_type()),
        is_data: shred.is_data(),
        is_code: shred.is_code(),
        last_in_slot: shred.last_in_slot(),
        data_complete: shred.data_complete(),
        payload: shred.payload().to_vec(),
        parent: shred.parent().map_err(|e| agave_error(&e)),
        data: shred
            .data()
            .map(<[u8]>::to_vec)
            .map_err(|e| agave_error(&e)),
        proof_size: shred
            .proof_size()
            .expect("a decoded shred has a proof size"),
        // solana-ledger's ShredVariant is private: the byte it decoded.
        variant_byte: shred.payload()[64],
        resigned: agave_layout::is_retransmitter_signed_variant(shred.payload()).unwrap(),
        flags: shred
            .is_data()
            .then(|| agave_layout::get_flags(shred.payload()).unwrap().bits()),
        // ShredData::reference_tick is test-only in solana-ledger: its mask on its flags.
        reference_tick: shred.is_data().then(|| {
            (agave_layout::get_flags(shred.payload()).unwrap()
                & agave::ShredFlags::SHRED_TICK_REFERENCE_MASK)
                .bits()
        }),
        sanitize: shred.sanitize().map_err(|e| agave_error(&e)),
        merkle_root: shred.merkle_root().map_err(|e| agave_error(&e)),
        chained_merkle_root: shred.chained_merkle_root().map_err(|e| agave_error(&e)),
        // `Shred::retransmitter_signature` is crate-private in solana-ledger.
        retransmitter_signature: agave_layout::get_retransmitter_signature(shred.payload())
            .map(|s| s.as_ref().to_vec())
            .map_err(|e| agave_error(&e)),
        verifies_leader: shred.verify(leader),
        verifies_other: shred.verify(other),
    }
}

fn sdk_decoded(shred: &Shred, leader: &Pubkey, other: &Pubkey) -> Decoded {
    Decoded {
        slot: shred.slot(),
        index: shred.index(),
        version: shred.version(),
        fec_set_index: shred.fec_set_index(),
        signature: shred.signature().as_ref().to_vec(),
        shred_type: shred.shred_type() as u8,
        is_data: shred.is_data(),
        is_code: shred.is_code(),
        last_in_slot: shred.last_in_slot(),
        data_complete: shred.data_complete(),
        payload: shred.payload().to_vec(),
        parent: shred.parent().map_err(|e| sdk_error(&e)),
        data: shred.data().map(<[u8]>::to_vec).map_err(|e| sdk_error(&e)),
        proof_size: shred.proof_size(),
        variant_byte: u8::from(shred.variant()),
        resigned: shred.variant().resigned(),
        flags: match shred {
            Shred::ShredData(data) => Some(data.flags()),
            Shred::ShredCode(_) => None,
        },
        reference_tick: match shred {
            Shred::ShredData(data) => Some(data.reference_tick()),
            Shred::ShredCode(_) => None,
        },
        sanitize: shred.sanitize().map_err(|e| sdk_error(&e)),
        merkle_root: shred.merkle_root().map_err(|e| sdk_error(&e)),
        chained_merkle_root: shred.chained_merkle_root().map_err(|e| sdk_error(&e)),
        retransmitter_signature: shred
            .retransmitter_signature()
            .map(|s| s.as_ref().to_vec())
            .map_err(|e| sdk_error(&e)),
        verifies_leader: shred.verify(leader),
        verifies_other: shred.verify(other),
    }
}

/// What the packet-level functions say, which run before a shred is decoded (Agave's
/// signature check is one of them).
#[derive(Debug, PartialEq)]
struct PacketView {
    shred_len: Option<usize>,
    signature: Option<Vec<u8>>,
    slot: Option<u64>,
    index: Option<u32>,
    version: Option<u16>,
    fec_set_index: Option<u32>,
    shred_type: Result<u8, String>,
    parent_offset: Option<u16>,
    flags: Result<u8, String>,
    coding_header: Option<(u16, u16, u16)>,
    merkle_root: Option<Hash>,
    chained_merkle_root: Option<Hash>,
    resigned: Result<bool, String>,
    retransmitter_signature: Result<Vec<u8>, String>,
    verifies_leader: bool,
    verifies_other: bool,
}

/// `bytes` as a received packet, for Agave's functions that take one. A packet holds at
/// most `PACKET_DATA_SIZE` bytes.
fn packet(bytes: &[u8]) -> Option<Packet> {
    let mut packet = Packet::default();
    packet
        .buffer_mut()
        .get_mut(..bytes.len())?
        .copy_from_slice(bytes);
    packet.meta_mut().size = bytes.len();
    Some(packet)
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
}

/// The chained Merkle root where solana-ledger's layout puts it: after the headers and
/// the shred type's `capacity` (its own `get_chained_merkle_root` is crate-private).
fn agave_chained_merkle_root(bytes: &[u8]) -> Option<Hash> {
    let proof_size = bytes.get(64)? & 0x0F;
    let resigned = agave_layout::is_retransmitter_signed_variant(bytes).ok()?;
    let offset = match agave_layout::get_shred_type(bytes).ok()? {
        agave::ShredType::Data => {
            agave::SIZE_OF_DATA_SHRED_HEADERS
                + agave::ShredData::capacity(proof_size, resigned).ok()?
        }
        // SIZE_OF_CODING_SHRED_HEADERS is private in solana-ledger.
        agave::ShredType::Code => 89 + agave::ShredCode::capacity(proof_size, resigned).ok()?,
    };
    let root = bytes.get(offset..offset + 32)?;
    Some(Hash::new_from_array(root.try_into().unwrap()))
}

/// Agave's `sigverify_shreds::verify_shred_cpu` with `leader` as the slot's leader. A
/// packet holds at most `PACKET_DATA_SIZE` bytes; for longer input this checks the part
/// `layout::get_shred` would keep, which is all `verify_shred_cpu` reads.
fn agave_verify_packet(bytes: &[u8], leader: &Pubkey) -> bool {
    let bytes = if bytes.len() > solana_packet::PACKET_DATA_SIZE {
        match agave_layout::get_shred_type(bytes) {
            Ok(agave::ShredType::Data) => &bytes[..sdk::DATA_SHRED_PAYLOAD_SIZE],
            Ok(agave::ShredType::Code) => &bytes[..sdk::CODE_SHRED_PAYLOAD_SIZE],
            Err(_) => return false,
        }
    } else {
        bytes
    };
    let mut packet = Packet::default();
    packet.buffer_mut()[..bytes.len()].copy_from_slice(bytes);
    packet.meta_mut().size = bytes.len();
    let mut leaders = SlotPubkeys::default();
    if let Some(slot) = agave_layout::get_slot(bytes) {
        leaders.insert(slot, *leader);
    }
    let cache = RwLock::new(LruCache::new(16));
    sigverify_shreds::verify_shred_cpu(PacketRef::from(&packet), &leaders, &cache)
}

fn agave_packet(bytes: &[u8], leader: &Pubkey, other: &Pubkey) -> PacketView {
    let shred_type = agave_layout::get_shred_type(bytes).ok();
    PacketView {
        // Agave's get_shred takes a packet; longer input is cut by type, as it would be.
        shred_len: match packet(bytes) {
            Some(packet) => agave_layout::get_shred(PacketRef::from(&packet)).map(<[u8]>::len),
            None => shred_type.map(|shred_type| match shred_type {
                agave::ShredType::Data => sdk::DATA_SHRED_PAYLOAD_SIZE,
                agave::ShredType::Code => sdk::CODE_SHRED_PAYLOAD_SIZE,
            }),
        },
        // get_signature, get_version, get_parent_offset and the coding header readers
        // are private in solana-ledger: the bytes where its layout puts them.
        signature: bytes.get(..64).map(<[u8]>::to_vec),
        slot: agave_layout::get_slot(bytes),
        index: agave_layout::get_index(bytes),
        version: le16(bytes, 77),
        fec_set_index: agave_layout::get_fec_set_index(bytes),
        shred_type: agave_layout::get_shred_type(bytes)
            .map(u8::from)
            .map_err(|e| agave_error(&e)),
        parent_offset: (shred_type == Some(agave::ShredType::Data))
            .then(|| le16(bytes, 83))
            .flatten(),
        flags: agave_layout::get_flags(bytes)
            .map(|flags| flags.bits())
            .map_err(|e| agave_error(&e)),
        coding_header: (shred_type == Some(agave::ShredType::Code))
            .then(|| Some((le16(bytes, 83)?, le16(bytes, 85)?, le16(bytes, 87)?)))
            .flatten(),
        merkle_root: agave_layout::get_merkle_root(bytes),
        chained_merkle_root: agave_chained_merkle_root(bytes),
        resigned: agave_layout::is_retransmitter_signed_variant(bytes).map_err(|e| agave_error(&e)),
        retransmitter_signature: agave_layout::get_retransmitter_signature(bytes)
            .map(|s| s.as_ref().to_vec())
            .map_err(|e| agave_error(&e)),
        verifies_leader: agave_verify_packet(bytes, leader),
        verifies_other: agave_verify_packet(bytes, other),
    }
}

fn sdk_packet(bytes: &[u8], leader: &Pubkey, other: &Pubkey) -> PacketView {
    PacketView {
        shred_len: wire::get_shred(bytes).map(<[u8]>::len),
        signature: wire::get_signature(bytes).map(|s| s.as_ref().to_vec()),
        slot: wire::get_slot(bytes),
        index: wire::get_index(bytes),
        version: wire::get_version(bytes),
        fec_set_index: wire::get_fec_set_index(bytes),
        shred_type: wire::get_shred_type(bytes)
            .map(|t| t as u8)
            .map_err(|e| sdk_error(&e)),
        parent_offset: wire::get_parent_offset(bytes),
        flags: wire::get_flags(bytes).map_err(|e| sdk_error(&e)),
        coding_header: wire::get_coding_header(bytes),
        merkle_root: wire::get_merkle_root(bytes),
        chained_merkle_root: wire::get_chained_merkle_root(bytes),
        resigned: wire::is_retransmitter_signed_variant(bytes).map_err(|e| sdk_error(&e)),
        retransmitter_signature: wire::get_retransmitter_signature(bytes)
            .map(|s| s.as_ref().to_vec())
            .map_err(|e| sdk_error(&e)),
        verifies_leader: wire::verify_shred(bytes, leader),
        verifies_other: wire::verify_shred(bytes, other),
    }
}

/// Tally of one comparison run.
#[derive(Default)]
struct Tally {
    inputs: usize,
    accepted: usize,
    rejected: usize,
    verified: usize,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.inputs += other.inputs;
        self.accepted += other.accepted;
        self.rejected += other.rejected;
        self.verified += other.verified;
    }
}

/// Runs both decoders and the packet functions on `bytes` and asserts they agree.
fn compare(bytes: &[u8], leader: &Pubkey, other: &Pubkey, what: &dyn Fn() -> String) -> Tally {
    let agave = agave::Shred::new_from_serialized_shred(bytes.to_vec());
    let ours = Shred::new_from_serialized_shred(bytes);
    let mut tally = Tally {
        inputs: 1,
        ..Tally::default()
    };
    match (&agave, &ours) {
        (Ok(agave), Ok(ours)) => {
            let decoded = agave_decoded(agave, leader, other);
            assert_eq!(decoded, sdk_decoded(ours, leader, other), "{}", what());
            // The packet readers agree with Agave's decoded shred where it has them.
            assert_eq!(
                wire::get_chained_merkle_root(bytes),
                agave.chained_merkle_root().ok(),
                "{}",
                what()
            );
            assert_eq!(wire::get_data(bytes).ok(), agave.data().ok(), "{}", what());
            if let Shred::ShredCode(code) = ours {
                let raw = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
                assert_eq!(code.num_data_shreds(), raw(83), "{}", what());
                assert_eq!(code.num_coding_shreds(), raw(85), "{}", what());
                assert_eq!(code.position(), raw(87), "{}", what());
                assert_eq!(
                    code.first_coding_index(),
                    code.index().checked_sub(u32::from(raw(87))),
                    "{}",
                    what()
                );
            }
            tally.accepted += 1;
            tally.verified += usize::from(decoded.verifies_leader);
        }
        (Err(agave), Err(ours)) => {
            assert_eq!(agave_error(agave), sdk_error(ours), "{}", what());
            tally.rejected += 1;
        }
        (agave, ours) => panic!(
            "decoders disagree on {}: agave {:?}, sdk {:?}",
            what(),
            agave.as_ref().map(|_| ()),
            ours.as_ref().map(|_| ())
        ),
    }
    assert_eq!(
        agave_packet(bytes, leader, other),
        sdk_packet(bytes, leader, other),
        "packet functions disagree on {}",
        what()
    );
    tally
}

// ---------------------------------------------------------------------------------------
// Shreds made by Agave's Shredder.

/// One `Shredder` call: an entry batch or a data slice, shredded by `leader`.
struct Batch {
    label: String,
    leader: Keypair,
    shreds: Vec<agave::Shred>,
    /// The entries, for batches made from entries.
    entries: Option<Vec<Entry>>,
}

impl Batch {
    fn data_payloads(&self) -> Vec<Vec<u8>> {
        self.shreds
            .iter()
            .filter(|shred| shred.is_data())
            .map(|shred| shred.payload().to_vec())
            .collect()
    }

    fn payloads(&self) -> Vec<Vec<u8>> {
        self.shreds.iter().map(|s| s.payload().to_vec()).collect()
    }
}

/// A deterministic pseudo-random source (SplitMix64) for the tests' choices.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }

    fn hash(&mut self) -> Hash {
        Hash::new_from_array(self.bytes(32).try_into().unwrap())
    }

    /// A key derived from this source, so that runs repeat exactly.
    fn keypair(&mut self) -> Keypair {
        Keypair::new_from_array(self.bytes(32).try_into().unwrap())
    }
}

/// Tick entries and entries with signed transactions.
fn entries(rng: &mut Rng, count: usize, with_transactions: bool) -> Vec<Entry> {
    use solana_message::Message;
    use solana_stream_sdk::VersionedTransaction;
    let signer = rng.keypair();
    (0..count)
        .map(|i| {
            let transactions = if with_transactions {
                (0..1 + i % 3)
                    .map(|_| {
                        let message = solana_message::VersionedMessage::Legacy(
                            Message::new_with_blockhash(&[], Some(&signer.pubkey()), &rng.hash()),
                        );
                        VersionedTransaction::try_new(message, &[&signer]).unwrap()
                    })
                    .collect()
            } else {
                vec![]
            };
            Entry {
                num_hashes: rng.below(1 << 20),
                hash: rng.hash(),
                transactions,
            }
        })
        .collect()
}

fn make_batches() -> Vec<Batch> {
    // Data capacity of an unsigned and a resigned data shred of a 32:32 batch, and of a
    // whole batch: the sizes where the Shredder's output changes shape.
    let capacity = agave::ShredData::capacity(sdk::PROOF_ENTRIES_FOR_32_32_BATCH, false).unwrap();
    let capacity_resigned =
        agave::ShredData::capacity(sdk::PROOF_ENTRIES_FOR_32_32_BATCH, true).unwrap();
    let batch = sdk::DATA_SHREDS_PER_FEC_BLOCK * capacity;
    let batch_resigned = sdk::DATA_SHREDS_PER_FEC_BLOCK * capacity_resigned;
    let mut rng = Rng(0x5eed_0001);
    let cache = ReedSolomonCache::default();
    let mut stats = ProcessShredsStats::default();
    let mut batches = Vec::new();
    // (slot, parent, reference tick, shred version, next data index, next code index)
    let layouts: [(u64, u64, u8, u16, u32, u32); 6] = [
        (0, 0, 0, 0, 0, 0),
        (1, 0, 1, 42, 0, 0),
        (1_000, 999, 63, 1, 32, 32),
        (400_000_000, 399_999_998, 64, u16::MAX, 64, 96),
        (
            u64::MAX,
            u64::MAX - u64::from(u16::MAX),
            255,
            7,
            1_000,
            2_000,
        ),
        (77, 3, 17, 9, 32_768 - 4 * 32, 32_768 - 4 * 32),
    ];
    let mut push = |label: String, leader: Keypair, shreds: Vec<agave::Shred>, entries| {
        batches.push(Batch {
            label,
            leader,
            shreds,
            entries,
        })
    };
    for (layout, &(slot, parent, tick, version, next_data, next_code)) in layouts.iter().enumerate()
    {
        let shredder = Shredder::new(slot, parent, tick, version).expect("valid shredder");
        let last = layout == 5;
        // Entry batches of several sizes, from none to several erasure batches.
        let counts: &[usize] = if last { &[0, 40] } else { &[0, 1, 3, 33, 400] };
        for &count in counts {
            for with_transactions in [false, true] {
                for is_last_in_slot in [false, true] {
                    let leader = rng.keypair();
                    let entries = entries(&mut rng, count, with_transactions);
                    let shreds = shredder.make_merkle_shreds_from_entries(
                        &leader,
                        &entries,
                        is_last_in_slot,
                        rng.hash(),
                        next_data,
                        next_code,
                        &cache,
                        &mut stats,
                    );
                    push(
                        format!(
                            "slot {slot} entries {count} txs {with_transactions} last {is_last_in_slot}"
                        ),
                        leader,
                        shreds,
                        Some(entries),
                    );
                }
            }
        }
        if last {
            continue;
        }
        // Data slices at the sizes where the number and kind of erasure batches change.
        let sizes = [
            1,
            capacity - 1,
            capacity,
            capacity + 1,
            batch_resigned - 1,
            batch_resigned,
            batch_resigned + 1,
            batch - 1,
            batch,
            batch + 1,
            batch + batch_resigned,
            batch + batch_resigned + 1,
        ];
        for (k, &size) in sizes.iter().enumerate() {
            // Alternate so each size is shredded both ways across the layouts.
            let is_last_in_slot = (k + layout) % 2 == 0;
            let leader = rng.keypair();
            let data = rng.bytes(size);
            let shreds = shredder
                .make_shreds_from_data_slice(
                    &leader,
                    &data,
                    is_last_in_slot,
                    rng.hash(),
                    next_data,
                    next_code,
                    &cache,
                    &mut stats,
                )
                .expect("shred a data slice");
            push(
                format!("slot {slot} data {size} bytes last {is_last_in_slot}"),
                leader,
                shreds,
                None,
            );
        }
    }
    // A retransmitter signs the resigned shreds, as Turbine does.
    let retransmitter = rng.keypair();
    for batch in &mut batches {
        for shred in &mut batch.shreds {
            if agave_layout::is_retransmitter_signed_variant(shred.payload()).unwrap() {
                let mut payload = shred.payload().to_vec();
                agave_layout::resign_shred(&mut payload, &retransmitter).unwrap();
                *shred = agave::Shred::new_from_serialized_shred(payload).unwrap();
            }
        }
    }
    batches
}

/// Representative shreds to corrupt: two of each kind (data or code, resigned or not,
/// last in slot, data complete), from different batches.
fn sample(batches: &[Batch]) -> Vec<(&Batch, &agave::Shred)> {
    let mut picked = Vec::new();
    let mut per_kind: HashMap<(bool, bool, bool, bool), usize> = HashMap::new();
    for batch in batches {
        let mut in_batch = HashSet::new();
        for shred in &batch.shreds {
            let resigned = agave_layout::is_retransmitter_signed_variant(shred.payload()).unwrap();
            let kind = (
                shred.is_data(),
                resigned,
                shred.last_in_slot(),
                shred.data_complete(),
            );
            let count = per_kind.entry(kind).or_default();
            if *count < 2 && in_batch.insert(kind) {
                *count += 1;
                picked.push((batch, shred));
            }
        }
    }
    picked
}

/// The batches are made once and shared: shredding is the slow part.
fn batches() -> &'static [Batch] {
    static BATCHES: OnceLock<Vec<Batch>> = OnceLock::new();
    BATCHES.get_or_init(make_batches)
}

// ---------------------------------------------------------------------------------------
// Tests.

#[test]
fn shredder_output_decodes_identically() {
    let batches = batches();
    let other = Rng(0x5eed_0101).keypair().pubkey();
    let mut tally = Tally::default();
    let (mut data, mut code, mut resigned, mut last) = (0, 0, 0, 0);
    for batch in batches {
        let leader = batch.leader.pubkey();
        for (i, shred) in batch.shreds.iter().enumerate() {
            let payload = shred.payload().to_vec();
            let t = compare(&payload, &leader, &other, &|| {
                format!("{} shred {i}", batch.label)
            });
            assert_eq!(
                t.accepted, 1,
                "{} shred {i}: Agave's own shred",
                batch.label
            );
            assert_eq!(
                t.verified, 1,
                "{} shred {i}: the leader's signature",
                batch.label
            );
            tally.add(t);
            // With a repair nonce or padding after the shred, as received.
            let mut padded = payload.clone();
            padded.extend_from_slice(&(i as u32).to_le_bytes());
            tally.add(compare(&padded, &leader, &other, &|| {
                format!("{} shred {i} with nonce", batch.label)
            }));
            data += usize::from(shred.is_data());
            code += usize::from(shred.is_code());
            resigned += usize::from(
                agave_layout::is_retransmitter_signed_variant(shred.payload()).unwrap(),
            );
            last += usize::from(shred.last_in_slot());
        }
    }
    println!(
        "shredder output: {} batches, {} shreds ({data} data, {code} code, {resigned} resigned, {last} last-in-slot), {} inputs compared",
        batches.len(),
        data + code,
        tally.inputs
    );
    assert!(data > 1_000 && code > 1_000 && resigned > 300 && last > 10);
}

#[test]
fn deshred_matches_shredder_deshred() {
    let batches = batches();
    let mut cases = 0usize;
    let mut ok = 0usize;
    let mut check = |payloads: &[Vec<u8>], what: &dyn Fn() -> String| {
        let agave = Shredder::deshred(payloads.iter()).map_err(|e| e.to_string());
        let ours = sdk::deshred(payloads.iter()).map_err(|e| e.to_string());
        assert_eq!(agave, ours, "{}", what());
        cases += 1;
        ok += usize::from(agave.is_ok());
    };
    let mut rng = Rng(0x5eed_0002);
    for batch in batches {
        let data = batch.data_payloads();
        let label = &batch.label;
        check(&data, &|| format!("{label}: all data shreds"));
        check(&batch.payloads(), &|| {
            format!("{label}: data and code shreds")
        });
        check(&[], &|| "no shreds".to_string());
        for k in 0..=data.len().min(40) {
            check(&data[..k], &|| format!("{label}: first {k}"));
            check(&data[data.len() - k..], &|| format!("{label}: last {k}"));
        }
        if data.len() > 1 {
            for gap in [0, 1, data.len() / 2, data.len() - 1] {
                let mut with_gap = data.clone();
                with_gap.remove(gap);
                check(&with_gap, &|| format!("{label}: without {gap}"));
                let mut duplicated = data.clone();
                duplicated.insert(gap, data[gap].clone());
                check(&duplicated, &|| format!("{label}: {gap} twice"));
            }
            let mut swapped = data.clone();
            swapped.swap(0, 1);
            check(&swapped, &|| format!("{label}: first two swapped"));
        }
        // Truncated and resized payloads, and corrupted size, flags and index fields.
        let last = data.len() - 1;
        for len in [
            0, 64, 65, 76, 77, 85, 86, 87, 88, 89, 100, 600, 1_051, 1_202,
        ] {
            let mut cut = data.clone();
            cut[last].truncate(len);
            check(&cut, &|| format!("{label}: last cut to {len}"));
        }
        for _ in 0..12 {
            let mut corrupted = data.clone();
            let i = rng.below(data.len() as u64) as usize;
            let (offset, value) = match rng.below(5) {
                0 => (86, rng.next() as u8),   // size, low byte
                1 => (87, rng.below(6) as u8), // size, high byte
                2 => (85, rng.next() as u8),   // flags
                3 => (73, rng.next() as u8),   // index
                _ => (
                    64,
                    [0x5a, 0xa5, 0x66, 0x76, 0x96, 0xb6, 0x86][rng.below(7) as usize],
                ), // variant
            };
            corrupted[i][offset] = value;
            check(&corrupted, &|| {
                format!("{label}: shred {i} byte {offset} = {value}")
            });
        }
        // Indices u32::MAX then 0 are not consecutive (Agave's checked_add); 5 then 6, with
        // the same bytes otherwise, are.
        if data.len() > 1 {
            for (first, second) in [(u32::MAX, 0u32), (5, 6)] {
                let mut pair = vec![data[0].clone(), data[1].clone()];
                pair[0][73..77].copy_from_slice(&first.to_le_bytes());
                pair[1][73..77].copy_from_slice(&second.to_le_bytes());
                pair[0][85] &= !0b1100_0000;
                pair[1][85] |= 0b0100_0000;
                check(&pair, &|| format!("{label}: indices {first} then {second}"));
            }
        }
        // The deshredded bytes decode to the entries that were shredded.
        if let Some(entries) = &batch.entries {
            let shreds: Vec<Shred> = data
                .iter()
                .map(|p| Shred::new_from_serialized_shred(p.as_slice()).unwrap())
                .collect();
            let ours = sdk::deshred(shreds.iter().map(Shred::payload)).unwrap();
            assert_eq!(
                solana_stream_sdk::decode_entries(&ours).unwrap(),
                *entries,
                "{label}"
            );
            // And the UDP receiver's own entry points, which take the SDK's shreds.
            assert_eq!(
                solana_stream_sdk::deshred_shreds_to_entries(&shreds).unwrap(),
                *entries,
                "{label}"
            );
            assert!(
                matches!(
                    solana_stream_sdk::deshred_shreds_to_component(&shreds),
                    Ok(solana_stream_sdk::BlockComponentPayload::Entries(decoded)) if decoded == *entries
                ),
                "{label}"
            );
        }
    }
    println!("deshred: {cases} shred lists compared, {ok} accepted by both");
    assert!(ok > 100 && cases - ok > 1_000);
}

#[test]
fn corrupted_bytes_are_judged_identically() {
    let batches = batches();
    let picked = sample(batches);
    let other = Rng(0x5eed_0102).keypair().pubkey();
    let mut tally = Tally::default();
    for (batch, shred) in &picked {
        let leader = batch.leader.pubkey();
        let base = shred.payload().to_vec();
        let header_len = if shred.is_data() { 88 } else { 89 };
        // Every header byte, several ways.
        for offset in 0..header_len {
            let original = base[offset];
            let mut values = vec![
                original ^ 0x01,
                original ^ 0x80,
                original ^ 0xFF,
                0x00,
                0xFF,
            ];
            values.sort_unstable();
            values.dedup();
            for value in values {
                let mut bytes = base.clone();
                bytes[offset] = value;
                tally.add(compare(&bytes, &leader, &other, &|| {
                    format!("{}: header byte {offset} = {value:#04x}", batch.label)
                }));
            }
        }
        // Every byte after the headers: data or erasure shard, chained root, proof,
        // retransmitter signature. Most break the Merkle root, some only the signature.
        for offset in header_len..base.len() {
            let mut bytes = base.clone();
            bytes[offset] ^= 0x01;
            tally.add(compare(&bytes, &leader, &other, &|| {
                format!("{}: byte {offset} flipped", batch.label)
            }));
        }
        // An all-zero signature: on the shred as made, and on shreds whose Merkle root
        // cannot be computed (a data shred's index below its FEC set index; a coding
        // shred's position past every proof), where only the root check stands between
        // the signature and acceptance.
        let mut no_root = base.clone();
        if shred.is_data() {
            let index = u32::from_le_bytes(base[73..77].try_into().unwrap());
            no_root[79..83].copy_from_slice(&(index + 1).to_le_bytes());
        } else {
            no_root[87..89].copy_from_slice(&u16::MAX.to_le_bytes());
        }
        for (which, mut bytes) in [("as made", base.clone()), ("without a root", no_root)] {
            bytes[..64].fill(0);
            tally.add(compare(&bytes, &leader, &other, &|| {
                format!("{}: all-zero signature, {which}", batch.label)
            }));
        }
        // Every value of the variant byte.
        for value in 0..=u8::MAX {
            let mut bytes = base.clone();
            bytes[64] = value;
            tally.add(compare(&bytes, &leader, &other, &|| {
                format!("{}: variant {value:#04x}", batch.label)
            }));
        }
        // Every truncation, and bytes past the payload up to a full packet and beyond.
        for len in 0..base.len() {
            tally.add(compare(&base[..len], &leader, &other, &|| {
                format!("{}: cut to {len}", batch.label)
            }));
        }
        for extra in 1..=(solana_packet::PACKET_DATA_SIZE + 16 - base.len()) {
            let mut bytes = base.clone();
            bytes.extend((0..extra).map(|i| i as u8));
            tally.add(compare(&bytes, &leader, &other, &|| {
                format!("{}: {extra} bytes appended", batch.label)
            }));
        }
    }
    println!(
        "corruptions: {} base shreds, {} inputs compared ({} accepted, {} rejected; {} verify)",
        picked.len(),
        tally.inputs,
        tally.accepted,
        tally.rejected,
        tally.verified
    );
    assert!(picked.len() >= 8, "sample covers the kinds of shreds");
    assert!(tally.accepted > 1_000 && tally.rejected > 1_000);
}

/// The fields `sanitize` checks, at and around each limit, alone and in pairs.
#[test]
fn header_fields_at_their_limits_are_judged_identically() {
    let batches = batches();
    let other = Rng(0x5eed_0103).keypair().pubkey();
    let mut tally = Tally::default();
    let set_u16 = |b: &mut Vec<u8>, o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    let set_u32 = |b: &mut Vec<u8>, o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    let set_u64 = |b: &mut Vec<u8>, o: usize, v: u64| b[o..o + 8].copy_from_slice(&v.to_le_bytes());
    let indices = [
        0u32,
        1,
        31,
        32,
        63,
        64,
        1_000,
        32_703,
        32_735,
        32_767,
        32_768,
        32_769,
        u32::MAX,
    ];
    let counts = [
        0u16,
        1,
        2,
        31,
        32,
        33,
        64,
        255,
        256,
        257,
        32_767,
        32_768,
        u16::MAX,
    ];
    let slots = [0u64, 1, 2, 64, 65_535, 65_536, u64::MAX];
    for batch in batches.iter().take(40) {
        let leader = batch.leader.pubkey();
        for shred in [
            batch.shreds.iter().find(|s| s.is_data()),
            batch.shreds.iter().find(|s| s.is_code()),
        ]
        .into_iter()
        .flatten()
        {
            let base = shred.payload().to_vec();
            let label = |what: String| format!("{}: {what}", batch.label);
            for &index in &indices {
                for &fec_set_index in &indices {
                    let mut b = base.clone();
                    set_u32(&mut b, 73, index);
                    set_u32(&mut b, 79, fec_set_index);
                    tally.add(compare(&b, &leader, &other, &|| {
                        label(format!("index {index} fec_set_index {fec_set_index}"))
                    }));
                }
            }
            for &slot in &slots {
                for parent_offset in [0u16, 1, 2, 64, u16::MAX] {
                    let mut b = base.clone();
                    set_u64(&mut b, 65, slot);
                    set_u16(&mut b, 83, parent_offset);
                    tally.add(compare(&b, &leader, &other, &|| {
                        label(format!("slot {slot} parent_offset {parent_offset}"))
                    }));
                }
            }
            if shred.is_data() {
                for flags in 0..=u8::MAX {
                    let mut b = base.clone();
                    b[85] = flags;
                    tally.add(compare(&b, &leader, &other, &|| {
                        label(format!("flags {flags:#010b}"))
                    }));
                }
                for size in (0..=1_210u16).chain([u16::MAX - 1, u16::MAX]) {
                    let mut b = base.clone();
                    set_u16(&mut b, 86, size);
                    tally.add(compare(&b, &leader, &other, &|| {
                        label(format!("size {size}"))
                    }));
                }
            } else {
                for &num_data in &counts {
                    for &num_coding in &counts {
                        let mut b = base.clone();
                        set_u16(&mut b, 83, num_data);
                        set_u16(&mut b, 85, num_coding);
                        tally.add(compare(&b, &leader, &other, &|| {
                            label(format!("num_data {num_data} num_coding {num_coding}"))
                        }));
                    }
                }
                for &position in &counts {
                    for &index in &indices {
                        let mut b = base.clone();
                        set_u16(&mut b, 87, position);
                        set_u32(&mut b, 73, index);
                        tally.add(compare(&b, &leader, &other, &|| {
                            label(format!("position {position} index {index}"))
                        }));
                    }
                }
            }
        }
    }
    println!(
        "header limits: {} inputs compared ({} accepted, {} rejected)",
        tally.inputs, tally.accepted, tally.rejected
    );
    assert!(tally.accepted > 1_000 && tally.rejected > 10_000);
}

/// A genuine signature of the same leader, moved to another erasure batch, must not
/// verify. Batch k+1 chains to batch k: its chained-root field is batch k's Merkle root
/// R_k, and every shred of batch k carries the leader's signature over R_k. A verifier
/// that also accepted a signature over the chained root, or that remembered a signature
/// as good for a leader without the root it was checked over (what the root in Agave's
/// `(signature, leader, root)` cache key prevents), would accept a batch k+1 rebuilt with
/// any content and batch k's signature. Agave and the SDK must both reject, on the
/// decoded shred and on the packet; also between unrelated batches, backwards, and from
/// data shreds into coding shreds and back.
#[test]
fn leader_signature_moved_to_another_batch_is_rejected() {
    let mut rng = Rng(0x5eed_0006);
    let leader = rng.keypair();
    let other = rng.keypair().pubkey();
    let cache = ReedSolomonCache::default();
    let mut stats = ProcessShredsStats::default();
    let capacity = agave::ShredData::capacity(sdk::PROOF_ENTRIES_FOR_32_32_BATCH, false).unwrap();
    // One call, three chained batches: two full ones and a resigned last one.
    let chained = Shredder::new(1_000, 999, 1, 7)
        .unwrap()
        .make_shreds_from_data_slice(
            &leader,
            &rng.bytes(2 * sdk::DATA_SHREDS_PER_FEC_BLOCK * capacity + 1),
            true,
            rng.hash(),
            0,
            0,
            &cache,
            &mut stats,
        )
        .unwrap();
    // The same leader in another slot.
    let unrelated = Shredder::new(2_000, 1_998, 3, 7)
        .unwrap()
        .make_shreds_from_data_slice(
            &leader,
            &rng.bytes(1_000),
            false,
            rng.hash(),
            64,
            64,
            &cache,
            &mut stats,
        )
        .unwrap();
    let batches: Vec<Vec<&agave::Shred>> = chained
        .chunk_by(|a, b| a.fec_set_index() == b.fec_set_index())
        .chain(unrelated.chunk_by(|a, b| a.fec_set_index() == b.fec_set_index()))
        .map(|batch| batch.iter().collect())
        .collect();
    assert_eq!(
        batches.len(),
        4,
        "three chained batches and an unrelated one"
    );
    let root = |batch: &[&agave::Shred]| batch[0].merkle_root().unwrap();
    for k in 0..2 {
        // The chain the transplant exploits: batch k+1 names batch k's root.
        for shred in &batches[k + 1] {
            assert_eq!(shred.chained_merkle_root().unwrap(), root(&batches[k]));
        }
    }
    assert!(batches[2]
        .iter()
        .all(|s| agave_layout::is_retransmitter_signed_variant(s.payload()).unwrap()));

    // (from, to): forwards along the chain, backwards, and between unrelated batches.
    let moves = [
        (0, 1),
        (1, 2),
        (1, 0),
        (2, 1),
        (0, 2),
        (3, 0),
        (0, 3),
        (2, 3),
    ];
    let (mut rejected, mut tally) = (0, Tally::default());
    for (from, to) in moves {
        // A batch has one root, so its data and coding shreds carry the same signature.
        let data = batches[from]
            .iter()
            .find(|s| s.is_data())
            .unwrap()
            .signature();
        let code = batches[from]
            .iter()
            .find(|s| s.is_code())
            .unwrap()
            .signature();
        assert_eq!(data, code);
        for (source, signature) in [("data", data), ("coding", code)] {
            for shred in &batches[to] {
                let leader = leader.pubkey();
                // The genuine shred verifies: only the signature makes it fail below.
                assert!(shred.verify(&leader));
                let mut bytes = shred.payload().to_vec();
                bytes[..64].copy_from_slice(signature.as_ref());
                let target = if shred.is_data() { "data" } else { "coding" };
                let what = || {
                    format!("batch {from} signature from a {source} shred into batch {to} {target} shred {}", shred.index())
                };
                let decoded_agave = agave::Shred::new_from_serialized_shred(bytes.clone())
                    .is_ok_and(|s| s.verify(&leader));
                let decoded_sdk = Shred::new_from_serialized_shred(bytes.as_slice())
                    .is_ok_and(|s| s.verify(&leader));
                assert!(!decoded_agave, "Agave's Shred::verify accepts {}", what());
                assert!(
                    !agave_verify_packet(&bytes, &leader),
                    "Agave's verify_shred_cpu accepts {}",
                    what()
                );
                assert!(!decoded_sdk, "the SDK's Shred::verify accepts {}", what());
                assert!(
                    !wire::verify_shred(&bytes, &leader),
                    "the SDK's wire::verify_shred accepts {}",
                    what()
                );
                tally.add(compare(&bytes, &leader, &other, &what));
                rejected += 1;
            }
        }
    }
    println!(
        "transplanted signatures: {rejected} shreds, all rejected by both sides ({} decoded)",
        tally.accepted
    );
    assert_eq!(
        tally.accepted, rejected,
        "the shreds still decode; only the signature fails"
    );
}

/// Every variant byte: accepted exactly when Agave accepts it, with Agave's shred type,
/// proof size and resigned bit, and `u8::from` gives the byte back.
#[test]
fn variant_byte_round_trips_as_in_agave() {
    let mut accepted = 0;
    for byte in 0..=u8::MAX {
        let mut bytes = vec![0u8; 65];
        bytes[64] = byte;
        let agave = agave_layout::get_shred_type(&bytes).map(|t| {
            (
                u8::from(t),
                byte & 0x0F,
                agave_layout::is_retransmitter_signed_variant(&bytes).unwrap(),
            )
        });
        let ours = sdk::ShredVariant::try_from(byte).map(|variant| {
            assert_eq!(u8::from(variant), byte, "{byte:#04x} round trip");
            (
                sdk::ShredType::from(variant) as u8,
                variant.proof_size(),
                variant.resigned(),
            )
        });
        assert_eq!(agave.ok(), ours.ok(), "variant byte {byte:#04x}");
        accepted += usize::from(agave_layout::get_shred_type(&bytes).is_ok());
    }
    // 0x6?, 0x7?, 0x9? and 0xb?.
    assert_eq!(accepted, 64);
}

/// A coding shred's erasure batch has to end inside the slot: its last data shred
/// (`fec_set_index + num_data_shreds - 1`) and its last coding shred
/// (`index - position + num_coding_shreds - 1`) below 32,768. Each case puts one of them
/// two below to two past the limit with every other field valid, so the limit alone
/// decides, and both sides must flip from accept to reject at the same place.
#[test]
fn erasure_batch_ends_at_the_slot_limit_as_in_agave() {
    let other = Rng(0x5eed_0104).keypair().pubkey();
    let max = sdk::MAX_DATA_SHREDS_PER_SLOT as i64;
    assert_eq!(sdk::MAX_CODE_SHREDS_PER_SLOT as i64, max);
    let mut tally = Tally::default();
    for batch in batches().iter().step_by(4) {
        let leader = batch.leader.pubkey();
        let Some(shred) = batch.shreds.iter().find(|s| s.is_code()) else {
            continue;
        };
        let base = shred.payload().to_vec();
        for count in [2u16, 3, 32, 64, 256] {
            for past in -2i64..=2 {
                // The batch's last data shred at `max - 1 + past`.
                let fec_set_index = (max - i64::from(count) + past) as u32;
                let mut last_data = base.clone();
                last_data[79..83].copy_from_slice(&fec_set_index.to_le_bytes());
                last_data[83..85].copy_from_slice(&count.to_le_bytes());
                // The batch's last coding shred at `max - 1 + past`; position 0, so the
                // shred's own index is the first coding index (below the limit here).
                let index = (max - i64::from(count) + past) as u32;
                let mut last_code = base.clone();
                last_code[73..77].copy_from_slice(&index.to_le_bytes());
                last_code[85..87].copy_from_slice(&count.to_le_bytes());
                last_code[87..89].copy_from_slice(&0u16.to_le_bytes());
                for (bytes, which) in [(last_data, "data"), (last_code, "coding")] {
                    let t = compare(&bytes, &leader, &other, &|| {
                        format!(
                            "{}: last {which} shred {past} past the limit, {count} shreds",
                            batch.label
                        )
                    });
                    assert_eq!(
                        t.accepted == 1,
                        past <= 0,
                        "{}: last {which} shred {past} past the limit",
                        batch.label
                    );
                    tally.add(t);
                }
            }
        }
    }
    println!(
        "slot limit: {} inputs compared ({} accepted, {} rejected)",
        tally.inputs, tally.accepted, tally.rejected
    );
    assert!(tally.accepted > 100 && tally.rejected > 50);
}

/// Proof sizes other than the 6 of a 32:32 batch: the variant byte sets where the
/// proof starts, and a leaf index must be below `2^proof_size`. Each case is signed over
/// the root Agave computes, so both sides must accept the signature exactly when the
/// decoders agree there is a root.
#[test]
fn every_proof_size_gives_the_same_root_and_signature_check() {
    let batches = batches();
    let other = Rng(0x5eed_0105).keypair().pubkey();
    let mut tally = Tally::default();
    let mut rng = Rng(0x5eed_0003);
    for batch in batches.iter().step_by(7) {
        let leader = batch.leader.pubkey();
        for shred in batch.shreds.iter().step_by(5) {
            for proof_size in 0..=15u8 {
                let mut bytes = shred.payload().to_vec();
                bytes[64] = (bytes[64] & 0xF0) | proof_size;
                if shred.is_data() {
                    // A size that fits every proof size.
                    let size = 88 + rng.below(400) as u16;
                    bytes[86..88].copy_from_slice(&size.to_le_bytes());
                }
                // Sign the root Agave's decoder computes, if it computes one.
                if let Ok(root) = agave::Shred::new_from_serialized_shred(bytes.clone())
                    .and_then(|s| s.merkle_root())
                {
                    let signature = batch.leader.sign_message(root.as_ref());
                    bytes[..64].copy_from_slice(signature.as_ref());
                }
                tally.add(compare(&bytes, &leader, &other, &|| {
                    format!("{}: proof size {proof_size}", batch.label)
                }));
            }
        }
    }
    println!(
        "proof sizes: {} inputs compared ({} accepted, {} with the leader's signature)",
        tally.inputs, tally.accepted, tally.verified
    );
    assert!(tally.verified > 100);
}

/// Random bytes, most of them shaped like a shred: the decoder, the packet functions
/// and deshred must not panic, and must judge each input as Agave does.
#[test]
fn random_bytes_never_panic_and_are_judged_identically() {
    const ROUNDS: usize = 30_000;
    let mut rng = Rng(0x5eed_0004);
    let leader = rng.keypair();
    let other = rng.keypair().pubkey();
    let variants = [
        0x66u8, 0x76, 0x96, 0xb6, 0x60, 0x7f, 0x90, 0xbf, 0x5a, 0xa5, 0x00, 0xff,
    ];
    let lengths = [
        0usize, 1, 63, 64, 65, 72, 73, 76, 77, 82, 83, 84, 87, 88, 89, 1_202, 1_203, 1_204, 1_227,
        1_228, 1_229, 1_232, 1_280,
    ];
    let mut tally = Tally::default();
    let mut previous: Vec<Vec<u8>> = Vec::new();
    for round in 0..ROUNDS {
        let len = if rng.below(3) == 0 {
            rng.below(1_300) as usize
        } else {
            lengths[rng.below(lengths.len() as u64) as usize]
        };
        let mut bytes = rng.bytes(len);
        if len > 88 && rng.below(4) != 0 {
            // Plausible headers: a known variant and small numbers.
            bytes[64] = variants[rng.below(variants.len() as u64) as usize];
            let fec_set_index = (rng.below(1_100) as u32) * 32;
            let index = fec_set_index + rng.below(70) as u32;
            bytes[73..77].copy_from_slice(&index.to_le_bytes());
            bytes[79..83].copy_from_slice(&fec_set_index.to_le_bytes());
            bytes[83..85].copy_from_slice(&(rng.below(40) as u16).to_le_bytes());
            bytes[85] = rng.next() as u8;
            if bytes[64] & 0xF0 >= 0x90 {
                bytes[86..88].copy_from_slice(&(88 + rng.below(1_000) as u16).to_le_bytes());
            } else {
                bytes[85..87].copy_from_slice(&(rng.below(40) as u16).to_le_bytes());
                bytes[87..89].copy_from_slice(&(rng.below(40) as u16).to_le_bytes());
            }
            if rng.below(2) == 0 {
                // Sign whatever root there is, so verification passes now and then.
                if let Some(root) = wire::get_merkle_root(&bytes) {
                    let signature = leader.sign_message(root.as_ref());
                    bytes[..64].copy_from_slice(signature.as_ref());
                }
            }
        }
        tally.add(compare(&bytes, &leader.pubkey(), &other, &|| {
            format!("round {round}: {len} bytes {:02x?}", &bytes[..len.min(96)])
        }));
        // Deshred over the last few inputs.
        previous.push(bytes);
        if previous.len() > 3 {
            previous.remove(0);
        }
        let agave = Shredder::deshred(previous.iter()).map_err(|e| e.to_string());
        let ours = sdk::deshred(previous.iter()).map_err(|e| e.to_string());
        assert_eq!(agave, ours, "round {round}: deshred");
    }
    println!(
        "random bytes: {} inputs ({} accepted, {} rejected, {} verify), no panic",
        tally.inputs, tally.accepted, tally.rejected, tally.verified
    );
    assert!(tally.accepted > 100 && tally.verified > 10);
}
