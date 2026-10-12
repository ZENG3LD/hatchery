# hatchery-c2 deploy

C2 and the node are equal peers: the node dials C2 (or C2 dials the node) and WireGuard stays inside the node. HQ talks only to C2. All addresses, endpoints and tokens come from arguments and environment variables; nothing is baked in.

Modes (all configurable, none is the only way):

- **Local process**: `hatchery-c2 --api-listen ADDR --control-endpoint ENDPOINT ...` with `GATE4AGENT_C2_TOKEN` and `GATE4AGENT_NODE_TOKEN_<NODE_ID>` in the environment.
- **Service / VPS**: `hatchery-c2.service` reads `/etc/hatchery/c2.env` (`C2_ARGS=...` plus the token variables).
- **Docker**: `docker-build.sh` then run the image with the same arguments and environment.
- **QEMU**: boot a guest with the same binary (see `crates/hatchery-node/deploy/node/qemu-guest.sh` for the guest recipe; C2 uses the same layout).

CLI: `hatchery-c2ctl` (package `hatchery-c2-client`).

## Lineage

These crates were moved from gate4agent (`gate4agent-c2`, `gate4agent-c2-client`, `gate4agent-c2-protocol`, history preserved from gate4agent branch `migrate/node-core-extract` at 579a8ad). The gate4agent 0.4.8 releases stay frozen on crates.io. The C2 <-> node integration tests live in `crates/hatchery-node/tests/c2_*.rs` (the node consumes C2 as a dev-dependency).

## Service on a VPS (tested layout)

1. Create a system user `hatchery`, install `hatchery-c2` (and `hatchery-c2ctl`) to `/usr/local/bin`, create `/etc/hatchery/` (0700) and `c2.env` (0600).
2. `c2.env`: `C2_ARGS=--node NODE_ID=accept --node-listen TUNNEL_IP:PORT --api-listen 127.0.0.1:PORT --control-endpoint /run/hatchery-c2/control.sock [--control-tunnel PRIVATE_IP:PORT]`, `GATE4AGENT_C2_TOKEN=<random hex>`, `GATE4AGENT_NODE_TOKEN_<NODE_ID upper-cased>=<random hex>`. Generate tokens on the host (`openssl rand -hex 32`); do not print or commit them.
3. Node call-home: create a WireGuard interface on the C2 host (`wg-quick`, own subnet, one UDP port open in the firewall). `--node-listen` binds the C2 tunnel address only. Open the node-listen TCP port only on that interface (`ufw allow in on <wg-if> from <subnet> to any port PORT proto tcp`); without it the node's dial times out although the handshake succeeds.
4. Enroll a node: the node generates its own private key (`wg genkey`, file mode 0600, never shown to an agent); the operator receives only the public key and adds `[Peer] PublicKey, AllowedIPs = <node tunnel ip>/32`. That peer entry is the approval. Give the node its token over a secure channel.
5. HQ clients on other hosts: `--control-tunnel` binds a TCP control listener on a private tunnel address (not 0.0.0.0). Components that only accept a local endpoint (the harness) reach it through a local unix socket forwarded with `systemd-socket-proxyd` (see the hatchery-server deploy README).
6. Health: `curl -s http://127.0.0.1:PORT/health` on the API address; `journalctl -u hatchery-c2` shows `node called home` and `node attached to relay`.

## Enrollment (automatic node registration, branch feat/c2-enrollment-registry)
Instead of adding each node by hand, start C2 with `--enroll-listen ADDR --enroll-registry FILE --enroll-pool C2_TUNNEL_IP/PREFIX --enroll-wg-interface IF --enroll-wg-endpoint HOST:PORT` (plus `--node-listen` on the tunnel address). The unit needs `AmbientCapabilities=CAP_NET_ADMIN` so C2 can `wg set` peers; the registry file lives in a state directory owned by the service user. Operators issue one-time codes and change statuses on the loopback API (`/v1/enroll/...`, bearer); nodes run `gate4agent-node enroll` and then `gate4agent-node --enrolled`. Full procedure and evidence: the P1 handoff in the docs repository. Token nodes (`--node ID=accept`) keep working beside enrolled ones.
