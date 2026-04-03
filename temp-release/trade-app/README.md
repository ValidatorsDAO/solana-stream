# trade-app

PumpSwap auto-trading bot that detects new pools via Geyser gRPC and executes buy/sell trades automatically.

## Overview

- Streams all transactions involving the PumpSwap AMM (`pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA`) via Geyser gRPC
- Detects `create_pool` instructions using `ultima-swap-pumpfun`
- Automatically buys new pools and monitors for sell targets
- Exposes an HTTP API (default port 3000) for configuration, status, and logs

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

| Variable              | Required | Default                              | Description              |
|-----------------------|----------|--------------------------------------|--------------------------|
| `GRPC_ENDPOINT`       | Yes      | —                                    | Geyser gRPC endpoint URL |
| `X_TOKEN`             | No       | —                                    | Auth token for gRPC      |
| `SOLANA_RPC_ENDPOINT` | No       | `https://api.mainnet-beta.solana.com`| Solana JSON-RPC URL      |
| `API_PORT`            | No       | `3000`                               | HTTP API port            |
| `CONFIG_PATH`         | No       | `config.jsonc`                       | Geyser filter config     |

### Trade config (via API)

| Field                 | Default         | Description                                |
|-----------------------|-----------------|--------------------------------------------|
| `buy_amount_lamports` | `100000000`     | SOL to spend per buy (0.1 SOL)             |
| `sell_multiplier`     | `1.5`           | Sell when price hits buy_price × this      |
| `slippage_bps`        | `300`           | Slippage tolerance in basis points (3%)    |
| `max_positions`       | `5`             | Max concurrent open positions              |

## HTTP API

All endpoints accept and return JSON. Base URL: `http://localhost:3000`

### Config

| Method | Path          | Description                              |
|--------|---------------|------------------------------------------|
| GET    | `/api/config` | Get current trade configuration          |
| PUT    | `/api/config` | Update trade config (partial, all fields optional) |

**PUT /api/config body:**
```json
{
  "buy_amount_lamports": 100000000,
  "sell_multiplier": 2.0,
  "slippage_bps": 500,
  "max_positions": 3
}
```

### Trading

| Method | Path                  | Description                                      |
|--------|-----------------------|--------------------------------------------------|
| POST   | `/api/trade/start`    | Start trading (checks wallet balance)            |
| POST   | `/api/trade/stop`     | Stop trading (open positions are not closed)     |
| GET    | `/api/trade/status`   | Get running state, active position count, balance|

**GET /api/trade/status response:**
```json
{
  "running": true,
  "active_positions": 2,
  "wallet_balance": 1.23
}
```

### Logs

| Method | Path        | Query params              | Description             |
|--------|-------------|---------------------------|-------------------------|
| GET    | `/api/logs` | `limit` (default 100), `offset` (default 0) | Trade logs (newest first) |

### Wallet

| Method | Path           | Description                                 |
|--------|----------------|---------------------------------------------|
| GET    | `/api/wallet`  | Get wallet pubkey and SOL balance           |

### Watch Address

| Method | Path                   | Description                             |
|--------|------------------------|-----------------------------------------|
| PUT    | `/api/watch-address`   | Change the AMM program address to watch |

**PUT /api/watch-address body:**
```json
{ "address": "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA" }
```

## Architecture

```
main.rs
├── Geyser stream          — connects to gRPC, reconnects with backoff, pipes updates
├── Processor              — receives updates, detects create_pool, spawns trade tasks
├── Trade engine           — buy on new pool, sell when multiplier target hit
├── API server (Axum)      — HTTP endpoints for config / control / logs
└── Shared state (AppState) — Arc<RwLock<AppState>> shared across all tasks
```

### Key files

| File                       | Purpose                                          |
|----------------------------|--------------------------------------------------|
| `src/main.rs`              | Entry point, wires up all tasks                  |
| `src/state.rs`             | AppState, TradeConfig, Position, TradeLog types  |
| `src/engine.rs`            | handle_new_pool, check_and_sell_positions        |
| `src/api.rs`               | Axum router and all HTTP handlers                |
| `src/wallet.rs`            | Keypair load/generate, balance fetch             |
| `src/handlers/processor.rs`| Geyser update dispatcher                        |
| `src/runtime/runner.rs`    | Geyser stream lifecycle with reconnect backoff   |
| `src/runtime/subscription.rs` | Builds GeyserSubscribeRequest from config    |
| `src/utils/blocktime.rs`   | Latency monitor                                  |
| `src/utils/config.rs`      | config.jsonc types and SDK conversions           |

## Wallet

On startup, `wallet.json` is loaded from the working directory. If it does not exist, a new keypair is generated and saved. Keep `wallet.json` secure — it contains your private key.

Fund the wallet with SOL before calling `POST /api/trade/start`. The start endpoint returns an error with the pubkey and required amount if the balance is insufficient.

## Notes

- Trade transactions are submitted via `tokio::spawn` — the Geyser stream is never blocked by RPC calls.
- Fee recipient index is randomly rotated (0–7) per transaction.
- Trade logs are kept in memory (last 10,000 entries). They reset on restart.
- `POST /api/trade/stop` does **not** close open positions — they remain monitored until their sell target is hit or the process exits.
