# hatchery-c2 deploy

C2 and the node are equal peers: the node dials C2 (or C2 dials the node) and WireGuard stays inside the node. HQ talks only to C2. All addresses, endpoints and tokens come from arguments and environment variables; nothing is baked in.

Modes (all configurable, none is the only way):

- **Local process**: `hatchery-c2 --api-listen ADDR --control-endpoint ENDPOINT ...` with `GATE4AGENT_C2_TOKEN` and `GATE4AGENT_NODE_TOKEN_<NODE_ID>` in the environment.
- **Service / VPS**: `hatchery-c2.service` reads `/etc/hatchery/c2.env` (`C2_ARGS=...` plus the token variables).
- **Docker**: `docker-build.sh` then run the image with the same arguments and environment.
- **QEMU**: boot a guest with the same binary (see `crates/hatchery-node/deploy/node/qemu-guest.sh` for the guest recipe; C2 uses the same layout).

CLI: `hatchery-c2ctl` (package `hatchery-c2-client`).

## Lineage

These crates were moved from gate4agent (`gate4agent-c2`, `gate4agent-c2-client`, `gate4agent-c2-protocol`, history preserved from gate4agent branch `migrate/node-core-extract` at 579a8ad). The gate4agent 0.4.8 releases stay frozen on crates.io. The C2 <-> node integration tests live in `crates/hatchery-node/tests/c2_*.rs` (node and C2 resolve different native sqlite stacks, so the node workspace is separate and consumes C2 as a dev-dependency).
