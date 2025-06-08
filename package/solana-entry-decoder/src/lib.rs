use napi::bindgen_prelude::*;
use napi_derive::napi;
use solana_entry::entry::Entry;

#[napi]
#[allow(dead_code)]
fn decode_solana_entries(data: Buffer) -> napi::Result<serde_json::Value> {
    let entries: Vec<Entry> = bincode::deserialize(data.as_ref())
        .map_err(|e| Error::from_reason(format!("Deserialize failed: {}", e)))?;

    let json = serde_json::to_value(&entries)
        .map_err(|e| Error::from_reason(format!("JSON conversion failed: {}", e)))?;
    Ok(json)
}
