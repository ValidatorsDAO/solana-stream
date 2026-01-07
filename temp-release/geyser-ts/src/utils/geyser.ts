import {
  CommitmentLevel,
  SubscribeRequestAccountsDataSlice,
  SubscribeRequestFilterAccounts,
  SubscribeRequestFilterBlocks,
  SubscribeRequestFilterBlocksMeta,
  SubscribeRequestFilterEntry,
  SubscribeRequestFilterSlots,
  SubscribeRequestFilterTransactions,
} from '@validators-dao/solana-stream-sdk'

export interface SubscribeRequest {
  accounts: {
    [key: string]: SubscribeRequestFilterAccounts
  }
  slots: {
    [key: string]: SubscribeRequestFilterSlots
  }
  transactions: {
    [key: string]: SubscribeRequestFilterTransactions
  }
  transactionsStatus: {
    [key: string]: SubscribeRequestFilterTransactions
  }
  blocks: {
    [key: string]: SubscribeRequestFilterBlocks
  }
  blocksMeta: {
    [key: string]: SubscribeRequestFilterBlocksMeta
  }
  entry: {
    [key: string]: SubscribeRequestFilterEntry
  }
  commitment?: CommitmentLevel | undefined
  accountsDataSlice: SubscribeRequestAccountsDataSlice[]
  ping?: any
  fromSlot?: string
}
