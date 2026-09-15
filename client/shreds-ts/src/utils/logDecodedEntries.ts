import { bs58 } from '@validators-dao/solana-stream-sdk'

export function logDecodedEntries(decodedEntries: any) {
  if (!Array.isArray(decodedEntries)) {
    console.warn('⚠️ decodedEntries is not an array:', decodedEntries)
    return
  }

  decodedEntries.forEach((entry: any, entryIdx: number) => {
    console.log(`\n✅ Entry #${entryIdx + 1}`)
    console.log(
      `  - Hash: ${entry.hash ? bs58.encode(Buffer.from(entry.hash)) : 'N/A'}`,
    )
    console.log(`  - Num Hashes: ${entry.num_hashes ?? 'N/A'}`)

    if (!Array.isArray(entry.transactions)) {
      console.warn('⚠️ transactions is not an array:', entry.transactions)
      return
    }

    entry.transactions.forEach((tx: any, txIdx: number) => {
      console.log(`\n📄 Transaction #${txIdx + 1}`)

      if (!tx || !tx.message || !Array.isArray(tx.message)) {
        console.warn('⚠️ Invalid transaction structure:', tx)
        return
      }

      const signaturesBase58 = Array.isArray(tx.signatures)
        ? tx.signatures
            .slice(1)
            .map((sig: number[]) => bs58.encode(Buffer.from(sig)))
        : []

      console.log(`  - Signatures:`, signaturesBase58)

      const prefix = tx.message[0]
      const version = typeof prefix === 'number' ? prefix - 0x80 : 'legacy'
      const message = typeof prefix === 'number' ? tx.message[1] : prefix
      console.log(`  - Version: ${version}`)
      if (version === 1) console.log('  - Transaction config:', message?.config)

      if (message) {
        if (Array.isArray(message.accountKeys)) {
          console.log(`  🔑 Account Keys:`)
          message.accountKeys.forEach((key: number[], idx: number) => {
            if (Array.isArray(key)) {
              console.log(`    [${idx}] ${bs58.encode(Buffer.from(key))}`)
            } else {
              console.warn(`    [${idx}] Invalid key format:`, key)
            }
          })
        } else {
          console.warn(
            '⚠️ accountKeys is undefined or not an array:',
            message.accountKeys,
          )
        }

        if (Array.isArray(message.instructions)) {
          console.log(`  ⚙️ Instructions:`)
          message.instructions.forEach((inst: any, instIdx: number) => {
            console.log(`    [${instIdx}]`)
            console.log(
              `      - Program ID Index: ${inst.programIdIndex ?? 'N/A'}`,
            )
            console.log(
              `      - Accounts: ${Array.isArray(inst.accounts) ? inst.accounts.join(', ') : 'N/A'}`,
            )
            console.log(
              `      - Data: ${inst.data ? bs58.encode(Buffer.from(inst.data)) : 'N/A'}`,
            )
          })
        } else {
          console.warn(
            '⚠️ instructions is undefined or not an array:',
            message.instructions,
          )
        }

        console.log(
          `  📌 Recent Blockhash: ${(message.recentBlockhash ?? message.lifetimeSpecifier) ? bs58.encode(Buffer.from(message.recentBlockhash ?? message.lifetimeSpecifier)) : 'N/A'}`,
        )
      } else {
        console.warn('⚠️ transaction message is undefined:', tx.message)
      }
    })
  })
}
