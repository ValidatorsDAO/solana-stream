import { Connection, clusterApiUrl } from '@solana/web3.js'
import 'dotenv/config'

const MAX_CACHE_SIZE = 20
const MAX_FETCHING_SIZE = 20
const MAX_LATENCIES = 100

export const slotTimestampCache = new Map<number, number>()
export const receivedTransactions = new Map<
  number,
  Array<{ receivedAt: Date; tx: string }>
>()

const slotFetching = new Set<number>()
const latencyBuffer: number[] = []

const SOLANA_RPC_ENDPOINT =
  process.env.SOLANA_RPC_ENDPOINT || clusterApiUrl('mainnet-beta')
const connection = new Connection(SOLANA_RPC_ENDPOINT)

function cacheSlotTimestamp(slot: number, timestamp: number) {
  slotTimestampCache.set(slot, timestamp)
  if (slotTimestampCache.size > MAX_CACHE_SIZE) {
    const oldestKey = slotTimestampCache.keys().next().value
    if (oldestKey !== undefined) {
      slotTimestampCache.delete(oldestKey)
    }
  }
}

async function getCachedSlotTimestamp(slot: number): Promise<number | null> {
  if (!slot) return null
  if (slotTimestampCache.has(slot)) {
    return slotTimestampCache.get(slot)!
  }

  if (slotFetching.has(slot)) {
    return null
  }

  slotFetching.add(slot)

  try {
    const timestamp = await connection.getBlockTime(slot)
    if (timestamp !== null) {
      cacheSlotTimestamp(slot, timestamp)
    }
    return timestamp
  } catch (error: any) {
    if (!error.message.includes('Block not available')) {
      console.error(`🚨 Error fetching blockTime for slot ${slot}:`, error)
    }
    return null
  } finally {
    slotFetching.delete(slot)
    while (slotFetching.size > MAX_FETCHING_SIZE) {
      const oldestSlot = slotFetching.values().next().value
      if (oldestSlot) {
        slotFetching.delete(oldestSlot)
      }
    }
  }
}

function recordLatency(latencyMs: number) {
  if (latencyBuffer.length >= MAX_LATENCIES) latencyBuffer.shift()
  latencyBuffer.push(latencyMs)
}

function calculateAverageLatency() {
  if (latencyBuffer.length === 0) return 0
  const sum = latencyBuffer.reduce((a, b) => a + b, 0)
  return sum / latencyBuffer.length
}

export function startLatencyCheck(intervalMs: number = 420) {
  setInterval(async () => {
    const slots = Array.from(receivedTransactions.keys())

    for (const slot of slots) {
      const txArray = receivedTransactions.get(slot)!
      const blockTime = await getCachedSlotTimestamp(slot)

      if (blockTime !== null) {
        const blockTimeMs = blockTime * 1000

        txArray.forEach(({ receivedAt, tx }) => {
          const latencyMs = receivedAt.getTime() - blockTimeMs - 500
          recordLatency(latencyMs)

          console.log(`\nSlot: ${slot}`)
          console.log(`Tx: ${tx}`)
          console.log(`⏰ BlockTime: ${new Date(blockTimeMs).toISOString()}`)
          console.log(`📥 ReceivedAt: ${receivedAt.toISOString()}`)
          console.log(`🚀 Latency: ${latencyMs} ms`)
          console.log(
            `📊 Average Latency (latest ${latencyBuffer.length}): ${calculateAverageLatency().toFixed(2)} ms`,
          )
        })

        receivedTransactions.delete(slot)
      }
    }
  }, intervalMs)
}
