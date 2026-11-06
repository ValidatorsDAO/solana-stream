import { GeyserClient } from '@validators-dao/solana-stream-sdk'
import type { ReconnectOptions } from '@validators-dao/solana-stream-sdk'

import { plaintextTokenWarning, redactUrl } from '@/utils/redact'

let warnedMissingToken = false

/**
 * How `subscribeWithReconnect()` reconnects: up to 6 reconnects in a row after 100 ms,
 * 200 ms, … 3.2 s (about 6 s in all), replaying from the last confirmed slot. A
 * connection that delivers a block refills the budget. When it is spent, runGeyser
 * subscribes again.
 */
export const RECONNECT_OPTIONS: ReconnectOptions = {
  backoff: { initialIntervalMs: 100, multiplier: 2, maxRetries: 6 },
  policy: 'Replay',
  slotsBehind: 0,
}

export const createEnvClient = () => {
  const token = process.env.X_TOKEN?.trim()
  if (!token && !warnedMissingToken) {
    console.warn('X_TOKEN not set. Connecting without auth.')
    warnedMissingToken = true
  }
  const endpoint = process.env.GEYSER_ENDPOINT || 'http://localhost:10000'
  console.log('Connecting to', redactUrl(endpoint))
  const warning = plaintextTokenWarning(endpoint, token)
  if (warning) {
    console.warn(warning)
  }

  return new GeyserClient(endpoint, token || undefined, undefined, RECONNECT_OPTIONS)
}
