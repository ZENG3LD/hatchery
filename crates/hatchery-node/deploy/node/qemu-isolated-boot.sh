#!/bin/sh
# Runs inside the QEMU guest as root, started by the guest init when the operator's guest
# environment file sets HATCHERY_GUEST_ISOLATION=1. Order matters:
#   1. enroll (node generates its keys inside the guest; the one-time code comes from the env file)
#   2. unmount the provisioning share, so nothing from the host stays readable by any process
#   3. start the node: enrolled, egress kill-switch, provider sessions as the `agent` user
#   4. optional self-test (HATCHERY_GUEST_SELFTEST=1) that prints P4_TEST lines on the console
# Inputs (guest env file): HATCHERY_ENROLL_ADDR, HATCHERY_ENROLL_CODE, HATCHERY_NODE_ID,
# NODE_ARGS, HATCHERY_GUEST_SELFTEST, HATCHERY_GUEST_FIXTURE_PROVIDER, HATCHERY_UNDERLAY_PROBE.
T=/dev/ttyS0
say() { echo "$*" > "$T"; }
export PATH=/opt/bin:$PATH
mkdir -p /opt/bin /var/lib/hatchery/enroll /run/hatchery /tmp/ws /home/agent
cp -a /mnt/kit/bin/. /opt/bin/
NODE=/opt/bin/gate4agent-node
ST=/var/lib/hatchery
GATEWAY=${HATCHERY_UNDERLAY_PROBE:-10.0.2.2}
chmod 0700 "$ST" /run/hatchery
chown 1000:1000 /home/agent /tmp/ws

if [ "${HATCHERY_GUEST_FIXTURE_PROVIDER:-0}" = 1 ]; then
  # Stand-in provider (no account, no download): reports who it runs as and what it can reach.
  cat > /opt/bin/claude << 'ENDFIX'
#!/bin/sh
case "$1" in --version) echo "2.0.0 (Claude Code)"; exit 0 ;; esac
echo "FIXTURE_UID=$(id -u) GID=$(id -g) HOME=$HOME"
echo "FIXTURE_KEYFILE=$(cat /var/lib/hatchery/enroll/identity.key 2>&1 | head -c 80)"
echo "FIXTURE_STATEDIR=$(ls /var/lib/hatchery/enroll 2>&1 | head -c 80)"
echo "FIXTURE_KITSHARE=$(ls /mnt/kit 2>&1 | wc -l) entries"
echo "FIXTURE_ENV_SECRETS=$(env | grep -c -E 'ENROLL_CODE|WG_PRIVATE|NODE_TOKEN')"
echo "FIXTURE_NFT=$(nft list ruleset 2>&1 | head -c 60)"
echo "FIXTURE_READY"
exec cat
ENDFIX
  chmod 0755 /opt/bin/claude
fi

if ping -c1 -W2 "$GATEWAY" > /dev/null 2>&1; then say "P4_BASELINE underlay_ping=reachable"; else say "P4_BASELINE underlay_ping=unreachable"; fi

if "$NODE" enroll --c2-enroll "$HATCHERY_ENROLL_ADDR" --node-id "$HATCHERY_NODE_ID" --kind qemu \
     --state-dir "$ST/enroll" > /tmp/enroll.log 2>&1; then
  say "P4_ENROLL ok: $(cat /tmp/enroll.log)"
else
  say "P4_ENROLL failed: $(cat /tmp/enroll.log)"
