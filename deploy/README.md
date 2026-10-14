# Deploying hatchery (two-host pattern)

A generic guide. All names below are placeholders: `example.org`, `edge.example.org`,
`enroll.edge.example.org`, tunnel addresses from `10.0.0.0/8`, documentation IPs from
RFC 5737 (`203.0.113.0/24`). No secrets belong in this repository; keep tokens and keys in
environment files outside the units (`EnvironmentFile=`), readable only by the service user.

## Roles

- **Edge (public host): `hatchery-c2` only.** C2 is an opaque byte restreamer for node (g4a)
  and messenger (m4a) traffic, not part of the server. Nodes connect to it; it holds no keys
  of core, makes no decisions, and does not parse the g4a protocol. It sees envelopes (event
  number, recipient, opaque bytes) and keeps a bounded replay buffer. A reverse proxy (for
  example Caddy) terminates public TLS, may serve a static landing page, and forwards the
  operator HQ site to core (see "Plane split"). Enrollment uses `enroll.<your-domain>`.
  **The edge stays light:** no keys of core, no backups (not even encrypted), no HQ web, no
  operator messenger client. Treat it as the most exposed host.
- **Core (private host, no public listener):**
  - `hatchery-server` (registry, rights and crypto, tasks filtered by role, vault,
    provisioning, policy, harness);
  - `hatchery-hq-web` (operator web UI, listens on loopback);
  - the operator's messenger client (an m4a client daemon holding the operator identity,
    reached by hq-web through a local socket);
  - the m4a messenger server.
  Agent memory is not kept on core; it lives on the node.
- **Operator device:** only a browser. The operator never runs WireGuard or any agent on it.
- **Backups** live on core or on a separate operator box, never on the edge.

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
5. Core: install `hatchery-hq-web` on loopback and the operator m4a client next to it.
   On the edge, add a reverse-proxy site that authenticates the operator and forwards to the
   operator stream (see "Plane split"). Tokens stay in `EnvironmentFile=`, never in the repo.

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


## Plane split: data plane and control plane

Two separate streams pass through the same edge, each on its own WireGuard interface between
edge and core:

- **Data plane:** nodes ↔ C2 ↔ core drain (above).
- **Control plane:** operator → core. The edge does not interpret this traffic; it forwards
  opaque TCP from a loopback listener to hq-web on core over its own tunnel.

Public path for the operator:

```
browser ──HTTPS──> edge reverse proxy (TLS + passkey / auth gate)
                   └─> loopback opaque TCP forwarder on edge (e.g. 127.0.0.1:18500)
                       └─> WireGuard (control-plane interface) ──> core hatchery-hq-web
```

Core has no public subdomain and no public listener. The operator never joins WireGuard from
their own device. Terminating the inner node↔core Noise layer on the edge is not supported.

Links opened from a node's PTY must not open in the operator's own browser: HQ opens them in
the node's own browser, restreamed into HQ, so the operator's browser never fetches them.

### Environment for hq-web (core)

| Variable | Required | Meaning |
|---|---|---|
| `HATCHERY_HQ_LISTEN` | no | listen address, loopback (e.g. `127.0.0.1:18500`) |
| `HATCHERY_HQ_BASE_PATH` | no | asset prefix behind a reverse proxy, e.g. `/app` |
| `HATCHERY_HQ_PUBLIC_HOSTS` | no | Host header values accepted from the edge proxy (`host[:port]`, comma separated) |
| `HATCHERY_HQ_C2_API` | yes | C2 API address via the core→edge tunnel |
| `HATCHERY_HQ_C2_CONTROL` | yes | core operator control socket on the same host |
| `HATCHERY_HQ_C2_TOKEN` (or `GATE4AGENT_C2_TOKEN`) | yes | C2 operator token; env file only |
| `HATCHERY_HQ_SERVER` | no | local hatchery-server control endpoint |
| `HATCHERY_HQ_BIDI` | no | port of the bidirectional relay, if used |
| `HATCHERY_HQ_LIBRARY` | no | path to the session library |
| `HATCHERY_HQ_STATE_DIR` | no | directory for the operator's sent-message journal |
| `HATCHERY_HQ_CHATS` | no | sidebar chats: `label=node-id` or `label=room:NAME`, comma separated |
| `HATCHERY_HQ_M4A_SERVER` | for m4a | messenger server name; m4a profile and DM return 503 when unset |
| `HATCHERY_HQ_M4A_CLI`, `HATCHERY_HQ_M4A_AS`, `HATCHERY_HQ_M4A_SOCK` | for m4a | operator m4a client binary, nick and local socket (all three or none) |
| `HATCHERY_HQ_M4A_GROUPS` | no | group rooms shown in HQ, comma separated |

## Builds: Alpine/musl only

Static binaries are built with `deploy/build/build-musl.sh` inside `rust:alpine`; see
`deploy/build/README.md`. The binaries run on any Linux host (edge, core, Alpine QEMU guests)
regardless of glibc. Do not use Ubuntu-based builders. If a dependency fails under musl, fix it
inside Alpine (static packages, vendored features) rather than switching distributions.
