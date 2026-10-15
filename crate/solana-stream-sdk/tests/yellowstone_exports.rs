use solana_stream_sdk::{
    GeyserCompressedAccountFilterSet, GeyserCuckooHashAlgorithm, GeyserMessage,
    GeyserSubscribeDeshredRequest, GeyserSubscribeRequest, GeyserSubscribeRequestFilterBlockFooter,
    GeyserSubscribeRequestFilterDeshredTransactions, GeyserSubscribeUpdate,
    GeyserTokenAccountExpansionControlFlag, GeyserUpdateBlockFooter, GeyserUpdateEntryUpdateParent,
    GeyserUpdateOneof,
};

#[test]
fn exposes_yellowstone_13_proto_helpers() {
    let filter = GeyserCompressedAccountFilterSet::with_capacity(1).expect("create filter");
    let account_filter = filter.to_account_filter();
    assert!(account_filter.cuckoo_accounts_filter.is_some());

    let request = GeyserSubscribeDeshredRequest {
        deshred_transactions: [(
            "all".to_string(),
            GeyserSubscribeRequestFilterDeshredTransactions {
                vote: Some(false),
                account_include: Vec::new(),
                account_exclude: Vec::new(),
                account_required: Vec::new(),
                include_update_parent: None,
            },
        )]
        .into(),
        ping: None,
        slots: Default::default(),
    };
    assert_eq!(request.deshred_transactions.len(), 1);
    assert_eq!(GeyserCuckooHashAlgorithm::SipHash as i32, 0);
    assert_eq!(GeyserTokenAccountExpansionControlFlag::All as i32, 0);
}

#[test]
fn preserves_v1_config_presence_through_protobuf() {
    use solana_stream_sdk::yellowstone_grpc_proto::{
        prelude::Message as SolanaMessage, prost::Message as _,
        solana::storage::confirmed_block::TransactionConfig,
    };
    // Even an empty config distinguishes a v1 message from v0.
    let message = SolanaMessage {
        config: Some(TransactionConfig::default()),
        ..Default::default()
    };
    let decoded = SolanaMessage::decode(message.encode_to_vec().as_slice()).unwrap();
    assert!(decoded.config.is_some());
}

#[test]
fn exposes_yellowstone_alpenglow_updates() {
    let footer = GeyserSubscribeUpdate {
        filters: vec!["footer".to_string()],
        update_oneof: Some(GeyserUpdateOneof::BlockFooter(GeyserUpdateBlockFooter {
            slot: 1_001,
            bank_id: 7,
            bank_hash: vec![0xab; 32],
            ..Default::default()
        })),
        created_at: None,
    };
    let decoded = GeyserSubscribeUpdate::decode(footer.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, footer);
    let Some(GeyserUpdateOneof::BlockFooter(decoded_footer)) = decoded.update_oneof else {
        panic!("expected a BlockFooter update");
    };
    assert_eq!(
        (
            decoded_footer.slot,
            decoded_footer.bank_id,
            decoded_footer.bank_hash
        ),
        (1_001, 7, vec![0xab; 32])
    );

    let update_parent = GeyserSubscribeUpdate {
        filters: vec!["entry".to_string()],
        update_oneof: Some(GeyserUpdateOneof::EntryUpdateParent(
            GeyserUpdateEntryUpdateParent {
                slot: 1_002,
                cleared_bank_id: 8,
                parent_slot: 1_000,
                ..Default::default()
            },
        )),
        created_at: None,
    };
    let decoded = GeyserSubscribeUpdate::decode(update_parent.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, update_parent);
    let Some(GeyserUpdateOneof::EntryUpdateParent(decoded_parent)) = decoded.update_oneof else {
        panic!("expected an EntryUpdateParent update");
    };
    assert_eq!(
        (
            decoded_parent.slot,
            decoded_parent.cleared_bank_id,
            decoded_parent.parent_slot
        ),
        (1_002, 8, 1_000)
    );

    let request = GeyserSubscribeRequest {
        block_footer: [(
            "footer".to_string(),
            GeyserSubscribeRequestFilterBlockFooter {
                include_certificates: Some(true),
            },
        )]
        .into(),
        ..Default::default()
    };
    let decoded = GeyserSubscribeRequest::decode(request.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, request);
    assert_eq!(
        decoded.block_footer["footer"].include_certificates,
        Some(true)
    );
}
