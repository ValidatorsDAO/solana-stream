import {
  GeyserClient,
  bs58,
  CommitmentLevel,
  SubscribeRequestFilterTransactions,
} from '@validators-dao/solana-stream-sdk'
import 'dotenv/config'
import { receivedTransactions, startLatencyCheck } from '@/utils/checkLatency'
import { SubscribeRequest } from '@/utils/geyser'

// const PUMP_FUN_MINT_AUTHORITY = 'TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM'
const PUMP_FUN_PROGRAM_ID = '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'

const pumpfun: SubscribeRequestFilterTransactions = {
  accountInclude: [PUMP_FUN_PROGRAM_ID],
  accountExclude: [],
  accountRequired: [],
}

const request: SubscribeRequest = {
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

const geyser = async () => {
  console.log('Starting geyser client...')
  const maxRetries = 2000000
  let warnedMissingToken = false

  const createClient = () => {
    const token = process.env.X_TOKEN?.trim()
    if (!token && !warnedMissingToken) {
      console.warn('X_TOKEN not set. Connecting without auth.')
      warnedMissingToken = true
    }
    const endpoint = process.env.GEYSER_ENDPOINT || 'http://localhost:10000'
    console.log('Connecting to', endpoint)

    return new GeyserClient(endpoint, token || undefined, undefined)
  }

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

  const connect = async (): Promise<void> => {
    let retries = 0

    while (retries <= maxRetries) {
      try {
        const client = createClient()
        await client.connect()
        const version = await client.getVersion()
        console.log('version: ', version)
        const stream = await client.subscribe()
        stream.on('data', async (data: any) => {
          if (data.transaction != undefined) {
            // Checking Latency
            const slot = Number(data.transaction.slot)
            const receivedAt = new Date()
            const txSignature = bs58.encode(
              new Uint8Array(data.transaction.transaction.signature),
            )

            if (!receivedTransactions.has(slot)) {
              receivedTransactions.set(slot, [])
            }
            receivedTransactions.get(slot)!.push({ receivedAt, tx: txSignature })
            return
          }
          if (data.account != undefined) {
            const accounts = data.account
            const rawPubkey = accounts.account.pubkey
            const rawTxnSignature = accounts.account.txnSignature
            const pubkey = bs58.encode(new Uint8Array(rawPubkey))
            const txnSignature = bs58.encode(new Uint8Array(rawTxnSignature))
            console.log('pubkey:', pubkey)
            console.log('txnSignature:', txnSignature)
            return
          }
          // console.log('data:', JSON.stringify(data, null, 2))
        })

        const streamClosed = new Promise<void>((_, reject) => {
          stream.on('error', (e: any) => reject(e))
          stream.on('end', () => reject(new Error('Stream ended')))
          stream.on('close', () => reject(new Error('Stream closed')))
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

        await streamClosed
      } catch (error) {
        retries += 1
        if (retries > maxRetries) {
          throw error
        }
        console.error(`Connection failed. Retrying ...`, error)
        const delayMs = Math.min(1000 * 2 ** Math.min(retries, 5), 30000)
        await sleep(delayMs)
      }
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
// Checking Latency
startLatencyCheck()
