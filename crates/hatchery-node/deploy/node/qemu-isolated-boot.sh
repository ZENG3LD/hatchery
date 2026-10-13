#!/bin/sh
# Runs inside the QEMU guest as root, started by the guest init when the operator's guest
# environment file sets HATCHERY_GUEST_ISOLATION=1. Order matters:
#   1. enroll (node generates its keys inside the guest; the one-time code comes from the env file)
#   2. unmount the provisioning share, so nothing from the host stays readable by any process
#   3. split key holder from network admin: `net-up` (root) brings up WireGuard and installs the
#      egress kill-switch, the WireGuard key moves to a root-only directory, then
#      `drop-privileges` starts the long-lived node as the `node` user with only CAP_SETUID and
#      CAP_SETGID (no CAP_NET_ADMIN, no_new_privs); it starts provider sessions as `agent`
#   4. optional self-test (HATCHERY_GUEST_SELFTEST=1) that prints P4_TEST lines on the console
# Inputs (guest env file): HATCHERY_ENROLL_ADDR, HATCHERY_ENROLL_CODE, HATCHERY_NODE_ID,
# NODE_ARGS, HATCHERY_GUEST_SELFTEST, HATCHERY_GUEST_FIXTURE_PROVIDER, HATCHERY_UNDERLAY_PROBE.
T=/dev/ttyS0
say() { echo "$*" > "$T"; }
export PATH=/opt/bin:$PATH
mkdir -p /opt/bin /var/lib/hatchery/enroll /run/hatchery /tmp/ws /home/agent
cp -a /mnt/kit/bin/. /opt/bin/
chown -R 0:0 /opt/bin 2>/dev/null; chmod -R a+rX /opt/bin; chmod 0755 /opt /opt/bin
# The share is staged 0600 by the host; unprivileged users (node, agent) must be able to load the libraries too.
chmod -R a+rX /lib/x86_64-linux-gnu /lib64 /usr/lib 2> /dev/null
NODE=/opt/bin/gate4agent-node
ST=/var/lib/hatchery
: "${HATCHERY_ENROLL_ADDR:?set HATCHERY_ENROLL_ADDR (host:port of the C2 enroll listener) in the guest env file}"
: "${HATCHERY_NODE_ID:?set HATCHERY_NODE_ID in the guest env file}"
GATEWAY=${HATCHERY_UNDERLAY_PROBE:-10.0.2.2}
INTERNET_PROBE=${HATCHERY_INTERNET_PROBE:?set HATCHERY_INTERNET_PROBE to a public IP the guest must NOT reach (isolation probe)}
DNS_PROBE=${HATCHERY_DNS_PROBE:?set HATCHERY_DNS_PROBE to a public DNS resolver IP the guest must NOT reach}
chmod 0700 "$ST" /run/hatchery
chown 1000:1000 /home/agent /tmp/ws
mkdir -p /home/node /var/lib/hatchery-net
chown 1001:1001 /home/node
chmod 0700 /var/lib/hatchery-net

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

# Key split: the WireGuard key is only needed by `net-up`; the node user gets the identity key only.
mv "$ST/enroll/wg.key" /var/lib/hatchery-net/wg.key
chown -R 1001:1001 "$ST" /run/hatchery
if ! "$NODE" net-up --enroll-state "$ST/enroll" --wg-key /var/lib/hatchery-net/wg.key --kill-switch > /tmp/netup.log 2>&1; then
  say "P4_NETUP failed: $(cat /tmp/netup.log)"
else
  say "P4_NETUP $(cat /tmp/netup.log)"
