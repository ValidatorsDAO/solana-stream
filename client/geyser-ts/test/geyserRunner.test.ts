import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { test } from 'node:test'

import { runGeyser } from '../src/lib/geyserRunner.ts'

const SERVER_MESSAGE =
  'request too large: 70000 bytes exceeds max_decoding_message_size of 65536 bytes'

// What @triton-one/yellowstone-grpc 9.0.0 emits when the server ends the stream with
// RESOURCE_EXHAUSTED, as captured from the native client against a tonic server: an
// 'error' with the tonic status text in `cause`, then 'close'.
const napiResourceExhausted = () => {
  const cause = Object.assign(
    new Error(
      `code: 'Some resource has been exhausted', message: ${JSON.stringify(SERVER_MESSAGE)}`,
    ),
    { code: 'GenericFailure' },
  )
  return Object.assign(
    new Error('subscribe stream receive failed', { cause }),
    { code: 'GenericFailure' },
  )
}

class ExhaustedStream extends EventEmitter {
  write(_request: unknown, callback: (error: Error | null) => void) {
    callback(null)
    setImmediate(() => {
      this.emit('error', napiResourceExhausted())
      this.emit('close')
    })
    return true
  }
}

test('RESOURCE_EXHAUSTED stops the runner with the server message', async () => {
  let clients = 0
  const createClient = () => {
    clients += 1
    return {
      connect: async () => {},
      getVersion: async () => ({ version: 'test' }),
      subscribe: async () => new ExhaustedStream(),
    } as any
  }

  const outcome = await Promise.race([
    runGeyser({
      onUpdate: () => {},
      createClient,
      request: {
        accounts: {},
        slots: { slots: {} },
        transactions: {},
        transactionsStatus: {},
        blocks: {},
        blocksMeta: {},
        entry: {},
        accountsDataSlice: [],
      },
      maxRetries: 1,
      logSubscriptions: false,
    }).then(
      () => 'returned',
      (error: unknown) => error,
    ),
    // A runner that retries reconnects after its first backoff (2 s).
    new Promise((resolve) => setTimeout(resolve, 1000, 'still retrying')),
  ])

  assert.equal(clients, 1, 'the runner reconnected after RESOURCE_EXHAUSTED')
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
