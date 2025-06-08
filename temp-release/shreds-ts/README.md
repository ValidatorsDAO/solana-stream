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

# Solana Shreds Client Example

This is an example demonstrating how to use the Shredstream client from the [@validators-dao/solana-stream-sdk](https://www.npmjs.com/package/@validators-dao/solana-stream-sdk) to stream and decode Solana shreds in real-time.

<a href="https://solana.com/">
  <img src="https://storage.slv.dev/PoweredBySolana.svg" alt="Powered By Solana" width="200px" height="95px">
</a>

## Installation

Make sure you have installed the necessary package:

```bash
pnpm i
```

## Usage

Make sure your `.env` file contains the correct `SHREDS_ENDPOINT` variable.

```bash
pnpm dev
```

Here's how to use the Shredstream client with entry decoding:

```typescript
import {
  ShredstreamProxyClient,
  credentials,
  ShredsCommitmentLevel,
  ShredsSubscribeEntriesRequestFns,
  decodeSolanaEntries,
  bs58,
} from '@validators-dao/solana-stream-sdk'
import 'dotenv/config'

const endpoint = process.env.SHREDS_ENDPOINT!.replace(/^https?:\/\//, '')

const client = new ShredstreamProxyClient(endpoint, credentials.createSsl())

const request = ShredsSubscribeEntriesRequestFns.create({
  accounts: {
    pumpfun: {
      account: [],
      owner: [],
      filters: [],
    },
  },
  transactions: {},
  slots: {},
  commitment: ShredsCommitmentLevel.PROCESSED,
})

const connect = async () => {
  console.log('Connecting to:', endpoint)

  const stream = client.subscribeEntries(request)

  stream.on('data', (data) => {
    console.log(`\n🟢 Received slot: ${data.slot}`)

    const decodedEntries = decodeSolanaEntries(data.entries)

    if (!Array.isArray(decodedEntries)) {
      console.warn('⚠️ decodedEntries is not an array:', decodedEntries)
      return
    }

    decodedEntries.forEach((entry: any, entryIdx: number) => {
      console.log(`\n✅ Entry #${entryIdx + 1}`)
      console.log(
        `  - Hash: ${entry.hash ? bs58.encode(Buffer.from(entry.hash)) : 'N/A'}`,
      )
      console.log(`  - Num Hashes: ${entry.num_hashes ?? 'N/A'}`)

      if (!Array.isArray(entry.transactions)) {
        console.warn('⚠️ transactions is not an array:', entry.transactions)
        return
      }

      entry.transactions.forEach((tx: any, txIdx: number) => {
        console.log(`\n📄 Transaction #${txIdx + 1}`)

        if (!tx || !tx.message || !Array.isArray(tx.message)) {
          console.warn('⚠️ Invalid transaction structure:', tx)
          return
        }

        const signaturesBase58 = Array.isArray(tx.signatures)
          ? tx.signatures
              .slice(1)
              .map((sig: number[]) => bs58.encode(Buffer.from(sig)))
          : []

        console.log(`  - Signatures:`, signaturesBase58)

        const message = tx.message[0]

        if (message) {
          if (Array.isArray(message.accountKeys)) {
            console.log(`  🔑 Account Keys:`)
            message.accountKeys.forEach((key: number[], idx: number) => {
              if (Array.isArray(key)) {
                console.log(`    [${idx}] ${bs58.encode(Buffer.from(key))}`)
              } else {
                console.warn(`    [${idx}] Invalid key format:`, key)
              }
            })
          } else {
            console.warn(
              '⚠️ accountKeys is undefined or not an array:',
              message.accountKeys,
            )
          }

          if (Array.isArray(message.instructions)) {
            console.log(`  ⚙️ Instructions:`)
            message.instructions.forEach((inst: any, instIdx: number) => {
              console.log(`    [${instIdx}]`)
              console.log(
                `      - Program ID Index: ${inst.programIdIndex ?? 'N/A'}`,
              )
              console.log(
                `      - Accounts: ${Array.isArray(inst.accounts) ? inst.accounts.join(', ') : 'N/A'}`,
              )
              console.log(
                `      - Data: ${inst.data ? bs58.encode(Buffer.from(inst.data)) : 'N/A'}`,
              )
            })
          } else {
            console.warn(
              '⚠️ instructions is undefined or not an array:',
              message.instructions,
            )
          }

          console.log(
            `  📌 Recent Blockhash: ${message.recentBlockhash ? bs58.encode(Buffer.from(message.recentBlockhash)) : 'N/A'}`,
          )
        } else {
          console.warn('⚠️ message[0] is undefined:', tx.message)
        }
      })
    })
  })

  stream.on('error', (err) => {
    console.error('🚨 Stream error:', err)
    console.log('♻️ Reconnecting...')
    setTimeout(connect, 5000)
  })

  stream.on('end', () => {
    console.log('🔚 Stream ended, reconnecting...')
    setTimeout(connect, 5000)
  })
}

connect()
```

## ⚠️ Experimental Filtering Feature Notice

The filtering functionality provided by this SDK is currently experimental. Occasionally, data may not be fully available, and filters may not be applied correctly.

If you encounter such cases, please report them by opening an issue at: https://github.com/ValidatorsDAO/solana-stream/issues

Your feedback greatly assists our debugging efforts and overall improvement of this feature.

Other reports and suggestions are also highly appreciated.

You can also join discussions or share feedback on Validators DAO's Discord community:
https://discord.gg/C7ZQSrCkYR

## Support

For questions and further support, please visit the [Validators DAO Discord](https://discord.gg/C7ZQSrCkYR).

## License

The package is available as open source under the terms of the
[Apache-2.0 License](https://www.apache.org/licenses/LICENSE-2.0).

## Code of Conduct

Everyone interacting in the Validators DAO project’s codebases, issue trackers, chat rooms
and mailing lists is expected to follow the
[code of conduct](https://github.com/ValidatorsDAO/solana-stream/blob/main/CODE_OF_CONDUCT.md).
