export interface UpdateQueue<T> {
  /** Adds an item unless `maxQueueSize` items are waiting; returns false when dropped. */
  enqueue: (item: T) => boolean
  /**
   * Adds an item that must not be dropped (a `Disconnected` event), using a small reserve
   * above `maxQueueSize`. Returns false only when the reserve is full as well.
   */
  enqueueReserved: (item: T) => boolean
  next: () => Promise<T | undefined>
  /** Takes the oldest item without waiting; undefined when the queue is empty. */
  shift: () => T | undefined
  /** Resolves when an item is queued (at once if one already is). */
  wait: () => Promise<void>
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

  const shift = (): T | undefined => {
    if (queueCount === 0) {
      return undefined
    }
    const item = queue[queueHead]
    queue[queueHead] = undefined
    queueHead = (queueHead + 1) % capacity
    queueCount -= 1
    return item
  }

  const wait = (): Promise<void> =>
    queueCount > 0
      ? Promise.resolve()
      : new Promise<void>((resolve) => queueWaiters.push(resolve))

  const next = async (): Promise<T | undefined> => {
    await wait()
    return shift()
  }

  const size = () => queueCount

  return { enqueue, enqueueReserved, next, shift, wait, size }
}
