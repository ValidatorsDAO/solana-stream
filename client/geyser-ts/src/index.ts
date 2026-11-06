import 'dotenv/config'

import { trackTransactionLatency } from '@/handlers/latency'
import {
  logAccountUpdate,
  logTransactionSignature,
} from '@/handlers/logUpdate'
import { createEnvClient } from '@/lib/createClient'
import { DisconnectedEvent, runGeyser } from '@/lib/geyserRunner'
import { loadRuntimeConfig } from '@/lib/runtimeConfig'
import { receivedTransactions, startLatencyCheck } from '@/utils/checkLatency'
import { getSubscribeRequest } from '@/utils/filter'

const onTransaction = (transactionUpdate: any) => {
  trackTransactionLatency(transactionUpdate)

  // TODO: Add your trade logic here. Split it into src/handlers if it grows.
  logTransactionSignature(transactionUpdate)
}

const onAccount = (accountUpdate: any) => {
  // TODO: Add your account-based logic here.
  logAccountUpdate(accountUpdate)
}

const onUpdate = (update: any) => {
  if (update.transaction != undefined) {
    onTransaction(update.transaction)
    return
  }
  if (update.account != undefined) {
    onAccount(update.account)
  }
}

// The connection dropped while these banks were in flight: you hold part of their content.
// Drop what you derived from them; the replay resends them whole. A replay also resends
// banks you already hold whole, so keep your handling idempotent.
const onDisconnected = ({ partialBanks }: DisconnectedEvent) => {
  for (const bank of partialBanks) {
    receivedTransactions.delete(Number(bank.slot))
  }
}

const main = async () => {
  try {
    await runGeyser({
      onUpdate,
      onDisconnected,
      createClient: createEnvClient,
      request: getSubscribeRequest(),
      ...loadRuntimeConfig(),
    })
  } catch (error) {
    // runGeyser throws only on an error that reconnecting cannot fix.
    console.error(error)
    process.exit(1)
  }
}

main()
// Checking Latency
startLatencyCheck()
