import {
  GeyserClient,
  bs58,
  CommitmentLevel,
  SubscribeRequestFilterTransactions,
} from '@validators-dao/solana-stream-sdk'
import 'dotenv/config'
import { receivedTransactions, startLatencyCheck } from '@/utils/checkLatency'
import { SubscribeRequest } from '@/utils/geyser'
import { readFileSync, watchFile } from 'node:fs'
import { resolve as resolvePath } from 'node:path'

const isEnabled = (value: string | undefined, fallback: boolean): boolean => {
  if (value === undefined) {
    return fallback
  }
  const normalized = value.trim().toLowerCase()
  if (normalized === '') {
    return fallback
  }
  return ['1', 'true', 'yes', 'on'].includes(normalized)
}

// const PUMP_FUN_MINT_AUTHORITY = 'TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM'
const PUMP_FUN_PROGRAM_ID = '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'

const pumpfun: SubscribeRequestFilterTransactions = {
  vote: false,
  failed: false,
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
  const maxQueueSize = 10000
  const dropLogIntervalMs = 10000
  const metricsIntervalMs = 10000
  const logMetrics = isEnabled(process.env.GEYSER_LOG_METRICS, false)
  const logDrops = isEnabled(process.env.GEYSER_LOG_DROPS, true)
  const logSubscriptions = isEnabled(process.env.GEYSER_LOG_SUBSCRIBE, true)
  const subscribeFile = process.env.GEYSER_SUBSCRIBE_FILE?.trim()

  const updateQueue: Array<any | undefined> = new Array(maxQueueSize)
  let queueHead = 0
  let queueTail = 0
  let queueCount = 0
  const queueWaiters: Array<() => void> = []
  let droppedUpdates = 0
  let receivedUpdates = 0
  let processedUpdates = 0
  let lastDropLog = 0
  let lastSeenSlot = 0
  let lastReceivedAt = 0
  let lastProcessedAt = 0
  let lastMetricsReceived = 0
  let lastMetricsProcessed = 0
  let lastMetricsDropped = 0
  let lastSubscribeErrorLog = 0

  let currentRequest: SubscribeRequest = request
  let activeStream: any = null

  const toSlotNumber = (value: unknown): number | null => {
    if (typeof value !== 'string' && typeof value !== 'number') {
      return null
    }
    const slot = Number(value)
    return Number.isFinite(slot) ? slot : null
  }

  const getUpdateSlot = (update: any): number | null => {
    return (
      toSlotNumber(update?.transaction?.slot) ??
      toSlotNumber(update?.account?.slot) ??
      toSlotNumber(update?.slot?.slot) ??
      toSlotNumber(update?.block?.slot) ??
      toSlotNumber(update?.blockMeta?.slot) ??
      toSlotNumber(update?.entry?.slot)
    )
  }

  const enqueueUpdate = (update: any) => {
    if (queueCount >= maxQueueSize) {
      droppedUpdates += 1
      if (logDrops) {
        const now = Date.now()
        if (now - lastDropLog >= dropLogIntervalMs) {
          console.warn(
            `Dropping updates (queue full). dropped=${droppedUpdates}`,
          )
          lastDropLog = now
        }
      }
      return
    }
    updateQueue[queueTail] = update
    queueTail = (queueTail + 1) % maxQueueSize
    queueCount += 1
    const waiter = queueWaiters.shift()
    if (waiter) {
      waiter()
    }
  }

  const nextUpdate = async (): Promise<any | undefined> => {
    if (queueCount === 0) {
      await new Promise<void>((resolve) => queueWaiters.push(resolve))
    }
    if (queueCount === 0) {
      return undefined
    }
    const update = updateQueue[queueHead]
    updateQueue[queueHead] = undefined
    queueHead = (queueHead + 1) % maxQueueSize
    queueCount -= 1
    return update
  }

  const processUpdate = (data: any) => {
    const updateSlot = getUpdateSlot(data)
    if (updateSlot !== null && updateSlot > lastSeenSlot) {
      lastSeenSlot = updateSlot
    }
    processedUpdates += 1
    lastProcessedAt = Date.now()

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
  }

  const processUpdates = async (): Promise<void> => {
    while (true) {
      const update = await nextUpdate()
      if (update != undefined) {
        processUpdate(update)
      }
    }
  }

  if (logMetrics) {
    setInterval(() => {
      const receivedDelta = receivedUpdates - lastMetricsReceived
      const processedDelta = processedUpdates - lastMetricsProcessed
      const droppedDelta = droppedUpdates - lastMetricsDropped
      lastMetricsReceived = receivedUpdates
      lastMetricsProcessed = processedUpdates
      lastMetricsDropped = droppedUpdates

      const now = Date.now()
      const lastUpdateAge =
        lastReceivedAt > 0 ? now - lastReceivedAt : undefined
      const lastProcessedAge =
        lastProcessedAt > 0 ? now - lastProcessedAt : undefined
      const intervalSec = metricsIntervalMs / 1000
      const receivedRate = Math.round(receivedDelta / intervalSec)
      const processedRate = Math.round(processedDelta / intervalSec)
      const droppedRate = Math.round(droppedDelta / intervalSec)

      console.log(
        `metrics queue=${queueCount} receivedRate=${receivedRate}/s processedRate=${processedRate}/s droppedRate=${droppedRate}/s lastSeenSlot=${lastSeenSlot} lastUpdateMs=${lastUpdateAge ?? 'n/a'} lastProcessedMs=${lastProcessedAge ?? 'n/a'}`,
      )
    }, metricsIntervalMs)
  }

  void processUpdates()

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

  const buildSubscribeRequest = (
    baseRequest: SubscribeRequest,
    includeFromSlot: boolean,
  ): SubscribeRequest => {
    const nextRequest: SubscribeRequest = { ...baseRequest }
    const hasFromSlot =
      nextRequest.fromSlot !== undefined && nextRequest.fromSlot !== ''
    if (includeFromSlot && lastSeenSlot > 0 && !hasFromSlot) {
      nextRequest.fromSlot = String(Math.max(lastSeenSlot - 1, 0))
    }
    return nextRequest
  }

  const writeSubscription = async (
    stream: any,
    baseRequest: SubscribeRequest,
    includeFromSlot: boolean,
  ): Promise<void> => {
    const subscribeRequest = buildSubscribeRequest(
      baseRequest,
      includeFromSlot,
    )
    await new Promise<void>((resolve, reject) => {
      stream.write(subscribeRequest, (err: any) => {
        if (!err) {
          resolve()
        } else {
          console.error('Request error:', err)
          reject(err)
        }
      })
    })
  }

  const applySubscription = (nextRequest: SubscribeRequest, reason: string) => {
    currentRequest = nextRequest
    if (logSubscriptions) {
      console.log(`Subscription updated (${reason}).`)
    }
    if (activeStream) {
      void writeSubscription(activeStream, currentRequest, false).catch(
        (error) => {
          console.warn('Subscription update failed:', error)
        },
      )
    }
  }

  const normalizeRequest = (
    input: Partial<SubscribeRequest>,
  ): SubscribeRequest => ({
    ...request,
    ...input,
    accounts: input.accounts ?? request.accounts,
    slots: input.slots ?? request.slots,
    transactions: input.transactions ?? request.transactions,
    transactionsStatus: input.transactionsStatus ?? request.transactionsStatus,
    blocks: input.blocks ?? request.blocks,
    blocksMeta: input.blocksMeta ?? request.blocksMeta,
    entry: input.entry ?? request.entry,
    accountsDataSlice: input.accountsDataSlice ?? request.accountsDataSlice,
  })

  const loadSubscriptionFile = (filePath: string): SubscribeRequest | null => {
    try {
      const raw = readFileSync(filePath, 'utf8')
      const parsed = JSON.parse(raw) as Partial<SubscribeRequest>
      if (!parsed || typeof parsed !== 'object') {
        throw new Error('Invalid subscription JSON')
      }
      return normalizeRequest(parsed)
    } catch (error) {
      const now = Date.now()
      if (now - lastSubscribeErrorLog >= 10000) {
        console.warn('Failed to load subscription file:', filePath, error)
        lastSubscribeErrorLog = now
      }
      return null
    }
  }

  if (subscribeFile) {
    const filePath = resolvePath(subscribeFile)
    const applyFromFile = () => {
      const nextRequest = loadSubscriptionFile(filePath)
      if (nextRequest) {
        applySubscription(nextRequest, `file:${filePath}`)
      }
    }
    if (logSubscriptions) {
      console.log(`Watching subscription file: ${filePath}`)
    }
    applyFromFile()
    watchFile(filePath, { interval: 1000 }, (curr, prev) => {
      if (curr.mtimeMs !== prev.mtimeMs) {
        applyFromFile()
      }
    })
  }

  const connect = async (): Promise<void> => {
    let retries = 0

    while (retries <= maxRetries) {
      let sawNonHeartbeat = false
      try {
        const client = createClient()
        await client.connect()
        const version = await client.getVersion()
        console.log('version: ', version)
        const stream = await client.subscribe()
        activeStream = stream
        const sendPing = () => {
          stream.write({ ping: { id: 1 } }, (err: any) => {
            if (err) {
              console.warn('Ping response failed:', err)
            }
          })
        }

        stream.on('data', (data: any) => {
          if (data?.ping != undefined) {
            sendPing()
            return
          }
          if (data?.pong != undefined) {
            return
          }
          sawNonHeartbeat = true
          receivedUpdates += 1
          lastReceivedAt = Date.now()
          enqueueUpdate(data)
        })

        const streamClosed = new Promise<void>((_, reject) => {
          const handleClose = (error: Error) => {
            activeStream = null
            reject(error)
          }
          stream.on('error', (e: any) => handleClose(e))
          stream.on('end', () => handleClose(new Error('Stream ended')))
          stream.on('close', () => handleClose(new Error('Stream closed')))
        })

        await writeSubscription(stream, currentRequest, true)
        await streamClosed
      } catch (error) {
        activeStream = null
        retries = sawNonHeartbeat ? 0 : retries + 1
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
