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

# @validators-dao/solana-stream-sdk

Solana Stream SDK by Validators DAO - A TypeScript SDK for streaming Solana blockchain data.

<a href="https://solana.com/">
  <img src="https://storage.slv.dev/PoweredBySolana.svg" alt="Powered By Solana" width="200px" height="95px">
</a>

## What's New in v1.1.0

- Refreshed starter layout and docs to highlight trading hooks
- Yellowstone Geyser gRPC connection upgraded to an NAPI-RS-powered client for better backpressure
- NAPI-powered Shreds client/decoder so TypeScript can tap Rust-grade throughput
- Improved backpressure handling and up to 4x streaming efficiency (400% improvement)
- Faster real-time Geyser streams for TypeScript clients with lower overhead

## Production-Ready Geyser Client (TypeScript Best Practices)

- Ping/Pong handling to keep Yellowstone gRPC streams alive
- Exponential reconnect backoff plus `fromSlot` gap recovery
- Bounded in-memory queue with drop logging for backpressure safety
- Hot-swappable subscriptions via a JSON file (no reconnect)
- Optional runtime metrics logging (rates, queue size, drops)
- Default filters drop vote/failed transactions to reduce traffic

Tip: start with slots, then add filters as needed. When resuming from `fromSlot`,
duplicates are expected.

## Performance Highlights

- NAPI-powered Geyser gRPC and Shreds client/decoder for high-throughput streaming
- TypeScript ergonomics with Rust-grade performance under the hood
- For the absolute fastest signal path, see Rust UDP Shreds in the repo:
  https://github.com/ValidatorsDAO/solana-stream#shreds-udp-pumpfun-watcher-rust

## Installation

```bash
npm install @validators-dao/solana-stream-sdk
```

Or with pnpm:

```bash
pnpm add @validators-dao/solana-stream-sdk
```

## Usage

### Geyser Client (TypeScript)

For a production-ready starter (JSON subscriptions, backpressure handling, reconnects),
see https://github.com/ValidatorsDAO/solana-stream/tree/main/client/geyser-ts.

```typescript
import {
  GeyserClient,
  bs58,
  CommitmentLevel,
  SubscribeRequestFilterTransactions,
} from '@validators-dao/solana-stream-sdk'
import 'dotenv/config'

// const PUMP_FUN_MINT_AUTHORITY = 'TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM'
const PUMP_FUN_PROGRAM_ID = '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'

const pumpfun: SubscribeRequestFilterTransactions = {
  vote: false,
  failed: false,
  accountInclude: [PUMP_FUN_PROGRAM_ID],
  accountExclude: [],
  accountRequired: [],
}

const request = {
  accounts: {},
  slots: {},
  transactions: { pumpfun },
  transactionsStatus: {},
  blocks: {},
  blocksMeta: {},
  entry: {},
  accountsDataSlice: [],
  commitment: CommitmentLevel.PROCESSED,
}

const main = async () => {
  const endpoint = process.env.GEYSER_ENDPOINT || 'http://localhost:10000'
  const token = process.env.X_TOKEN?.trim()
  const client = new GeyserClient(endpoint, token || undefined, undefined)

  await client.connect()
  const stream = await client.subscribe()

  stream.on('data', (data: any) => {
    if (data?.ping != undefined) {
      stream.write({ ping: { id: 1 } }, () => undefined)
      return
    }
    if (data?.pong != undefined) {
      return
    }
    if (data?.transaction != undefined) {
      const signature = data.transaction.transaction.signature
      const txSignature = bs58.encode(new Uint8Array(signature))

      // TODO: Add your trade logic here.
      console.log('tx:', txSignature)
    }
  })

  await new Promise<void>((resolve, reject) => {
    stream.write(request, (err: any) => {
      if (!err) {
        resolve()
      } else {
        console.error('Request error:', err)
        reject(err)
      }
    })
  })
}

void main()
```

If your endpoint requires authentication, set the `X_TOKEN` environment variable with your gRPC token.

Please note that the url endpoint in the example is for demonstration purposes. You should replace it with the actual endpoint you are using.

### Shreds Client