fi
export XDG_STATE_HOME="$ST"
# shellcheck disable=SC2086
"$NODE" drop-privileges --user node -- "$NODE" --enrolled --enroll-state "$ST/enroll" --no-wg --run-as agent \
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
# Split: the long-lived node is not root, holds only setuid/setgid, and cannot reach the network
# configuration or the WireGuard key.
NODEPID=$(cat /tmp/node.pid)
check node_process_is_not_root 0 sh -c "[ \"\$(awk '/^Uid:/{print \$2}' /proc/$NODEPID/status)\" = 1001 ]"
check node_caps_are_setid_only 0 sh -c "[ \"\$(awk '/^CapEff:/{print \$2}' /proc/$NODEPID/status)\" = 00000000000000c0 ] && [ \"\$(awk '/^CapBnd:/{print \$2}' /proc/$NODEPID/status)\" = 00000000000000c0 ]"
check node_has_no_new_privs 0 sh -c "[ \"\$(awk '/^NoNewPrivs:/{print \$2}' /proc/$NODEPID/status)\" = 1 ]"
NODEAS="$NODE drop-privileges --user node --"
check node_user_cannot_read_wg_key 1 $NODEAS cat /var/lib/hatchery-net/wg.key
check node_user_cannot_flush_firewall 1 $NODEAS nft flush ruleset
check node_user_cannot_delete_tunnel 1 $NODEAS ip link del hatch0
check node_user_can_read_its_identity 0 $NODEAS sh -c "test -r $ST/enroll/identity.key"
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
check root_internet_dropped 1 nc -w 2 "$INTERNET_PROBE" 443
# Remote build (optional): HATCHERY_GUEST_BUILD="RECIPE SIGNER_PUBLIC_KEY BUILD_ENDPOINT". The guest
# holds no signing key and no build-host credential; it asks C2 as the unprivileged node user.
# The first request is expected to be refused (neutral); the operator then raises the status and
# the retries must succeed with an artifact whose signature and hash the node verified itself.
if [ -n "${HATCHERY_GUEST_BUILD:-}" ]; then
  set -- $HATCHERY_GUEST_BUILD
  B_RECIPE=$1; B_SIGNER=$2; B_ENDPOINT=$3
  build_once() {
    $NODEAS "$NODE" build --c2-enroll "$B_ENDPOINT" --state-dir "$ST/enroll" --recipe "$B_RECIPE" \
      --source-ref main --trusted-signer "$B_SIGNER" --out /tmp/built.bin
  }
  check build_refused_while_not_trusted 1 build_once
  n=0
  until out=$(build_once 2>&1); do
    n=$((n + 1)); [ "$n" -ge 60 ] && break; sleep 4
  done
  case "$out" in
    *signature_ok=true*) say "P4_TEST remote_build result=PASS out=$out content=$(head -c 60 /tmp/built.bin | tr '\n' ' ') sha256_local=$(sha256sum /tmp/built.bin | cut -d' ' -f1)" ;;
    *) say "P4_TEST remote_build result=FAIL out=$out" ;;
  esac
fi
# Window for the HQ side to drive a provider session through C2 before the tunnel is cut.
say "P4_PHASE_A_DONE; tunnel is cut in ${HATCHERY_VPNDOWN_DELAY:-90}s"
sleep "${HATCHERY_VPNDOWN_DELAY:-90}"
# VPN down: the tunnel interface is removed (stand-in for gateway/proxy gone). No leak.
before=$(nft list chain inet hatchery_killswitch output 2>/dev/null | sed -n 's/.*counter packets \([0-9]*\).*/\1/p')
ip link del hatch0 2> /dev/null
sleep 2
check vpn_down_underlay_ping_dropped 1 ping -c1 -W2 "$GATEWAY"
check vpn_down_underlay_tcp_dropped 1 nc -w 2 "$GATEWAY" "$ENROLL_PORT"
check vpn_down_internet_dropped 1 nc -w 2 "$INTERNET_PROBE" 443
check vpn_down_dns_dropped 1 nc -u -w 2 -z "$DNS_PROBE" 53
after=$(nft list chain inet hatchery_killswitch output 2>/dev/null | sed -n 's/.*counter packets \([0-9]*\).*/\1/p')
say "P4_COUNTER dropped_packets_before=${before:-?} after=${after:-?}"
say "P4_SELFTEST_DONE"
