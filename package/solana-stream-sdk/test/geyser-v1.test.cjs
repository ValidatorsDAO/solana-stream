const assert = require('node:assert/strict')
const { test } = require('node:test')
const { SubscribeUpdate } = require('@triton-one/yellowstone-grpc')

for (const config of [
  {},
  {
    priorityFee: '12345',
    computeUnitLimit: 200000,
    loadedAccountsDataSizeLimit: 64000,
    heapSize: 32768,
  },
]) {
  test(`Geyser preserves v1 config ${Object.keys(config).length ? 'values' : 'presence'}`, () => {
    const update = SubscribeUpdate.fromPartial({
      transaction: {
        slot: '123',
        transaction: {
          transaction: { message: { versioned: true, config } },
        },
      },
    })
    const decoded = SubscribeUpdate.decode(
      SubscribeUpdate.encode(update).finish(),
    )
    const actual = decoded.transaction.transaction.transaction.message.config
    assert.ok(actual, 'empty config must still distinguish v1 from v0')
    for (const [key, value] of Object.entries(config))
      assert.equal(actual[key], value)
  })
}
