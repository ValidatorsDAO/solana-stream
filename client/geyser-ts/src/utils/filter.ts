import {
  CommitmentLevel,
  SubscribeRequestFilterTransactions,
} from '@validators-dao/solana-stream-sdk'

import { SubscribeRequest } from '@/utils/geyser'

const PUMP_FUN_PROGRAM_ID = '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'

const pumpfun: SubscribeRequestFilterTransactions = {
  vote: false,
  failed: false,
  accountInclude: [PUMP_FUN_PROGRAM_ID],
  accountExclude: [],
  accountRequired: [],
}

export const getSubscribeRequest = (): SubscribeRequest => ({
  accounts: {},
  slots: {},
  transactions: { pumpfun },
  transactionsStatus: {},
  blocks: {},
  blocksMeta: {},
  entry: {},
  accountsDataSlice: [],
  commitment: CommitmentLevel.PROCESSED,
})
