#!/bin/sh
# Stage a slim context and build the hatchery-server image. Does not push. Writes no secrets.
set -eu
HERE=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REPO=$(CDPATH= cd -- "$HERE/../../.." && pwd)
WS=$(CDPATH= cd -- "$REPO/.." && pwd)
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
copy_tree() {
  mkdir -p "$2"
  tar -C "$1" --exclude target --exclude .git --exclude node_modules -cf - . | tar -C "$2" -xf -
}
copy_tree "$REPO" "$STAGE/hatchery"
docker build -f "$STAGE/hatchery/crates/hatchery-server/deploy/Dockerfile" -t hatchery-server:latest "$@" "$STAGE"
