use napi::bindgen_prelude::*;
use napi_derive::napi;
use solana_entry::entry::Entry;
use solana_transaction::versioned::{TransactionVersion, VersionedTransaction};

fn stringify_v1_priority_fee(transaction: &VersionedTransaction, json: &mut serde_json::Value) {
    if transaction.version() == TransactionVersion::Number(1) {
        // napi's serde-json conversion turns signed-range integers into JS
        // numbers, which lose precision above 2^53. This new v1 field is always
        // a decimal string at the Node boundary, including explicit zero.
        if let Some(fee) = json.pointer_mut("/message/1/config/priorityFee") {
            if let Some(lamports) = fee.as_u64() {
                *fee = serde_json::Value::String(lamports.to_string());
            }
        }
    }
}

#[napi]
#[allow(dead_code)]
fn decode_solana_entries(data: Buffer) -> napi::Result<serde_json::Value> {
    let entries: Vec<Entry> = wincode::deserialize(data.as_ref())
        .map_err(|e| Error::from_reason(format!("Deserialize failed: {}", e)))?;

    // Entry's modern wire schema uses wincode. Keep the existing JSON fields
    // while letting VersionedTransaction represent legacy, v0 and v1 messages.
    let json = entries
        .iter()
        .map(|entry| {
            let mut json = serde_json::json!({
                "num_hashes": entry.num_hashes,
                "hash": entry.hash,
                "transactions": entry.transactions,
            });
            if let Some(transactions) = json
                .get_mut("transactions")
                .and_then(serde_json::Value::as_array_mut)
            {
                for (transaction, json) in entry.transactions.iter().zip(transactions) {
                    stringify_v1_priority_fee(transaction, json);
                }
            }
            json
        })
        .collect::<Vec<_>>();
    Ok(serde_json::Value::Array(json))
}
