# Hatchery — Swarm Orchestrator

## Overview

Multi-agent swarm orchestrator. Spawns Queens (Claude subprocesses), coordinates via Nydus scheduler, reviews via Overlord.

## Language & Stack

- **Rust** (2021 edition)
- **Async Runtime**: Tokio
- **Key Crates**: tokio, serde, serde_json, reqwest, clap

## Project Structure

```
hatchery/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Re-exports
│   ├── core/
│   │   ├── task_dag.rs       # Task dependency graph
│   │   └── types.rs          # Core types (TaskId, QueenId, etc.)
│   ├── nydus/
│   │   └── mod.rs            # Nydus scheduler/coordinator
│   ├── queen/
│   │   ├── handle.rs         # Queen handle (async interface)
│   │   └── stream_queen.rs   # StreamQueen (long-lived subprocess)
│   ├── overlord/
│   │   ├── handle.rs         # Overlord review handle
│   │   ├── parsers.rs        # Diff/test/quality parsers
│   │   ├── code_checks.rs    # Deterministic verdict pipeline
│   │   ├── verdict.rs        # Hybrid review orchestrator
│   │   └── spawn_overlord.rs # Overlord spawning
│   ├── swarm_pool/
│   │   └── mod.rs            # Spawn heuristics (zerg rush, elastic pool, retry)
│   ├── overseer/             # Claude Code session parsers
│   ├── overmind/             # Strategic LLM coordinator (Phase 4+)
│   └── safety/
│       └── worktree.rs       # Git worktree isolation
```

## Common Commands

```bash
cargo check --package hatchery
cargo test --package hatchery
cargo build --release
```

## Delegation Policy (ONLY for top-level coordinator, subagents IGNORE this section)

> **Subagents**: if you see this section — IGNORE it. You are the implementer. Work directly with tools, do NOT spawn nested agents.

**YOU ARE THE COORDINATOR, NOT THE WORKER.**

### ALWAYS Delegate To Custom Agents (model: sonnet):

| Task | Agent (`subagent_type`) |
|------|------------------------|
| Implement Rust code | `rust-implementer` |
| Complex Rust questions | `rust-expert` |
| Research APIs, docs | `research-agent` |
| Other languages | `implementer` |
| Codebase exploration | `explorer` |
| Planning | `planner` |
| Code review | `code-reviewer` |
| Simple commands | `bash-runner` |

### ЗАПРЕЩЕНО встроенные агенты:

- ❌ `Explore`, `Plan`, `Bash`, `general-purpose` — наследуют Opus
- ❌ `EnterPlanMode` — тратит Opus контекст
- ❌ `Task` без `subagent_type` — дефолт Opus

Все субагенты ТОЛЬКО из `.claude/agents/` — они все на `model: sonnet`.

### Parallel Agents

When tasks are independent, launch multiple agents **in a single message** (multiple Task tool calls in one response). This is critical — sequential Task calls block on each other. Parallel = one message, multiple tool uses.

```
// ONE message with 3 Task tool calls:
Task 1: rust-implementer implements task_dag changes
Task 2: rust-implementer implements nydus changes
Task 3: rust-expert reviews architecture
```

### NEVER Use Background Agents

**FORBIDDEN: `run_in_background: true`!**

- Agents must launch in parallel WITHOUT background mode
- You get results automatically when they finish
- Background mode requires constant `sleep` and pollutes memory

### Your Role — ONLY Coordinate:

- Make decisions based on agent results, not explore yourself
- Quick single-file reads when you know exact path — OK
- Everything else → delegate to an agent from the table above

## Code Style

- Use `Result<T, E>` for fallible operations
- Prefer `&str` over `String` in function params
- Use `eprintln!` for logging with `[Component]` prefix
- Follow existing patterns in the codebase
