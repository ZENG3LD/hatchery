#!/bin/sh
# Stage a slim context and build the mode-2 image. Does not push. Does not write secrets.
set -eu
HERE=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REPO=$(CDPATH= cd -- "$HERE/../../../.." && pwd)
WS=$(CDPATH= cd -- "$REPO/.." && pwd)
STAGE=$(mktemp -d)
cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

copy_tree() {
  src=$1
  dest=$2
  mkdir -p "$dest"
  tar -C "$src" \
    --exclude target \
    --exclude .git \
    --exclude node_modules \
    -cf - . | tar -C "$dest" -xf -
}

copy_tree "$REPO" "$STAGE/hatchery"

docker build \
  -f "$STAGE/hatchery/crates/hatchery-node/deploy/node/Dockerfile" \
  -t gate4agent-node:kit \
  "$@" \
  "$STAGE"
