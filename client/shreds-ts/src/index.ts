import {
  ShredstreamProxyClient,
  credentials,
  ShredsCommitmentLevel,
  ShredsSubscribeEntriesRequestFns,
} from '@validators-dao/solana-stream-sdk'

import 'dotenv/config'

const endpoint = process.env.SHREDS_ENDPOINT!.replace(/^https?:\/\//, '')

const client = new ShredstreamProxyClient(endpoint, credentials.createSsl())

const request = ShredsSubscribeEntriesRequestFns.create({
  accounts: {},
  transactions: {},
  slots: {},
  commitment: ShredsCommitmentLevel.PROCESSED,
})

const connect = async () => {
  console.log('Connecting to:', endpoint)

  const stream = client.subscribeEntries(request)
  stream.on('data', (data) => {
    console.log(`Received slot: ${data.slot}`)
    console.log('Raw entries:', data.entries)
  })

  stream.on('error', (err) => {
    console.error('Stream error:', err)
    console.log('Reconnecting...')
    setTimeout(connect, 5000)
  })

  stream.on('end', () => {
    console.log('Stream ended, reconnecting...')
    setTimeout(connect, 5000)
  })
}

connect()
