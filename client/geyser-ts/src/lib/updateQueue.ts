export interface UpdateQueue<T> {
  /** Adds an item unless `maxQueueSize` items are waiting; returns false when dropped. */
  enqueue: (item: T) => boolean
  /**
   * Adds an item that must not be dropped (a `Disconnected` event), using a small reserve
   * above `maxQueueSize`. Returns false only when the reserve is full as well.
   */
  enqueueReserved: (item: T) => boolean
  next: () => Promise<T | undefined>
  size: () => number
}

const RESERVED_SLOTS = 64

export const createUpdateQueue = <T>(maxQueueSize: number): UpdateQueue<T> => {
  const capacity = maxQueueSize + RESERVED_SLOTS
  const queue: Array<T | undefined> = new Array(capacity)
  let queueHead = 0
  let queueTail = 0
  let queueCount = 0
  const queueWaiters: Array<() => void> = []

  const push = (item: T, limit: number): boolean => {
    if (queueCount >= limit) {
      return false
    }
    queue[queueTail] = item
    queueTail = (queueTail + 1) % capacity
    queueCount += 1
    const waiter = queueWaiters.shift()
    if (waiter) {
      waiter()
    }
    return true
  }

  const enqueue = (item: T): boolean => push(item, maxQueueSize)
  const enqueueReserved = (item: T): boolean => push(item, capacity)

  const next = async (): Promise<T | undefined> => {
    if (queueCount === 0) {
      await new Promise<void>((resolve) => queueWaiters.push(resolve))
    }
    if (queueCount === 0) {
      return undefined
    }
    const item = queue[queueHead]
    queue[queueHead] = undefined
    queueHead = (queueHead + 1) % capacity
    queueCount -= 1
    return item
  }

  const size = () => queueCount

  return { enqueue, enqueueReserved, next, size }
}
