export interface UpdateQueue<T> {
  enqueue: (update: T) => boolean
  next: () => Promise<T | undefined>
  size: () => number
}

export const createUpdateQueue = <T>(maxQueueSize: number): UpdateQueue<T> => {
  const updateQueue: Array<T | undefined> = new Array(maxQueueSize)
  let queueHead = 0
  let queueTail = 0
  let queueCount = 0
  const queueWaiters: Array<() => void> = []

  const enqueue = (update: T): boolean => {
    if (queueCount >= maxQueueSize) {
      return false
    }
    updateQueue[queueTail] = update
    queueTail = (queueTail + 1) % maxQueueSize
    queueCount += 1
    const waiter = queueWaiters.shift()
    if (waiter) {
      waiter()
    }
    return true
  }

  const next = async (): Promise<T | undefined> => {
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

  const size = () => queueCount

  return { enqueue, next, size }
}
