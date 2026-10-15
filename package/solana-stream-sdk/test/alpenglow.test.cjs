const assert = require('node:assert/strict')
const { test } = require('node:test')
const { SubscribeUpdate } = require('@triton-one/yellowstone-grpc')

const roundTrip = (partial) =>
  SubscribeUpdate.decode(
    SubscribeUpdate.encode(SubscribeUpdate.fromPartial(partial)).finish(),
  )

test('Geyser keeps Alpenglow block footer and update-parent fields', () => {
  const bankHash = Uint8Array.from({ length: 32 }, (_, i) => i + 1)
  const footer = roundTrip({
    blockFooter: { slot: '456', bankId: '7', bankHash },
  }).blockFooter
  assert.ok(footer, 'blockFooter must survive the round trip')
  assert.equal(footer.slot, '456')
  assert.equal(footer.bankId, '7')
  assert.deepEqual(Array.from(footer.bankHash), Array.from(bankHash))

  const parent = roundTrip({
    entryUpdateParent: { slot: '456', clearedBankId: '5', parentSlot: '455' },
  }).entryUpdateParent
  assert.ok(parent, 'entryUpdateParent must survive the round trip')
  assert.equal(parent.clearedBankId, '5')
  assert.equal(parent.parentSlot, '455')

  const slot = roundTrip({ slot: { slot: '456', status: 0, bankId: '7' } }).slot
  assert.ok(slot, 'slot update must survive the round trip')
  assert.equal(slot.bankId, '7')
})
