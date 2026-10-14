#!/bin/sh
# Build static musl binaries of Hatchery inside Alpine.
# Usage: deploy/build/build-musl.sh [-e ENV_FILE] <package[:bin]>...
#   ENV_FILE: optional KEY=VALUE file passed to cargo (for option_env! in private builds).
#   Output: $OUT_DIR (default ./dist-musl). Caches: $CACHE_DIR (default ~/.cache/hatchery-musl).
set -eu
ENV_FILE=""
[ "${1:-}" = "-e" ] && { ENV_FILE="$2"; shift 2; }
[ $# -gt 0 ] || { echo "usage: $0 [-e env] pkg[:bin]..." >&2; exit 2; }
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT_DIR=${OUT_DIR:-$PWD/dist-musl}; CACHE_DIR=${CACHE_DIR:-$HOME/.cache/hatchery-musl}
IMAGE=${IMAGE:-hatchery-musl-builder}
DOCKER=${DOCKER:-docker}
mkdir -p "$OUT_DIR" "$CACHE_DIR/registry" "$CACHE_DIR/git" "$CACHE_DIR/target"
$DOCKER build --network=host -q -t "$IMAGE" -f "$ROOT/deploy/build/Dockerfile.alpine" "$ROOT/deploy/build" >/dev/null
ENVARGS=""; [ -n "$ENV_FILE" ] && ENVARGS="--env-file $(cd "$(dirname "$ENV_FILE")" && pwd)/$(basename "$ENV_FILE")"
CMD=""
for spec in "$@"; do
  pkg=${spec%%:*}; bin=${spec#*:}; [ "$bin" = "$spec" ] && bin=$pkg
  CMD="$CMD cargo build --release --locked -p $pkg --bin $bin && cp /target/release/$bin /out/ &&"
done
$DOCKER run --rm --network=host $ENVARGS -e CARGO_TARGET_DIR=/target \
  -v "$ROOT:/src:ro" -v "$OUT_DIR:/out" -v "$CACHE_DIR/target:/target" \
  -v "$CACHE_DIR/registry:/usr/local/cargo/registry" -v "$CACHE_DIR/git:/usr/local/cargo/git" \
  "$IMAGE" sh -c "$CMD true && chown -R $(id -u):$(id -g) /out"
for spec in "$@"; do bin=${spec#*:}; [ "$bin" = "$spec" ] && bin=${spec%%:*}; ls -l "$OUT_DIR/$bin"; done
