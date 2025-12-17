# Shreds-UDP-RS (Temp Release)

Ultra-simple Rust starter that listens for Shredstream over **UDP** and prints basic stats. No heartbeat is required—once you run it, it is ready to receive packets immediately.

- It does **not assume proto** for UDP. It logs a hex preview of each packet and *then* best-effort tries to decode it as a `shredstream::Entry` proto. Adjust the decode path to match the Jito shredstream proxy payload you receive.

## Quick Start

### 1) Configure

Create `.env` (or export env vars):

```env
SHREDS_UDP_BIND_ADDR=0.0.0.0:10001
SOLANA_RPC_ENDPOINT=https://api.mainnet-beta.solana.com
```

`SHREDS_UDP_BIND_ADDR` is the local socket to bind. Point your Shredstream sender at this `ip:port`.

### 2) Run

```bash
RUST_LOG=info cargo run
```

The client binds, logs every packet (size + hex preview), then best-effort decodes:
- If it matches `shredstream::Entry` proto, it bincode-decodes `Vec<Entry>`, counts txs, and reports latency vs block time.
- If not, it still logs raw payload info so you can inspect/adjust decoding quickly.

### Notes

- Ensure the sender can reach the bound `ip:port` (consider NAT/firewall).
- If your payload format differs from the `shredstream::Entry` proto, adjust decoding accordingly in `src/main.rs`.
