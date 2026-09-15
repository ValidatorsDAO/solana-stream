const assert = require('node:assert/strict')
const { createPrivateKey, createPublicKey, sign } = require('node:crypto')
const { test } = require('node:test')
const binding = process.env.SOLANA_ENTRY_DECODER_BINDING
const { decodeSolanaEntries } = binding ? require(binding) : require('../dist')

// Independent wire fixtures, signed with a deterministic test-only key.
// No RPC calls, production keys, or captured customer traffic are used.
const privateKey = createPrivateKey({
  key: Buffer.concat([
    Buffer.from('302e020100300506032b657004220420', 'hex'),
    Buffer.alloc(32, 7),
  ]),
  format: 'der',
  type: 'pkcs8',
})
const key = createPublicKey(privateKey)
  .export({ format: 'der', type: 'spki' })
  .subarray(-32)
const u64 = (value) => {
  const b = Buffer.alloc(8)
  b.writeBigUInt64LE(BigInt(value))
  return b
}
const u32 = (value) => {
  const b = Buffer.alloc(4)
  b.writeUInt32LE(value)
  return b
}
const hash = Buffer.alloc(32)
const header = Buffer.from([1, 0, 0])
const legacyMessage = Buffer.concat([
  header,
  Buffer.from([1]),
  key,
  hash,
  Buffer.from([0]),
])
const v0Message = Buffer.concat([
  Buffer.from([0x80]),
  legacyMessage,
  Buffer.from([0]),
])
const makeV1Message = (priorityFee) =>
  Buffer.concat([
    // Priority fee uses two mask bits; the other three settings use one each.
    Buffer.from([0x81]),
    header,
    u32(priorityFee === null ? 28 : 31),
    hash,
    Buffer.from([0, 1]),
    key,
    ...(priorityFee === null ? [] : [u64(priorityFee)]),
    u32(200000),
    u32(64000),
    u32(32768),
  ])
const v1Message = makeV1Message(12345n)
const oldTransaction = (message) =>
  Buffer.concat([Buffer.from([1]), sign(null, message, privateKey), message])
const v1Signature = sign(null, v1Message, privateKey)
const fixture = Buffer.concat([
  u64(1),
  u64(1),
  hash,
  u64(3),
  oldTransaction(legacyMessage),
  oldTransaction(v0Message),
  v1Message,
  v1Signature,
])

test('decodes signed legacy, v0 and v1 with unchanged entry JSON fields', () => {
  const entries = decodeSolanaEntries(fixture)
  assert.equal(entries.length, 1)
  assert.equal(entries[0].num_hashes, 1)
  assert.deepEqual(entries[0].hash, [...hash])
  const transactions = entries[0].transactions
  assert.equal(transactions.length, 3)
  assert.equal(typeof transactions[0].message[0], 'object')
  assert.equal(transactions[1].message[0], 0x80)
  assert.equal(transactions[2].message[0], 0x81)
  assert.deepEqual(Buffer.from(transactions[2].signatures[1]), v1Signature)
  assert.deepEqual(transactions[2].message[1].config, {
    priorityFee: '12345',
    computeUnitLimit: 200000,
    loadedAccountsDataSizeLimit: 64000,
    heapSize: 32768,
  })
})

test('preserves every v1 priority-fee integer as an exact decimal string', () => {
  for (const fee of [
    9007199254740993n,
    0n,
    9007199254740991n,
    9007199254740992n,
    9223372036854775807n,
    9223372036854775808n,
    18446744073709551615n,
  ]) {
    const message = makeV1Message(fee)
    const wire = Buffer.concat([
      u64(1),
      u64(1),
      hash,
      u64(1),
      message,
      sign(null, message, privateKey),
    ])
    const transaction = decodeSolanaEntries(wire)[0].transactions[0]
    const decodedFee = transaction.message[1].config.priorityFee
    assert.equal(BigInt(decodedFee), fee)
    assert.equal(decodedFee, fee.toString())
  }
})

test('keeps an unspecified v1 priority fee null', () => {
  const message = makeV1Message(null)
  const wire = Buffer.concat([
    u64(1),
    u64(1),
    hash,
    u64(1),
    message,
    sign(null, message, privateKey),
  ])
  const config = decodeSolanaEntries(wire)[0].transactions[0].message[1].config
  assert.equal(config.priorityFee, null)
  assert.equal(config.computeUnitLimit, 200000)
})

test('accepts ledger zero padding', () => {
  assert.deepEqual(
    decodeSolanaEntries(Buffer.concat([fixture, Buffer.alloc(64)])),
    decodeSolanaEntries(fixture),
  )
})

test('rejects empty, truncated and impossible-length input', () => {
  for (const bytes of [
    Buffer.alloc(0),
    fixture.subarray(0, fixture.length - 1),
    Buffer.alloc(8, 255),
  ]) {
    assert.throws(() => decodeSolanaEntries(bytes))
  }
})
