import { CommitmentLevel } from '@validators-dao/solana-stream-sdk'

import { SubscribeRequest } from '@/utils/geyser'

export const getFallbackRequest = (): SubscribeRequest => ({
  accounts: {},
  slots: {},
  transactions: {},
  transactionsStatus: {},
  blocks: {},
  blocksMeta: {},
  entry: {},
  accountsDataSlice: [],
  commitment: CommitmentLevel.PROCESSED,
})
