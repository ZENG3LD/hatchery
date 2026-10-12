# hatchery-node kit (binary `gate4agent-node`)

Default build is the full kit. Feature `kit` (on by default) links:

1. `dig2browser` — crates.io `0.5.3` (local node browser machine)
2. `mail4agent` — crates.io `0.4.8` (lib face; no sibling checkout needed)
3. session-restore — `claude-session-restore` and `codex-session-restore` 0.1.3, `grok-session-restore` and `kimi-session-restore` 0.1.4, all from crates.io
4. kernel WireGuard — feature `wireguard`, `ip link add type wireguard` / `wg`. Node and C2 are peers (either side may dial). Not the UDP+ChaCha mesh underlay, not `open_wireguard_daemon_stub`, and not HQ.

Features: `m4a`, `d2b`, `session-restore`, `wireguard` (-> `gate4agent-node-core/wireguard`) and `memory` (a tool the agent calls; the node never starts a memory daemon). `kit` enables all of them and is the default. There is no light mode (no bare build); a subset can be built with `--no-default-features --features ...` for diagnostics.

The binary is still named `gate4agent-node`: agents see the g4a surface plus provider tools, nothing hatchery-specific.

Provider CLIs (`claude`, `codex`, `kimi`, `grok`) are packages. They are not copied credentials.
The node flag `--install-provider-clis` is **off by default**. When set (or when
`INSTALL_PROVIDER_CLIS=1` for the Docker/QEMU install script), the install path
checks PATH for each binary and downloads only the missing ones. It does not run
provider logins. Without the flag, nothing is downloaded. Mode 1's unit file does
not pass the flag.

## Mode 1 — service (implemented)

Entry point: the `gate4agent-node` binary from a default-feature build, installed as `deploy/node/gate4agent-node.service`.

The process links the four cores at startup (`force_link`) and starts the mail4agent mailbox on a background thread. It does not start Chrome or a tunnel unless the operator sets `GATE4AGENT_WG_*`. Those variables name a key *path* and the peer; the unit file does not contain secrets. If `ip link add type wireguard` fails, the process exits. It does not fall back to the mesh stub.

## Mode 2 — container (image build)

Entry point: `deploy/node/Dockerfile`, invoked by `deploy/node/docker-build.sh`. The image runs the same kit binary. `ENTRYPOINT` is `gate4agent-node`. No secret is copied into the image. Provider CLIs stay uninstalled unless `INSTALL_PROVIDER_CLIS=1` and `PROVIDER_CLI_INSTALL` is set at build time (`install-provider-clis.sh`).

## Mode 3 — QEMU guest

Entry point: `deploy/node/qemu-guest.sh`. It builds a minimal Alpine initramfs and boots it under QEMU with the same kit binary as mode 1. A virtio-9p mount (`kit`) carries the binary, its libraries, `ip`, and `wg`. The QEMU NIC is user-mode or a tap and is only the underlay. Kernel WireGuard is not a QEMU network. When `GATE4AGENT_WG_*` is set in the guest environment file, the node process creates the tunnel and dials C2 over it, the same as mode 1. The script does not provision secrets; it copies an operator-supplied environment file and the private-key file that file names.

## Node kinds and isolation (P4)

`hatchery-enroll` carries the defaults per kind (`gate4agent-node isolation-profile KIND` prints them):

| kind | egress kill-switch | provider sessions | tunnel | note |
| --- | --- | --- | --- | --- |
| `local` | off | same user (opt in with `--run-as`) | OS WireGuard (`--no-wg`) or node-managed | light by default; keys in a 0600 state dir |
| `docker` | on (needs `NET_ADMIN`, `nft` in the image) | unprivileged | node-managed | |
| `qemu` | on | unprivileged, share unmounted | node-managed inside the guest | the primary isolated space |
| `remote` | off (host policy) | unprivileged | node-managed | somebody else's machine |

Flags (also environment: `HATCHERY_KILL_SWITCH=1`, `HATCHERY_RUN_AS=user`):

- `--run-as USER`: every provider session starts through `gate4agent-node exec-as USER -- <provider>`, which drops groups, gid and uid, proves the drop cannot be reversed, sets HOME, and execs. The node keeps running as the key holder. Root is refused as the target.
- `--kill-switch`: before serving, install an nftables table `hatchery_killswitch` that drops everything except loopback, the tunnel interface, and UDP to the tunnel endpoint (an IP literal; a hostname is refused because resolving it is a leak). Rules match interface names, so removing the tunnel interface leaves traffic dropped. If the ruleset cannot be installed the node exits (fails closed). `iptables-restore` is the fallback for IPv4 endpoints.
- `gate4agent-node kill-switch --wg-if hatch0 --endpoint IP:PORT [--apply]` prints (or installs) the same ruleset for operators who apply it by hand.

### QEMU guest, isolated mode

`qemu-guest.sh` with `HATCHERY_GUEST_ISOLATION=1` in the guest environment file does, inside the guest and in this order: enroll with the one-time code (the node generates its keys in a root-only state dir in the guest), unmount the provisioning share, start the node with `--enrolled --kill-switch --run-as agent`. Environment file (operator-supplied, 0600, never in git): `HATCHERY_ENROLL_ADDR` (reachable underlay address of the C2 enrollment listener), `HATCHERY_ENROLL_CODE`, `HATCHERY_NODE_ID`, `NODE_ARGS`. Optional test inputs: `HATCHERY_GUEST_SELFTEST=1`, `HATCHERY_GUEST_FIXTURE_PROVIDER=1`, `HATCHERY_VPNDOWN_DELAY=SECONDS`; the guest then prints `P4_*` lines on the serial console (see the P4 handoff for a run). Operator steps on the C2 side: issue a code (`POST /v1/enroll/codes`), start the guest, wait for the node to appear `neutral`, then approve (`POST /v1/enroll/nodes/ID/status`). One VM per account: run one guest per account, each with its own node id, code and state; nothing is shared between guests.

The guest's NIC is only an underlay. Use a user-mode NIC or a tap you control; the kill-switch makes the NIC useless for anything but the tunnel.

### Local kind and a physical keyring

Keep the local path simple. The default is a 0600 state directory owned by a dedicated OS user, with `--run-as` for sessions if you want the agent unable to read it. For the operator/admin key (the C2 API token, the enrollment codes) use the OS keyring (Secret Service, Keychain, Credential Manager) or a hardware key (for example an SSH/age key on a hardware token) and pass the secret to `hatchery-c2` through its environment at start; the node never needs it. This is a convenience for people trying the kit on one machine, not a guarantee against a hostile local admin. Strong isolation of keys is the job of the isolated kinds (qemu, docker) and of remote build/sign (P5).
