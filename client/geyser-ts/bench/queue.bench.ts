// Throughput of the runner's event queue: enqueue N events in bursts the way the
// stream's 'data' handler does, and time how long the consumer loop takes to hand them
// all to a no-op handler.
//
//   node --expose-gc --import tsx bench/queue.bench.ts
import { createUpdateQueue } from '../src/lib/updateQueue.ts'

const N = 2_000_000
const BURST = 1_000

const produce = (enqueue: (n: number) => boolean) =>
  new Promise<void>((resolve) => {
    let sent = 0
    const burst = () => {
      for (let i = 0; i < BURST && sent < N; i += 1) {
        if (enqueue(sent)) sent += 1
        else break
      }
      if (sent < N) setImmediate(burst)
      else resolve()
    }
    burst()
  })

// The consumer before: one awaited next() per event.
const awaitEach = async () => {
  const queue = createUpdateQueue<number>(10_000)
  let seen = 0
  const started = performance.now()
  const consumer = (async () => {
    while (seen < N) {
      const item = await queue.next()
      if (item !== undefined) seen += 1
    }
  })()
  await produce(queue.enqueue)
  await consumer
  return performance.now() - started
}

// The consumer after: take what is queued synchronously, wait only when it is empty,
// and yield to the event loop every 1024 events so the stream keeps being read.
const drain = async () => {
  const queue = createUpdateQueue<number>(10_000)
  let seen = 0
  const started = performance.now()
  const consumer = (async () => {
    let sinceYield = 0
    while (seen < N) {
      const item = queue.shift()
      if (item === undefined) {
        await queue.wait()
        continue
      }
      seen += 1
      if (++sinceYield >= 1024) {
        sinceYield = 0
        await new Promise((resolve) => setImmediate(resolve))
      }
    }
  })()
  await produce(queue.enqueue)
  await consumer
  return performance.now() - started
}

// Warm up both, then interleave them so drift (GC, JIT, thermal) hits both alike.
// Run with --expose-gc to collect between runs.
const gc = (globalThis as { gc?: () => void }).gc ?? (() => {})
const variants = [
  ['await each', awaitEach],
  ['drain', drain],
] as const
const times = new Map<string, number[]>(variants.map(([name]) => [name, []]))
for (const [, run] of variants) await run()
for (let round = 0; round < 9; round += 1) {
  for (const [name, run] of variants) {
    gc()
    times.get(name)!.push(await run())
  }
}
for (const [name, list] of times) {
  const sorted = [...list].sort((a, b) => a - b)
  const median = sorted[Math.floor(sorted.length / 2)]
  console.log(
    `${name.padEnd(10)} median ${median.toFixed(0)} ms for ${N} events = ${((N / median) * 1000 / 1e6).toFixed(2)} M events/s (min ${sorted[0].toFixed(0)}, max ${sorted[sorted.length - 1].toFixed(0)})`,
  )
}
