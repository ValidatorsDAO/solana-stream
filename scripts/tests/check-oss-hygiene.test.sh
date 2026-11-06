#!/usr/bin/env bash
# Self-test for scripts/check-oss-hygiene.sh: each rule this repository relies on must
# block a known-bad sample and let a known-good one through. A gate that is never
# tested quietly stops catching things.
#
#   bash scripts/tests/check-oss-hygiene.test.sh
set -uo pipefail
export LC_ALL=C

source_root="$(git rev-parse --show-toplevel)" || exit 2
tmp="$(mktemp -d)" || exit 2
trap 'rm -rf "$tmp"' EXIT

failures=0
# check <name> <allow|block> <path> <content>
check() {
  local name="$1" expect="$2" path="$3" content="$4" repo="$tmp/$RANDOM$RANDOM"
  mkdir -p "$repo/scripts" "$repo/$(dirname "$path")"
  cp "$source_root/scripts/check-oss-hygiene.sh" "$source_root/scripts/oss-public-hosts.txt" "$repo/scripts/"
  printf '%s\n' "$content" > "$repo/$path"
  (
    cd "$repo" && git init -q && git add -A
  ) || exit 2
  local rc=0
  (cd "$repo" && bash scripts/check-oss-hygiene.sh >/dev/null 2>&1) || rc=$?
  if { [ "$expect" = block ] && [ "$rc" -eq 0 ]; } || { [ "$expect" = allow ] && [ "$rc" -ne 0 ]; }; then
    echo "FAIL $name: expected $expect, gate exited $rc"
    failures=$((failures + 1))
  else
    echo "ok   $name"
  fi
}

check ".env file" block ".env" "X_TOKEN=abc"
check "nested .env.local" block "client/geyser-ts/.env.local" "X_TOKEN=abc"
check ".env.example template" allow "client/geyser-ts/.env.example" "GEYSER_ENDPOINT=http://grpc-ams1.erpc.global"
check ".env.sample template" allow "client/geyser-rs/.env.sample" "GRPC_ENDPOINT=http://grpc-ams1.erpc.global"
check ".env.example content is still scanned" block "client/geyser-ts/.env.example" \
  "AUTH=Bearer abcdefghijklmnopqrstuvwxyz0123456789ABCD"
check "documented ERPC host" allow "README.md" "SHREDS_ENDPOINT=http://shreds-ams6-1.erpc.global"
check "undocumented ERPC host" block "README.md" "See https://internal-admin.erpc.global"
check "private repository reference" block "README.md" "Tracked in elsoul/some-private-repo#1"
check "public erpc-sdk reference" allow "README.md" "See elsoul/erpc-sdk"
check "developer home path" block "README.md" "Built in /Users/someone/dev"
check "Solana keypair array" block "wallet.txt" \
  "[$(printf '1,%.0s' $(seq 1 63))1]"
check "PEM private key" block "key.txt" "-----BEGIN PRIVATE KEY-----"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed"
  exit 1
fi
echo "all checks passed"
