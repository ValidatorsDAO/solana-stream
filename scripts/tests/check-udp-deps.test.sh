#!/usr/bin/env bash
# Self-test for scripts/check-udp-deps.sh: every banned crate must fail the gate and be
# named, the version bans must bite exactly below their fixed versions in SemVer order
# (pre-releases of the fixed version included), a clean graph (with look-alike names that
# are allowed) must pass, the whole input must be read (a banned crate after 2,000 lines,
# or on a last line without a newline), and input that is not a `cargo tree` graph must
# fail rather than pass by matching nothing.
#
#   bash scripts/tests/check-udp-deps.test.sh
set -uo pipefail
export LC_ALL=C

source_root="$(git rev-parse --show-toplevel)" || exit 2
gate="$source_root/scripts/check-udp-deps.sh"

# A clean graph in the format CI feeds the gate (`cargo tree --prefix none`), with crates
# whose names contain a banned name, and the allowed versions of the two version bans.
clean='solana-stream-sdk v5.0.0 (/work/crate/solana-stream-sdk)
bytes v1.12.1
serde_derive v1.0.229 (proc-macro)
solana-rpc-client v4.4.1
reqwest v0.12.28 (*)
hyper-rustls v0.27.10
rustls v0.23.46
openssl-probe v0.2.2
solana-runtime-transaction v4.4.1 (*)
solana-perf v4.4.1
curve25519-dalek v4.1.3 (*)
curve25519-dalek-derive v0.1.1 (proc-macro)
ed25519 v2.2.3
ed25519-dalek v2.2.0'

failures=0
# check <name> <pass|fail> <input> [text the output must contain]
check() {
  check_input "$1" "$2" "$(printf '%s\n' "$3")
" "${4:-}"
}
# check_input <name> <pass|fail> <exact input> [text the output must contain]
check_input() {
  local name="$1" expect="$2" input="$3" needle="${4:-}" out rc=0
  out="$(printf '%s' "$input" | bash "$gate" 2>&1)" || rc=$?
  if [ "$expect" = pass ] && [ "$rc" -ne 0 ]; then
    echo "FAIL $name: expected to pass, gate exited $rc: $out"
    failures=$((failures + 1))
  elif [ "$expect" = fail ] && [ "$rc" -eq 0 ]; then
    echo "FAIL $name: expected to fail, gate passed: $out"
    failures=$((failures + 1))
  elif [ -n "$needle" ] && [[ "$out" != *"$needle"* ]]; then
    echo "FAIL $name: output does not name '$needle': $out"
    failures=$((failures + 1))
  else
    echo "ok   $name"
  fi
}

check "clean graph" pass "$clean"

# Each crate banned in every version, as cargo prints it (the `(*)` of a repeated
# subtree, a git source, a proc-macro), anywhere after the root.
for line in \
  'solana-ledger v4.4.1' \
  'solana-runtime v4.4.1 (*)' \
  'solana-storage-bigtable v4.4.1' \
  'agave-precompiles v4.4.1' \
  'rocksdb v0.24.0 (https://github.com/anza-xyz/rust-rocksdb?rev=4dba80df#4dba80df)' \
  'librocksdb-sys v0.17.3+10.4.2 (https://github.com/anza-xyz/rust-rocksdb?rev=4dba80df#4dba80df)' \
  'bindgen v0.72.1' \
  'clang-sys v1.9.1 (*)' \
  'openssl v0.10.75' \
  'openssl-sys v0.9.118 (*)' \
  'native-tls v0.2.18' \
  'hyper-tls v0.6.0' \
  'bzip2 v0.6.1' \
  'bzip2-sys v0.1.13+1.0.8' \
  'libbz2-rs-sys v0.2.6'; do
  crate="${line%% *}"
  check "banned $crate" fail "$clean
$line" "builds $crate v"
done

# The version bans, at their boundaries, in SemVer order: a pre-release of the fixed
# version is below it, build metadata does not count, parts compare as numbers.
for version in 1.0.1 1.99.99 2.0.0-rc.3 2.0.0-pre.1 2.0.0-rc.3+build.1; do
  check "ed25519-dalek $version" fail "$clean
ed25519-dalek v$version" "builds ed25519-dalek v$version"
done
for version in 2.0.0 2.0.0+build.1 2.0.1-alpha 10.0.0; do
  check "ed25519-dalek $version" pass "$clean
ed25519-dalek v$version"
done
for version in 3.2.0 4.1.2 4.1.3-pre.0 4.1.3-rc.1+build.1 0.99.0; do
  check "curve25519-dalek $version" fail "$clean
curve25519-dalek v$version (*)" "builds curve25519-dalek v$version"
done
for version in 4.1.3 4.1.3+build.1 4.1.10 4.2.0-pre.1 5.0.0; do
  check "curve25519-dalek $version" pass "$clean
curve25519-dalek v$version"
done

# Several at once are all named.
check "two banned crates, first named" fail "$clean
openssl-sys v0.9.118
librocksdb-sys v0.17.3+10.4.2" "builds librocksdb-sys"
check "two banned crates, second named" fail "$clean
openssl-sys v0.9.118
librocksdb-sys v0.17.3+10.4.2" "builds openssl-sys"

# The whole input is read. The CI input is about 1,800 lines; here 2,000 clean ones,
# with a banned crate near the end, or on a last line that has no newline.
long="$clean"
for i in $(seq 1 2000); do
  long="$long
filler-$i v0.$((i % 97)).$i"
done
check "2,000 clean lines" pass "$long"
check "banned crate after 2,000 lines" fail "$long
openssl-sys v0.9.118 (*)
filler-last v1.0.0" "builds openssl-sys v0.9.118"
check "banned version after 2,000 lines" fail "$long
curve25519-dalek v4.1.3-pre.0" "builds curve25519-dalek v4.1.3-pre.0"
check_input "banned crate on a last line without a newline" fail "$clean
libbz2-rs-sys v0.2.6" "builds libbz2-rs-sys v0.2.6"

# Input that is not the graph fails instead of passing by matching nothing.
check "empty input" fail ""
check "another root" fail "geyser-rs v5.0.0 (/work/client/geyser-rs)
bytes v1.12.1"
check "another root with the SDK below it (a client's graph)" fail "shreds-udp-rs v5.0.0 (/work/client/shreds-udp-rs)
solana-stream-sdk v5.0.0 (/work/crate/solana-stream-sdk)
bytes v1.12.1"
check "tree prefixes left on" fail "solana-stream-sdk v5.0.0 (/work/crate/solana-stream-sdk)
├── bytes v1.12.1"
check "an error message instead of a graph" fail "error: package ID specification \`x\` did not match any packages"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed"
  exit 1
fi
echo "all checks passed"
