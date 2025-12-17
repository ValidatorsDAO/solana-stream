# Shreds-UDP-RS (Temp Release)

Minimal UDP client using `solana-stream-sdk` v1.1.0 from crates.io. Public settings live in `settings.jsonc` (jsonc comments allowed) and are embedded at build time; secrets like RPC go in environment variables.

## Usage

1) Set secrets via env:
```env
SOLANA_RPC_ENDPOINT=https://api.mainnet-beta.solana.com
```

2) Run (settings are in `settings.jsonc`):
```bash
RUST_LOG=info cargo run
```

Notes:
- UDP shreds are processed directly; RPC commitment (processed/confirmed/finalized) is not used. Failed transactions may be shown.
- When amounts/kind cannot be parsed, `❓` is shown. PRs to improve extraction are welcome.
- Log legend: prefix `🎯` (program hit) / `🐣` (authority hit); action `🐣` create, `🟢` buy, `🔻` sell, `🪙` other, `❓` unknown.

## Misc

- Ensure the receive port is reachable (NAT/firewall).
- Payload decoding follows the SDK’s Shredstream Entry path; adjust `src/main.rs` if your payload format differs.
