import { bs58 } from '@validators-dao/solana-stream-sdk'

import { receivedTransactions } from '@/utils/checkLatency'

export const trackTransactionLatency = (transactionUpdate: any) => {
  const slot = Number(transactionUpdate?.slot)
  if (!Number.isFinite(slot)) {
    return
  }
  const signature = transactionUpdate?.transaction?.signature
  if (!signature) {
    return
  }
  const receivedAt = new Date()
  const txSignature = bs58.encode(new Uint8Array(signature))

  if (!receivedTransactions.has(slot)) {
    receivedTransactions.set(slot, [])
  }
  receivedTransactions.get(slot)!.push({ receivedAt, tx: txSignature })
}
