#!/usr/bin/env bash
# Run the official MIT-licensed Gitleaks binary. Archive pins are from the
# v8.30.1 release checksums. A cached archive is verified on EVERY invocation;
# a fresh private extraction avoids trusting a previously extracted executable.
set -euo pipefail
export LC_ALL=C
version=8.30.1
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64) platform=darwin_arm64; sha=b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5 ;;
  Darwin/x86_64) platform=darwin_x64; sha=dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709 ;;
  Linux/x86_64) platform=linux_x64; sha=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb ;;
  Linux/aarch64|Linux/arm64) platform=linux_arm64; sha=e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080 ;;
  *) echo 'unsupported Gitleaks platform' >&2; exit 2 ;;
esac
cache="${XDG_CACHE_HOME:-$HOME/.cache}/solana-stream/gitleaks/$version"
mkdir -p "$cache"
archive="$cache/gitleaks_${version}_${platform}.tar.gz"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
verify() {
  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$1")
  else
    actual=$(shasum -a 256 "$1")
  fi
  actual=${actual%% *}
  if [ "$actual" != "$sha" ]; then
    echo 'Gitleaks archive checksum mismatch' >&2
    return 1
  fi
}
if [ ! -f "$archive" ]; then
  curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
    --output "$scratch/download" \
    "https://github.com/gitleaks/gitleaks/releases/download/v${version}/gitleaks_${version}_${platform}.tar.gz"
  verify "$scratch/download"
  mv "$scratch/download" "$archive"
fi
verify "$archive"
tar -xzf "$archive" -C "$scratch" gitleaks
"$scratch/gitleaks" "$@"
