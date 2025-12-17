# shreds-udp-rs

Rust starter that listens for Shredstream over **UDP** and prints basic stats.
Heartbeats are not required; point your Shredstream sender at the bound `ip:port`.

## Usage

1) 設定ファイルは `client/shreds-udp-rs/settings.jsonc`

リポジトリ同梱の `settings.jsonc` を編集し、そのままビルドしてください（jsonc コメント可）。ビルド時にバイナリへ埋め込まれるので、実行時に `.env` や `SHREDS_UDP_CONFIG` は不要です。

2) RPC などシークレットは環境変数で指定:

```env
SOLANA_RPC_ENDPOINT=https://api.mainnet-beta.solana.com
```

3) Run:

```bash
cargo run -p shreds-udp-rs
```

## What gets logged
- Minimal by default: successful deshreds + watch hits.
- プレフィックス: `🎯` program hit, `🐣` authority hit（両方なら `🎯🐣`）。`auth_match=[...]` に最大2件の authority マッチ。
- アクション: `🐣` create（数量が付いていれば kind 表示は `create/buy`）、`🟢` buy、`🔻` sell、`🪙` その他。
- `SHREDS_UDP_LOG_ENTRIES=1` shows first non-vote signatures per FEC set.
- `SHREDS_UDP_LOG_DESHRED_ATTEMPTS=1` dumps batch status before each deshred (noisy, for debugging gaps).
- `SHREDS_UDP_LOG_DESHRED_ERRORS=1` re-enables detailed decode-failure logs (otherwise suppressed for speed).

## Config file (JSON/TOML)
Keys and動き（サマリ）:
- `bind_addr`: 受信アドレス。
- `log_*`: デフォルト静か。`log_watch_hits`のみtrue。
- `require_code_match`/`strict_*`: FECチェックの厳しさ。
- `slot_window_*`/`*_ttl_ms`: 古い/将来スロットの抑制とTTL。
- `watch_program_ids`/`watch_authorities`: ヒット判定対象。pump.funがデフォルト。
- `token_program_ids`: 空なら Token/Token-2022。指定すれば上書き。
- `pump_min_lamports`: pump.fun buy/sell の SOL 金額がこのラップポート未満ならログを抑制（0で無効）。create に数量が付いている場合も同じしきい値で抑制。
- `mint_finder` は内部で複合: pump.fun (create/create_v2: accounts[0], buy/sell/buy_exact_sol_in: accounts[2]) + トップレベルSPL Token MintTo/Initialize系（tag 0/7/14/20, accounts[0]）。

## Notes on mint detection
- Only fires on Token / Token-2022 instructions with tags 0, 7, 14, 20.
- Assumes the mint account is `accounts[0]` in the instruction (standard SPL layout).
- Swaps alone (e.g., pump.fun swap) will not emit `mint=...`; look for actual MintTo/InitializeMint calls.
