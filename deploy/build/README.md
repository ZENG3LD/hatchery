# Static builds (Alpine/musl)

```sh
deploy/build/build-musl.sh [-e ENV_FILE] <package[:bin]>...
# e.g.
deploy/build/build-musl.sh hatchery-server hatchery-c2 hatchery-hq-web hatchery-node:gate4agent-node
```

- Builder image: `Dockerfile.alpine` (`rust:alpine` + static openssl/sqlite). No Ubuntu.
- Output: `$OUT_DIR` (default `./dist-musl`); caches in `$CACHE_DIR` (default
  `~/.cache/hatchery-musl`). `IMAGE` and `DOCKER` (e.g. `podman`) can be overridden.
- `-e ENV_FILE`: optional `KEY=VALUE` file for `option_env!` defaults in private builds.
  Keep it outside the repository. Never put tokens or keys in it; they are read at runtime.
- Do not set `+crt-static` via `RUSTFLAGS`; `rust:alpine` already links statically and the
  flag breaks proc-macro crates.
- If a crate needs a system library without a static Alpine package, vendor it through the
  crate's feature (for example the vendored feature of `libdbus-sys`) and fix it here.
