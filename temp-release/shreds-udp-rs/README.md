# shreds-udp-rs

Minimal Rust client that listens for Shredstream over **UDP** and prints signal-first logs. No heartbeat required—just point your sender to the bound `ip:port`.

## Quick start

1) Edit `settings.jsonc` (jsonc comments allowed). It is embedded into the binary at build time, so no runtime `SHREDS_UDP_CONFIG` is needed.
2) Provide secrets (e.g., RPC) via env:
```env
SOLANA_RPC_ENDPOINT=https://api.mainnet-beta.solana.com
```
3) Run (pump.fun defaults, one-call):
```bash
cargo run
```
(`handle_pumpfun_watcher` keeps the pump.fun watcher/detailer wired up for a quick start.)

4) Modular pipeline (custom sinks/watchers):
```bash
GENERIC_WATCH_PROGRAM_IDS=YourProgramIdHere cargo run --bin generic_logger
```
`generic_logger` shows the layered API (5 layers: `decode_udp_datagram` → `insert_shred` → `deshred_shreds_to_entries` → `collect_watch_events` → any sink) with `SplTokenMintFinder` only. Leave `GENERIC_WATCH_*` unset to just log slots/entries without pump.fun defaults.

## Deshred decode troubleshooting
- Use `solana-stream-sdk >= 1.2.2` for Direct Shreds UDP. Agave 3.x serializes deshredded entries with `wincode`; SDK 1.2.0 tried `bincode` first in the UDP helper, and SDK 1.2.1 could still decode from the middle of a multi-FEC entry segment.
- Errors such as `entry decode failed: invalid value: integer ..., expected a valid transaction message version`, `continue signal on byte-three`, `io error: unexpected end of file`, or `alias encoding, expected strict form encoding` usually mean the deshredded entry bytes are being decoded with the wrong codec.
- UDP packet sizes around 1203/1228 bytes are normal Merkle shred sizes and do not by themselves indicate truncation. If `tcpdump` shows packets but all deshreds fail with the errors above, update the SDK/example before tuning socket buffers or firewall rules.

## Log legend
- Prefix: `🎯` program hit, `🐣` authority hit (`🎯🐣` means both)
- Action: `🐣` create (`create/buy` when amounts are present), `🟢` buy, `🔻` sell, `🪙` other, `❓` missing/unknown
- Pump.fun SOL values are instruction limits (max for buy/create, min for sell); actual fills require event/meta data (e.g., Geyser/RPC).
- Votes are skipped by default (`skip_vote_txs=true`)
- Set `SHREDS_UDP_LOG_*` to enable raw/shreds/entries/deshred debug logs; defaults are quiet except `log_watch_hits`
- Latency monitor uses a DashMap-backed slot tracker to reduce lock contention (enabled via `SHREDS_UDP_ENABLE_LATENCY=1`).

## Config (JSONC/TOML keys)
- `bind_addr`: listener address
- `log_*`: logging toggles (only `log_watch_hits` is true by default)
- `require_code_match` / `strict_*`: FEC strictness
- `slot_window_*` / `*_ttl_ms`: slot window and eviction TTLs
- `watch_program_ids` / `watch_authorities`: targets to watch (pump.fun defaults)
- `token_program_ids`: empty = Token + Token-2022
- `pump_min_lamports`: drop pump.fun buy/sell below this SOL limit threshold (0 = no filter). Applies to create-with-amount too.
- `mint_finder`: composite of pump.fun (create/create_v2 accounts[0], buy/sell/buy_exact_sol_in accounts[2]) + SPL Token MintTo/Initialize (tags 0/7/14/20, accounts[0])
- UDP shreds are processed directly; RPC commitment (processed/confirmed/finalized) is not used. Failed txs also appear; unknown amounts may show `❓`.

### Modular hooks for custom watchers/detailers
- Use `ShredsUdpConfig::watch_config_no_defaults()` or build `ProgramWatchConfig::new(...)` to avoid pump.fun fallbacks.
- Pipeline building blocks (5 layers): 1) `decode_udp_datagram` (receive/prefilter) → 2) `insert_shred` (FEC buffer) → 3) `deshred_shreds_to_entries` (deshred) → 4) `collect_watch_events` (watcher/detailer) → 5) any sink (log/queue/custom processing).
- State helpers: `ShredsUdpState::{remove_batch, mark_completed, mark_suppressed}` mirror the default cleanup performed by the one-call handler.
- Quick-start convenience: `handle_pumpfun_watcher` runs the full pump.fun-oriented stack in one call before you dive into customizations.
- Sample custom hook: set `SHREDS_UDP_CUSTOM_HOOK=1` to enable the placeholder hook in `main.rs`, then replace its body to push hits to your own sink (queue, RPC call, etc.). `collect_watch_events` delivers structured hits; `pump_min_lamports` continues to filter buys/sells.

## Notes on mint detection
- Triggers on Token/Token-2022 instructions with tags 0, 7, 14, 20 (assumes mint at accounts[0]).
- Swaps alone do not emit `mint=...`; look for MintTo/InitializeMint calls.
