#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 ELSOUL LABO B.V.
# SPDX-License-Identifier: Apache-2.0
#
# Modified for the SDK: index scanning, exact host list, and SDK paths.
# Taken from elsoul/erpc-sdk for this repository: the host list is the public ERPC
# hosts this repository documents, and committed `.env.example` / `.env.sample`
# templates are allowed (their content is still scanned).
# Repository hygiene gate for this public repository. Runs in pre-commit
# (lefthook, staged files only) and in CI (every tracked file).
#
# It is the second net behind gitleaks. gitleaks looks for credential *values*;
# this script looks for things that are not credentials but still must not be
# published: non-public hosts, infrastructure identifiers, and internal
# documents.
#
#   scripts/check-oss-hygiene.sh [files...]    # default: all tracked files
#
# Policy and scope: CONTRIBUTING.md. Security reports: SECURITY.md.
set -uo pipefail

# Every pattern below is matched against bytes, so the locale must not decide
# what a character class means. Under en_US.UTF-8 — which is what the GitHub
# runners set — a bash range follows collation order, so `[A-Z]` also matches
# lowercase and `[!A-Za-z0-9._-]` stops matching accented letters. Both turned
# live controls into no-ops that still passed their self-tests on a machine
# with no LANG set. The self-tests run under both locales to keep this honest.
export LC_ALL=C

cd "$(git rev-parse --show-toplevel)" || exit 2
scratch="$(mktemp -d)" || exit 2
trap 'rm -rf "$scratch"' EXIT
staged=false
if [ "${1:-}" = --staged ]; then
  staged=true
  shift
  [ "$#" -eq 0 ] || { echo '--staged takes no file arguments' >&2; exit 2; }
  git diff --cached --name-only --diff-filter=ACMR -z > "$scratch/files" || exit 2
elif [ "$#" -gt 0 ]; then
  printf '%s\0' "$@" > "$scratch/files"
else
  git ls-files -z > "$scratch/files" || exit 2
fi
[ -s "$scratch/files" ] || exit 0
files=()
while IFS= read -r -d '' f; do files+=("$f"); done < "$scratch/files"

# Binary-ish, vendored and license files are skipped.
skip_re='(\.lock$|\.png$|\.jpg$|\.gif$|\.ico$|\.woff2?$|^LICENSES/|^LICENSE$|/node_modules/|^target/)'

# Files that quote these patterns on purpose.
# Only the files that genuinely need it: each quotes a pattern this gate
# matches. An exemption is a blanket bypass of every content check, so a
# file that passes without one does not get one.
self_re='^(scripts/check-oss-hygiene\.sh|scripts/tests/check-oss-hygiene\.test\.sh)$'

# Only the public ERPC hosts this repository documents. This is a hygiene list,
# not a general URL gate or a runtime destination restriction;
# scripts/check-erpc-endpoints.mjs checks that they are still in service.
allowlist_src='scripts/oss-public-hosts.txt'
if "$staged"; then
  git show ":$allowlist_src" > "$scratch/hosts" || exit 2
else
  cat "$allowlist_src" > "$scratch/hosts" || exit 2
fi
allowed_erpc_hosts=$(cat "$scratch/hosts")
[ -n "$allowed_erpc_hosts" ] || { echo "could not read the host allowlist from $allowlist_src" >&2; exit 2; }
while IFS= read -r host || [ -n "$host" ]; do
  [[ "$host" =~ ^[a-z0-9-]+\.erpc\.global$ ]] || {
    echo "invalid host allowlist entry" >&2; exit 2;
  }
done < "$scratch/hosts"

erpc_host_allowed() {
  local host="${1%.}" a
  host=$(printf '%s' "$host" | tr '[:upper:]' '[:lower:]')
  while IFS= read -r a; do
    [ "$host" = "$a" ] && return 0
  done <<EOF
$allowed_erpc_hosts
EOF
  return 1
}

# Documents that belong in the company's internal documentation, never here.
# Matched by naming convention rather than by listing private titles, so
# this public file does not enumerate what those documents are called.
internal_doc_re='(^|/)(architecture|design|[a-z0-9-]+-design|[a-z0-9-]+-assessment|[a-z0-9-]+-records|[a-z0-9-]+-acceptance|classification)\.md$'

status=0
report() {
  printf '%s:%s: %s\n' "$1" "$2" "$3" >&2
  status=1
}

