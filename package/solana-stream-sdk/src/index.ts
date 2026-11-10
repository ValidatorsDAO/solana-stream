import Client, {
  ClientReconnectDuplexStream,
  CommitmentLevel,
  RECONNECT_FILTER_KEY,
  SubscribeRequestAccountsDataSlice,
  SubscribeRequestFilterAccounts,
  SubscribeRequestFilterBlockFooter,
  SubscribeRequestFilterBlocks,
  SubscribeRequestFilterBlocksMeta,
  SubscribeRequestFilterEntry,
  SubscribeRequestFilterSlots,
  SubscribeRequestFilterTransactions,
} from '@triton-one/yellowstone-grpc'
import bs58 from 'bs58'
export type {
  BankRef,
  ChannelOptions,
  ReconnectEvent,
  ReconnectOptions,
  SubscribeRequest,
  SubscribeUpdate,
} from '@triton-one/yellowstone-grpc'
export {
  bs58,
  ClientReconnectDuplexStream,
  CommitmentLevel,
  RECONNECT_FILTER_KEY,
  SubscribeRequestAccountsDataSlice,
  SubscribeRequestFilterAccounts,
  SubscribeRequestFilterBlockFooter,
  SubscribeRequestFilterBlocks,
  SubscribeRequestFilterBlocksMeta,
  SubscribeRequestFilterEntry,
  SubscribeRequestFilterSlots,
  SubscribeRequestFilterTransactions,
}

export {
  GeyserClient,
  geyserChannelOptions,
  DEFAULT_GEYSER_CHANNEL_OPTIONS,
  HIGH_THROUGHPUT_GRPC_WINDOW_BYTES,
} from './geyser-client'

import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
const { decodeSolanaEntries } = require('@validators-dao/solana-entry-decoder')

const { ShredsClient } = require('@validators-dao/solana-shreds-client')

enum ShredsClientCommitmentLevel {
  Processed = 'Processed',
  Finalized = 'Finalized',
  Confirmed = 'Confirmed',
}

export { decodeSolanaEntries, ShredsClient, ShredsClientCommitmentLevel }

export { classifyBlockMarker } from './block-marker'
export type { BlockMarkerKind } from './block-marker'
