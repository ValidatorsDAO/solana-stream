import {
  ShredstreamProxyClient,
  credentials,
  ShredsCommitmentLevel,
  ShredsSubscribeEntriesRequestFns,
  // decodeSolanaEntries,
} from '@validators-dao/solana-stream-sdk'
import 'dotenv/config'
// import { logDecodedEntries } from '@/utils/logDecodedEntries'

const rawEndpoint = process.env.SHREDS_ENDPOINT!
const endpoint = rawEndpoint.replace(/^https?:\/\//, '')

const client = rawEndpoint.startsWith('https://')
  ? new ShredstreamProxyClient(endpoint, credentials.createSsl())
  : new ShredstreamProxyClient(endpoint, credentials.createInsecure())

// Filter is experimental
const request = ShredsSubscribeEntriesRequestFns.create({
  accounts: {
    pumpfun: {
      account: ['6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'],
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

    // You can see data with decoding entries
    // const decodedEntries = decodeSolanaEntries(data.entries)
    // logDecodedEntries(decodedEntries)
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
