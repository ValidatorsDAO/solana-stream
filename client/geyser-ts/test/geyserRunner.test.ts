import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { test } from 'node:test'

import { runGeyser } from '../src/lib/geyserRunner.ts'

const SERVER_MESSAGE =
  'request too large: 70000 bytes exceeds max_decoding_message_size of 65536 bytes'

const REQUEST = {
  accounts: {},
  slots: { slots: {} },
  transactions: {},
  transactionsStatus: {},
  blocks: {},
  blocksMeta: {},
  entry: {},
  blockFooter: {},
  accountsDataSlice: [],
}

// The error @triton-one/yellowstone-grpc 10.0.0 emits from subscribeWithReconnect() when
// the server ends the stream with RESOURCE_EXHAUSTED and the retry budget is spent, as
// captured from the native client against a tonic server: an 'error' with the tonic
// status text in `cause`, then 'close'.
const napiResourceExhausted = () => {
  const cause = Object.assign(
    new Error(
      `code: 'Some resource has been exhausted', message: ${JSON.stringify(SERVER_MESSAGE)}`,
    ),
    { code: 'GenericFailure' },
  )
  return Object.assign(
    new Error('reconnect stream receive failed', { cause }),
    { code: 'GenericFailure' },
  )
}

const disconnected = (partialBanks: Array<{ slot: string; bankId: string }>) => ({
  type: 'Disconnected',
  partialBanks,
  replayFromSlot: null,
})

const accountUpdate = (slot: string) => ({
  type: 'Update',
  update: { filters: ['accounts'], account: { slot, account: {} } },
})

/** A reconnect stream that emits `events` (or an error) once listeners are attached. */
class ScriptedStream extends EventEmitter {
  constructor(events: unknown[], error?: Error) {
    super()
    setImmediate(() => {
      for (const event of events) this.emit('data', event)
      if (error) {
        this.emit('error', error)
        this.emit('close')
      }
    })
  }
  write(_request: unknown, callback: (error?: unknown) => void) {
    callback()
    return true
  }
}

const fakeClient = (stream: () => ScriptedStream) =>
  ({
    connect: async () => {},
    getVersion: async () => ({ version: 'test' }),
    subscribeWithReconnect: async () => stream(),
  }) as any

test('RESOURCE_EXHAUSTED stops the runner with the server message', async () => {
  let clients = 0
  const createClient = () => {
    clients += 1
    // The stream retries within its budget (two Disconnected events), then fails.
    return fakeClient(
      () =>
        new ScriptedStream(
          [disconnected([]), disconnected([])],
          napiResourceExhausted(),
        ),
    )
  }

  const outcome = await Promise.race([
    runGeyser({
      onUpdate: () => {},
      createClient,
      request: REQUEST,
      maxRetries: 1,
      logSubscriptions: false,
    }).then(
      () => 'returned',
      (error: unknown) => error,
    ),
    // A runner that subscribes again does so after its first backoff (2 s).
    new Promise((resolve) => setTimeout(resolve, 1000, 'still retrying')),
  ])

  assert.equal(clients, 1, 'the runner subscribed again after RESOURCE_EXHAUSTED')
  assert.ok(
    outcome instanceof Error,
    `the runner did not stop on RESOURCE_EXHAUSTED: ${String(outcome)}`,
  )
  assert.match(outcome.message, /RESOURCE_EXHAUSTED/)
  assert.ok(
    outcome.message.includes(SERVER_MESSAGE),
    `the runner did not report the server message: ${outcome.message}`,
  )
})

test('Disconnected reaches onDisconnected in order, even when the queue is full', async () => {
  const seen: string[] = []
  const done = new Promise<void>((resolve) => {
    void runGeyser({
      onUpdate: (update) => {
        seen.push(`update:${update.account.slot}`)
      },
      onDisconnected: ({ partialBanks }) => {
        seen.push(
          `disconnected:${partialBanks.map((b) => `${b.slot}/${b.bankId}`).join(',')}`,
        )
        resolve()
      },
      // Captured from 10.0.0 for a connection dropped after one account write of bank
      // (42, 7). With a queue of one, the second update is dropped; the Disconnected
      // event after it must not be.
      createClient: () =>
        fakeClient(
          () =>
            new ScriptedStream([
              accountUpdate('41'),
              accountUpdate('42'),
              disconnected([{ slot: '42', bankId: '7' }]),
            ]),
        ),
      request: REQUEST,
      maxQueueSize: 1,
      logDrops: false,
      logSubscriptions: false,
    })
  })

  await Promise.race([
    done,
    new Promise((resolve) => setTimeout(resolve, 1000)),
  ])
  assert.deepEqual(seen, ['update:41', 'disconnected:42/7'])
})
