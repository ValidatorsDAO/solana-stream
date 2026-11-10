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
import type { ChannelOptions, ReconnectOptions } from '@triton-one/yellowstone-grpc'
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

// A fixed 16 MiB window on BOTH levels avoids a client-side HTTP/2
// throughput ceiling on high-volume streams. Adaptive mode ignores the fixed
// sizes, so keep it off unless the caller explicitly opts back in.
export const HIGH_THROUGHPUT_GRPC_WINDOW_BYTES = 16 * 1024 * 1024

export const DEFAULT_GEYSER_CHANNEL_OPTIONS: Readonly<ChannelOptions> = {
  grpcInitialStreamWindowSize: HIGH_THROUGHPUT_GRPC_WINDOW_BYTES,
  grpcInitialConnectionWindowSize: HIGH_THROUGHPUT_GRPC_WINDOW_BYTES,
  grpcHttp2AdaptiveWindow: false,
}

/** Return independent channel options. Overrides win; the caller's object is not mutated. */
export function geyserChannelOptions(overrides?: ChannelOptions): ChannelOptions {
  return { ...DEFAULT_GEYSER_CHANNEL_OPTIONS, ...overrides }
}

/**
 * Yellowstone client with throughput-oriented HTTP/2 receive windows.
 * The constructor is compatible with the upstream Client, including the
 * optional fourth argument for reconnect configuration.
 */
export class GeyserClient extends Client {
  constructor(
    endpoint: string,
    xToken?: string,
    channelOptions?: ChannelOptions,
    reconnectOptions?: ReconnectOptions,
  ) {
    super(endpoint, xToken, geyserChannelOptions(channelOptions), reconnectOptions)
  }
}

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
