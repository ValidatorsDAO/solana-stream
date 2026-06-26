use solana_stream_sdk::{
    GeyserCompressedAccountFilterSet, GeyserCuckooHashAlgorithm, GeyserSubscribeDeshredRequest,
    GeyserSubscribeRequestFilterDeshredTransactions, GeyserTokenAccountExpansionControlFlag,
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
