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
# Banned below a version: the two vulnerabilities deny.toml ignores by id because they
# reach only the dev-dependency solana-ledger. An ignore applies to the whole graph, so
# this is what keeps them out of what users build. Each bound is the first fixed release.
banned_below=(
  "ed25519-dalek 2.0.0"    # RUSTSEC-2022-0093, fixed in 2.0.0
  "curve25519-dalek 4.1.3" # RUSTSEC-2024-0344, fixed in 4.1.3
)

# version_lt VERSION BOUND: VERSION is lower than the release BOUND in SemVer precedence,
# which is Cargo's and RustSec's: build metadata (`+...`) does not count, the numeric
# parts compare as numbers (4.1.10 > 4.1.3), and a pre-release (`-rc.1`) is lower than
# its release (2.0.0-rc.3 < 2.0.0). A part that is not a number fails closed (lower).
version_lt() {
  local version="${1%%+*}" bound="$2" core pre="" i a b
  local -a parts bound_parts
  core="${version%%-*}"
  if [ "$core" != "$version" ]; then
    pre="${version#*-}"
  fi
  IFS=. read -r -a parts <<<"$core"
  IFS=. read -r -a bound_parts <<<"$bound"
  for i in 0 1 2; do
    a="${parts[i]:-0}"
    b="${bound_parts[i]}"
    if [[ ! "$a" =~ ^[0-9]{1,18}$ ]]; then
      return 0
    fi
    if ((10#$a < 10#$b)); then
      return 0
    fi
    if ((10#$a > 10#$b)); then
      return 1
    fi
  done
  [ -n "$pre" ]
}

for rule in "${banned_below[@]}"; do
  if [[ ! "${rule#* }" =~ ^[0-9]{1,18}\.[0-9]{1,18}\.[0-9]{1,18}$ ]]; then
    echo "check-udp-deps: a bound must be a release (MAJOR.MINOR.PATCH): $rule" >&2
    exit 2
  fi
done

lines=0
root=0
found=()
# `|| [ -n "$line" ]`: the last line counts even without a trailing newline.
while IFS= read -r line || [ -n "$line" ]; do
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
    if [ "$name" = "${rule% *}" ] && version_lt "$version" "${rule#* }"; then
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
