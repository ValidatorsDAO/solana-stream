<p align="center">
  <a href="https://slv.dev/" target="_blank">
    <img src="https://storage.validators.solutions/SolanaStreamSDK.jpg" alt="SolanaStreamSDK" />
  </a>
  <a href="https://twitter.com/intent/follow?screen_name=ValidatorsDAO" target="_blank">
    <img src="https://img.shields.io/twitter/follow/ValidatorsDAO.svg?label=Follow%20@ValidatorsDAO" alt="Follow @ValidatorsDAO" />
  </a>
  <a href="https://www.npmjs.com/package/@validators-dao/solana-stream-sdk">
    <img alt="NPM Version" src="https://img.shields.io/npm/v/@validators-dao/solana-stream-sdk?color=268bd2&label=version&logo=npm">
  </a>
  <a href="https://www.npmjs.com/package/@validators-dao/solana-stream-sdk">
    <img alt="NPM Downloads" src="https://img.shields.io/npm/dt/@validators-dao/solana-stream-sdk?color=cb4b16&label=npm%20downloads">
  </a>
  <a aria-label="License" href="https://github.com/ValidatorsDAO/solana-stream/blob/main/LICENSE.txt">
    <img alt="" src="https://badgen.net/badge/license/Apache/blue">
  </a>
  <a aria-label="Code of Conduct" href="https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md">
    <img alt="" src="https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg">
  </a>
</p>

# Solana Geyser Client Example (geyser-rs)

Resilient Geyser gRPC sample using the [solana-stream-sdk](https://github.com/ValidatorsDAO/solana-stream). It shows how to filter transactions/accounts/slots/blocks and keep the stream healthy in production (ping/pong, backoff, gap recovery).

<a href="https://solana.com/">
  <img src="https://storage.slv.dev/PoweredBySolana.svg" alt="Powered By Solana" width="200px" height="95px">
</a>

## Quick start

1) `cd client/geyser-rs` (or `cd temp-release/geyser-rs` for the temp bundle)
2) Provide env:
```env
GRPC_ENDPOINT=https://your-geyser-grpc-endpoint
X_TOKEN=your_token_if_needed    # optional header for auth
SOLANA_RPC_ENDPOINT=https://api.mainnet-beta.solana.com
CONFIG_PATH=./config.jsonc      # optional override
```
3) Edit `config.jsonc` (JSONC allowed). Example filters are below.
4) Run:
```bash
RUST_LOG=info cargo run
```

## What this sample includes (best-practice defaults)
- Config-driven filters for transactions/accounts/slots/blocks; sample tx filter removes vote/failed tx (bandwidth savings)
- Ping/Pong handling to keep Yellowstone gRPC connections alive
- Gap recovery with `from_slot` based on the last seen slot (resumes from `slot-1`)
- Exponential reconnect backoff that resets after successful traffic
- Ingress/processing split via a bounded channel (10_000); slow consumers are warned and updates may be dropped when full
- Latency monitor using `SOLANA_RPC_ENDPOINT` for blocktime lookups

### macOS libclang note
If you hit `@rpath/libclang.dylib` errors (common on Apple Silicon), point to Homebrew LLVM:
```bash
export LIBCLANG_PATH=/opt/homebrew/opt/llvm/lib
export DYLD_LIBRARY_PATH=/opt/homebrew/opt/llvm/lib
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
```
Then run `cargo run` as usual.

## Sample `config.jsonc`

The defaults follow the recommended "drop vote/failed tx" pattern; expand as needed.

```jsonc
{
  "commitment": "Processed",
  "transactions": {
    "example": {
      "account_include": ["6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"],
      "account_exclude": [],
      "account_required": [],
      "vote": false,
      "failed": false
    },
  },
  "accounts": {
    "example": {
      "account": [],
      "owner": [],
      "filters": [],
    },
  },
  "slots": {
    "example": {
      "filter_by_commitment": true,
      "interslot_updates": false,
    },
  },
  "blocks": {
    "example": {
      "account_include": [],
      "include_transactions": true,
      "include_accounts": false,
      "include_entries": false,
    },
  },
  "blocks_meta": {
    "example": {},
  },
  "entry": {
    "example": {},
  },
}
```

⚠️ The sample endpoints above are placeholders—configure your own Geyser gRPC endpoint and auth token.

## Notes
- `CONFIG_PATH` can point to any JSONC file; default is `config.jsonc` in this folder.
- `SOLANA_RPC_ENDPOINT` is only used for blocktime/latency logging; the stream itself is pure gRPC.
- Adjust `UPDATE_CHANNEL_CAPACITY` in `src/main.rs` if you need tighter or looser backpressure.

If you encounter issues or have suggestions, please open an issue:

- [GitHub Issues](https://github.com/ValidatorsDAO/solana-stream/issues)

Join discussions on Validators DAO's Discord:

- [Discord Community](https://discord.gg/C7ZQSrCkYR)

## License

Open source under the [Apache-2.0 License](https://www.apache.org/licenses/LICENSE-2.0).

## Code of Conduct

Everyone interacting in Validators DAO project’s repositories is expected to follow the [code of conduct](https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md).
