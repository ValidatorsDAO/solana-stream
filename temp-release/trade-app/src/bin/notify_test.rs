//! Send all i18n notification templates to the configured Discord webhook.
//!
//! Reads `WEBHOOK_URL` and `TRADE_APP_LANG` from `.env`. Useful for verifying
//! that Discord notifications render correctly in every language without
//! running the full trading bot.
//!
//! Usage:
//!   cargo run --bin notify-test
//!   TRADE_APP_LANG=ja cargo run --bin notify-test
//!   cargo run --bin notify-test -- all   # cycle through every language

#[path = "../i18n.rs"]
mod i18n;
#[path = "../webhook.rs"]
mod webhook;

use webhook::notify_discord;

async fn send_all(url: &str, label: &str) {
    let pool = "8xQw5GQ3YMYVrnF4Ym1WbpQJ2aK7k2Qf3hJzXvD9mN1p";
    let mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    let tx = "5Uh3K1J4bD8Zz9tC7Qp2Nm3Xa1Rf6wGe4Vc9Ls7Tk2Bo8Yj";
    let ts = chrono::Utc::now().to_rfc3339();

    let header = format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n🧪 **i18n notify-test** ({})", label);
    notify_discord(url, &header).await;

    notify_discord(url, &i18n::pool_qualified_log(pool, mint, 12.3456, &ts)).await;
    notify_discord(url, &i18n::pool_qualified_webhook(pool, mint, 12.3456, &ts)).await;
    notify_discord(url, &i18n::buy_confirmed(pool, mint, 0.0100, 1_234_567, tx)).await;
    notify_discord(
        url,
        &i18n::retreat_burn(
            pool, mint, "timeout 300s",
            "🔴", "", -0.003210, -32.1,
            0.0100, 0.006790, false,
        ),
    )
    .await;
    notify_discord(
        url,
        &i18n::trade_complete(
            "🟢", pool, mint,
            "+", 0.001234, 12.3,
            0.0100, 0.011234, true,
        ),
    )
    .await;
    notify_discord(url, &i18n::sell_failed_onchain(pool, tx)).await;
    notify_discord(url, &i18n::sell_tx_unknown(pool, tx)).await;
    notify_discord(url, &i18n::sell_send_failed(pool, "RpcError: transaction simulation failed")).await;
    notify_discord(url, &i18n::error_notify(pool, mint, "Fetch pool failed after 8 retries")).await;
    notify_discord(url, &i18n::position_restored(1_234_567, 9_876_543)).await;
    notify_discord(
        url,
        &i18n::trading_started("Hx9...AbC", 1.2345, 0.0100, 1.1),
    )
    .await;
    notify_discord(url, &i18n::trading_stopped(2)).await;
    notify_discord(url, &i18n::grpc_started()).await;
    notify_discord(url, &i18n::grpc_stopped()).await;
    notify_discord(url, &i18n::trading_already_running()).await;
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let url = std::env::var("WEBHOOK_URL")
        .map_err(|_| anyhow::anyhow!("WEBHOOK_URL is not set in environment"))?;

    let args: Vec<String> = std::env::args().collect();
    let cycle_all = args.iter().any(|a| a == "all");

    if cycle_all {
        // Override TRADE_APP_LANG per iteration — requires separate processes
        // because i18n::init_from_env stores the lang in a OnceLock. So here
        // we just print guidance and run the currently-configured lang once.
        log::warn!(
            "`all` mode: OnceLock prevents switching lang mid-process. \
             Run once per language instead, e.g.:\n  \
             for L in en ja zh ru vi; do TRADE_APP_LANG=$L cargo run --bin notify-test; done"
        );
    }

    i18n::init_from_env();
    let label = format!("{:?}", i18n::lang());
    log::info!("Sending {} notifications to webhook…", label);
    send_all(&url, &label).await;
    log::info!("Done.");
    Ok(())
}
