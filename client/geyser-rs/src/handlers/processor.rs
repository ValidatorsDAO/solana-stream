use crate::utils::blocktime::{prepare_log_message, TransactionsBySlot};
use solana_stream_sdk::{GeyserDisconnected, GeyserReconnectEvent, GeyserSubscribeUpdate};
use tokio::sync::mpsc;

pub async fn process_updates(
    mut events_rx: mpsc::Receiver<GeyserReconnectEvent>,
    transactions_by_slot: TransactionsBySlot,
) {
    while let Some(event) = events_rx.recv().await {
        match event {
            GeyserReconnectEvent::Update(update) => handle_update(&update, &transactions_by_slot),
            GeyserReconnectEvent::Disconnected(disconnected) => {
                handle_disconnect(&disconnected, &transactions_by_slot)
            }
        }
    }
}

fn handle_update(update: &GeyserSubscribeUpdate, transactions_by_slot: &TransactionsBySlot) {
    // TODO: Add your trade logic here. This is the main hook for every update.
    // Match on update.update_oneof and branch per event type as needed.
    prepare_log_message(update, transactions_by_slot);
}

fn handle_disconnect(disconnected: &GeyserDisconnected, transactions_by_slot: &TransactionsBySlot) {
    // The connection dropped while these banks were in flight: you hold part of their
    // content. Drop what you derived from them; the replay from
    // `disconnected.replay_from_slot` resends the ones at or above it whole, and every
    // update after this event comes from the new connection. A replay also resends banks
    // you already hold whole, so make your handling idempotent.
    for bank in &disconnected.partial_banks {
        transactions_by_slot.remove(&bank.slot);
    }
}
