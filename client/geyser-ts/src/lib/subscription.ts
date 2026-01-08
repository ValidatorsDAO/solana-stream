import { readFileSync } from 'node:fs'

import { getFallbackRequest } from '@/utils/fallback'
import { SubscribeRequest } from '@/utils/geyser'

export const normalizeRequest = (
  input: Partial<SubscribeRequest>,
): SubscribeRequest => {
  const baseRequest = getFallbackRequest()
  return {
    ...baseRequest,
    ...input,
    accounts: input.accounts ?? baseRequest.accounts,
    slots: input.slots ?? baseRequest.slots,
    transactions: input.transactions ?? baseRequest.transactions,
    transactionsStatus: input.transactionsStatus ?? baseRequest.transactionsStatus,
    blocks: input.blocks ?? baseRequest.blocks,
    blocksMeta: input.blocksMeta ?? baseRequest.blocksMeta,
    entry: input.entry ?? baseRequest.entry,
    accountsDataSlice: input.accountsDataSlice ?? baseRequest.accountsDataSlice,
  }
}

export const loadSubscriptionFile = (filePath: string): SubscribeRequest => {
  const raw = readFileSync(filePath, 'utf8')
  const parsed = JSON.parse(raw) as Partial<SubscribeRequest>
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Invalid subscription JSON')
  }
  return normalizeRequest(parsed)
}

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
