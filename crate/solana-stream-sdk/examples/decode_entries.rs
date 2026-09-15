//! Inspect a saved raw Entry.entries payload without connecting to a provider.
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: decode_entries <entries.bin>")?;
    let entries = solana_stream_sdk::decode_entries(&fs::read(path)?)?;
    let transactions: usize = entries.iter().map(|entry| entry.transactions.len()).sum();
    println!("entries={} transactions={transactions}", entries.len());
    Ok(())
}
