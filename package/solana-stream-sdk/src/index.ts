import bs58 from 'bs58'
import Client from '@triton-one/yellowstone-grpc'
export { bs58, Client as GeyserClient }
export type {
  CommitmentLevel,
  SubscribeRequestAccountsDataSlice,
  SubscribeRequestFilterAccounts,
  SubscribeRequestFilterBlocks,
  SubscribeRequestFilterBlocksMeta,
  SubscribeRequestFilterEntry,
  SubscribeRequestFilterSlots,
  SubscribeRequestFilterTransactions,
} from '@triton-one/yellowstone-grpc'

export {
  CommitmentLevel as ShredsCommitmentLevel,
  ShredstreamProxyClient,
  ShredstreamClient,
  SubscribeEntriesRequest as ShredsSubscribeEntriesRequestFns,
  Entry as ShredsEntryFns,
} from './generated/shredstream'

export type {
  SubscribeEntriesRequest as ShredsSubscribeEntriesRequest,
  SubscribeRequestFilterAccounts as ShredsSubscribeRequestFilterAccounts,
  SubscribeRequestFilterTransactions as ShredsSubscribeRequestFilterTransactions,
  SubscribeRequestFilterSlots as ShredsSubscribeRequestFilterSlots,
  Entry as ShredsEntry,
} from './generated/shredstream'

export { credentials, Metadata } from '@grpc/grpc-js'

import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
const { decodeSolanaEntries } = require('@validators-dao/solana-entry-decoder')

export { decodeSolanaEntries }
