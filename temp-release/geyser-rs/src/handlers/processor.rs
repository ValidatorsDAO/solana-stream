use crate::utils::blocktime::{prepare_log_message, TransactionsBySlot};
use solana_stream_sdk::GeyserSubscribeUpdate;
use tokio::sync::mpsc;

pub async fn process_updates(
    mut updates_rx: mpsc::Receiver<GeyserSubscribeUpdate>,
    transactions_by_slot: TransactionsBySlot,
) {
    while let Some(update) = updates_rx.recv().await {
        handle_update(&update, &transactions_by_slot);
    }
}

fn handle_update(update: &GeyserSubscribeUpdate, transactions_by_slot: &TransactionsBySlot) {
    // TODO: Add your trade logic here. This is the main hook for every update.
    // Match on update.update_oneof and branch per event type as needed.
    prepare_log_message(update, transactions_by_slot);
}
