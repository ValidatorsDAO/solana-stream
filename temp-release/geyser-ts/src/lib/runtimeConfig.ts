import { isEnabled } from '@/lib/env'

export interface RuntimeConfig {
  logMetrics: boolean
  logDrops: boolean
  logSubscriptions: boolean
}

export const loadRuntimeConfig = (): RuntimeConfig => ({
  logMetrics: isEnabled(process.env.GEYSER_LOG_METRICS, false),
  logDrops: isEnabled(process.env.GEYSER_LOG_DROPS, true),
  logSubscriptions: isEnabled(process.env.GEYSER_LOG_SUBSCRIBE, true),
})
