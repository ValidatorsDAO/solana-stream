const assert = require('node:assert/strict')
const { test } = require('node:test')

// The classifier is pure TypeScript with no native dependency, so the test loads
// the source directly (Node type stripping) instead of dist/index.js.
const load = () => import('../src/block-marker.ts')

const markerPayload = (variant, inner, padTo = 0) => {
  const bytes = [
    ...new Array(8).fill(0),
    1,
    0,
    variant,
    inner.length & 0xff,
    inner.length >> 8,
    ...inner,
  ]
  while (bytes.length < padTo) bytes.push(0)
  return Uint8Array.from(bytes)
}

const footerBody = () => [
  1,
  ...new Array(32).fill(0x33),
  0,
  0,
  0x65,
  0xcd,
  0x1d,
  0,
  0,
  0,
  5,
  ...Buffer.from('agave'),
  0,
  0,
  0,
]
const parentBody = () => [
  1,
  ...new Array(8).fill(7),
  ...new Array(32).fill(0x22),
]
const headerBody = () => [
  1,
  ...new Array(8).fill(9),
  ...new Array(32).fill(0x11),
]

test('classifyBlockMarker separates markers from entry batches', async () => {
  const { classifyBlockMarker } = await load()

  assert.equal(classifyBlockMarker(markerPayload(0, footerBody())), 'BlockFooter')
  assert.equal(classifyBlockMarker(markerPayload(1, headerBody())), 'BlockHeader')
  assert.equal(
    classifyBlockMarker(markerPayload(2, parentBody())),
    'UpdateParent',
  )
  assert.equal(
    classifyBlockMarker(markerPayload(3, [1, 0, 0, 0])),
    'GenesisCertificate',
  )

  // Zero padding after the declared length keeps the kind.
  const paddedFooter = markerPayload(0, footerBody(), 30816)
  assert.equal(paddedFooter.length, 30816)
  assert.equal(classifyBlockMarker(paddedFooter), 'BlockFooter')
  const paddedParent = markerPayload(2, parentBody(), 30816)
  assert.equal(paddedParent.length, 30816)
  assert.equal(classifyBlockMarker(paddedParent), 'UpdateParent')

  // Empty batches (aborted slot) and entry batches are not markers.
  assert.equal(classifyBlockMarker(new Uint8Array(8)), null)
  assert.equal(classifyBlockMarker(new Uint8Array(72)), null)
  assert.equal(classifyBlockMarker(new Uint8Array(3)), null)
  assert.equal(
    classifyBlockMarker(Uint8Array.from([2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0])),
    null,
  )

  // Not a well-formed V1 marker, or an unknown variant.
  const badVersion = markerPayload(0, footerBody())
  badVersion[8] = 2
  assert.equal(classifyBlockMarker(badVersion), 'Unknown')
  assert.equal(classifyBlockMarker(markerPayload(9, [1, 2, 3])), 'Unknown')
  const trailing = markerPayload(0, footerBody(), 200)
  trailing[199] = 1
  assert.equal(classifyBlockMarker(trailing), 'Unknown')
  const shortBody = markerPayload(0, footerBody())
  assert.equal(classifyBlockMarker(shortBody.subarray(0, 20)), 'Unknown')
})
