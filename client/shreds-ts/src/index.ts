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
