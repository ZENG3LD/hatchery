# Backlog: node-mail wake is dead since A2; wake bridge has no supervision

## Symptom
- Node logs `node-mail wake: inbox unavailable` every 10 s.
- The door returns `{"error":"unavailable"}` (HTTP 502) for `mail.inbox`. The external wake bridge logs `502 Bad Gateway` in a loop.

## Root cause
- `mail.inbox` / `mail.ack` / `mail.send` are signed node calls to the C2 enroll listener.
- Since A2, C2 never dials core. `handle_node_call` applies calls only through a `node_call_sink`. No production sink is wired, so every call gets `410 use restreamer uplink`.
- The node maps that to "unavailable". The node mailbox therefore cannot be read through C2 at all. This is not a node, bridge or credential problem.
- Fix direction: carry node-mail ops over the restreamer uplink (Noise to core), or wire a `NodeCallSink` that enqueues into the drain. The second option keeps C2 opaque.

## Why the wake bridge "dies"
1. The node supervisor script used to start the bridge in its own restart loop. When the node's built-in wake (`--wake-policy`, `node_mail_to`) replaced the bridge, that loop was commented out. Since then the bridge only runs when started by hand, and it does not survive a supervisor restart or a reboot.
2. Even when it runs, it only produces errors until the mailbox path above is fixed.
3. Do not run both the bridge and the node's built-in `node_mail_to` once mail works again: both poll the same inbox and would double-wake. Pick one (prefer the built-in).

## Supervision instruction (to implement later)
- Hosts with systemd: a user unit with `Restart=always`, `RestartSec=5` and `After=` the node unit. Log to the journal.
- Hosts without systemd (container boxes): keep exactly one supervisor script, holding a flock, that owns all loops (node, and the bridge only if used). It is started by the box boot hook. No ad-hoc `nohup` loops.
- Health: alert when the door's `mail.inbox` returns non-200 for more than 5 minutes.
