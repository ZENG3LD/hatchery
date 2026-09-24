# hatchery

hatchery is the agent control plane: a node/c2/harness/TUI stack for
running, observing, and orchestrating CLI coding-agent sessions (first
tier: Claude Code, Codex, Kimi, Grok), built on the `gate4agent` transport
core. A node wraps one machine's providers (PTY/inline sessions, the file
browser, local git, worktrees); c2 relays any number of nodes to their
clients; a harness — light or full — is the one stateful backend a client
app talks to, behind a single app-facing protocol, adding task kanban,
session context, and delivery on top of the c2 transport; the TUI is the
current client, and it speaks only the harness operator wire in either
mode.

## Layers

One direction of wrapping: providers → node → c2 → harness → client app.
Providers live in `gate4agent` (see [Built on](#built-on) below); node and
everything above it is `hatchery-*` in this repository. Crate names below
are prefixed `hatchery-` (e.g. `-node` = `hatchery-node`) unless noted.

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
  `-observation-service`.
- **Node** — wraps providers on one machine: PTY/inline sessions, the file
  browser, local git, worktrees: `-node-protocol`, `-node-wire`, `-node`
  (bin `gate4agent-node`).
- **C2** — relays any number of nodes to their clients and routes commands
  (spawn, session control) down to nodes: `-c2-protocol`, `-c2-client`
  (bin `gate4agent-c2ctl`), `-c2` (bin `gate4agent-c2`).
- **Harness** — the stateful backend behind one app-facing protocol: task
  kanban over SQLite, session extraction/continuation, delivery of
  skills/plugins/MCP config, an operator surface: `-harness-protocol`,
  `-harness-engine`, `-harness-service` (bin `gate4agent-harness`),
  `-harness-api`, `-harness-client` (bin `gate4agent-harnessctl`),
  `-harness-mcp` (bin `gate4agent-harness-mcp`), `-harness-delivery`,
  `-harness-light` (stateless, serves the same operator wire straight over
  c2 with no task kernel behind it).
- **Client** — `crates/hatchery-tui`, its own nested cargo workspace: bins
  `gate4agent-tui` (against a durable harness) and `gate4agent-tui-light`
  (hosts `hatchery-harness-light` in-process). Neither app speaks c2
  itself.
- **Build stamp** — `hatchery-build-stamp`: a git content-hash of this
  repository's own working tree, carried by every wire handshake instead
  of a hand-typed protocol version number.

Binary, pipe, and env-var names above still carry the `gate4agent-`/
`GATE4AGENT_` prefix inherited from the repository this stack was split
out of — a later step renames these runtime identifiers to `hatchery-*`.

## Repository layout

Everything in this repository lives under `crates/`: the workbench layers
(node, c2, harness, TUI) and the engine substrate under them. Every
`hatchery-*` crate that needs the transport core depends on `gate4agent`'s
crates by path (e.g. `../../../gate4agent/crates/gate4agent-types`) rather
than through crates.io — a checkout of this repository is only complete
once `gate4agent` is checked out beside it.

`crates/hatchery-tui` is its own nested cargo workspace (see
[The TUI's uzor dependency](#the-tuis-uzor-dependency) below); every other
`hatchery-*` crate is a member of the root workspace at the repository
root.

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

All builds share the workspace's own `target/` (the root `target/` plus
`crates\hatchery-tui\target\` for the TUI's own nested workspace). A
per-run `--target-dir` is a full copy of the dependency build and they
pile up fast; when two builds overlap, Cargo's build lock simply makes the
second wait. Tests gated by `require_windows_headless_supervisor_for_test()`
reject themselves outright if run any other way.

## The TUI's uzor dependency

`crates/hatchery-tui` depends on the `uzor-tui` crate from crates.io (the
uzor UI framework, maintained by the same owner). It pulls from crates.io,
not a sibling checkout, so it adds no repository requirement beyond the
one every `hatchery-*` crate already has (`gate4agent`, see
[Built on](#built-on)). It also links `gate4agent-arcade`'s engine and
`pet-bastion` game crates by path, for the status bar's pet overlay — a
further sibling repository (`../gate4agent-arcade` next to this one).

## Built on

- **[gate4agent](https://github.com/ZENG3LD/gate4agent)** — the transport core library this stack
  is built on: spawning, streaming, and resuming CLI coding-agent
  subprocesses over PTY/pipe/ACP/daemon transports. A sibling repository,
  linked by path, not vendored — every `hatchery-*` crate that touches a
  provider depends on one of its crates directly.
- **[mail4agent](https://github.com/ZENG3LD/mail4agent)** — agent mail as its
  own service, local API on `127.0.0.1:18301`. The harness keeps no mailbox
  of its own.

## License

MIT
