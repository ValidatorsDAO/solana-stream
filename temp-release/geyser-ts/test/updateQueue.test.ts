import assert from 'node:assert/strict'
import { test } from 'node:test'

import { createUpdateQueue } from '../src/lib/updateQueue.ts'

test('the queue hands items out in order, waits when empty and keeps a reserve', async () => {
  const queue = createUpdateQueue<number>(2)
  assert.equal(queue.shift(), undefined)

  let woke = false
  const waiting = queue.wait().then(() => {
    woke = true
  })
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(woke, false, 'wait() resolved on an empty queue')
  assert.ok(queue.enqueue(1))
  await waiting
  assert.ok(woke, 'wait() did not resolve after enqueue()')

  assert.ok(queue.enqueue(2))
  assert.equal(queue.enqueue(3), false, 'enqueue() went past maxQueueSize')
  assert.ok(queue.enqueueReserved(4), 'enqueueReserved() found no room above maxQueueSize')
  assert.deepEqual(
    [queue.shift(), queue.shift(), queue.shift(), queue.shift()],
    [1, 2, 4, undefined],
  )

  const pending = queue.next()
  const raced = await Promise.race([
    pending.then(() => 'resolved'),
    new Promise((resolve) => setTimeout(resolve, 20, 'waiting')),
  ])
  assert.equal(raced, 'waiting', 'next() resolved on an empty queue')
  queue.enqueue(5)
  assert.equal(await pending, 5)
})
