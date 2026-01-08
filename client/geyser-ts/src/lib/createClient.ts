import { GeyserClient } from '@validators-dao/solana-stream-sdk'

let warnedMissingToken = false

export const createEnvClient = () => {
  const token = process.env.X_TOKEN?.trim()
  if (!token && !warnedMissingToken) {
    console.warn('X_TOKEN not set. Connecting without auth.')
    warnedMissingToken = true
  }
  const endpoint = process.env.GEYSER_ENDPOINT || 'http://localhost:10000'
  console.log('Connecting to', endpoint)

  return new GeyserClient(endpoint, token || undefined, undefined)
}
