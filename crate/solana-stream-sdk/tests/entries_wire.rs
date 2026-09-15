use solana_hash::Hash;
use solana_keypair::Keypair;
use solana_message::{v0, v1, Message, MessageHeader, VersionedMessage};
use solana_signer::Signer;
use solana_stream_sdk::{decode_entries, Entry, VersionedTransaction};

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

#[test]
fn mixed_legacy_v0_v1_preserves_transactions_and_signatures() {
    let expected = mixed_entries();
    let wire = wincode::serialize(&expected).unwrap();
    let decoded = decode_entries(&wire).unwrap();
    assert_eq!(decoded, expected);
    assert!(matches!(
        decoded[0].transactions[0].message,
        VersionedMessage::Legacy(_)
    ));
    assert!(matches!(
        decoded[0].transactions[1].message,
        VersionedMessage::V0(_)
    ));
    assert!(matches!(
        decoded[0].transactions[2].message,
        VersionedMessage::V1(_)
    ));
    for transaction in &decoded[0].transactions {
        transaction.verify_and_hash_message().unwrap();
    }
    assert_eq!(wincode::serialize(&decoded).unwrap(), wire);
}

#[test]
fn truncated_v1_signature_is_rejected() {
    let mut wire = wincode::serialize(&mixed_entries()).unwrap();
    wire.pop();
    assert!(decode_entries(&wire).is_err());
}

#[test]
fn oversized_vector_length_and_empty_input_are_rejected() {
    assert!(decode_entries(&u64::MAX.to_le_bytes()).is_err());
    assert!(decode_entries(&[]).is_err());
}

#[test]
fn ledger_zero_padding_is_accepted() {
    let expected = mixed_entries();
    let mut wire = wincode::serialize(&expected).unwrap();
    wire.extend_from_slice(&[0; 256]);
    assert_eq!(decode_entries(&wire).unwrap(), expected);
}

#[cfg(feature = "udp")]
#[test]
fn udp_reassembly_preserves_mixed_transactions_across_fec_sets() {
    use solana_ledger::shred::{ProcessShredsStats, ReedSolomonCache, Shred, Shredder};
    let keypair = Keypair::new_from_array([8; 32]);
    let expected = vec![mixed_entries().pop().unwrap(); 128];
    let shredder = Shredder::new(2, 1, 0, 42).unwrap();
    let mut stats = ProcessShredsStats::default();
    let cache = ReedSolomonCache::default();
    let shreds: Vec<_> = shredder
        .make_merkle_shreds_from_entries(
            &keypair,
            &expected,
            true,
            Hash::default(),
            0,
            0,
            &cache,
            &mut stats,
        )
        .filter(Shred::is_data)
        .collect();
    assert!(shreds.len() > 32, "fixture must cross an FEC boundary");
    assert_eq!(
        solana_stream_sdk::deshred_shreds_to_entries(&shreds).unwrap(),
        expected
    );
}
