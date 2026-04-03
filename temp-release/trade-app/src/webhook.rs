use log::{error, info};
use serde_json::json;

/// Send a notification to a Discord webhook.
/// Fire-and-forget: errors are logged but don't block the caller.
pub async fn notify_discord(webhook_url: &str, content: &str) {
    let client = reqwest::Client::new();
    let body = json!({
        "content": content,
    });

    info!(
        "Sending webhook notification: {}",
        content.replace('\n', " | ")
    );

    match client.post(webhook_url).json(&body).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                info!(
                    "Webhook notification sent: {}",
                    content.replace('\n', " | ")
                );
            } else {
                error!(
                    "Webhook returned status {}: {:?} for content: {}",
                    resp.status(),
                    resp.text().await.unwrap_or_default(),
                    content.replace('\n', " | ")
                );
            }
        }
        Err(e) => {
            error!(
                "Webhook request failed: {:?} for content: {}",
                e,
                content.replace('\n', " | ")
            );
        }
    }
}
