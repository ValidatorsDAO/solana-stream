# trade-app

PumpSwap auto-trading bot that detects new pools via Geyser gRPC and executes the full trade lifecycle automatically.

## Trade Lifecycle

Each trade follows a complete lifecycle — **one atomic task from detection to cleanup**:

```
Pool Detected → Buy → Confirm → Monitor → Sell → Confirm → (repeat if partial) → Burn Dust / Retreat Burn → Close ATA → Profit Notification
```

1. **Detect** — Geyser gRPC streams `create_pool` events from PumpSwap AMM
2. **Buy** — Send buy tx, wait for on-chain confirmation before proceeding
3. **Monitor** — Poll pool reserves via gRPC metadata (zero-latency) or RPC fallback
4. **Sell** — When target multiplier hit, send sell tx, confirm on-chain
5. **Drain** — If tokens remain (PumpSwap exact-output), re-sell automatically
6. **Close** — Burn any dust tokens, close the graduated token ATA, recover rent (~0.002 SOL)
7. **Notify** — Send final Discord notification with total profit/loss

### Position Status Flow

```
Active → Selling → Active (partial sell, tokens remain)
                 → Closed (all sold, ATA burned+closed) ✅
                 → Sold   (ATA close failed, manual cleanup needed) ⚠️
                 → Active (on-chain failure, retry)
```

## Overview

- Streams all PumpSwap AMM transactions via Geyser gRPC
- Detects `create_pool` instructions using `ultima-swap-pumpfun`
- Buy/sell use on-chain confirmation — status transitions and Discord notifications only fire after confirmed success
- Partial sells are automatically retried until the position is fully drained
- Dust tokens (< 10,000 raw or 0 lamport output) are burned and ATA closed
- Exposes an HTTP API (default port 3000) for configuration, status, logs, and profit tracking

## Quick Start

```bash
# 1. Copy and fill in your credentials
cp .env.sample .env
# Edit .env: set GRPC_ENDPOINT, X_TOKEN, SOLANA_RPC_ENDPOINT

# 2. Build
cargo build --release

# 3. Run (wallet.json is auto-generated on first start)
RUST_LOG=info cargo run --release
```

On first run, `wallet.json` is created in the working directory. Fund the displayed pubkey with SOL before starting trading.

## Configuration

### Environment variables (`.env`)

| Variable               | Required | Default                               | Description                |
|------------------------|----------|---------------------------------------|----------------------------|
| `GRPC_ENDPOINT`        | Yes      | —                                     | Geyser gRPC endpoint URL   |
| `X_TOKEN`              | No       | —                                     | Auth token for gRPC         |
| `SOLANA_RPC_ENDPOINT`  | No       | `https://api.mainnet-beta.solana.com` | Solana JSON-RPC URL         |
| `SEND_RPC_ENDPOINT`    | No       | same as `SOLANA_RPC_ENDPOINT`         | RPC for sending txs         |
| `API_PORT`             | No       | `3000`                                | HTTP API port               |
| `CONFIG_PATH`          | No       | `config.jsonc`                        | Geyser filter config        |
| `DISCORD_WEBHOOK_URL`  | No       | —                                     | Discord webhook for alerts  |
| `REDIS_URL`            | No       | —                                     | Redis for persistent logs   |

### Trade config (via API)

| Field                   | Default    | Description                                          |
|-------------------------|------------|------------------------------------------------------|
| `buy_amount_lamports`   | `100000`   | SOL to spend per buy (lamports, 0.0001 SOL)          |
| `sell_multiplier`       | `1.1`      | Sell when value hits buy_price × this                |
| `slippage_bps`          | `500`      | Slippage tolerance in basis points (5%)              |
| `max_positions`         | `1`        | Max concurrent open positions                        |
| `min_pool_sol_lamports` | `100000`   | Minimum pool SOL to trigger a buy (lamports)         |
| `sell_timeout_secs`     | `300`      | Force exit after this many seconds, even if target not hit |
| `exit_pool_sol_lamports`| `1000000`  | If pool WSOL drops below this, retreat immediately   |

## HTTP API

All endpoints accept and return JSON. Base URL: `http://localhost:3000`

OpenAPI docs available at `/docs`.

### Config

| Method | Path          | Description                                      |
|--------|---------------|--------------------------------------------------|
| GET    | `/api/config` | Get current trade configuration                  |
| PUT    | `/api/config` | Update trade config (partial, all fields optional) |

### Trading

| Method | Path                    | Description                                      |
|--------|-------------------------|--------------------------------------------------|
| POST   | `/api/trade/start`      | Start trading (add `?mode=sell_only` to only sell) |
| POST   | `/api/trade/stop`       | Stop trading                                     |
| GET    | `/api/trade/status`     | Running state, positions, balance                |