For a working starter that includes latency checks, see
https://github.com/ValidatorsDAO/solana-stream/tree/main/client/shreds-ts.

Here's a minimal example:

```typescript
import {
  ShredsClient,
  ShredsClientCommitmentLevel,
  // decodeSolanaEntries,
} from '@validators-dao/solana-stream-sdk'
import 'dotenv/config'

const endpoint = process.env.SHREDS_ENDPOINT!

const client = new ShredsClient(endpoint)

// The filter is experimental
const request = {
  accounts: {},
  transactions: {},
  slots: {},
  commitment: ShredsClientCommitmentLevel.Processed,
}

const connect = () => {
  console.log('Connecting to:', endpoint)

  client.subscribeEntries(
    JSON.stringify(request),
    (_error: any, buffer: any) => {
      if (!buffer) {
        return
      }
      const { slot } = JSON.parse(buffer)

      // You can decode entries as needed
      // const decodedEntries = decodeSolanaEntries(new Uint8Array(entries))

      console.log('slot:', slot)
    },
  )
}

connect()
```

Ensure the environment variable `SHREDS_ENDPOINT` is set correctly.

## Features

- **Geyser Client**: Direct access to the Yellowstone gRPC client for real-time Solana data streaming
- **Shredstream Client**: Real-time entry streaming and decoding from Solana Shreds
- **TypeScript Types**: Comprehensive TypeScript types for all filter and subscription interfaces
- **Utilities**: Includes bs58 for Solana address and data encoding/decoding, gRPC utilities, and entry decoding functions
- **Full Type Safety**: Complete TypeScript support with detailed type definitions

## Exported Types and Utilities

### Geyser Client Types

- `GeyserClient`: Main client for connecting to Yellowstone gRPC streams.
- `CommitmentLevel`: Solana commitment levels (e.g., processed, confirmed, finalized).
- `SubscribeRequestFilterAccounts`: Filters for account subscriptions.
- `SubscribeRequestFilterTransactions`: Filters for transaction subscriptions.
- `SubscribeRequestFilterBlocks`: Filters for block subscriptions.
- `SubscribeRequestFilterBlocksMeta`: Filters for block metadata subscriptions.
- `SubscribeRequestFilterSlots`: Filters for slot subscriptions.
- `SubscribeRequestFilterEntry`: Filters for entry subscriptions.
- `SubscribeRequestAccountsDataSlice`: Data slice configuration for account subscriptions.
- `bs58`: Base58 encoding/decoding utilities for Solana addresses and data.

### Shredstream Client

- `ShredsClient`: Client for streaming Solana shreds through shreds endpoints.
- `ShredsClientCommitmentLevel`: Solana commitment levels (e.g., processed, confirmed, finalized).

### Utility Exports

- `decodeSolanaEntries`: Function to decode raw Solana shred entry data into structured, human-readable formats.

## Dependencies

- Yellowstone gRPC client: For gRPC streaming capabilities
- `bs58`: For base58 encoding/decoding
- `@validators-dao/solana-entry-decoder`: Utility for decoding Solana shred entries.
- `@validators-dao/solana-shreds-client`: Solana Shreds Client for Scale. (NAPI-RS)

## ⚠️ Experimental Filtering Feature Notice

The filtering functionality provided by this SDK is currently experimental. Occasionally, data may not be fully available, and filters may not be applied correctly.

If you encounter such cases, please report them by opening an issue at: https://github.com/ValidatorsDAO/solana-stream/issues

Your feedback greatly assists our debugging efforts and overall improvement of this feature.

Other reports and suggestions are also highly appreciated.

You can also join discussions or share feedback on Validators DAO's Discord community:
https://discord.gg/C7ZQSrCkYR

## Repository

This package is part of the [Solana Stream](https://github.com/ValidatorsDAO/solana-stream) monorepo.

## Support

For issues and support, please visit our [Discord](https://discord.gg/C7ZQSrCkYR).

## License

The package is available as open source under the terms of the
[Apache-2.0 License](https://www.apache.org/licenses/LICENSE-2.0).

## Code of Conduct

Everyone interacting in the Validators DAO project’s codebases, issue trackers, chat rooms
and mailing lists is expected to follow the
[code of conduct](https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md).
