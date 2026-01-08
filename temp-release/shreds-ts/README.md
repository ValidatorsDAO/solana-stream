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

Make sure your `.env` file contains the correct `SHREDS_ENDPOINT` and `SOLANA_RPC_ENDPOINT` variable.

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
