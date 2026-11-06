import type { SubscribeRequest } from '@/utils/geyser'

/**
 * `request` with `fromSlot` one slot before `lastSeenSlot`, so a new subscription resumes
 * where the last one stopped. A `fromSlot` set in `request` is kept.
 */
export const withResumeSlot = (
  request: SubscribeRequest,
  lastSeenSlot: number,
): SubscribeRequest => {
  const hasFromSlot = request.fromSlot !== undefined && request.fromSlot !== ''
  if (lastSeenSlot <= 0 || hasFromSlot) {
    return request
  }
  return { ...request, fromSlot: String(Math.max(lastSeenSlot - 1, 0)) }
}

/** Writes a request (a filter change or a ping) to an open stream. */
export const writeRequest = (
  stream: { write: (chunk: unknown, callback: (error?: unknown) => void) => unknown },
  request: Partial<SubscribeRequest>,
): Promise<void> =>
  new Promise<void>((resolve, reject) => {
    stream.write(request, (error) => (error ? reject(error) : resolve()))
  })
