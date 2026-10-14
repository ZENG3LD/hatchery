# Session-local server pool (design)

Status: design only. No implementation yet.

## Goal

Give each agent session a small pool of **local Rust HTTP servers** that ship
inside the node (next to dig2browser / d2b). The agent starts and stops them
through a **session-bound subservice** of the node's big MCP. Ports are bound
on loopback (or a session-scoped netns/alias) and are reachable from that
session's dig2browser profile only — not from other sessions, and not from the
public edge.

Typical uses: static site preview, a throwaway HTTP API the agent is coding
against, a mock webhook receiver, a local OpenAPI stub.

## Architecture rules (unchanged)

- Core never sees nodes; C2 stays an opaque restreamer.
- Nested crypto stays: browser traffic already rides the restream uplink.
- Servers are **process-local to the node**, owned by the session lifecycle
  (spawn with the session, SIGTERM on stop/end, hard kill on timeout).
- No public listen: bind `127.0.0.1` (or session IP). Exposure to the operator
  is only through the existing session browser tunnel.

## Surface

### MCP subservice (session-scoped)

Suggested tools (names illustrative):

| Tool | Purpose |
| --- | --- |
| `local_server.list` | Catalog of shipped server kinds + running instances for this session |
| `local_server.start` | `{ kind, root|cwd, port?: "auto", env? }` → `{ id, url, port }` |
| `local_server.stop` | `{ id }` |
| `local_server.logs` | Tail / ring buffer for one instance |

The subservice is registered only while the session holds the capability
(same pattern as other kit MCP overlays). Ports chosen from a per-session
range; dig2browser is told the base URL so "Open" lands on the right port.

### Server kinds (v1)

1. **static** — serve a directory (SPA fallback optional).
2. **http** — run a user-supplied binary/crate entry that speaks HTTP, or a
   thin axum wrapper around a handler script.
3. Later: **sse / ws mock**, **openapi mock** from a YAML file.

## Candidate crates (survey)

Checked on this box under the local `tesserax-maint` checkout (ZENG3LD's own
lightweight server primitives) and dig2browser. Findings:

### Prefer as a dependency: `tesserax-http` (+ `tesserax`)

- Crate family already ships with the teammate stack (Apache-2.0 / MIT).
- `tesserax::ServerBuilder` + `tesserax_http::HttpExt`:
  - `with_static_dir(StaticDir)` for static hosting
  - self-describing routes, OpenAPI, SSE/WS hubs, security guards, CORS
  - lifecycle hooks suitable for start/stop from a supervisor
- Fits "local Rust servers" without inventing a new HTTP stack.
- **Caveat:** tesserax is a back-office / single-writer family; we would take
  only the HTTP surface (or a slim feature set), not the full SWC kernel, for
  session toys. Keep the dependency graph small (feature flags).

### dig2browser (already on the node)

- Owns the session browser; not itself a general static/http server pool.
- Natural place to **register** session-local base URLs so the browser can
  navigate without leaving the profile.
- `dig2browser-runtime-lightweight` is a bounded static-document runtime for
  browser pages — useful for rendering, not for hosting agent-built sites.

### uzor

- Local docs under `uzor-websession-docs` describe UI/framework work
  (tokens, docking, harness). No standalone "mini HTTP server" crate found
  on the box suitable to depend on for static/http hosting.
- Treat uzor as UI/session chrome, not as the server pool.

## Options

| Option | Pros | Cons |
| --- | --- | --- |
| **A. Depend on `tesserax-http`** for static + thin HTTP | Reuses OSS the team already maintains; guards/OpenAPI free | Need a careful feature cut so the node kit does not pull the whole SWC stack |
| **B. Tiny in-tree `hatchery-local-http`** (axum + tower-http `ServeDir` only) | Minimal deps, OSS-agnostic story stays simple | Duplicates what tesserax-http already does well |
| **C. Shell out to `python -m http.server` / caddy** | Fast to prototype | Breaks the "Rust servers shipped with d2b" goal; weaker lifecycle/isolation |

**Decision: A — depend on `tesserax-http` (ZENG3LD's own lightweight server
primitives).** `kind=static` uses `with_static_dir`; `kind=http` launches a
user binary with a fixed `PORT` env. Option B remains a fallback only if the
feature cut is too heavy for the node binary budget. Avoid C except as a spike.

### Current usage in m4a / hatchery / g4a (read-only survey)

On this box, `Cargo.toml` / `Cargo.lock` under hatchery, mail4agent, and
gate4agent were scanned for `tesserax`:

- **hatchery:** not a dependency today.
- **mail4agent:** uses `tesserax-store` (and related pins) for product/example
  surfaces — not `tesserax-http` yet.
- **gate4agent / g4a:** not a dependency today.

So the session-local pool would be the first hatchery/node consumer of
`tesserax-http`; mail4agent already trusts the tesserax family via store.

## Lifecycle sketch

```
session start
  └─ MCP overlay registers local_server.*
agent: local_server.start(kind=static, root=./dist)
  └─ node spawns supervised child (tesserax-http static)
  └─ allocates 127.0.0.1:P from session port pool
  └─ returns url; optional auto-open in session browser
session end / local_server.stop
  └─ SIGTERM → wait → SIGKILL; release port
```

## Non-goals (v1)

- Public or fleet-wide ingress
- Terminating TLS on the node for these toys
- Cross-session port sharing
- Replacing dig2browser

## Open questions

1. Port pool size and conflict policy when two sessions pick the same kind.
2. Whether `kind=http` accepts only allowlisted binaries under the worktree.
3. How much of tesserax-http to vendor vs crates.io pin once versions stabilize.
