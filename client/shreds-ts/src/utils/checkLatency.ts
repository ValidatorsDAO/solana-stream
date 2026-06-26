import { createSolanaRpc, mainnet, type Slot } from '@solana/kit'
import 'dotenv/config'

export const MAX_CACHE_SIZE = 20
export const slotTimestampCache = new Map<number, number>()

const SOLANA_RPC_ENDPOINT =
  process.env.SOLANA_RPC_ENDPOINT || 'https://api.mainnet-beta.solana.com'
const rpc = createSolanaRpc(mainnet(SOLANA_RPC_ENDPOINT))

export const receivedSlots = new Map<number, Array<{ receivedAt: Date }>>()

export function cacheSlotTimestamp(slot: number, timestamp: number) {
  if (slotTimestampCache.has(slot)) {
    slotTimestampCache.delete(slot)
  }
  slotTimestampCache.set(slot, timestamp)

  if (slotTimestampCache.size > MAX_CACHE_SIZE) {
    const oldestKey = slotTimestampCache.keys().next().value
    if (oldestKey !== undefined) {
      slotTimestampCache.delete(oldestKey)
    }
  }
}

const slotFetching = new Set<number>()

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
    const timestamp = await rpc.getBlockTime(BigInt(slot) as Slot).send()
    if (timestamp !== null) {
      cacheSlotTimestamp(slot, Number(timestamp))
    }
    return timestamp === null ? null : Number(timestamp)
  } catch (error: any) {
    if (error.message.includes('Block not available')) {
      return null
    } else {
      console.error(`🚨 Error fetching blockTime for slot ${slot}:`, error)
      return null
    }
  } finally {
    slotFetching.delete(slot)
  }
}

const latencyBuffer: number[] = []
const MAX_LATENCY_BUFFER_SIZE = 420

function recordLatency(latencyMs: number) {
  if (latencyBuffer.length >= MAX_LATENCY_BUFFER_SIZE) {
    latencyBuffer.shift()
  }
  latencyBuffer.push(latencyMs)
}

function calculateAverageLatency(): number {
  if (latencyBuffer.length === 0) return 0
  const sum = latencyBuffer.reduce((a, b) => a + b, 0)
  return sum / latencyBuffer.length
}

export function startLatencyCheck(intervalMs: number = 420) {
  setInterval(async () => {
    const slots = Array.from(receivedSlots.keys())

    for (const slot of slots) {
      const entryArray = receivedSlots.get(slot)
      if (!entryArray) {
        continue
      }
      const blockTime = await getCachedSlotTimestamp(slot)

      if (blockTime !== null) {
        const blockTimeMs = blockTime * 1000

        entryArray.forEach((entryData, idx) => {
          const receivedAtMs = entryData.receivedAt.getTime()
          const latencyMs = receivedAtMs - blockTimeMs - 500

          recordLatency(latencyMs)

          console.log(`Slot ${slot}, Entry #${idx + 1}`)
          console.log(`  ⏰ BlockTime: ${new Date(blockTimeMs).toISOString()}`)
          console.log(`  📥 ReceivedAt: ${entryData.receivedAt.toISOString()}`)
          console.log(`  🚀 Adjusted Latency: ${latencyMs} ms\n`)
        })

        receivedSlots.delete(slot)

        const avgLatency = calculateAverageLatency()
        console.log(
          `📊 Average Latency (last ${latencyBuffer.length} entries): ${avgLatency.toFixed(2)} ms\n`,
        )
      }
    }
  }, intervalMs)
}
