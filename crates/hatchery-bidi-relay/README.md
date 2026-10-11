# hatchery-bidi-relay

The C2 side of the bidirectional browser stream: a dumb byte relay. `POST /session/{id}` opens a slot, `GET /session/{id}/node` and `/session/{id}/hq` (WebSocket) are the two directions. Binary frames are copied from one socket to the other; nothing is decoded, and the only log line is `bytes=N`.

- Node side: `dig2-stream-node --c2 http://C2_ADDR:PORT --session ID ...` (crate `dig2browser`).
- HQ side: `hatchery-bidi --c2 http://C2_ADDR:PORT --session ID ...` (crate `hatchery-bidi-client`).
- Run it on the C2 host bound to the C2 WireGuard tunnel address (`hatchery-bidi-relay --listen TUNNEL_IP:PORT`). It has no authentication of its own: never bind it to a public address.

Lineage: ported from gate4agent `gate4agent-bidi-relay` (commit fa8b66e, `publish = false` there). Differences: renamed, publishable, a peer that disconnects stops being routed into so frames wait (bounded) for its reconnect, and `tests/pump.rs` checks both directions by SHA-256 and a reconnect.

## Buffering for an absent peer

Frames sent before the other side connects (or while it reconnects) wait in a
per-direction queue and are delivered in order when it arrives. The queue is
bounded: 256 frames and 64 MiB by default (`--max-pending-frames N`,
`--max-pending-bytes N`, or `RelayLog::with_limits` in code). Beyond a bound the
oldest frames are dropped, counted per session, and a line
`bidi-relay WARN session=ID pending buffer full: dropped_frames=.. dropped_bytes=.. total_dropped=..`
is logged. Only counts are logged, never frame content.
