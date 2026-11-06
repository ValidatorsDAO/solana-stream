import type {
  BankRef,
  GeyserClient,
  ReconnectEvent,
} from '@validators-dao/solana-stream-sdk'

import { resourceExhaustedMessage } from '@/lib/grpcStatus'
import { createUpdateQueue } from '@/lib/updateQueue'
import { getUpdateSlot } from '@/lib/updateSlots'
import { withResumeSlot, writeRequest } from '@/lib/subscription'
import type { SubscribeRequest } from '@/utils/geyser'

const YIELD_EVERY = 1024

export type UpdateHandler = (update: any) => void | Promise<void>

/**
 * The connection dropped and the stream is reconnecting. Drop what you derived from
 * `partialBanks`; the replay from `replayFromSlot` resends the ones at or above it whole.
 */
export interface DisconnectedEvent {
  partialBanks: BankRef[]
  replayFromSlot: string | null
}

export type DisconnectedHandler = (
  event: DisconnectedEvent,
) => void | Promise<void>

export interface GeyserRunnerOptions {
  onUpdate: UpdateHandler
  onDisconnected?: DisconnectedHandler
  /** Builds a client; pass reconnect options to its constructor (see createClient.ts). */
  createClient: () => GeyserClient
  request: SubscribeRequest
  /** New subscriptions in a row, after the stream gave up, before runGeyser throws. */
  maxRetries?: number
  maxQueueSize?: number
  dropLogIntervalMs?: number
  metricsIntervalMs?: number
  logMetrics?: boolean
  logDrops?: boolean
  logSubscriptions?: boolean
}

/**
 * Streams with `subscribeWithReconnect()`: the stream reconnects by itself and replays from
 * the last confirmed slot. When it gives up (its retry budget is spent), the runner
 * subscribes again with `fromSlot` from the last slot it saw. Throws when the server
 * rejects the request with RESOURCE_EXHAUSTED, or after `maxRetries` failed subscriptions.
 */
export const runGeyser = async ({
  onUpdate,
  onDisconnected,
  createClient,
  request,
  maxRetries = 2000000,
  maxQueueSize = 10000,
  dropLogIntervalMs = 10000,
  metricsIntervalMs = 10000,
  logMetrics = false,
  logDrops = true,
  logSubscriptions = true,
}: GeyserRunnerOptions): Promise<void> => {
  console.log('Starting geyser client...')

  const eventQueue = createUpdateQueue<ReconnectEvent>(maxQueueSize)
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

  const currentRequest: SubscribeRequest = request

  const enqueueUpdate = (event: ReconnectEvent) => {
    if (!eventQueue.enqueue(event)) {
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
    }
  }

  const runHandler = (handler: () => void | Promise<void>) => {
    try {
      const result = handler()
      if (result && typeof (result as Promise<void>).catch === 'function') {
        ;(result as Promise<void>).catch((error) => {
          console.warn('Update handler failed:', error)
        })
      }
    } catch (error) {
      console.warn('Update handler failed:', error)
    }
  }

  const processEvent = (event: ReconnectEvent) => {
    if (event.type === 'Disconnected') {
      const { partialBanks, replayFromSlot } = event
      if (onDisconnected) {
        runHandler(() => onDisconnected({ partialBanks, replayFromSlot }))
      }
      return
    }
    const data = event.update
    const updateSlot = getUpdateSlot(data)
    if (updateSlot !== null && updateSlot > lastSeenSlot) {
      lastSeenSlot = updateSlot
    }
    processedUpdates += 1
    lastProcessedAt = Date.now()
    runHandler(() => onUpdate(data))
  }

  // Take what is queued without a promise per event, wait only when the queue is empty,
  // and yield to the event loop every YIELD_EVERY events so the stream keeps being read.
  // About 4x the throughput of awaiting next() per event (bench/queue.bench.ts).
  const processEvents = async (): Promise<void> => {
    let sinceYield = 0
    while (true) {
      const event = eventQueue.shift()
      if (event === undefined) {
        sinceYield = 0
        await eventQueue.wait()
        continue
      }
      processEvent(event)
      if (++sinceYield >= YIELD_EVERY) {
        sinceYield = 0
        await new Promise((resolve) => setImmediate(resolve))
      }
    }
  }

  void processEvents()

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
        `metrics queue=${eventQueue.size()} receivedRate=${receivedRate}/s processedRate=${processedRate}/s droppedRate=${droppedRate}/s lastSeenSlot=${lastSeenSlot} lastUpdateMs=${lastUpdateAge ?? 'n/a'} lastProcessedMs=${lastProcessedAge ?? 'n/a'}`,
      )
    }, metricsIntervalMs)
  }

  const sleep = (ms: number) =>
    new Promise((resolve) => setTimeout(resolve, ms))

  let retries = 0
  while (retries <= maxRetries) {
    let sawUpdates = false
    try {
      const client = createClient()
      await client.connect()
      const version = await client.getVersion()
      console.log('version: ', version)
      if (logSubscriptions) {
        console.log('Sending subscription request...')
      }
      // The server replays only from the first request of a stream (Yellowstone gRPC 17),
      // so the resume point goes into this one. The stream keeps track of it from here on.
      const stream = await client.subscribeWithReconnect(
        withResumeSlot(currentRequest, lastSeenSlot),
      )

      const streamClosed = new Promise<void>((_, reject) => {
        stream.on('error', (error: any) => reject(error))
        stream.on('end', () => reject(new Error('Stream ended')))
        stream.on('close', () => reject(new Error('Stream closed')))
      })

      stream.on('data', (event: ReconnectEvent) => {
        if (event.type === 'Disconnected') {
          console.warn(
            `Geyser connection dropped; reconnecting from slot ${event.replayFromSlot ?? 'tip'}, ${event.partialBanks.length} partial bank(s) to discard`,
          )
          if (!eventQueue.enqueueReserved(event)) {
            console.error('Dropping a Disconnected event: the queue is full')
          }
          return
        }
        const update = event.update as any
        if (update?.ping != undefined) {
          writeRequest(stream, { ping: { id: 1 } }).catch((error) =>
            console.warn('Ping response failed:', error),
          )
          return
        }
        if (update?.pong != undefined) {
          return
        }
        sawUpdates = true
        receivedUpdates += 1
        lastReceivedAt = Date.now()
        enqueueUpdate(event)
      })

      await streamClosed
    } catch (error) {
      const exhausted = resourceExhaustedMessage(error)
      if (exhausted !== null) {
        throw new Error(
          `The gRPC server ended the subscription with RESOURCE_EXHAUSTED: ${exhausted}. ` +
            'Not reconnecting: the server rejects the same request again. Send a smaller ' +
            'request (fewer filters, accounts or owners), or split it across connections.',
          { cause: error },
        )
      }
      retries = sawUpdates ? 0 : retries + 1
      if (retries > maxRetries) {
        throw error
      }
      console.error(`Connection failed. Retrying ...`, error)
      const delayMs = Math.min(1000 * 2 ** Math.min(retries, 5), 30000)
      await sleep(delayMs)
    }
  }
}
