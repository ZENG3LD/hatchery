# Verifying a deployment by hand (text instructions, no scripts shipped)

All commands need only published crates (`cargo install hatchery-node hatchery-c2 hatchery-c2-client hatchery-server hatchery-bidi-relay hatchery-bidi-client`, plus `cargo install dig2browser --bin dig2-stream-node`). Build hosts need `pkg-config`, `libssl-dev`, a C toolchain and `cmake`. Runtime hosts need `iproute2` and `wireguard-tools` for the WireGuard modes. Tokens come from the environment only: `GATE4AGENT_C2_TOKEN`, `GATE4AGENT_NODE_TOKEN_<NODE_ID>` for C2 and `GATE4AGENT_NODE_TOKEN` for the node. Unix sockets live in a directory with mode 0700.

## 1. Provider CLIs through the node PTY
1. Install the provider CLIs from their official installers (claude, codex, kimi, grok). No login is needed. Other CLIs (gemini, opencode, cursor) are not supported providers and the node refuses to spawn them.
2. Start `hatchery-c2 --node n1=SOCK_DIR/node.sock --api-listen 127.0.0.1:PORT --control-endpoint SOCK_DIR/c2.sock`, then `gate4agent-node --node-id n1 --workspace primary=/abs/dir --endpoint SOCK_DIR/node.sock --api-listen 127.0.0.1:PORT2` (use a separate `HOME` if you do not want provider state touched).
3. With a C2 client (`hatchery-c2-client` library: `C2Client::status`, `connect_local`) send `NodeRequest::Spawn{provider, mode: Pty, ...}`, collect `TerminalFrame` events (the first frames carry the provider banner), send `TerminalBytes` (for example a Down arrow `ESC [ B` on a menu screen) and expect the frame to change, send `Resize` and expect a frame with the new size, then `Stop`. Plain `Input` text rejects control characters; use `TerminalBytes`/`TerminalControl`.
4. Only one operator may be connected to C2 at a time; stop the harness first if it holds the connection.

## 2. WireGuard between network namespaces, byte pump
1. `ip netns add c2ns; ip netns add nodens`, join them with a veth pair (for example 192.168.77.1/24 and .2), loopback up in both.
2. In `c2ns`: `ip link add wg0 type wireguard`, `wg set wg0 listen-port 51820 private-key C2.key peer NODE_PUB allowed-ips 10.99.0.2/32`, address 10.99.0.1/24, link up.
3. Start C2 in `c2ns` with `--node n1=accept --node-listen 10.99.0.1:18500`.
4. Start the node in `nodens` as root with `GATE4AGENT_WG_INTERFACE=g4anode GATE4AGENT_WG_PRIVATE_KEY=NODE.key GATE4AGENT_WG_ADDRESS=10.99.0.2 GATE4AGENT_WG_PEER_ADDRESS=10.99.0.1 GATE4AGENT_WG_PEER_KEY=C2_PUB GATE4AGENT_WG_PEER_ENDPOINT=192.168.77.1:51820` and `--c2-dial 10.99.0.1:18500`. The log shows "node called home" on the C2 side and `wg show` a handshake.
5. Browser stream: in `c2ns` run `hatchery-bidi-relay --listen 10.99.0.1:18443 --log relay.log`; in `nodens`, as a non-root user, run `dig2-stream-node --c2 http://10.99.0.1:18443 --session s1 --profile DIR --evidence node.txt`; in `c2ns` run `hatchery-bidi --c2 http://10.99.0.1:18443 --session s1 --out hq.png --before b.png --after a.png --clock clock.txt --click 120,90,left`. The node evidence shows the click applied (`dom=clicks: 1 at 120,90`) and the relay log shows `node->hq` and `hq->node` byte counts only.
6. Integrity and reconnect are covered in code by `crates/hatchery-bidi-relay/tests/pump.rs` (SHA-256 per frame both ways, reconnecting peer receives what waited for it; the relay keeps at most 4 frames for an absent peer).
7. Node restart: kill the node, delete its WireGuard interface (`ip link del g4anode`), start it again; C2 logs "node called home" again.

## 3. Docker
Run `crates/hatchery-c2/deploy/docker-build.sh`, `crates/hatchery-server/deploy/docker-build.sh`, `crates/hatchery-node/deploy/node/docker-build.sh` (images `hatchery-c2`, `hatchery-server`, `gate4agent-node:kit`). Smoke: start the C2 container with a volume at `/run/h` (host dir mode 0700) and tokens via `-e`; start the node and server containers with `--network container:<c2 container>` and the same volume; the server health answers on its loopback control port; `POST /sub/memory/start` with the control bearer starts memory on 18200. Mount a provider CLI read-only into the node container to spawn a PTY session.

## 4. QEMU guest
1. Prepare an env file for the guest (`GATE4AGENT_NODE_TOKEN`, the `GATE4AGENT_WG_*` variables with the endpoint of the host as seen from the guest NIC, for example `10.0.2.2:51820` for user-mode networking, and `NODE_ARGS="--node-id n1 --workspace primary=/tmp/ws --endpoint /run/gate4agent/node.sock --c2-dial TUNNEL_IP:PORT"`). Put the guest private key in `$SHARE/guest.key` and reference it as `/mnt/kit/guest.key`.
2. On the host create a WireGuard interface for C2 (address 10.98.0.1/24, listen 51820, peer allowed-ips 10.98.0.2/32) and start C2 with `--node n1=accept --node-listen 10.98.0.1:PORT`.
3. Run `KIT_BIN=... SHARE=... WORKDIR=... GUEST_ENV_FILE=... GUEST_MEM=1536 sh qemu-guest.sh` as root. It downloads Alpine netboot, builds the initramfs, boots (KVM first, TCG when KVM prints nothing) and the guest log shows `WIREGUARD_MODULE=loaded`, `NODE_ATTACHED`. Copy a static provider binary (for example codex, musl) into `$SHARE/bin` to spawn a session through C2.
