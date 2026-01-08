import { GeyserClient } from '@validators-dao/solana-stream-sdk'
import { watchFile } from 'node:fs'
import { resolve as resolvePath } from 'node:path'

import { getFallbackRequest } from '@/utils/fallback'
import { SubscribeRequest } from '@/utils/geyser'
import { createUpdateQueue } from '@/lib/updateQueue'
import { getUpdateSlot } from '@/lib/updateSlots'
import { loadSubscriptionFile, writeSubscription } from '@/lib/subscription'

export type UpdateHandler = (update: any) => void | Promise<void>

export interface GeyserRunnerOptions {
  onUpdate: UpdateHandler
  createClient: () => GeyserClient
  maxRetries?: number
  maxQueueSize?: number
  dropLogIntervalMs?: number
  metricsIntervalMs?: number
  logMetrics?: boolean
  logDrops?: boolean
  logSubscriptions?: boolean
  subscribeFile?: string
}

export const runGeyser = async ({
  onUpdate,
  createClient,
  maxRetries = 2000000,
  maxQueueSize = 10000,
  dropLogIntervalMs = 10000,
  metricsIntervalMs = 10000,
  logMetrics = false,
  logDrops = true,
  logSubscriptions = true,
  subscribeFile,
}: GeyserRunnerOptions): Promise<void> => {
  console.log('Starting geyser client...')

  const updateQueue = createUpdateQueue<any>(maxQueueSize)
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

  let currentRequest: SubscribeRequest = getFallbackRequest()
  let activeStream: any = null

  const enqueueUpdate = (update: any) => {
    if (!updateQueue.enqueue(update)) {
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

  const processUpdate = (data: any) => {
    const updateSlot = getUpdateSlot(data)
    if (updateSlot !== null && updateSlot > lastSeenSlot) {
      lastSeenSlot = updateSlot
    }
    processedUpdates += 1
    lastProcessedAt = Date.now()

    try {
      const result = onUpdate(data)
      if (result && typeof (result as Promise<void>).catch === 'function') {
        ;(result as Promise<void>).catch((error) => {
          console.warn('Update handler failed:', error)
        })
      }
    } catch (error) {
      console.warn('Update handler failed:', error)
    }
  }

  const processUpdates = async (): Promise<void> => {
    while (true) {
      const update = await updateQueue.next()
      if (update != undefined) {
        processUpdate(update)
      }
    }
  }

  void processUpdates()

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
        `metrics queue=${updateQueue.size()} receivedRate=${receivedRate}/s processedRate=${processedRate}/s droppedRate=${droppedRate}/s lastSeenSlot=${lastSeenSlot} lastUpdateMs=${lastUpdateAge ?? 'n/a'} lastProcessedMs=${lastProcessedAge ?? 'n/a'}`,
      )
    }, metricsIntervalMs)
  }

  const applySubscription = (nextRequest: SubscribeRequest, reason: string) => {
    currentRequest = nextRequest
    if (logSubscriptions) {
      console.log(`Subscription updated (${reason}).`)
    }
    if (activeStream) {
      void writeSubscription(activeStream, currentRequest, lastSeenSlot, false).catch(
        (error) => {
          console.warn('Subscription update failed:', error)
        },
      )
    }
  }

  if (subscribeFile) {
    const filePath = resolvePath(subscribeFile)
    const applyFromFile = () => {
      try {
        const nextRequest = loadSubscriptionFile(filePath)
        applySubscription(nextRequest, `file:${filePath}`)
      } catch (error) {
        const now = Date.now()
        if (now - lastSubscribeErrorLog >= 10000) {
          console.warn('Failed to load subscription file:', filePath, error)
          lastSubscribeErrorLog = now
        }
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

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

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

        await writeSubscription(stream, currentRequest, lastSeenSlot, true)
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
