# hatchery-server deploy

One supervisor process hosts the subservices. Everything is configured by environment variables or an optional TOML file (`HATCHERY_SERVER_CONFIG`); env wins. No domain, IP, key or bot name is compiled in. The control listener is loopback only; reach it from elsewhere through a tunnel.

| Subservice | Feature | How it runs | Started |
|---|---|---|---|
| harness + observation | `harness` | supervised `hatchery-harness` process (observation engine/store live inside it) | at server start when `HATCHERY_HARNESS_BIN` (or `[harness]`) is set |
| m4a | `m4a` | in-process key vault of the node messenger (`m4a-agent` 0.4); master key from the variable named by `HATCHERY_M4A_MASTER_KEY_VAR` | at server start when configured |
| memory | `memory` | supervised `memory4agent-server` (crates.io `memory4agent` 0.3), loopback, port 18200-18299 | **never automatically**; only through the authenticated control request an agent-facing MCP tool triggers |

`kit` (all of the above) is the default; there is no light mode.

Why processes: the harness and memory4agent are standalone services with their own sqlite and lifecycle (memory4agent pins rusqlite 0.32 and a tree-sitter C stack), so the server supervises them as child processes and embeds only m4a. The control endpoint env (`HATCHERY_SERVER_CONTROL_LISTEN`, `HATCHERY_SERVER_CONTROL_TOKEN`) is the same pair the node-side `g4a_memory_start` tool reads.

## Control (loopback)

- `GET /health`: subservice status.
- `POST /sub/<name>/start`, `POST /sub/<name>/stop`: `Authorization: Bearer $HATCHERY_SERVER_CONTROL_TOKEN`. The token is a node-side credential; agents never see it, and agents never see this server: the node exposes memory to them as an ordinary provider tool.

## Modes (all configurable)

- **Local process**: run `hatchery-server` with the environment set.
- **Service / VPS**: `hatchery-server.service` reads `/etc/hatchery/server.env`.
- **Docker**: `docker-build.sh`, mount a persistent volume at `/var/lib/hatchery`, pass secrets via env or a secret file.
- **QEMU**: boot a guest with the same binaries and a persistent data volume (guest recipe: `crates/hatchery-node/deploy/node/qemu-guest.sh`).

Data of the memory index is a rebuildable cache; encrypt the volume at the host level (the index itself is not encrypted). The memory port must never be published to WireGuard or outside: it has no authentication.