### Logs & Profit

| Method | Path                  | Query params          | Description                     |
|--------|-----------------------|-----------------------|---------------------------------|
| GET    | `/api/logs`           | `limit`, `offset`     | Trade logs (newest first)       |
| GET    | `/api/trades/history` | `limit`, `offset`     | Trade history from Redis        |
| GET    | `/api/trades/{id}`    | —                     | Single trade by ID              |
| GET    | `/api/trades/profit`  | —                     | Buy→Sell pairs with P&L summary |

**GET /api/trades/profit response:**
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

### Wallet & gRPC

| Method | Path                 | Description                            |
|--------|----------------------|----------------------------------------|
| GET    | `/api/wallet`        | Wallet pubkey and SOL balance          |
| PUT    | `/api/watch-address` | Change AMM program address to watch    |
| POST   | `/api/grpc/start`    | Start gRPC stream                      |
| POST   | `/api/grpc/stop`     | Stop gRPC stream                       |

## Discord Notifications

When `DISCORD_WEBHOOK_URL` is set, the bot sends:

- **✅ Buy Confirmed** — pool, mint, SOL spent, tokens received, tx sig
- **🟢/🔴 Trade Complete** — final profit/loss with buy→sell totals, ATA close status
- **⚠️ Retreat Burn** — timeout or liquidity collapse triggered burn+close with recorded realized P&L
- **❌ Sell Failed** — on-chain failure with auto-retry notice
- **⚠️ Sell TX Unknown** — confirmation timeout

Example final notification:
```
🟢 Trade Complete
Pool: `AncPq3Lp5iAeos59nyX6QnLMywmQKRWUX7HasNE1VTPX`
Base Mint: `GAZo2pnrem5VmaJuuPVNc2DhkF6XC2hg8BYTaXukDL5n`
💰 +0.000248 SOL (+248.0%)
Buy: `0.000100 SOL` → Sell: `0.000348 SOL`
ATA: Closed ✅
```

## Architecture

```
main.rs
├── Geyser gRPC stream    — connects, reconnects with backoff, pipes transaction updates
├── Processor              — detects create_pool / swap events, dispatches to trade engine
├── Trade engine           — buy → confirm → sell loop → burn+close ATA → profit notification
├── API server (Axum)      — HTTP endpoints for config / control / logs / profit
├── Webhook                — Discord notifications
└── Shared state           — Arc<RwLock<AppState>> shared across all tasks
```

### Key files

| File                          | Purpose                                             |
|-------------------------------|-----------------------------------------------------|
| `src/main.rs`                 | Entry point, wires up all tasks                     |
| `src/state.rs`                | AppState, TradeConfig, Position, TradeLog types     |
| `src/engine.rs`               | handle_new_pool, check_and_sell, burn_and_close_ata |
| `src/api.rs`                  | Axum router, all HTTP handlers, OpenAPI spec        |
| `src/webhook.rs`              | Discord webhook notifications                       |
| `src/wallet.rs`               | Keypair load/generate, balance fetch                |
| `src/handlers/processor.rs`   | Geyser update dispatcher, swap detection            |
| `src/runtime/runner.rs`       | Geyser stream lifecycle with reconnect backoff      |
| `src/runtime/subscription.rs` | Builds GeyserSubscribeRequest from config           |

## PumpSwap Naming Convention

PumpSwap uses reversed naming that can be confusing:

| PumpSwap term | Actual meaning                              |
|---------------|---------------------------------------------|
| **base**      | WSOL (always)                               |
| **quote**     | Graduated token (the meme coin)             |
| **Buy**       | Spend quote (graduated) → receive base (WSOL) = **our sell** |
| **Sell**      | Spend base (WSOL) → receive quote (graduated) = **our buy**  |

## Wallet

On startup, `wallet.json` is loaded from the working directory. If it does not exist, a new keypair is generated and saved. **Keep `wallet.json` secure** — it contains your private key.

Fund the wallet with SOL before calling `POST /api/trade/start`.

## Position Restore

On startup (or `POST /api/trade/start`), the bot scans the wallet for non-zero token balances and attempts to find corresponding PumpSwap pools. Found tokens are restored as Active positions with sell monitors. This allows the bot to recover after restarts without losing track of open positions.

## Notes

- All tx submissions use `skip_preflight: true` (pool may only be at confirmed commitment)
- Fee recipient index is randomly rotated (0–7) per transaction
- Trade logs are kept in memory (last 10,000 entries) and optionally persisted to Redis
- `sell_only` mode skips new pool detection and only sells existing positions
