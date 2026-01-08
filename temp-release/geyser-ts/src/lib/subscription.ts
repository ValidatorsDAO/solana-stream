import { SubscribeRequest } from '@/utils/geyser'

export const buildSubscribeRequest = (
  baseRequest: SubscribeRequest,
  lastSeenSlot: number,
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

export const writeSubscription = async (
  stream: any,
  baseRequest: SubscribeRequest,
  lastSeenSlot: number,
  includeFromSlot: boolean,
): Promise<void> => {
  const subscribeRequest = buildSubscribeRequest(
    baseRequest,
    lastSeenSlot,
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
