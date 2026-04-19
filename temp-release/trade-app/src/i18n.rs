//! Notification message templates with multi-language support.
//!
//! Language is selected via the `TRADE_APP_LANG` environment variable.
//! Supported: `en` (default), `ja`, `zh`, `ru`, `vi`.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
    Zh,
    Ru,
    Vi,
}

impl Lang {
    fn parse(raw: &str) -> Self {
        let s = raw.trim().to_ascii_lowercase();
        let code = s.split(['_', '-', '.']).next().unwrap_or("");
        match code {
            "ja" | "jp" | "jpn" => Lang::Ja,
            "zh" | "cn" | "chi" | "zho" => Lang::Zh,
            "ru" | "rus" => Lang::Ru,
            "vi" | "vn" | "vie" => Lang::Vi,
            _ => Lang::En,
        }
    }
}

static LANG: OnceLock<Lang> = OnceLock::new();

/// Initialize the global language from the `TRADE_APP_LANG` env var.
/// Safe to call multiple times — only the first call takes effect.
pub fn init_from_env() {
    let lang = std::env::var("TRADE_APP_LANG")
        .map(|v| Lang::parse(&v))
        .unwrap_or(Lang::En);
    let _ = LANG.set(lang);
    log::info!("Notification language: {:?}", lang);
}

pub fn lang() -> Lang {
    *LANG.get().unwrap_or(&Lang::En)
}

// ─── Templates ───────────────────────────────────────────────────────────────

pub fn pool_qualified_log(pool: &str, base_mint: &str, sol_reserves: f64, ts: &str) -> String {
    match lang() {
        Lang::En => format!(
            "🆕 Pool qualified — Pool: {} | Base Mint: {} | SOL reserves: {:.4} | Time: {}",
            pool, base_mint, sol_reserves, ts
        ),
        Lang::Ja => format!(
            "🆕 プール検出 — プール: {} | ベースMint: {} | SOL残高: {:.4} | 時刻: {}",
            pool, base_mint, sol_reserves, ts
        ),
        Lang::Zh => format!(
            "🆕 符合条件的资金池 — 资金池: {} | 基础Mint: {} | SOL储备: {:.4} | 时间: {}",
            pool, base_mint, sol_reserves, ts
        ),
        Lang::Ru => format!(
            "🆕 Пул подходит — Пул: {} | Base Mint: {} | Резервы SOL: {:.4} | Время: {}",
            pool, base_mint, sol_reserves, ts
        ),
        Lang::Vi => format!(
            "🆕 Pool đủ điều kiện — Pool: {} | Base Mint: {} | Dự trữ SOL: {:.4} | Thời gian: {}",
            pool, base_mint, sol_reserves, ts
        ),
    }
}

pub fn pool_qualified_webhook(pool: &str, base_mint: &str, sol_reserves: f64, ts: &str) -> String {
    match lang() {
        Lang::En => format!(
            "🆕 **Pool Qualified for Trade**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             SOL Reserves: `{:.4} SOL`\n\
             Timestamp: {}",
            pool, base_mint, sol_reserves, ts
        ),
        Lang::Ja => format!(
            "🆕 **取引対象プールを検出**\n\
             プール: `{}`\n\
             ベースMint: `{}`\n\
             SOL残高: `{:.4} SOL`\n\
             時刻: {}",
            pool, base_mint, sol_reserves, ts
        ),
        Lang::Zh => format!(
            "🆕 **符合交易条件的资金池**\n\
             资金池: `{}`\n\
             基础Mint: `{}`\n\
             SOL储备: `{:.4} SOL`\n\
             时间戳: {}",
            pool, base_mint, sol_reserves, ts
        ),
        Lang::Ru => format!(
            "🆕 **Пул подходит для торговли**\n\
             Пул: `{}`\n\
             Base Mint: `{}`\n\
             Резервы SOL: `{:.4} SOL`\n\
             Время: {}",
            pool, base_mint, sol_reserves, ts
        ),
        Lang::Vi => format!(
            "🆕 **Pool đủ điều kiện giao dịch**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             Dự trữ SOL: `{:.4} SOL`\n\
             Thời gian: {}",
            pool, base_mint, sol_reserves, ts
        ),
    }
}

