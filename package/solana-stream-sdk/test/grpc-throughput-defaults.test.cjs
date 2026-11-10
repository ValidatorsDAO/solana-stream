const assert = require('node:assert/strict')
const { test } = require('node:test')

test('Geyser uses fixed 16 MiB flow-control windows by default', async () => {
  const { GeyserClient, HIGH_THROUGHPUT_GRPC_WINDOW_BYTES, DEFAULT_GEYSER_CHANNEL_OPTIONS } =
    await import('../dist/index.js')
  assert.equal(HIGH_THROUGHPUT_GRPC_WINDOW_BYTES, 16 * 1024 * 1024)
  assert.deepEqual(DEFAULT_GEYSER_CHANNEL_OPTIONS, {
    grpcInitialStreamWindowSize: HIGH_THROUGHPUT_GRPC_WINDOW_BYTES,
    grpcInitialConnectionWindowSize: HIGH_THROUGHPUT_GRPC_WINDOW_BYTES,
    grpcHttp2AdaptiveWindow: false,
  })
  // The upstream Client records channel options before connect(); no network access.
  const client = new GeyserClient('http://localhost:10000', undefined, undefined)
  assert.deepEqual(client._channelOptions, DEFAULT_GEYSER_CHANNEL_OPTIONS)
})

test('caller options override defaults without mutating their input', async () => {
  const { GeyserClient, geyserChannelOptions } = await import('../dist/index.js')
  const caller = { grpcInitialStreamWindowSize: 8 * 1024 * 1024, grpcHttp2AdaptiveWindow: true }
  const options = geyserChannelOptions(caller)
  assert.equal(options.grpcInitialStreamWindowSize, 8 * 1024 * 1024)
  assert.equal(options.grpcInitialConnectionWindowSize, 16 * 1024 * 1024)
  assert.equal(options.grpcHttp2AdaptiveWindow, true)
  assert.deepEqual(caller, {
    grpcInitialStreamWindowSize: 8 * 1024 * 1024,
    grpcHttp2AdaptiveWindow: true,
  })
  const client = new GeyserClient('http://localhost:10000', undefined, caller, {
    policy: 'Replay',
  })
  assert.deepEqual(client._channelOptions, options)
  assert.deepEqual(client._reconnectOptions, { policy: 'Replay' })
})
