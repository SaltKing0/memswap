#!/usr/bin/env bash
# Package the Python + Node wrapper distributions for a memswap release.
# Usage: ./bindings/package.sh <version> [target-dir]
# Produces: dist/python/*.whl, dist/node/memswap-node-<version>.tgz
set -euo pipefail

VERSION="${1:?usage: package.sh <version> [target-dir]}"
TARGET_DIR="${2:-target}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
PY="$DIST/python"
NODE="$DIST/node"

rm -rf "$DIST"
mkdir -p "$PY" "$NODE"

# --- Python wheel (zero-dep ctypes binding + bundled libmemswap_ffi) ---
echo "==> building python wheel"
LIBDIR="$ROOT/bindings/python/memswap/lib"
mkdir -p "$LIBDIR"
for f in "$ROOT/$TARGET_DIR/release"/libmemswap_ffi.so \
         "$ROOT/$TARGET_DIR/release"/libmemswap_ffi.dylib \
         "$ROOT/$TARGET_DIR/release"/memswap_ffi.dll; do
  [ -f "$f" ] && cp "$f" "$LIBDIR/"
done
if [ -z "$(ls "$LIBDIR" 2>/dev/null)" ]; then
  echo "error: no libmemswap_ffi in $ROOT/$TARGET_DIR/release — build it first" >&2
  exit 1
fi
( cd "$ROOT/bindings/python" && python3 -m build --wheel --outdir "$PY" )
rm -rf "$LIBDIR"  # keep the repo clean; the wheel has its own copy

# --- Node tarball (koffi binding; lib fetched from GitHub Releases at install) ---
echo "==> packing node tarball"
( cd "$ROOT/bindings/node" && npm pack --pack-destination "$NODE" )

echo "==> done"
ls -la "$PY" "$NODE"
