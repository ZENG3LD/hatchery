# Two-host deploy pattern used for hatchery.fyi (no secrets here)

- Edge (public): `hatchery-c2` only, plus Caddy serving the static landing from `site/hatchery/`. Enrollment is plain TCP on a dedicated port at `enroll.<domain>` (DNS-only, not proxied: nodes speak a signed HTTP/1 request to C2 directly, Caddy is not in the path). After enrollment, the node uses WireGuard to the edge (`wg-hatch`) for call-home and door calls.
- Core (private): `hatchery-server` (provisioning, messenger, tasks, policy, harness). Core and edge talk over a private WireGuard link: C2 operator API reaches core through a socket proxy bound to the edge tunnel address; C2 reaches the server control through a socket proxy bound to the core tunnel address. Firewall rules allow those two ports only from the peer tunnel address.
- Web HQ is not deployed: it runs on the operator's own machine and reaches C2 API, C2 control socket and server control through SSH port/socket forwards. Tokens stay in the HQ process environment; the browser never sees them.
- C2 unit needs `CAP_NET_ADMIN` (it adds and removes WireGuard peers on enroll/revoke) and a `StateDirectory` for the registry.
- Landings: `site/hatchery/index.html` (hatchery) and `site/mail4agent/index.html` (served at the exact root of the messenger domain; every other path still goes to the messenger).
