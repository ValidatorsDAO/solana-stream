import { execFileSync, spawnSync } from 'node:child_process'

const target = process.argv[2]
if (
  process.argv.length !== 3 ||
  !['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu'].includes(target)
) {
  throw new Error('Expected one supported Linux GNU Rust target')
}

const rustVersion = execFileSync('rustc', ['--version', '--verbose'], {
  encoding: 'utf8',
})
const host = /^host: (.+)$/m.exec(rustVersion)?.[1]
if (!host) throw new Error('Could not determine the Rust host target')

const env = { ...process.env }
if (host !== target) {
  // protobuf-src builds protoc for the host. NAPI's global Zig CC/CXX must
  // not make that build-time executable target the foreign architecture.
  env.HOST_CC ??= 'cc'
  env.HOST_CXX ??= 'c++'
}
env.CARGO_BUILD_JOBS ??= '4'

const result = spawnSync(
  'napi',
  [
    'build',
    '--platform',
    '--release',
    '--target',
    target,
    '--zig',
    '--zig-abi-suffix',
    '2.17',
    'dist',
  ],
  { env, stdio: 'inherit' },
)
if (result.error) throw result.error
process.exitCode = result.status ?? 1