fi
unset HATCHERY_ENROLL_CODE
ENROLL_PORT=${HATCHERY_ENROLL_ADDR##*:}

cd / || true
if umount /mnt/kit 2> /dev/null; then say "P4_SHARE dropped"; else say "P4_SHARE still-mounted"; fi

export HOME=/root XDG_STATE_HOME="$ST"
# shellcheck disable=SC2086
"$NODE" --enrolled --enroll-state "$ST/enroll" --kill-switch --run-as agent \
  --endpoint /run/hatchery/node.sock ${NODE_ARGS:-} > /tmp/node.log 2>&1 &
echo $! > /tmp/node.pid

i=0
up=0
while [ "$i" -lt 90 ]; do
  hs=$(wg show hatch0 latest-handshakes 2> /dev/null | awk '{print $2}')
  if [ -n "$hs" ] && [ "$hs" != 0 ]; then up=1; break; fi
  kill -0 "$(cat /tmp/node.pid)" 2> /dev/null || break
  i=$((i + 1)); sleep 1
done
if [ "$up" = 1 ]; then say "P4_TUNNEL handshake-ok after ${i}s"; else say "P4_TUNNEL no-handshake"; fi
say "P4_NODELOG $(head -c 600 /tmp/node.log | tr '\n' '|')"
sleep 3
say "P4_RULES $(nft list table inet hatchery_killswitch 2>&1 | tr '\n' ' ' | tr -s ' ')"

[ "${HATCHERY_GUEST_SELFTEST:-0}" = 1 ] || exit 0
AS="$NODE exec-as agent --"
check() { # name, expected (0 = command must succeed, 1 = must fail), command...
  name=$1; want=$2; shift 2
  out=$("$@" 2>&1); rc=$?
  [ "$rc" -ne 0 ] && got=1 || got=0
  if [ "$got" = "$want" ]; then r=PASS; else r=FAIL; fi
  say "P4_TEST $name result=$r rc=$rc out=$(echo "$out" | head -c 100 | tr '\n' ' ')"
}
# Agent account cannot reach the node's keys or the host share, cannot touch the firewall.
check agent_uid_is_1000 0 $AS sh -c '[ "$(id -u)" = 1000 ]'
check agent_cannot_read_identity_key 1 $AS cat "$ST/enroll/identity.key"
check agent_cannot_list_state_dir 1 $AS ls "$ST/enroll"
check agent_cannot_read_wg_key 1 $AS sh -c "cat $ST/enroll/wg.key"
check host_share_is_unmounted 0 sh -c '[ -z "$(ls /mnt/kit 2>/dev/null)" ]'
check agent_cannot_flush_firewall 1 $AS nft flush ruleset
check agent_cannot_delete_tunnel 1 $AS ip link del hatch0
check agent_cannot_open_underlay_tcp 1 $AS nc -w 2 "$GATEWAY" "$ENROLL_PORT"
# Kill-switch: the underlay is closed to everything but the tunnel's own UDP.
check root_underlay_ping_dropped 1 ping -c1 -W2 "$GATEWAY"
check root_underlay_tcp_dropped 1 nc -w 2 "$GATEWAY" "$ENROLL_PORT"
check root_internet_dropped 1 nc -w 2 1.1.1.1 443
# Window for the HQ side to drive a provider session through C2 before the tunnel is cut.
say "P4_PHASE_A_DONE; tunnel is cut in ${HATCHERY_VPNDOWN_DELAY:-90}s"
sleep "${HATCHERY_VPNDOWN_DELAY:-90}"
# VPN down: the tunnel interface is removed (stand-in for gateway/proxy gone). No leak.
before=$(nft list chain inet hatchery_killswitch output 2>/dev/null | sed -n 's/.*counter packets \([0-9]*\).*/\1/p')
ip link del hatch0 2> /dev/null
sleep 2
check vpn_down_underlay_ping_dropped 1 ping -c1 -W2 "$GATEWAY"
check vpn_down_underlay_tcp_dropped 1 nc -w 2 "$GATEWAY" "$ENROLL_PORT"
check vpn_down_internet_dropped 1 nc -w 2 1.1.1.1 443
check vpn_down_dns_dropped 1 nc -u -w 2 -z 8.8.8.8 53
after=$(nft list chain inet hatchery_killswitch output 2>/dev/null | sed -n 's/.*counter packets \([0-9]*\).*/\1/p')
say "P4_COUNTER dropped_packets_before=${before:-?} after=${after:-?}"
say "P4_SELFTEST_DONE"
