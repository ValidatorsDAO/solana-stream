import { GeyserClient } from '@validators-dao/solana-stream-sdk'

import { plaintextTokenWarning, redactUrl } from '@/utils/redact'

let warnedMissingToken = false

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

  return new GeyserClient(endpoint, token || undefined, undefined)
}
