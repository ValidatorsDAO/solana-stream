# Shreds-UDP-RS (Temp Release)

シンプルな UDP 版クライアントです。`solana-stream-sdk` v1.0.1 を crates.io から利用しています。設定は同梱の `settings.jsonc` をビルド時にバイナリへ埋め込みます（非シークレットのみ）。RPC などシークレットは環境変数で上書きしてください。

## 使い方

1) `.env` を用意（または環境変数をエクスポート）

```env
SOLANA_RPC_ENDPOINT=https://api.mainnet-beta.solana.com
```

2) 実行

```bash
RUST_LOG=info cargo run
```

デコードは `solana-stream-sdk::shreds_udp` に任せています。`watch_program_ids` などの公開設定は `settings.jsonc` を編集してビルドしてください（jsonc コメント可）。シークレットは環境変数で上書きできます（例: `SOLANA_RPC_ENDPOINT`）。

- UDP shreds を直接処理するので、RPC commitment (processed/confirmed/finalized) には依存しません。shreds に載ったものはそのままログに流れます（失敗トランザクションも表示されます）。
- 失敗トランザクションや金額を取得できないケースでは、アイコン/数量に `❓` を出すことがあります。取得精度を上げるための改善 PR は歓迎です。

## 備考

- NAT/ファイアウォールで受信ポートが開いていることを確認してください。
- payload 形式は SDK 標準の shredstream Entry デコードに沿います。必要に応じて `src/main.rs` を調整してください。
