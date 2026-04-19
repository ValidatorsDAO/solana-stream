# Trade App — PumpSwap Auto-Trading Bot

> ## ⚠️ READ THIS FIRST — Risk & Safety Notice
>
> This project is **a reference implementation** demonstrating Solana transaction read/write and Geyser gRPC streaming usage. It is provided **as-is, for educational and experimental purposes**.
>
> - **No trading happens until you fund the auto-generated `wallet.json`.** The bot is dormant on a zero-balance wallet — sending SOL to it is the explicit opt-in.
> - **Once trading starts, you can lose your entire deposit.** PumpSwap pools are highly volatile, liquidity can collapse, RPCs can fail, and on-chain conditions change faster than retries can react. Only fund what you can afford to lose.
> - **`wallet.json` holds your private key.** **Back it up immediately** after first launch and keep at least one offline copy. Anyone with this file controls the funds. We recommend [**SLV Backup**](https://slv.dev/en/doc/backup/quickstart/) for an encrypted, recoverable backup workflow.
> - **AI tooling (Claude Code, Cursor, etc.) can unintentionally delete or overwrite files**, including `wallet.json`. Always work in version control, keep external backups, and double-check before running destructive commands. **Do not rely solely on the AI's judgment to preserve your wallet.**
> - This software is licensed under Apache-2.0 — no warranty, no liability. You are responsible for any funds the bot manages.

---

Auto-trading bot for Solana PumpSwap (Pump.fun AMM). Detects new pool creation via Geyser gRPC in real-time and executes the full trade lifecycle: **buy → sell → ATA close** — fully automated.

## Features

- **Geyser gRPC** — zero-latency new pool detection
- **Full lifecycle automation**: detect → buy → take-profit or retreat → ATA close → notify
- **TX confirmation**: status transitions and notifications only fire after on-chain confirmation
- **Partial sell handling**: PumpSwap exact-output may sell partially — auto-retries until fully drained
- **Timeout retreat**: force exit after configurable timeout, even if profit target is not hit
- **Liquidity collapse detection**: instant retreat when pool WSOL falls below threshold
- **Dust cleanup**: unsellable dust tokens are burned, ATA closed, rent recovered (~0.002 SOL)
- **Discord notifications**: buy confirmed, trade complete, retreat burn — all with P&L
- **Redis persistence**: trade history stored in Redis
- **REST API + OpenAPI**: full control via API, Swagger UI at `/docs`

---

## Trade Lifecycle

```
Pool Detected → Buy → TX Confirm → Sell Monitor → Sell → TX Confirm
  ↓ (partial fill)                                       ↓
  Retry ←──────────────────────────────────────────────┘
  ↓ (fully sold or dust)
  Burn + ATA Close → Rent Recovery → Profit Notification (Discord + Redis)
  ↓ (timeout or liquidity collapse)
  Forced Retreat → Sell attempt → (unsellable) → Burn → Notification
```

### Position Status Flow

```
Active → Selling → Active   (partial sell, tokens remaining)
                 → Closed   (fully sold, ATA closed) ✅
                 → Active   (on-chain failure, auto-retry)
Active → Selling → Closed   (timeout / liquidity collapse retreat) ⚠️
Active → Closed             (dust → burn + close)
```

---

## Quick Start

```bash
# 1. Initialize from template (slv CLI)
slv bot init
# → Select "trade-app"

# 2. Or manual setup
cp .env.sample .env
# Edit .env: set GRPC_ENDPOINT, SOLANA_RPC_ENDPOINT

# 3. Build
cargo build --release

# 4. Run (wallet.json is auto-generated on first start)
./target/release/trade-app

# 5. Fund the wallet with SOL
#    Minimum: 0.013 SOL (buy 0.0001 + ATA rent 0.004 + fee reserve 0.01)

# 6. Start trading
curl -X POST http://localhost:3000/api/trade/start
```

---

## Environment Variables (`.env`)

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `GRPC_ENDPOINT` | ✅ | — | Geyser gRPC endpoint |
| `X_TOKEN` | | — | gRPC auth token |
| `SOLANA_RPC_ENDPOINT` | | `https://edge.erpc.global?api-key=<api-key>` | RPC for reads. Set explicitly, or provide `ERPC_API_KEY` to build this URL automatically. Final fallback (no env set): `https://api.mainnet-beta.solana.com` (public, rate-limited — not recommended for production). |
| `SOLANA_SEND_RPC_ENDPOINT` | | `https://edge.erpc.global?api-key=<api-key>` | RPC for sending TXs. Set explicitly, or provide `ERPC_SEND_API_KEY` to build this URL automatically. Falls back to the read RPC when unset. A dedicated send endpoint is recommended. |
| `ERPC_API_KEY` | | — | API key used to build the read RPC URL when `SOLANA_RPC_ENDPOINT` is unset. Get one at [erpc.global](https://erpc.global). |
| `ERPC_SEND_API_KEY` | | — | API key used to build the send RPC URL when `SOLANA_SEND_RPC_ENDPOINT` is unset. |
| `API_PORT` | | `3000` | HTTP API port |
| `API_TOKEN` | | — | Bearer token for API auth (all endpoints require it when set) |
| `WEBHOOK_URL` | | — | Discord Webhook URL |
| `REDIS_URL` | | — | Redis URL (e.g. `redis://127.0.0.1:6379`) |
| `CONFIG_PATH` | | `config.jsonc` | Geyser filter config file |
| `AUTO_LOOP` | | `false` | Keep buying new pools after each cycle. When `false` (default), the bot **stops automatically after one buy/sell cycle completes** so you can review the result before the next run. Accepts `true/1/yes/on` and `false/0/no/off`. |
| `TRADE_APP_LANG` | | `en` | Notification & log language. Supported: `en`, `ja`, `zh`, `ru`, `vi` (with common aliases like `jp`, `cn`, `vn`). |

> **⚠️ Never commit `.env` or `wallet.json`.** Both are in `.gitignore`.
>
> **Back up `wallet.json` to a safe location (offline / encrypted) before funding it.** Losing this file = losing the funds.

---

## Trade Configuration (`/api/config`)

`GET /api/config` to read, `PUT /api/config` to partial-update.

| Field | Default | Description |
|-------|---------|-------------|
| `buy_amount_lamports` | `100000` (0.0001 SOL) | Amount to spend per buy (lamports) |
| `sell_multiplier` | `1.1` | Take profit at buy_price × this |
| `slippage_bps` | `500` (5%) | Slippage tolerance (basis points) |
| `max_positions` | `1` | Max concurrent positions |
| `min_pool_sol_lamports` | `100000` (0.0001 SOL) | Minimum pool liquidity to trigger buy |
| `sell_timeout_secs` | `300` (5 min) | Force exit after this many seconds |
| `exit_pool_sol_lamports` | `1000000` (0.001 SOL) | Retreat if pool WSOL drops below this |
| `auto_loop` | `false` | Same as `AUTO_LOOP` env. Live-tunable via `PUT /api/config`. |

### Configuration Examples

```bash
# Conservative: 0.001 SOL buy, 2x target, 2 min timeout
curl -X PUT http://localhost:3000/api/config \
  -H 'Content-Type: application/json' \
  -d '{"buy_amount_lamports": 1000000, "sell_multiplier": 2.0, "sell_timeout_secs": 120}'

# Aggressive: 0.01 SOL, 1.05x target, 10 min timeout
curl -X PUT http://localhost:3000/api/config \
  -H 'Content-Type: application/json' \
  -d '{"buy_amount_lamports": 10000000, "sell_multiplier": 1.05, "sell_timeout_secs": 600}'
```

---

## REST API

Base URL: `http://localhost:3000` | OpenAPI docs: `http://localhost:3000/docs`

### Config

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/config` | Get current trade config |
| `PUT` | `/api/config` | Partial update trade config |

### Trading

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/trade/start` | Start trading (`?mode=sell_only` for sell-only) |
| `POST` | `/api/trade/stop` | Stop trading |
| `GET` | `/api/trade/status` | Running state, positions, balance |

### History & Profit

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/logs` | Trade logs (`?limit=100&offset=0`) |
| `GET` | `/api/trades/history` | Trade history from Redis |
| `GET` | `/api/trades/{id}` | Single trade by ID |
| `GET` | `/api/trades/profit` | Buy→Sell pair P&L summary |

### Wallet & gRPC

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/wallet` | Wallet pubkey and SOL balance |
| `PUT` | `/api/watch-address` | Change AMM program address to watch |
| `POST` | `/api/grpc/start` | Start gRPC stream |
| `POST` | `/api/grpc/stop` | Stop gRPC stream |

### Response Examples

<details>
<summary>GET /api/trade/status</summary>

```json
{
  "running": true,
  "grpc_streaming": true,
  "active_positions": 1,
  "wallet_balance": 0.025,
  "phase": "position_open"
}
```
</details>

<details>
<summary>GET /api/trades/profit</summary>

```json
{
  "pairs": [
    {
      "pool": "AncPq3...",
      "base_mint": "GAZo2p...",
      "buy_sol": 0.0001,
      "sell_sol": 0.000348,
      "profit_sol": 0.000248,
      "profit_pct": 248.0,
      "buy_tx": "4xnKac...",
      "sell_tx": "34G1dL...",
      "buy_time": "2026-04-03T20:46:00Z",
      "sell_time": "2026-04-03T21:38:32Z"
    }
  ],
  "total_profit_sol": 0.000248,
  "total_buys": 1,
  "total_sells": 1
}
```
</details>

---

## Discord Notifications

Sent when `WEBHOOK_URL` is set:

| Notification | Trigger |
|-------------|---------|
| ✅ **Buy Confirmed** | Buy TX confirmed on-chain |
| 🟢/🔴 **Trade Complete** | All sold + ATA closed (profit/loss) |
| ⚠️ **Retreat Burn** | Timeout or liquidity collapse, burned + closed |
| ❌ **Sell Failed (on-chain)** | Sell TX failed (auto-retry) |

```
🟢 Trade Complete
Pool: `AncPq3Lp5i...`
Base Mint: `GAZo2pnrem...`
💰 +0.000248 SOL (+248.0%)
Buy: `0.000100 SOL` → Sell: `0.000348 SOL`
ATA: Closed ✅
```

```
⚠️ Retreat Burn
Pool: `D5J2zWXJU9...`
Base Mint: `6DM57pN8Th...`
Reason: `timeout 300s`
🔴 -0.000100 SOL (-100.0%)
Buy: `0.000100 SOL` → Realized: `0.000000 SOL`
ATA: Closed ✅
```

---

## Architecture

```
main.rs
├── Geyser gRPC Stream      ← Real-time PumpSwap TX stream
├── Processor                ← Detect create_pool / swap events
├── Trade Engine             ← Buy → sell → burn + close lifecycle
│   ├── confirm_transaction   ← On-chain TX confirmation
│   ├── check_and_sell        ← Profit target / timeout / liquidity check
│   └── burn_and_close_ata    ← Dust burn + rent recovery
├── API Server (Axum)        ← REST API + OpenAPI
├── Webhook                  ← Discord notifications
└── Shared State             ← Arc<RwLock<AppState>>
```

### File Structure

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point |
| `src/state.rs` | AppState, TradeConfig, Position, TradeLog |
| `src/engine.rs` | handle_new_pool, check_and_sell, burn_and_close_ata |
| `src/api.rs` | Axum router, HTTP handlers, OpenAPI spec |
| `src/webhook.rs` | Discord webhook notifications |
| `src/wallet.rs` | Keypair load/generate |
| `src/handlers/processor.rs` | Geyser update processing, swap/pool detection |
| `src/runtime/runner.rs` | Geyser stream management (reconnect backoff) |
| `src/runtime/subscription.rs` | GeyserSubscribeRequest builder |
| `src/runtime/settings.rs` | Environment variable loading |
| `src/utils/config.rs` | config.jsonc parser |
| `config.jsonc` | Geyser filter configuration |

---

## PumpSwap Naming Convention

PumpSwap uses reversed naming compared to typical DeFi:

| PumpSwap Term | Actual Meaning |
|---------------|----------------|
| **base** | WSOL (always) |
| **quote** | Graduated token (meme coin) |
| **Buy** instruction | Spend quote (graduated) → receive base (WSOL) = **our sell** |
| **Sell** instruction | Spend base (WSOL) → receive quote (graduated) = **our buy** |

---

## Wallet

On first start, `wallet.json` is auto-generated as a vanity keypair whose pubkey ends with `SLV` (~0.5–1 s on a modern multi-core CPU).

- **`wallet.json` contains your private key** — keep it secure.
- **Back it up immediately** before funding. Losing this file = losing the funds. AI tools that touch the working directory may delete or overwrite it; do not rely on the AI to preserve it for you.
- **Recommended backup tool:** [SLV Backup](https://slv.dev/en/doc/backup/quickstart/) — encrypted, multi-device recovery designed for Solana key material. Run it once after the first launch, then again any time the wallet changes.
- Fund the displayed pubkey with SOL before starting.
- Minimum required: **0.013 SOL** (buy + ATA rent + fee reserve).

---

## Position Restore

On startup, the bot scans wallet token balances and finds corresponding PumpSwap pools via `getProgramAccounts`. Tokens are restored as Active positions with sell monitors. This allows recovery after restarts without losing positions.

---

## Requirements

- **Rust** (stable, 2021 edition)
- **Solana Geyser gRPC** endpoint
- **Solana RPC** endpoint (read + send)
- **Redis** (optional, for persistence)

---

## License

Apache-2.0
