# Deploying hatchery (two-host pattern)

A generic guide. All names below are placeholders: `example.org`, `edge.example.org`,
`enroll.edge.example.org`, tunnel addresses from `10.0.0.0/8`, documentation IPs from
RFC 5737 (`203.0.113.0/24`). No secrets belong in this repository; keep tokens and keys in
environment files outside the units (`EnvironmentFile=`), readable only by the service user.

## Roles

- **Edge (public host): `hatchery-c2` only.** C2 is a restreamer, not part of the server.
  Nodes connect to it; it holds no keys of core, makes no decisions, and does not parse the
  g4a protocol. It sees envelopes (event number, recipient, opaque bytes) and keeps a bounded
  replay buffer. A reverse proxy (for example Caddy) may serve a static landing page.
  Enrollment is plain TCP on a dedicated port at `enroll.<your-domain>` (DNS only, not proxied).
- **Core (private host): `hatchery-server`** (registry, rights and crypto, tasks, vault,
  provisioning, policy, harness). Core has no public listener.
- **HQ web / TUI** is a client of core. It runs on the operator's own machine and keeps only
  UI state.

## Direction of traffic: core drains C2

C2 never pushes to core. Core opens one long-lived stream to C2 over a private WireGuard link
and drains it:

1. C2 listens for the drain stream on its tunnel address only: `--drain-listen 10.0.0.1:PORT`.
   (`--server-upstream` is removed; C2 ignores it with a warning.)
2. Core connects to that address, reads numbered events and acknowledges the last one it
   processed. After a reconnect it asks for everything after N.
3. The buffer is bounded by size and age. On overflow C2 applies back-pressure to the node,
   which retries from its own journal. If core asks for a number already gone from the buffer,
   C2 fetches the gap from node journals itself. Core never sees or addresses nodes, and nodes
   are never told what sits behind C2.

## Layers of encryption

1. WireGuard between node and C2, and between core and C2.
2. A layer for C2: the node seals to the C2 key; C2 removes it.
3. An inner Noise layer for core: C2 hands the node a second public key (core's) and holds no
   private half. Only core opens it. Core rotates that key through C2.

## Minimal steps

1. Core and edge: create a WireGuard pair (for example edge `10.0.0.1/32`, core `10.0.0.2/32`,
   endpoint `edge.example.org:51820` or `203.0.113.10:51820`).
2. Edge: install `hatchery-c2` with `crates/hatchery-c2/deploy/hatchery-c2.service`. The unit
   needs `CAP_NET_ADMIN` (it adds and removes WireGuard peers on enroll and revoke) and a
   `StateDirectory` for the registry. Put `C2_ARGS` (node listen, `--drain-listen`, API and
   control endpoints) into its env file.
3. Core: install `hatchery-server` with `crates/hatchery-server/deploy/hatchery-server.service`
   and point it at the C2 drain address on the tunnel.
4. Nodes: see `crates/hatchery-node/deploy/node/README.md`; they enroll at
   `enroll.<your-domain>` and only ever talk to C2.
5. Operator: run HQ locally and reach core through SSH port or socket forwards. Tokens stay in
   the HQ process environment, never in files in the repo.

## Environment variables

Scripts and units carry no real domains, keys, nicks or paths. Deployment-specific values come
only from the environment; a missing required variable stops the script with a clear message.
Defaults, where present, are generic (QEMU user-net addresses, private ranges, upstream mirrors).

| Variable | Used by | Required | Meaning |
|---|---|---|---|
| `C2_ARGS` | `hatchery-c2.service` env file | yes | node listen, `--drain-listen TUNNEL_IP:PORT`, API and control endpoints |
| `HATCHERY_HQ_M4A_SERVER` | HQ web | for m4a features | messenger server name; no default, m4a profile and DM return 503 when unset |
| `HATCHERY_ENROLL_ADDR` | `qemu-isolated-boot.sh` (guest env file) | yes | `host:port` of the C2 enroll listener |
| `HATCHERY_ENROLL_CODE` | guest env file | yes | one-time enroll code; never commit it |
| `HATCHERY_NODE_ID` | guest env file | yes | node id to enroll as |
| `HATCHERY_INTERNET_PROBE` | `qemu-isolated-boot.sh` | yes | public IP the isolated guest must NOT reach |
| `HATCHERY_DNS_PROBE` | `qemu-isolated-boot.sh` | yes | public DNS resolver IP the isolated guest must NOT reach |
| `HATCHERY_UNDERLAY_PROBE` | `qemu-isolated-boot.sh` | no (`10.0.2.2`) | underlay gateway to probe |
| `GATE4AGENT_NODE_TOKEN`, `GATE4AGENT_WG_*` | node unit / guest env file | yes | node token and WireGuard settings (address, peer address, endpoint such as `203.0.113.10:51820`, keys) |
| `KIT_BIN`, `SHARE`, `WORKDIR`, `GUEST_ENV_FILE`, `GUEST_MEM`, `QEMU_*`, `GUEST_*`, `ALPINE_*` | `qemu-guest.sh` | no | build and boot settings for the QEMU guest |
| `INSTALL_PROVIDER_CLIS`, `PROVIDER_CLI_INSTALL` | `install-provider-clis.sh` | no | opt-in install of provider CLIs from official packages |

Landing pages, if wanted, live in `site/` and can be served by the edge reverse proxy.

## Build-time defaults (operator builds only)

`HATCHERY_HQ_M4A_SERVER`, `HATCHERY_HQ_C2_API`, `HATCHERY_HQ_C2_CONTROL` (hq-web) and `HATCHERY_ENROLL_ADDR` (node `enroll`/`call`/`build` when `--c2-enroll` is absent) are read at runtime first; if unset, a value present in the environment of `cargo build` is used (`option_env!`). Keep your values in a private env file outside the repo and source it before building. Tokens and keys are never baked; crates.io builds contain no values.


## Operator WireGuard path (HQ on core)

HQ web runs on core (loopback). The operator device joins a dedicated WireGuard
interface on the edge (`wg-op`); the edge only forwards opaque TCP to core over the
existing core↔edge tunnel. Nodes stay on their own WG interface and only talk to C2.

Typical env on core for hq-web:

| Variable | Meaning |
|---|---|
| `HATCHERY_HQ_LISTEN` | `127.0.0.1:18500` |
| `HATCHERY_HQ_C2_API` | C2 API via the core→edge tunnel address |
| `HATCHERY_HQ_C2_CONTROL` | core operator control socket on the same host |
| `HATCHERY_HQ_SERVER` | local hatchery-server control |
| `HATCHERY_HQ_PUBLIC_HOSTS` | edge operator VIP `host:port` (Host header the opaque proxy does not rewrite) |

The operator opens `http://<edge-wg-op-vip>:18500/app/` after bringing up their WG peer.
Core has no public subdomain. Keep tokens in `EnvironmentFile=` only.

## Alpine/musl builds

Static binaries: `deploy/build/build-musl.sh [-e env] pkg[:bin]...`.
Uses `rust:alpine` (no Ubuntu). Output in `$OUT_DIR` (default `./dist-musl`).
