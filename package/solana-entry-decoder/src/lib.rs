use napi::bindgen_prelude::*;
use napi_derive::napi;
use solana_entry::entry::Entry;

#[napi]
#[allow(dead_code)]
fn decode_solana_entries(data: Buffer) -> napi::Result<serde_json::Value> {
    let entries: Vec<Entry> = wincode::deserialize(data.as_ref())
        .map_err(|e| Error::from_reason(format!("Deserialize failed: {}", e)))?;

    // Entry's modern wire schema uses wincode. Keep the existing JSON fields
    // while letting VersionedTransaction represent legacy, v0 and v1 messages.
    let json = entries
        .iter()
        .map(|entry| serde_json::json!({
            "num_hashes": entry.num_hashes,
            "hash": entry.hash,
            "transactions": entry.transactions,
        }))
        .collect::<Vec<_>>();
    Ok(serde_json::Value::Array(json))
}
