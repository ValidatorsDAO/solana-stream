use futures::StreamExt;
use napi::{
    threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
    Error, Result,
};
use napi_derive::napi;
use serde::Deserialize;
use serde_json::json;
use solana_stream_sdk::{
    CommitmentLevel as SDKCommitmentLevel, ShredstreamClient, SubscribeEntriesRequest,
    SubscribeRequestFilterAccounts, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions,
};
use std::collections::HashMap;

#[napi]
#[derive(Deserialize)]
pub enum CommitmentLevel {
    Processed,
    Confirmed,
    Finalized,
}

impl From<CommitmentLevel> for SDKCommitmentLevel {
    fn from(level: CommitmentLevel) -> Self {
        match level {
            CommitmentLevel::Processed => SDKCommitmentLevel::Processed,
            CommitmentLevel::Confirmed => SDKCommitmentLevel::Confirmed,
            CommitmentLevel::Finalized => SDKCommitmentLevel::Finalized,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SimpleFilterAccounts {
    #[serde(default)]
    pub account: Vec<String>,
    #[serde(default)]
    pub owner: Vec<String>,
    #[serde(default)]
    pub filters: Vec<String>,
    #[serde(default)]
    pub nonempty_txn_signature: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SimpleFilterTransactions {
    #[serde(default)]
    pub account_include: Vec<String>,
    #[serde(default)]
    pub account_exclude: Vec<String>,
    #[serde(default)]
    pub account_required: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SimpleFilterSlots {
    #[serde(default)]
    pub filter_by_commitment: Option<bool>,
    #[serde(default)]
    pub interslot_updates: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleEntriesRequest {
    #[serde(default)]
    pub accounts: Option<HashMap<String, SimpleFilterAccounts>>,
    #[serde(default)]
    pub transactions: Option<HashMap<String, SimpleFilterTransactions>>,
    #[serde(default)]
    pub slots: Option<HashMap<String, SimpleFilterSlots>>,
    pub commitment: Option<CommitmentLevel>,
}

#[napi]
pub struct ShredsClient {
    endpoint: String,
}

#[napi]
impl ShredsClient {
    #[napi(constructor)]
    pub fn new(endpoint: String) -> Self {
        Self { endpoint }
    }

    #[napi]
    pub fn subscribe_entries(
        &self,
        request_json: String,
        on_receive_callback: ThreadsafeFunction<String>,
    ) -> Result<()> {
        let endpoint = self.endpoint.clone();

        let request: SimpleEntriesRequest = serde_json::from_str(&request_json)
            .map_err(|e| Error::from_reason(format!("JSON parse error: {}", e)))?;

        let sdk_request = SubscribeEntriesRequest {
            accounts: request
                .accounts
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        SubscribeRequestFilterAccounts {
                            account: value.account,
                            owner: value.owner,
                            filters: vec![],
                            nonempty_txn_signature: Some(
                                value.nonempty_txn_signature.unwrap_or_default(),
                            ),
                        },
                    )
                })
                .collect(),

            transactions: request
                .transactions
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        SubscribeRequestFilterTransactions {
                            account_include: value.account_include,
                            account_exclude: value.account_exclude,
                            account_required: value.account_required,
                        },
                    )
                })
                .collect(),

            slots: request
                .slots
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        SubscribeRequestFilterSlots {
                            filter_by_commitment: value.filter_by_commitment,
                            interslot_updates: value.interslot_updates,
                        },
                    )
                })
                .collect(),

            commitment: request
                .commitment
                .map(|c| SDKCommitmentLevel::from(c) as i32),
        };

        napi::tokio::spawn(async move {
            if let Err(e) = run_stream(endpoint, sdk_request, on_receive_callback).await {
                eprintln!("Stream error: {}", e);
            }
        });

        Ok(())
    }
}

async fn run_stream(
    endpoint: String,
    request: SubscribeEntriesRequest,
    on_receive_callback: ThreadsafeFunction<String>,
) -> Result<()> {
    let mut client = ShredstreamClient::connect(&endpoint)
        .await
        .map_err(|e| Error::from_reason(format!("Connection error: {}", e)))?;

    let mut stream = client
        .subscribe_entries(request)
        .await
        .map_err(|e| Error::from_reason(format!("Subscription error: {}", e)))?;

    while let Some(slot_entry) = stream.next().await {
        match slot_entry {
            Ok(data) => {
                let json_data = json!({
                    "slot": data.slot,
                    "entries": data.entries,
                });

                on_receive_callback.call(
                    Ok(json_data.to_string()),
                    ThreadsafeFunctionCallMode::NonBlocking,
                );
            }
            Err(e) => {
                eprintln!("Stream error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
