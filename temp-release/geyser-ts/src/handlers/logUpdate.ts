import { bs58 } from '@validators-dao/solana-stream-sdk'

export const logTransactionSignature = (transactionUpdate: any) => {
  const signature = transactionUpdate?.transaction?.signature
  if (!signature) {
    return
  }
  const txSignature = bs58.encode(new Uint8Array(signature))
  console.log('tx:', txSignature)
}

export const logAccountUpdate = (accountUpdate: any) => {
  const rawPubkey = accountUpdate?.account?.pubkey
  const rawTxnSignature = accountUpdate?.account?.txnSignature
  if (!rawPubkey || !rawTxnSignature) {
    return
  }
  const pubkey = bs58.encode(new Uint8Array(rawPubkey))
  const txnSignature = bs58.encode(new Uint8Array(rawTxnSignature))
  console.log('pubkey:', pubkey)
  console.log('txnSignature:', txnSignature)
}
