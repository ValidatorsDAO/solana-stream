import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  plaintextTokenWarning,
  redactError,
  redactUrls,
} from '../src/utils/redact.ts'

test('URLs in log text lose credentials, path and query', () => {
  const text =
    'rpc=https://user:pw@edge.erpc.global:8443/v1/TOKEN?api-key=SECRET#frag, ' +
    'then "http://grpc-ams1.erpc.global" and (HTTPS://Example.com/?k=SECRET2)'
  const redacted = redactUrls(text)
  for (const secret of ['pw', 'TOKEN', 'SECRET', 'frag']) {
    assert.ok(!redacted.includes(secret), `${secret} leaked: ${redacted}`)
  }
  assert.equal(
    redacted,
    'rpc=https://edge.erpc.global:8443/…, then "http://grpc-ams1.erpc.global" and (https://example.com/…)',
  )
  // An RPC error whose cause carries the request URL, as fetch-based clients report it.
  const error = new Error('HTTP error (401)', {
    cause: new Error('request to https://edge.erpc.global/?api-key=SECRET failed'),
  })
  assert.ok(!redactError(error).includes('SECRET'), redactError(error))
})

test('a token over plain http:// to another host is warned about', () => {
  const warning = plaintextTokenWarning('http://grpc-ams1.erpc.global', 'SECRET')
  assert.ok(warning?.includes('http://grpc-ams1.erpc.global'), String(warning))
  assert.ok(!warning?.includes('SECRET'))
  assert.equal(plaintextTokenWarning('https://grpc.example.com', 't'), null)
  assert.equal(plaintextTokenWarning('http://grpc-ams1.erpc.global', undefined), null)
  assert.equal(plaintextTokenWarning('http://127.0.0.1:10000', 't'), null)
})
