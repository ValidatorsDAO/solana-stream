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

# Solana Geyser Client

<a href="https://solana.com/">
  <img src="https://storage.slv.dev/PoweredBySolana.svg" alt="Powered By Solana" width="200px" height="95px">
</a>

## Usage

```bash
cargo run
```

Ensure your `.env` file contains:

```env
GRPC_ENDPOINT=https://grpc.erpc.global/
X_TOKEN=your_token # Optional
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

Here's an example `config.jsonc`:

```jsonc
{
  "commitment": "Processed",
  "transactions": {
    "example": {
      "account_include": [],
      "account_exclude": [],
      "account_required": [],
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

## ⚠️ Experimental Feature Notice

This Geyser client and its filtering functionality are experimental. If you encounter issues or have suggestions, please open an issue:

- [GitHub Issues](https://github.com/ValidatorsDAO/solana-stream/issues)

Join discussions on Validators DAO's Discord:

- [Discord Community](https://discord.gg/C7ZQSrCkYR)

## License

Open source under the [Apache-2.0 License](https://www.apache.org/licenses/LICENSE-2.0).

## Code of Conduct

Everyone interacting in Validators DAO project’s repositories is expected to follow the [code of conduct](https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md).
