# Hatchery — Swarm Orchestrator

## Overview

Multi-agent swarm orchestrator. Spawns Queens (Claude subprocesses), coordinates via Nydus scheduler, reviews via Infestor.

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
│   ├── infestor/
│   │   ├── handle.rs         # Infestor review handle
│   │   └── spawn_infestor.rs # Infestor spawning
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

### ALWAYS Delegate:

| Task | Agent |
|------|-------|
| Implement Rust code | `rust-implementer` |
| Complex Rust questions | `rust-expert` |
| Research APIs, docs | `research-agent` |
| Other languages | `implementer` |
| Codebase exploration | Built-in `Explore` |
| Planning | Built-in `Plan` |

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

### NEVER Plan Directly — Delegate Planning

**FORBIDDEN: `EnterPlanMode`!**

- Do NOT enter plan mode yourself — it wastes expensive Opus context on codebase exploration
- Instead send `Plan` or `Explore` subagents for research and planning
- Your role: make decisions based on agent results, not explore yourself

## Code Style

- Use `Result<T, E>` for fallible operations
- Prefer `&str` over `String` in function params
- Use `eprintln!` for logging with `[Component]` prefix
- Follow existing patterns in the codebase