pub fn buy_confirmed(
    pool: &str,
    base_mint: &str,
    amount_sol: f64,
    tokens: u64,
    tx: &str,
) -> String {
    match lang() {
        Lang::En => format!(
            "✅ **Buy Confirmed**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             Amount: `{:.4} SOL`\n\
             Tokens: `{}`\n\
             Tx: <https://solscan.io/tx/{}>",
            pool, base_mint, amount_sol, tokens, tx
        ),
        Lang::Ja => format!(
            "✅ **購入完了**\n\
             プール: `{}`\n\
             ベースMint: `{}`\n\
             金額: `{:.4} SOL`\n\
             トークン数: `{}`\n\
             Tx: <https://solscan.io/tx/{}>",
            pool, base_mint, amount_sol, tokens, tx
        ),
        Lang::Zh => format!(
            "✅ **买入已确认**\n\
             资金池: `{}`\n\
             基础Mint: `{}`\n\
             金额: `{:.4} SOL`\n\
             代币数: `{}`\n\
             交易: <https://solscan.io/tx/{}>",
            pool, base_mint, amount_sol, tokens, tx
        ),
        Lang::Ru => format!(
            "✅ **Покупка подтверждена**\n\
             Пул: `{}`\n\
             Base Mint: `{}`\n\
             Сумма: `{:.4} SOL`\n\
             Токены: `{}`\n\
             Tx: <https://solscan.io/tx/{}>",
            pool, base_mint, amount_sol, tokens, tx
        ),
        Lang::Vi => format!(
            "✅ **Mua đã xác nhận**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             Số lượng: `{:.4} SOL`\n\
             Token: `{}`\n\
             Tx: <https://solscan.io/tx/{}>",
            pool, base_mint, amount_sol, tokens, tx
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn retreat_burn(
    pool: &str,
    base_mint: &str,
    reason: &str,
    emoji: &str,
    profit_sign: &str,
    profit_sol: f64,
    profit_pct: f64,
    buy_sol: f64,
    sell_sol: f64,
    close_ok: bool,
    burn_sig: Option<&str>,
) -> String {
    let (closed_en, closed_ja, closed_zh, closed_ru, closed_vi) = if close_ok {
        ("Closed ✅", "クローズ済 ✅", "已关闭 ✅", "Закрыто ✅", "Đã đóng ✅")
    } else {
        (
            "Close failed ⚠️",
            "クローズ失敗 ⚠️",
            "关闭失败 ⚠️",
            "Ошибка закрытия ⚠️",
            "Đóng thất bại ⚠️",
        )
    };
    let burn_tx_line = |label: &str| {
        burn_sig
            .map(|s| format!("\n{}: <https://solscan.io/tx/{}>", label, s))
            .unwrap_or_default()
    };
    match lang() {
        Lang::En => format!(
            "⚠️ **Retreat Burn**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             Reason: `{}`\n\
             {} **{}{:.6} SOL ({}{:.1}%)**\n\
             Buy: `{:.6} SOL` → Realized: `{:.6} SOL`\n\
             ATA: {}{}",
            pool, base_mint, reason,
            emoji, profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, closed_en, burn_tx_line("Burn Tx")
        ),
        Lang::Ja => format!(
            "⚠️ **強制撤退（バーン）**\n\
             プール: `{}`\n\
             ベースMint: `{}`\n\
             理由: `{}`\n\
             {} **{}{:.6} SOL ({}{:.1}%)**\n\
             購入: `{:.6} SOL` → 実現: `{:.6} SOL`\n\
             ATA: {}{}",
            pool, base_mint, reason,
            emoji, profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, closed_ja, burn_tx_line("バーンTx")
        ),
        Lang::Zh => format!(
            "⚠️ **撤退销毁**\n\
             资金池: `{}`\n\
             基础Mint: `{}`\n\
             原因: `{}`\n\
             {} **{}{:.6} SOL ({}{:.1}%)**\n\
             买入: `{:.6} SOL` → 实现: `{:.6} SOL`\n\
             ATA: {}{}",
            pool, base_mint, reason,
            emoji, profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, closed_zh, burn_tx_line("销毁交易")
        ),
        Lang::Ru => format!(
            "⚠️ **Экстренный выход (burn)**\n\
             Пул: `{}`\n\
             Base Mint: `{}`\n\
             Причина: `{}`\n\
             {} **{}{:.6} SOL ({}{:.1}%)**\n\
             Покупка: `{:.6} SOL` → Реализовано: `{:.6} SOL`\n\
             ATA: {}{}",
            pool, base_mint, reason,
            emoji, profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, closed_ru, burn_tx_line("Burn Tx")
        ),
        Lang::Vi => format!(
            "⚠️ **Thoát khẩn cấp (burn)**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             Lý do: `{}`\n\
             {} **{}{:.6} SOL ({}{:.1}%)**\n\
             Mua: `{:.6} SOL` → Đã nhận: `{:.6} SOL`\n\
             ATA: {}{}",
            pool, base_mint, reason,
            emoji, profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, closed_vi, burn_tx_line("Burn Tx")
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn trade_complete(
    emoji: &str,
    pool: &str,
    base_mint: &str,
    profit_sign: &str,
    profit_sol: f64,
    profit_pct: f64,
    buy_sol: f64,
    sell_sol: f64,
    sell_sig: &str,
    close_ok: bool,
) -> String {
    let (closed_en, closed_ja, closed_zh, closed_ru, closed_vi) = if close_ok {
        ("Closed ✅", "クローズ済 ✅", "已关闭 ✅", "Закрыто ✅", "Đã đóng ✅")
    } else {
        (
            "Close failed ⚠️",
            "クローズ失敗 ⚠️",
            "关闭失败 ⚠️",
            "Ошибка закрытия ⚠️",
            "Đóng thất bại ⚠️",
        )
    };
    match lang() {
        Lang::En => format!(
            "{} **Trade Complete**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             💰 **{}{:.6} SOL ({}{:.1}%)**\n\
             Buy: `{:.6} SOL` → Sell: `{:.6} SOL`\n\
             Sell Tx: <https://solscan.io/tx/{}>\n\
             ATA: {}",
            emoji, pool, base_mint,
            profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, sell_sig, closed_en
        ),
        Lang::Ja => format!(
            "{} **取引完了**\n\
             プール: `{}`\n\
             ベースMint: `{}`\n\
             💰 **{}{:.6} SOL ({}{:.1}%)**\n\
             購入: `{:.6} SOL` → 売却: `{:.6} SOL`\n\
             売却Tx: <https://solscan.io/tx/{}>\n\
             ATA: {}",
            emoji, pool, base_mint,
            profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, sell_sig, closed_ja
        ),
        Lang::Zh => format!(
            "{} **交易完成**\n\
             资金池: `{}`\n\
             基础Mint: `{}`\n\
             💰 **{}{:.6} SOL ({}{:.1}%)**\n\
             买入: `{:.6} SOL` → 卖出: `{:.6} SOL`\n\
             卖出交易: <https://solscan.io/tx/{}>\n\
             ATA: {}",
            emoji, pool, base_mint,
            profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, sell_sig, closed_zh
        ),
        Lang::Ru => format!(
            "{} **Сделка завершена**\n\
             Пул: `{}`\n\
             Base Mint: `{}`\n\
             💰 **{}{:.6} SOL ({}{:.1}%)**\n\
             Покупка: `{:.6} SOL` → Продажа: `{:.6} SOL`\n\
             Tx продажи: <https://solscan.io/tx/{}>\n\
             ATA: {}",
            emoji, pool, base_mint,
            profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, sell_sig, closed_ru
        ),
        Lang::Vi => format!(
            "{} **Giao dịch hoàn tất**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             💰 **{}{:.6} SOL ({}{:.1}%)**\n\
             Mua: `{:.6} SOL` → Bán: `{:.6} SOL`\n\
             Tx bán: <https://solscan.io/tx/{}>\n\
             ATA: {}",
            emoji, pool, base_mint,
            profit_sign, profit_sol, profit_sign, profit_pct,
            buy_sol, sell_sol, sell_sig, closed_vi
        ),
    }
}

pub fn sell_failed_onchain(pool: &str, sig: &str) -> String {
    match lang() {
        Lang::En => format!(
            "❌ **Sell Failed (on-chain)**\nPool: `{}`\nTx: <https://solscan.io/tx/{}>\nPosition reset to Active for retry.",
            pool, sig
        ),
        Lang::Ja => format!(
            "❌ **売却失敗（オンチェーン）**\nプール: `{}`\nTx: <https://solscan.io/tx/{}>\nリトライのためポジションをActiveに戻しました。",
            pool, sig
        ),
        Lang::Zh => format!(
            "❌ **卖出失败（链上）**\n资金池: `{}`\n交易: <https://solscan.io/tx/{}>\n持仓已重置为Active以便重试。",
            pool, sig
        ),
        Lang::Ru => format!(
            "❌ **Ошибка продажи (on-chain)**\nПул: `{}`\nTx: <https://solscan.io/tx/{}>\nПозиция сброшена в Active для повтора.",
            pool, sig
        ),
        Lang::Vi => format!(
            "❌ **Bán thất bại (on-chain)**\nPool: `{}`\nTx: <https://solscan.io/tx/{}>\nVị thế được đặt lại Active để thử lại.",
            pool, sig
        ),
    }
}

pub fn sell_tx_unknown(pool: &str, sig: &str) -> String {
    match lang() {
        Lang::En => format!(
            "⚠️ **Sell TX Unknown (timeout)**\nPool: `{}`\nTx: <https://solscan.io/tx/{}>\nPosition reset to Active.",
            pool, sig
        ),
        Lang::Ja => format!(
            "⚠️ **売却TX状態不明（タイムアウト）**\nプール: `{}`\nTx: <https://solscan.io/tx/{}>\nポジションをActiveに戻しました。",
            pool, sig
        ),
        Lang::Zh => format!(
            "⚠️ **卖出交易状态未知（超时）**\n资金池: `{}`\n交易: <https://solscan.io/tx/{}>\n持仓已重置为Active。",
            pool, sig
        ),
        Lang::Ru => format!(
            "⚠️ **Статус TX продажи неизвестен (таймаут)**\nПул: `{}`\nTx: <https://solscan.io/tx/{}>\nПозиция сброшена в Active.",
            pool, sig
        ),
        Lang::Vi => format!(
            "⚠️ **TX bán không rõ (timeout)**\nPool: `{}`\nTx: <https://solscan.io/tx/{}>\nVị thế được đặt lại Active.",
            pool, sig
        ),
    }
}

pub fn sell_send_failed(pool: &str, err: &str) -> String {
    match lang() {
        Lang::En => format!("❌ **Sell Send Failed**\nPool: `{}`\n{}", pool, err),
        Lang::Ja => format!("❌ **売却送信失敗**\nプール: `{}`\n{}", pool, err),
        Lang::Zh => format!("❌ **卖出发送失败**\n资金池: `{}`\n{}", pool, err),
        Lang::Ru => format!("❌ **Ошибка отправки продажи**\nПул: `{}`\n{}", pool, err),
        Lang::Vi => format!("❌ **Gửi lệnh bán thất bại**\nPool: `{}`\n{}", pool, err),
    }
}

pub fn error_notify(pool: &str, base_mint: &str, err: &str) -> String {
    match lang() {
        Lang::En => format!(
            "❌ **Error**\nPool: `{}`\nBase Mint: `{}`\n```{}```",
            pool, base_mint, err
        ),
        Lang::Ja => format!(
            "❌ **エラー**\nプール: `{}`\nベースMint: `{}`\n```{}```",
            pool, base_mint, err
        ),
        Lang::Zh => format!(
            "❌ **错误**\n资金池: `{}`\n基础Mint: `{}`\n```{}```",
            pool, base_mint, err
        ),
        Lang::Ru => format!(
            "❌ **Ошибка**\nПул: `{}`\nBase Mint: `{}`\n```{}```",
            pool, base_mint, err
        ),
        Lang::Vi => format!(
            "❌ **Lỗi**\nPool: `{}`\nBase Mint: `{}`\n```{}```",
            pool, base_mint, err
        ),
    }
}

pub fn position_restored(tokens: u64, est_value_lamports: u64) -> String {
    match lang() {
        Lang::En => format!(
            "Position restored from wallet: {} tokens, est. value {} lamports",
            tokens, est_value_lamports
        ),
        Lang::Ja => format!(
            "ウォレットからポジションを復元: {} トークン、推定価値 {} lamports",
            tokens, est_value_lamports
        ),
        Lang::Zh => format!(
            "从钱包恢复持仓: {} 代币，估值 {} lamports",
            tokens, est_value_lamports
        ),
        Lang::Ru => format!(
            "Позиция восстановлена из кошелька: {} токенов, оценка {} lamports",
            tokens, est_value_lamports
        ),
        Lang::Vi => format!(
            "Khôi phục vị thế từ ví: {} token, giá trị ước tính {} lamports",
            tokens, est_value_lamports
        ),
    }
}

pub fn trading_started(wallet: &str, balance_sol: f64, buy_sol: f64, sell_mult: f64) -> String {
    match lang() {
        Lang::En => format!(
            "▶️ Trading started — wallet: {} | balance: {:.4} SOL | buy: {:.4} SOL | sell: {}x",
            wallet, balance_sol, buy_sol, sell_mult
        ),
        Lang::Ja => format!(
            "▶️ 取引開始 — ウォレット: {} | 残高: {:.4} SOL | 購入額: {:.4} SOL | 売却倍率: {}x",
            wallet, balance_sol, buy_sol, sell_mult
        ),
        Lang::Zh => format!(
            "▶️ 交易已启动 — 钱包: {} | 余额: {:.4} SOL | 买入: {:.4} SOL | 卖出倍数: {}x",
            wallet, balance_sol, buy_sol, sell_mult
        ),
        Lang::Ru => format!(
            "▶️ Торговля запущена — кошелёк: {} | баланс: {:.4} SOL | покупка: {:.4} SOL | продажа: {}x",
            wallet, balance_sol, buy_sol, sell_mult
        ),
        Lang::Vi => format!(
            "▶️ Bắt đầu giao dịch — ví: {} | số dư: {:.4} SOL | mua: {:.4} SOL | bán: {}x",
            wallet, balance_sol, buy_sol, sell_mult
        ),
    }
}

pub fn trading_stopped(positions: usize) -> String {
    match lang() {
        Lang::En => format!("⏹️ Trading stopped — active positions: {}", positions),
        Lang::Ja => format!("⏹️ 取引停止 — アクティブポジション数: {}", positions),
        Lang::Zh => format!("⏹️ 交易已停止 — 活跃持仓: {}", positions),
        Lang::Ru => format!("⏹️ Торговля остановлена — активных позиций: {}", positions),
        Lang::Vi => format!("⏹️ Dừng giao dịch — vị thế đang mở: {}", positions),
    }
}

pub fn auto_loop_disabled_stopped() -> String {
    match lang() {
        Lang::En => "🏁 Single-shot cycle complete — trading stopped (set AUTO_LOOP=true to keep running)".to_string(),
        Lang::Ja => "🏁 1サイクル完了 — 取引を停止しました (連続実行は AUTO_LOOP=true で有効化)".to_string(),
        Lang::Zh => "🏁 单次循环已完成 — 交易已停止（设置 AUTO_LOOP=true 以持续运行）".to_string(),
        Lang::Ru => "🏁 Цикл завершён — торговля остановлена (для продолжения задайте AUTO_LOOP=true)".to_string(),
        Lang::Vi => "🏁 Hoàn tất một chu kỳ — đã dừng giao dịch (đặt AUTO_LOOP=true để chạy liên tục)".to_string(),
    }
}

pub fn grpc_started() -> String {
    match lang() {
        Lang::En => "📡 gRPC streaming started — pool detection & notifications active".to_string(),
        Lang::Ja => "📡 gRPCストリーミング開始 — プール検出と通知が有効です".to_string(),
        Lang::Zh => "📡 gRPC 流已启动 — 资金池检测与通知已激活".to_string(),
        Lang::Ru => "📡 gRPC-стриминг запущен — обнаружение пулов и уведомления активны".to_string(),
        Lang::Vi => "📡 Đã bật gRPC streaming — phát hiện pool và thông báo đang hoạt động".to_string(),
    }
}

pub fn grpc_stopped() -> String {
    match lang() {
        Lang::En => "⏸️ gRPC streaming stopped — pool detection paused".to_string(),
        Lang::Ja => "⏸️ gRPCストリーミング停止 — プール検出を一時停止".to_string(),
        Lang::Zh => "⏸️ gRPC 流已停止 — 资金池检测暂停".to_string(),
        Lang::Ru => "⏸️ gRPC-стриминг остановлен — обнаружение пулов приостановлено".to_string(),
        Lang::Vi => "⏸️ Đã dừng gRPC streaming — tạm dừng phát hiện pool".to_string(),
    }
}

pub fn trading_already_running() -> String {
    match lang() {
        Lang::En => "Trading already running.".to_string(),
        Lang::Ja => "取引は既に実行中です。".to_string(),
        Lang::Zh => "交易已在运行。".to_string(),
        Lang::Ru => "Торговля уже запущена.".to_string(),
        Lang::Vi => "Giao dịch đã đang chạy.".to_string(),
    }
}

pub fn insufficient_balance(
    pool: &str,
    base_mint: &str,
    wallet: &str,
    balance_sol: f64,
    needed_sol: f64,
) -> String {
    match lang() {
        Lang::En => format!(
            "💸 **Insufficient Balance**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             Wallet: `{}`\n\
             Balance: `{:.6} SOL` (need `{:.6} SOL`)\n\
             👉 Send SOL to the address below to resume trading:\n\
             `{}`\n\
             <https://solscan.io/account/{}>",
            pool, base_mint, wallet, balance_sol, needed_sol, wallet, wallet
        ),
        Lang::Ja => format!(
            "💸 **残高不足**\n\
             プール: `{}`\n\
             ベースMint: `{}`\n\
             ウォレット: `{}`\n\
             残高: `{:.6} SOL`（必要: `{:.6} SOL`）\n\
             👉 取引を再開するには以下のアドレスにSOLを送金してください:\n\
             `{}`\n\
             <https://solscan.io/account/{}>",
            pool, base_mint, wallet, balance_sol, needed_sol, wallet, wallet
        ),
        Lang::Zh => format!(
            "💸 **余额不足**\n\
             资金池: `{}`\n\
             基础Mint: `{}`\n\
             钱包: `{}`\n\
             余额: `{:.6} SOL`（需要: `{:.6} SOL`）\n\
             👉 请向以下地址转入SOL以恢复交易:\n\
             `{}`\n\
             <https://solscan.io/account/{}>",
            pool, base_mint, wallet, balance_sol, needed_sol, wallet, wallet
        ),
        Lang::Ru => format!(
            "💸 **Недостаточно средств**\n\
             Пул: `{}`\n\
             Base Mint: `{}`\n\
             Кошелёк: `{}`\n\
             Баланс: `{:.6} SOL` (нужно `{:.6} SOL`)\n\
             👉 Отправьте SOL на адрес ниже, чтобы возобновить торговлю:\n\
             `{}`\n\
             <https://solscan.io/account/{}>",
            pool, base_mint, wallet, balance_sol, needed_sol, wallet, wallet
        ),
        Lang::Vi => format!(
            "💸 **Số dư không đủ**\n\
             Pool: `{}`\n\
             Base Mint: `{}`\n\
             Ví: `{}`\n\
             Số dư: `{:.6} SOL` (cần `{:.6} SOL`)\n\
             👉 Gửi SOL đến địa chỉ dưới đây để tiếp tục giao dịch:\n\
             `{}`\n\
             <https://solscan.io/account/{}>",
            pool, base_mint, wallet, balance_sol, needed_sol, wallet, wallet
        ),
    }
}
