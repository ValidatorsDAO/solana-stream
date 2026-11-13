#!/usr/bin/env bash
# Fails when the `udp` feature of solana-stream-sdk builds a crate it must not: since 5.0
# the feature decodes shreds without solana-ledger, and with it went RocksDB (and libclang
# to build it), OpenSSL and native-tls, and bzip2. CI feeds it what a user of
# `solana-stream-sdk/udp` builds (dev-dependencies excluded):
#
#   cargo tree -p solana-stream-sdk --features udp -e normal,build --target all \
#     --locked --prefix none | bash scripts/check-udp-deps.sh
#
# Input: one `name vX.Y.Z [...]` per line, as `cargo tree --prefix none` prints it. Input
# that does not look like that graph (empty, another root, another line format) fails too,
# so a change in cargo's output cannot make the check pass by matching nothing.
#
# Self-test: scripts/tests/check-udp-deps.test.sh.
set -uo pipefail
export LC_ALL=C

# Banned in every version.
banned=(
  # Agave's validator crates: solana-ledger and the ones that brought the rest.
  solana-ledger solana-runtime solana-storage-bigtable agave-precompiles
  # RocksDB, and bindgen/libclang to build it.
  rocksdb librocksdb-sys bindgen clang-sys
  # OpenSSL, and native TLS for reqwest/hyper.
  openssl openssl-sys native-tls hyper-tls
  # bzip2 (libbz2-rs-sys is under the bzip2-1.0.6 licence).
  bzip2 bzip2-sys libbz2-rs-sys
)
# Banned below a version: the two advisories deny.toml ignores by id because they reach
# only the dev-dependency solana-ledger. An ignore applies to the whole graph, so this is
# what keeps them out of what users build.
banned_below=(
  "ed25519-dalek 2.0.0"    # RUSTSEC-2022-0093, fixed in 2.0.0
  "curve25519-dalek 4.1.3" # RUSTSEC-2024-0344, fixed in 4.1.3
)

# version_lt A B: A < B in version order.
version_lt() {
  [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -n 1)" = "$1" ]
}

lines=0
root=0
found=()
while IFS= read -r line; do
  [ -z "$line" ] && continue
  lines=$((lines + 1))
  if [[ ! "$line" =~ ^([A-Za-z0-9_-]+)\ v([0-9][^ ]*)( |$) ]]; then
    echo "check-udp-deps: not a \`cargo tree --prefix none\` line: $line" >&2
    exit 2
  fi
  name="${BASH_REMATCH[1]}"
  version="${BASH_REMATCH[2]}"
  if [ "$lines" -eq 1 ] && [ "$name" = solana-stream-sdk ]; then
    root=1
  fi
  for crate in "${banned[@]}"; do
    if [ "$name" = "$crate" ]; then
      found+=("$name v$version")
    fi
  done
  for rule in "${banned_below[@]}"; do
    if [ "$name" = "${rule% *}" ] && version_lt "${version%%+*}" "${rule#* }"; then
      found+=("$name v$version (below ${rule#* })")
    fi
  done
done

if [ "$root" -ne 1 ]; then
  echo "check-udp-deps: the input is not solana-stream-sdk's dependency graph (${lines} lines)" >&2
  exit 2
fi
if [ "${#found[@]}" -ne 0 ]; then
  printf '%s\n' "${found[@]}" | sort -u | while IFS= read -r crate; do
    echo "check-udp-deps: solana-stream-sdk/udp builds $crate"
  done
  exit 1
fi
echo "check-udp-deps: ok ($lines lines, no banned crate)"
