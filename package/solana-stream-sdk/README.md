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

## Installation

```bash
npm install @validators-dao/solana-stream-sdk
```

Or with pnpm:

```bash
pnpm add @validators-dao/solana-stream-sdk
```

## Usage

Example of using the GeyserClient to subscribe to Solana Pump Fun transactions and accounts:

```typescript
import {
  GeyserClient,
  bs58,
  CommitmentLevel,
  SubscribeRequestAccountsDataSlice,
  SubscribeRequestFilterAccounts,
  SubscribeRequestFilterBlocks,
  SubscribeRequestFilterBlocksMeta,
  SubscribeRequestFilterEntry,
  SubscribeRequestFilterSlots,
  SubscribeRequestFilterTransactions,
} from '@validators-dao/solana-stream-sdk'
import 'dotenv/config'

interface SubscribeRequest {
  accounts: {
    [key: string]: SubscribeRequestFilterAccounts
  }
  slots: {
    [key: string]: SubscribeRequestFilterSlots
  }
  transactions: {
    [key: string]: SubscribeRequestFilterTransactions
  }
  transactionsStatus: {
    [key: string]: SubscribeRequestFilterTransactions
  }
  blocks: {
    [key: string]: SubscribeRequestFilterBlocks
  }
  blocksMeta: {
    [key: string]: SubscribeRequestFilterBlocksMeta
  }
  entry: {
    [key: string]: SubscribeRequestFilterEntry
  }
  commitment?: CommitmentLevel | undefined
  accountsDataSlice: SubscribeRequestAccountsDataSlice[]
  ping?: any
}

// const PUMP_FUN_MINT_AUTHORITY = 'TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM'
const PUMP_FUN_PROGRAM_ID = '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'

const tran: SubscribeRequestFilterTransactions = {
  accountInclude: [PUMP_FUN_PROGRAM_ID],
  accountExclude: [],
  accountRequired: [],
}

const request: SubscribeRequest = {
  accounts: {
    pumpfun: {
      account: [],
      owner: [],
      filters: [],
    },
  },
  slots: {},
  transactions: { elsol: tran },
  transactionsStatus: {},
  blocks: {},
  blocksMeta: {},
  entry: {},
  accountsDataSlice: [],
  commitment: CommitmentLevel.PROCESSED,
}

const geyser = async () => {
  console.log('Starting geyser client...')
  const maxRetries = 2000000

  const createClient = () => {
    const token = process.env.X_TOKEN || ''
    console.log('X_TOKEN:', token)
    if (token === '') {
      throw new Error('X_TOKEN environment variable is not set')
    }
    const endpoint = `https://grpc-ams-3.erpc.global`
    console.log('Connecting to', endpoint)

    // @ts-ignore ignore
    return new GeyserClient(endpoint, token, undefined)
  }

  const connect = async (retries: number = 0): Promise<void> => {
    if (retries > maxRetries) {
      throw new Error('Max retries reached')
    }

    try {
      const client = createClient()
      const version = await client.getVersion()
      console.log('version: ', version)
      const stream = await client.subscribe()
      stream.on('data', async (data: any) => {
        if (data.transaction !== undefined) {
          const transaction = data.transaction
          const txnSignature = transaction.transaction.signature
          const tx = bs58.encode(new Uint8Array(txnSignature))
          console.log('tx:', tx)
          return
        }
        if (data.account === undefined) {
          return
        }
        // console.log('data:', JSON.stringify(data, null, 2))

        const accounts = data.account
        const rawPubkey = accounts.account.pubkey
        const rawTxnSignature = accounts.account.txnSignature
        const pubkey = bs58.encode(new Uint8Array(rawPubkey))
        const txnSignature = bs58.encode(new Uint8Array(rawTxnSignature))
        console.log('pubkey:', pubkey)
        console.log('txnSignature:', txnSignature)
      })

      stream.on('error', async (e: any) => {
        console.error('Stream error:', e)
        console.log(`Reconnecting ...`)
        await connect(retries + 1)
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
      }).catch((reason) => {
        console.error(reason)
        throw reason
      })
    } catch (error) {
      console.error(`Connection failed. Retrying ...`, error)
      await connect(retries + 1)
    }
  }

  await connect()
}

const main = async () => {
  try {
    await geyser()
  } catch (error) {
    console.log(error)
  }
}

main()
```

Please ensure you have the `X_TOKEN` environment variable set with your gRPC token for authentication.

Please note that the url endpoint in the example is for demonstration purposes. You should replace it with the actual endpoint you are using.

### Shreds Client

Here's how to use the SDK to subscribe to Solana Shreds and decode entries:

```typescript
import {
  ShredsClient,
  ShredsClientCommitmentLevel,
  // decodeSolanaEntries,
} from '@validators-dao/solana-stream-sdk'
import 'dotenv/config'
// import { logDecodedEntries } from '@/utils/logDecodedEntries'

import { receivedSlots, startLatencyCheck } from '@/utils/checkLatency'

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
      const receivedAt = new Date()
      if (buffer) {
        const {
          slot,
          // entries
        } = JSON.parse(buffer)

        // You can decode entries as needed
        // const decodedEntries = decodeSolanaEntries(new Uint8Array(entries))
        // logDecodedEntries(decodedEntries)

        if (!receivedSlots.has(slot)) {
          receivedSlots.set(slot, [{ receivedAt }])
        } else {
          receivedSlots.get(slot)!.push({ receivedAt })
        }
      }
    },
  )
}

connect()
startLatencyCheck()
```

Ensure the environment variable `SHREDS_ENDPOINT` is set correctly.

## Features

- **Geyser Client**: Direct access to Triton's Yellowstone gRPC client for real-time Solana data streaming
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

- `@triton-one/yellowstone-grpc`: For gRPC streaming capabilities
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
