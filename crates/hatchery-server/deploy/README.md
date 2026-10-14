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

## Service on a VPS (tested layout)

- Env file `/etc/hatchery/server.env` (0600) holds: `HATCHERY_SERVER_DATA_DIR`, `HATCHERY_SERVER_CONTROL_LISTEN`, `HATCHERY_SERVER_CONTROL_TOKEN`, `HATCHERY_MEMORY_BIN` and `GATE4AGENT_C2_TOKEN` (same value as the C2 host). The harness runtime is no longer part of the server; tasks, runs and sessions are served by the hub (`/v1/tasks`, `/v1/runs`, `/v1/sessions`).
- The harness accepts only a *local* `--c2-endpoint` (absolute unix path or loopback), never a remote TCP address. When C2 runs on another host, expose C2's `--control-tunnel` address over a private tunnel and forward it to an owner-only unix socket on the server host: a `.socket` unit with `ListenStream=/run/hatchery-c2-proxy/control.sock`, `SocketMode=0600`, owned by the service user (directory created by `tmpfiles.d`, mode 0700), and a `.service` unit running `/lib/systemd/systemd-socket-proxyd TUNNEL_IP:PORT`. Then `--c2-endpoint /run/hatchery-c2-proxy/control.sock`.
- Health proof: `curl -s 127.0.0.1:CONTROL_PORT/health` shows `harness: running`; `POST /sub/memory/start` (bearer = control token) shows `memory: running` on a port in 18200-18299, `stop` returns it to `stopped`.

## Provisioning of enrolled nodes (P2)
With m4a enabled and `HATCHERY_PROVISION_C2_ADDR=host:port` set (the C2 operator API, loopback or a tunnel to it), the `provision` subservice polls `GET /v1/enroll/nodes` every `HATCHERY_PROVISION_INTERVAL_SECS` (default 5) and keeps one m4a identity per node (key pair in the vault, master key from `HATCHERY_M4A_MASTER_KEY_VAR`), a mailbox address (`<node>@HATCHERY_MAIL_DOMAIN`) and room/capability grants by status in `<data dir>/provision/ledger.json` (public data, mode 0600). `HATCHERY_PROVISION_C2_TOKEN_VAR` names the variable that holds the C2 operator bearer (default `HATCHERY_PROVISION_C2_TOKEN`); `HATCHERY_M4A_SERVER_REF` names the messenger the identities belong to. Neutral gets only the lobby and a heartbeat; member adds mailbox, general room, mail and m4a; trusted adds ops, tasks and tools; suspended removes every grant and keeps the key; revoked deletes the key from the vault, final. The existing m4a-server stays for debug.
