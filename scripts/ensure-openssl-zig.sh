#!/usr/bin/env bash
set -euo pipefail

TARGET="${1:-}"
if [ -z "$TARGET" ]; then
  echo "Usage: $0 <x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu>" >&2
  exit 1
fi

if ! command -v zig >/dev/null 2>&1; then
  echo "zig not found in PATH" >&2
  exit 1
fi
if ! command -v perl >/dev/null 2>&1; then
  echo "perl not found in PATH" >&2
  exit 1
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "curl not found in PATH" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OPENSSL_VERSION="${OPENSSL_VERSION:-3.5.4}"

case "$TARGET" in
  x86_64-unknown-linux-gnu)
    TRIPLE="x86_64-linux-gnu.2.17"
    CONFIGURE_TARGET="linux-x86_64"
    ;;
  aarch64-unknown-linux-gnu)
    TRIPLE="aarch64-linux-gnu.2.17"
    CONFIGURE_TARGET="linux-aarch64"
    ;;
  *)
    echo "Unsupported target: $TARGET" >&2
    exit 1
    ;;
esac

OUT_ROOT="$ROOT_DIR/deps/openssl"
OUT_DIR="$OUT_ROOT/$TARGET"
if [ -f "$OUT_DIR/lib/libssl.a" ] && [ -f "$OUT_DIR/include/openssl/ssl.h" ]; then
  echo "OpenSSL already built: $OUT_DIR"
  exit 0
fi

SRC_ROOT="$ROOT_DIR/deps/openssl-src"
BUILD_ROOT="$ROOT_DIR/deps/openssl-build"
mkdir -p "$SRC_ROOT" "$BUILD_ROOT" "$OUT_DIR"

TARBALL="$SRC_ROOT/openssl-$OPENSSL_VERSION.tar.gz"
if [ ! -f "$TARBALL" ]; then
  TMP_TARBALL="$(mktemp "$SRC_ROOT/openssl-$OPENSSL_VERSION.tar.gz.XXXXXX")"
  curl -L --fail -o "$TMP_TARBALL" "https://www.openssl.org/source/openssl-$OPENSSL_VERSION.tar.gz"
  mv "$TMP_TARBALL" "$TARBALL"
fi

BUILD_DIR="$BUILD_ROOT/$TARGET"
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
tar -xzf "$TARBALL" -C "$BUILD_DIR" --strip-components=1

JOBS=4
if command -v getconf >/dev/null 2>&1; then
  JOBS="$(getconf _NPROCESSORS_ONLN || echo 4)"
elif command -v sysctl >/dev/null 2>&1; then
  JOBS="$(sysctl -n hw.ncpu || echo 4)"
fi

(
  cd "$BUILD_DIR"
  CC="zig cc -target $TRIPLE" \
  CXX="zig c++ -target $TRIPLE" \
  AR="zig ar" \
  RANLIB="zig ranlib" \
  CFLAGS="-fPIC" \
  ./Configure "$CONFIGURE_TARGET" no-shared no-module no-tests no-comp no-zlib no-zlib-dynamic \
    --prefix="$OUT_DIR" --openssldir="$OUT_DIR/ssl" --libdir=lib
  make -j"$JOBS"
  make install_sw
)
