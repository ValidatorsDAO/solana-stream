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
  blocksMeta: {
    pumpfun: {},
  },
  entry: {},
  accountsDataSlice: [],
  commitment: CommitmentLevel.PROCESSED,
}

let blocktime: string | null = null
const latencyList: number[] = []
const MAX_LATENCIES = 100

const calculateAverage = (latencies: number[]) => {
  if (latencies.length === 0) return 0
  const sum = latencies.reduce((acc, curr) => acc + curr, 0)
  return sum / latencies.length
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
    const endpoint = process.env.GEYSER_ENDPOINT || 'http://localhost:10000'
    console.log('Connecting to', endpoint)

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
        if (data.blockMeta != undefined) {
          blocktime =
            Number(data.blockMeta.blockTime.timestamp * 1000).toString() ?? '0'
        }

        if (data.transaction != undefined && blocktime) {
          const receiveTime = new Date()
          const transaction = data.transaction
          const txnSignature = transaction.transaction.signature
          const latency = Number(receiveTime) - Number(blocktime) - 500
          const tx = bs58.encode(new Uint8Array(txnSignature))
          latencyList.push(latency)

          if (latencyList.length > MAX_LATENCIES) {
            latencyList.shift()
          }

          const averageLatency = calculateAverage(latencyList).toFixed(2)
          console.log('tx:', tx)
          console.log('Latency:', latency)
          console.log('Average Latency (latest 100):', averageLatency, 'ms')
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
