# hatchery

hatchery is the operator HQ above provider nodes: harness, observation,
and TUI for running and orchestrating CLI coding-agent sessions (first
tier: Claude Code, Codex, Kimi, Grok). Providers, the node daemon, and C2
live in the sibling `gate4agent` repository (see [Built on](#built-on));
this repository keeps harness / observation / TUI / arcade. A harness —
light or full — is the one stateful backend a client app talks to, behind
a single app-facing protocol, adding task kanban, session context, and
delivery on top of the c2 transport; the TUI is the current client, and it
speaks only the harness operator wire in either mode.

## Layers

One direction of wrapping: providers → node → c2 → harness → client app.
Providers, the node and the c2 live in `gate4agent` (see [Built on](#built-on)
below); the harness, the observation side and the client app are `hatchery-*`
in this repository. Crate names below are prefixed `hatchery-` unless noted.

- **Providers** — blackbox vendor CLIs (Claude Code, Codex, Kimi, Grok,
  qwen-code) wrapped by the transport core in `gate4agent`: root crate
  `gate4agent`, `gate4agent-pty`, `-types`, `-adapters`, `-provider-ports`,
  `-catalog`, `-engine`, `-kernel`, `-handle`, `-tool-protocol`,
  `-tool-engine`, `-shell-history`, `-shell-capabilities`, `-shell-hooks`,
  `-shell-managed-hooks`, `-shell-one-shot`, `-shell-native`,
  `-runtime-native`. Not part of this repository — linked by path, see
  [Built on](#built-on).
- **Observation** — read-only monitoring facts projected from provider
  sessions, never prompts/transcripts/credentials: `-observation-protocol`,
  `-observation-api`, `-observation-engine`, `-observation-store`,
  `-observation-service`. The node names no observation type: the projection
  from the node's own control events, agent-stream `Blocked` chunks and
  record history summaries is `hatchery_observation_engine::node_projection`.
- **Node** — wraps providers on one machine: PTY/inline sessions, the file
  browser, local git, worktrees. `gate4agent`'s: `gate4agent-node-protocol`,
  `gate4agent-node-wire`, `gate4agent-node` (bin `gate4agent-node`).
- **C2** — relays any number of nodes to their clients and routes commands
  (spawn, session control) down to nodes. `gate4agent`'s:
  `gate4agent-c2-protocol`, `gate4agent-c2-client` (bin `gate4agent-c2ctl`),
  `gate4agent-c2` (bin `gate4agent-c2`).
- **Harness** — the stateful backend behind one app-facing protocol: task
  kanban over SQLite, session extraction/continuation, delivery of
  skills/plugins/MCP config, an operator surface: `-harness-protocol`,
  `-harness-engine`, `-harness-service` (bin `hatchery-harness`),
  `-harness-api`, `-harness-client` (bin `hatchery-harnessctl`),
  `-harness-mcp` (bin `hatchery-harness-mcp`), `-harness-delivery`,
  `-harness-light` (stateless, serves the same operator wire straight over
  c2 with no task kernel behind it).
- **Client** — `crates/hatchery-tui` (root workspace member): bins
  `hatchery-tui` (against a durable harness) and `hatchery-tui-light`
  (hosts `hatchery-harness-light` in-process). Neither app speaks c2
  itself.
- **Arcade** — `crates/hatchery-arcade/{engine,games/*,sweep,preview,bench}`
  as root workspace members (`hatchery-arcade-*`); hatchery-owned, not g4a.
- **Build stamp** — `gate4agent-build-stamp` (in `gate4agent`): a git
  content-hash of that repository's working tree, carried by every wire
  handshake instead of a hand-typed protocol version number. hatchery's
  binaries carry it through `gate4agent-node-protocol`.

Pipe and env-var names above still carry the `gate4agent-`/`GATE4AGENT_`
prefix inherited from the repository this stack was split out of — a later
step (§11.4 / brand Step 4b) renames those **hatchery-side** runtime
identifiers. TUI binaries are already `hatchery-tui` / `hatchery-tui-light`.
Full ledger: hatchery-websession-docs
`plans/harness-mcp-bin-env-domain-rename-2026-10-02.md` and `STATUS.md`.

| Runtime | Current | Deferred target / note |
|---|---|---|
| Helper bin | `hatchery-harness-mcp` | **done** wave C |
| Harness / ctl bins | `hatchery-harness`, `hatchery-harnessctl` | **done** wave C |
| Session MCP env | `HATCHERY_HARNESS_SESSION_*` | already final |
| Legacy direct MCP env | `GATE4AGENT_HARNESS_READ_*` | rename or retire; scrubbed on product spawn |
| Operator / TUI env | `GATE4AGENT_HARNESS_*`, `GATE4AGENT_TUI_*` | `HATCHERY_*` |
| Node / C2 tokens & pipes | `GATE4AGENT_NODE_*`, `GATE4AGENT_C2_*`, `gate4agent-node` / `-c2` | **keep** (g4a-owned; not hatchery rename) |
| Capability | `harness-mcp-read-proxy-v1` | **keep** (C2 negotiate) |

## Repository layout

Everything in this repository lives under `crates/`: harness, observation,
TUI, and arcade. Node and C2 are **not** in-tree — they are path
dependencies on a sibling `gate4agent` checkout. Every `hatchery-*` crate
that needs the transport core or node/C2 wire depends on
`../../../gate4agent/crates/<crate>` (directory name must be exactly
`gate4agent` next to `hatchery`; a symlink is fine). crates.io is not used
for those links.

For a complete build against current node/C2, check out `gate4agent` on
branch `websession` (node + C2 returned there). `master` is the older
library cut without that return.

`crates/hatchery-tui` and the `hatchery-arcade-*` crates are members of
the root workspace (see [The TUI's uzor dependency](#the-tuis-uzor-dependency)
below). There is no nested `[workspace]` under `crates/`.

## Local endpoints

| Layer | Local pipe | API |
|---|---|---|
| Node | `\\.\pipe\gate4agent-node` (Unix: local socket) | `127.0.0.1:18310` |
| C2 | `\\.\pipe\gate4agent-c2` (Unix: local socket) | `127.0.0.1:18320` |
| Harness | — | operator surface on `127.0.0.1:18330` |

The harness does not share the primary c2 — it connects out through a second
c2 instance of its own, on pipe `gate4agent-c2-harness` with API
`127.0.0.1:18321`. A live stack is therefore **four** processes: node, two
c2, harness. Bring one up without that second instance and the harness fails
at startup with a bare connect error.

Only node's `18310` and c2's `18320` are compiled-in defaults. `18321` and
`18330` are conventions passed on the command line (`--api-listen`,
`--read-bind`), so grepping the source for them finds nothing.

All of it is loopback/local-only; nothing here is reachable off the host by
default.

## Credentials

Env vars only — never pass a token as argv, never commit a value:

- `GATE4AGENT_NODE_TOKEN`, or `GATE4AGENT_NODE_TOKEN_<NORMALIZED_ID>` for a
  per-node override (id uppercased, non-alphanumeric characters replaced with
  `_`)
- `GATE4AGENT_C2_TOKEN`
- `GATE4AGENT_HARNESS_OPERATOR_TOKEN`

## Tests

Windows PTY/session-touching tests in the node/c2/harness crates run only
through the headless test supervisor — `gate4agent-testkit`'s
`windows-headless-supervisor` binary, which stays in the `gate4agent`
repository (sibling checkout, linked by path, not vendored here). It
suppresses Windows fault dialogs and enforces a hard per-test timeout that
plain `cargo test` cannot:

```
..\gate4agent\target\release\windows-headless-supervisor.exe <timeout_ms> <ABS path to test exe> --exact <test_fn>
```

All builds share the workspace's own root `target/`. A
per-run `--target-dir` is a full copy of the dependency build and they
pile up fast; when two builds overlap, Cargo's build lock simply makes the
second wait. Tests gated by `require_windows_headless_supervisor_for_test()`
reject themselves outright if run any other way.

## The TUI's uzor dependency

`crates/hatchery-tui` depends on the `uzor-tui` crate from crates.io (the
uzor UI framework, maintained by the same owner). It pulls from crates.io,
not a sibling checkout, so it adds no repository requirement beyond the
one every `hatchery-*` crate already has (`gate4agent`, see
[Built on](#built-on)). The status bar's pet overlay links **in-tree**
`crates/hatchery-arcade` (engine + pet-bastion). Arcade is part of hatchery,
not a `gate4agent` sibling and not part of g4a; packages are
`hatchery-arcade-*` and live in the root workspace.

## Built on

- **[gate4agent](https://github.com/ZENG3LD/gate4agent)** — providers,
  node daemon, and C2 this HQ talks to: spawning/streaming CLI agents over
  PTY/pipe/ACP, plus the node/C2 wire. Sibling checkout linked by path (see
  [Repository layout](#repository-layout)); use branch `websession` when
  building hatchery against current node/C2.
- **[mail4agent](https://github.com/ZENG3LD/mail4agent)** — agent mail as its
  own service, local API on `127.0.0.1:18301`. The harness keeps no mailbox
  of its own.

## License

MIT
