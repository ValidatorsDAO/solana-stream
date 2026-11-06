#!/usr/bin/env node
// Fails when a tracked text file names an ERPC host that is not in service, or a
// Shreds / gRPC host with the retired https:// scheme. The starter templates are
// copied as they are (`slv bot init` downloads temp-release/ from main), so a dead
// host here is the customer's first failed run.
//
//   node scripts/check-erpc-endpoints.mjs            # static check (CI)
//   node scripts/check-erpc-endpoints.mjs --resolve  # also resolve every host found
//
// When ERPC adds or retires an endpoint, update HOSTS and scripts/oss-public-hosts.txt.
// The current hosts are the ones the ERPC dashboard and the slv.dev quickstarts give out.
import { execFileSync } from 'node:child_process'
import { resolve4 } from 'node:dns/promises'
import { readFileSync } from 'node:fs'

// host -> schemes it is served on (checked 2026-11-05).
const HOSTS = new Map([
  ['erpc.global', ['https']],
  ['edge.erpc.global', ['https']],
  ['dashboard.erpc.global', ['https']],
  // Shreds and Geyser gRPC are plain http://; their https:// forms are retired.
  ...[
    'shreds-ams6-1',
    'shreds-ny6-1',
    'shreds-fra6-1',
    'shreds-ty6-1',
    'shreds-sgp6-1',
    'shreds-lon6-1',
    'grpc-ams1',
    'grpc-ny6-1',
    'grpc-fra1-1',
    'grpc-lon6-1',
    'grpc-tokyo-6',
    'grpc-sgp6-1',
  ].map((name) => [`${name}.erpc.global`, ['http']]),
])

const HOST_PATTERN = /(?:(https?):\/\/)?((?:[a-z0-9-]+\.)*erpc\.global)\b/gi

// Files that quote a bad host on purpose.
const SKIP = new Set(['scripts/tests/check-oss-hygiene.test.sh'])

const files = execFileSync('git', ['ls-files', '-z'], { encoding: 'utf8' })
  .split('\0')
  .filter((file) => file && !SKIP.has(file))

const problems = []
const seen = new Set()
for (const file of files) {
  let text
  try {
    text = readFileSync(file)
  } catch {
    continue // listed in the index but not on disk
  }
  if (text.subarray(0, 8000).includes(0)) continue // binary
  text.toString('utf8').split('\n').forEach((line, index) => {
    for (const [, scheme, rawHost] of line.matchAll(HOST_PATTERN)) {
      const host = rawHost.toLowerCase()
      const where = `${file}:${index + 1}`
      const schemes = HOSTS.get(host)
      if (!schemes) {
        problems.push(`${where}: ${host} is not an ERPC host in service`)
        continue
      }
      seen.add(host)
      if (scheme && !schemes.includes(scheme.toLowerCase())) {
        problems.push(
          `${where}: ${scheme}://${host} — this host is served on ${schemes.map((s) => `${s}://`).join(', ')} only`,
        )
      }
    }
  })
}

if (process.argv.includes('--resolve')) {
  for (const host of [...seen].sort()) {
    try {
      await resolve4(host)
    } catch (error) {
      problems.push(`${host} does not resolve (${error.code ?? error.message})`)
    }
  }
}

if (problems.length > 0) {
  console.error(problems.join('\n'))
  console.error(
    `\n${problems.length} ERPC endpoint problem(s). Current hosts: the ERPC dashboard and the slv.dev quickstarts.`,
  )
  process.exit(1)
}
console.log(`ERPC endpoints OK (${seen.size} hosts in ${files.length} tracked files)`)
