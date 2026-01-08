import { existsSync } from 'node:fs'

import { isEnabled } from '@/lib/env'

export interface RuntimeConfig {
  logMetrics: boolean
  logDrops: boolean
  logSubscriptions: boolean
  subscribeFile?: string
}

const resolveSubscribeFile = (): string | undefined => {
  const envValue = process.env.GEYSER_SUBSCRIBE_FILE?.trim()
  if (envValue) {
    return envValue
  }
  return existsSync('subscribe.json') ? 'subscribe.json' : undefined
}

export const loadRuntimeConfig = (): RuntimeConfig => ({
  logMetrics: isEnabled(process.env.GEYSER_LOG_METRICS, false),
  logDrops: isEnabled(process.env.GEYSER_LOG_DROPS, true),
  logSubscriptions: isEnabled(process.env.GEYSER_LOG_SUBSCRIBE, true),
  subscribeFile: resolveSubscribeFile(),
})