for f in "${files[@]}"; do
  f="${f#./}"
  # Path controls precede content exemptions, including extension skips.
  case "$f" in
    .env.example|.env.sample|*/.env.example|*/.env.sample) ;;
    .env|.env.*|*/.env|*/.env.*) report "$f" 1 ".env files must not be committed";;
  esac
  [[ "$f" =~ $skip_re ]] && continue

  # 0. Internal documents, matched on the path alone.
  if [[ "$f" =~ $internal_doc_re ]]; then
    report "$f" 1 "internal document: the canonical design and compliance records stay in the company's private documentation; see CONTRIBUTING.md"
  fi

  [[ "$f" =~ $self_re ]] && continue
  if "$staged"; then
    git show ":$f" > "$scratch/content" || { echo "cannot read staged file: $f" >&2; exit 2; }
    scan="$scratch/content"
  else
    [ -e "$f" ] || continue
    [ -f "$f" ] && [ ! -L "$f" ] || { echo "unsupported file: $f" >&2; exit 2; }
    scan="./$f"
  fi

  # 1. erpc.global hosts outside the published allowlist.
  while IFS=: read -r ln match; do
    [ -z "$ln" ] && continue
    if ! erpc_host_allowed "$match"; then
      report "$f" "$ln" "non-public erpc.global host: $match"
    fi
  done < <(grep -noiE '[A-Za-z0-9._-]+\.erpc\.global' "$scan" 2>/dev/null)

  # 2. Cloud account / zone / project identifiers.
  while IFS=: read -r ln match; do
    [ -z "$ln" ] && continue
    report "$f" "$ln" "infrastructure identifier: $match"
  done < <(grep -noiE '(account|zone|project)[_ -]?id[^0-9a-z]{0,8}[0-9a-f]{32}' "$scan" 2>/dev/null)

  # 2b. Bare 1Password vault or item identifiers.
  #
  # An `op://vault/item/field` reference is a pointer and is allowed, and our
  # convention puts the 26-character ids inside it — so the reference is
  # removed from the line first, which is what makes the exemption real
  # rather than a guard that can never fire. What is left is matched beside
  # the words vault or item, including the plural and the dashboard URL form
  # (.../vaults/<id>/items/<id>). The id itself must be lowercase: real ones
  # are, and matching case-insensitively flagged uppercase base32 in prose.
  op_ln=0
  while IFS= read -r op_line; do
    op_ln=$((op_ln + 1))
    case "$op_line" in *[Vv][Aa][Uu][Ll][Tt]*|*[Ii][Tt][Ee][Mm]*) ;; *) continue;; esac
    stripped=$(printf '%s' "$op_line" | sed 's|op://[A-Za-z0-9._/-]*||g')
    # `set -f` because the loop word-splits the match on purpose but must
    # not also glob it against the repository root: a `*` or `?` inside the
    # matched separator would otherwise expand to filenames.
    set -f
    for tok in $(printf '%s' "$stripped" | grep -oiE '(vault|item)s?([^A-Za-z0-9]{1,8}(id|identifier))?[^A-Za-z0-9]{1,8}[A-Za-z0-9]{26}([^A-Za-z0-9]|$)' 2>/dev/null); do
      id=$(printf '%s' "$tok" | grep -oE '[A-Za-z0-9]{26}$|[A-Za-z0-9]{26}' | tail -1)
      [ -n "$id" ] || continue
      # `[[:upper:]]` and not `[A-Z]`: under a non-C locale the range
      # follows collation order and matches lowercase too. LC_ALL=C above
      # already settles this; the class is correct on its own as well.
      case "$id" in *[[:upper:]]*) continue;; esac
      report "$f" "$op_ln" "bare 1Password identifier: $id"
    done
    set +f
  done < <(grep -a '' "$scan" 2>/dev/null)

  # 3. Bearer token literals.
  while IFS=: read -r ln match; do
    [ -z "$ln" ] && continue
    report "$f" "$ln" "bearer token literal: ${match:0:24}..."
  done < <(grep -noE 'Bearer [A-Za-z0-9._~+/-]{32,}' "$scan" 2>/dev/null)

  # 4. Credential assignments with a literal value.
  while IFS=: read -r ln match; do
    [ -z "$ln" ] && continue
    report "$f" "$ln" "credential assignment: ${match:0:40}..."
  done < <(grep -noE '(OP_SERVICE_ACCOUNT_TOKEN|OP_CONNECT_TOKEN|CLOUDFLARE_API_TOKEN|CARGO_REGISTRY_TOKEN|MINISIGN_SECRET_KEY|AWS_SECRET_ACCESS_KEY|R2_SECRET_ACCESS_KEY)[[:space:]]*[:=][[:space:]]*["'"'"']?[A-Za-z0-9._~+/-]{16,}' "$scan" 2>/dev/null)

  # 5. Blockchain key material (second net behind gitleaks).
  while IFS=: read -r ln match; do
    [ -z "$ln" ] && continue
    report "$f" "$ln" "looks like a Solana keypair array: ${match:0:32}..."
  done < <(grep -noE '\[([[:space:]]*[0-9]{1,3}[[:space:]]*,){63}[[:space:]]*[0-9]{1,3}[[:space:]]*\]' "$scan" 2>/dev/null)

  while IFS=: read -r ln match; do
    [ -z "$ln" ] && continue
    report "$f" "$ln" "looks like a private key: ${match:0:40}..."
  done < <(grep -noiE '(priv(ate)?[_-]?key|secret[_-]?key|signing[_-]?key|mnemonic|seed[_-]?phrase)["'"'"'[:space:]]{0,10}[:=][[:space:]]*["'"'"']?(0x)?[0-9a-fA-F]{64}' "$scan" 2>/dev/null)

  while IFS=: read -r ln match; do
    [ -z "$ln" ] && continue
    report "$f" "$ln" "PEM private key block"
  done < <(grep -noE -- '-----BEGIN [A-Z ]*PRIVATE KEY-----' "$scan" 2>/dev/null)

  # 6. References into the company's private repositories and documents.
  # Any sibling repository in the organisation, the internal documentation
  # convention, or a developer home path. Phrased so that this public file does
  # not itself name a private repository.
  while IFS=: read -r ln match; do
    [ -z "$ln" ] && continue
    # This project's own org paths. Named exactly: a trailing wildcard would
    # admit any private sibling whose name starts with the same prefix.
    case "$match" in
      elsoul/erpc-sdk|elsoul/erpc-sdk.git) continue;;
    esac
    report "$f" "$ln" "internal reference: $match"
  done < <(grep -noE '(elsoul/[A-Za-z0-9_.-]+|\.claude/docs/|/Users/[A-Za-z0-9_.-]+/)' "$scan" 2>/dev/null)


done

if [ "$status" -ne 0 ]; then
  cat >&2 <<'MSG'

check-oss-hygiene: FAILED

This repository is public. Fix the findings above before committing.
  - hygiene scope and host list: CONTRIBUTING.md
  - report an exposure privately: SECURITY.md
To bypass for a genuine false positive, adjust the pattern in this script in
the same commit and say why in the commit message. Never use `--no-verify`.
MSG
fi
exit "$status"
