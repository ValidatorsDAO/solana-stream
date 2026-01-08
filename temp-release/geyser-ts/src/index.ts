import 'dotenv/config'

import { trackTransactionLatency } from '@/handlers/latency'
import {
  logAccountUpdate,
  logTransactionSignature,
} from '@/handlers/logUpdate'
import { createEnvClient } from '@/lib/createClient'
import { runGeyser } from '@/lib/geyserRunner'
import { loadRuntimeConfig } from '@/lib/runtimeConfig'
import { startLatencyCheck } from '@/utils/checkLatency'
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

const main = async () => {
  try {
    await runGeyser({
      onUpdate,
      createClient: createEnvClient,
      request: getSubscribeRequest(),
      ...loadRuntimeConfig(),
    })
  } catch (error) {
    console.log(error)
  }
}

main()
// Checking Latency
startLatencyCheck()
