use solana_entry::block_component::{VersionedBlockMarker, VersionedUpdateParent};
use solana_hash::Hash;
use solana_keypair::Keypair;
use solana_message::{v0, v1, Message, MessageHeader, VersionedMessage};
use solana_signer::Signer;
use solana_stream_sdk::{
    classify_block_marker, decode_block_component, BlockComponentPayload, BlockMarkerKind, Entry,
    VersionedTransaction,
};

fn mixed_entries() -> Vec<Entry> {
    // A deterministic test-only key; these fixtures never submit transactions.
    let signer = Keypair::new_from_array([7; 32]);
    let header = MessageHeader {
        num_required_signatures: 1,
        num_readonly_signed_accounts: 0,
        num_readonly_unsigned_accounts: 0,
    };
    let messages = [
        VersionedMessage::Legacy(Message::new_with_blockhash(
            &[],
            Some(&signer.pubkey()),
            &Hash::default(),
        )),
        VersionedMessage::V0(v0::Message {
            header,
            account_keys: vec![signer.pubkey()],
            recent_blockhash: Hash::default(),
            instructions: vec![],
            address_table_lookups: vec![],
        }),
        VersionedMessage::V1(v1::Message {
            header,
            config: v1::TransactionConfig {
                priority_fee: Some(12345),
                compute_unit_limit: Some(200_000),
                loaded_accounts_data_size_limit: Some(64_000),
                heap_size: Some(32_768),
            },
            lifetime_specifier: Hash::default(),
            account_keys: vec![signer.pubkey()],
            instructions: vec![],
        }),
    ];
    vec![Entry {
        num_hashes: 1,
        hash: Hash::default(),
        transactions: messages
            .into_iter()
            .map(|message| VersionedTransaction::try_new(message, &[&signer]).unwrap())
            .collect(),
    }]
}

/// A deshredded marker payload: zero entry count, `VersionedBlockMarker` V1 tag,
/// variant byte and the length-prefixed body.
fn marker_payload(variant: u8, inner: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 8];
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.push(variant);
    bytes.extend_from_slice(&(inner.len() as u16).to_le_bytes());
    bytes.extend_from_slice(inner);
    bytes
}

fn footer_body() -> Vec<u8> {
    let mut footer = vec![1u8];
    footer.extend_from_slice(&[0x33; 32]);
    footer.extend_from_slice(&1_700_000_000_000_000_000u64.to_le_bytes());
    footer.push(5);
    footer.extend_from_slice(b"agave");
    footer.extend_from_slice(&[0, 0, 0]);
    footer
}

fn header_body() -> Vec<u8> {
    let mut header = vec![1u8];
    header.extend_from_slice(&11110u64.to_le_bytes());
    header.extend_from_slice(&[0x11; 32]);
    header
}

fn update_parent_body() -> Vec<u8> {
    let mut update_parent = vec![1u8];
    update_parent.extend_from_slice(&11111u64.to_le_bytes());
    update_parent.extend_from_slice(&[0x22; 32]);
    update_parent
}

fn genesis_body() -> Vec<u8> {
    let mut genesis = Vec::new();
    genesis.extend_from_slice(&11112u64.to_le_bytes());
    genesis.extend_from_slice(&[0x44; 32]);
    genesis.extend_from_slice(&[0x55; 192]);
    genesis.extend_from_slice(&4u64.to_le_bytes());
    genesis.extend_from_slice(&[0xff; 4]);
    genesis
}

fn decode_marker(payload: &[u8]) -> (BlockMarkerKind, Option<VersionedBlockMarker>) {
    match decode_block_component(payload).unwrap() {
        BlockComponentPayload::BlockMarker { kind, marker } => (kind, marker),
        BlockComponentPayload::Entries(entries) => {
            panic!("marker payload decoded as {} entries", entries.len())
        }
    }
}

#[test]
fn block_marker_bytes_are_classified_by_kind_and_never_decoded_as_entries() {
    let cases = [
        (
            marker_payload(0, &footer_body()),
            BlockMarkerKind::BlockFooter,
        ),
        (
            marker_payload(1, &header_body()),
            BlockMarkerKind::BlockHeader,
        ),
        (
            marker_payload(2, &update_parent_body()),
            BlockMarkerKind::UpdateParent,
        ),
        (
            marker_payload(3, &genesis_body()),
            BlockMarkerKind::GenesisCertificate,
        ),
    ];
    for (payload, kind) in &cases {
        assert_eq!(classify_block_marker(payload), Some(*kind));
        let (decoded_kind, decoded) = decode_marker(payload);
        assert_eq!(decoded_kind, *kind);
        assert!(decoded.is_some(), "{kind:?} body decodes");
    }

    let (kind, marker) = decode_marker(&marker_payload(2, &update_parent_body()));
    assert_eq!(kind, BlockMarkerKind::UpdateParent);
    let marker = marker.expect("UpdateParent marker body decodes");
    assert!(marker.is_update_parent());
    let VersionedBlockMarker::V1(v1) = marker;
    let VersionedUpdateParent::V1(update) = v1.as_update_parent().expect("UpdateParent variant");
    assert_eq!(update.new_parent_slot, 11111);

    let mut version_two = marker_payload(1, &header_body());
    version_two[8..10].copy_from_slice(&2u16.to_le_bytes());
    let mut trailing = marker_payload(1, &header_body());
    trailing.push(0);
    for payload in [marker_payload(9, &header_body()), version_two, trailing] {
        assert_eq!(
            classify_block_marker(&payload),
            Some(BlockMarkerKind::Unknown)
        );
        assert_eq!(
            decode_block_component(&payload).unwrap(),
            BlockComponentPayload::BlockMarker {
                kind: BlockMarkerKind::Unknown,
                marker: None,
            }
        );
    }
}

#[test]
fn entry_batches_and_zero_runs_decode_as_entries() {
    let expected = mixed_entries();
    let wire = wincode::serialize(&expected).unwrap();
    assert_eq!(classify_block_marker(&wire), None);
    assert_eq!(
        decode_block_component(&wire).unwrap(),
        BlockComponentPayload::Entries(expected)
    );

    for zeros in [vec![0u8; 8], vec![0u8; 72]] {
        assert_eq!(classify_block_marker(&zeros), None);
        assert_eq!(
            decode_block_component(&zeros).unwrap(),
            BlockComponentPayload::Entries(Vec::new())
        );
    }
}
