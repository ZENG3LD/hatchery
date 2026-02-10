---
name: agents-tasks
description: "Spawn parallel agents for complex tasks using Claude Code's native Task tool and Agent Teams. Use when you need to parallelize work, coordinate multiple agents, or decompose complex goals."
argument-hint: "<task-description> (describe what needs to be done in parallel)"
---

## Agents-Tasks: Native Parallel Execution

General-purpose pattern for decomposing tasks and executing them in parallel using Claude Code's built-in Task tool and Agent Teams.

### When to Use

- Task can be split into 2+ independent subtasks
- Multiple files/modules need changes simultaneously
- Research + implementation can happen in parallel
- Complex task needs decomposition before execution
- Need coordinated multi-agent work with shared state

### When NOT to Use

- Simple single-file edit → just do it directly
- Sequential dependency chain → use /carousel instead
- Batch same-type tasks → use /swarm instead

---

### Mode 1: Parallel Task Spawning (Default)

Decompose task into independent subtasks, spawn agents in ONE message (parallel execution).

**Pattern:**
```
1. Analyze the task
2. Identify independent subtasks
3. Choose agent type for each subtask
4. Spawn ALL independent agents in a single message (parallel)
5. Collect results
6. Handle dependencies: spawn next wave
7. Synthesize and report
```

**Agent Type Selection:**

| Task Type | Agent | When |
|-----------|-------|------|
| Rust code changes | `rust-implementer` | Writing/editing .rs files |
| Other languages | `implementer` | TypeScript, Python, Go, etc. |
| API/docs research | `research-agent` | Web research, API documentation |
| Architecture decisions | `rust-expert` | Trait design, unsafe code review |
| Codebase exploration | `Explore` | Finding files, understanding patterns |
| Design/planning | `Plan` | Implementation strategy, architecture |
| Shell commands | `Bash` | Build, test, deploy operations |

**Example — Feature implementation:**
```
User: "Add WebSocket support to the trading engine"

Wave 1 (parallel):
  Task(research-agent): "Research WebSocket protocols for Binance, Bybit, OKX. Document message formats."
  Task(Explore): "Find all existing WebSocket code in the codebase. Map current architecture."
  Task(Plan): "Design WebSocket manager architecture. Consider: reconnection, heartbeat, multiplexing."

Wave 2 (after Wave 1 completes, parallel):
  Task(rust-implementer): "Implement WebSocketManager struct based on the plan. File: src/ws/manager.rs"
  Task(rust-implementer): "Implement message parser for Binance WS format. File: src/ws/parsers/binance.rs"
  Task(rust-implementer): "Implement message parser for Bybit WS format. File: src/ws/parsers/bybit.rs"

Wave 3 (sequential):
  Task(rust-implementer): "Write tests for WebSocket manager. Run cargo test."
```

---

### Mode 2: Agent Teams (Persistent Coordination)

For long-running tasks where agents need to communicate with each other.

**When to use Teams vs Tasks:**
- Tasks: fire-and-forget, result comes back to you
- Teams: agents talk to each other, shared task list, persistent state

**Pattern:**
```
1. TeamCreate(team_name="feature-x", description="Implementing feature X")
2. Spawn teammates with Task tool (team_name parameter)
3. Create shared task list with TaskCreate
4. Teammates claim tasks, work, report via SendMessage
5. Coordinate: reassign blocked tasks, handle failures
6. Shutdown teammates when done
7. TeamDelete to clean up
```

**Example — Full-stack feature:**
```
TeamCreate(team_name="auth-system")

Spawn:
  Task(name="researcher", subagent_type="research-agent", team_name="auth-system",
       prompt="Research JWT + OAuth2 patterns for Rust. Report findings to team.")
  Task(name="backend-dev", subagent_type="rust-implementer", team_name="auth-system",
       prompt="Implement auth middleware. Wait for researcher's findings first.")
  Task(name="tester", subagent_type="rust-implementer", team_name="auth-system",
       prompt="Write auth tests. Wait for backend-dev to finish implementation.")

Coordinator monitors progress via task list and messages.
```

---

### Execution Rules

1. **ALWAYS parallelize independent work** — never serialize what can run concurrently
2. **Wave pattern** — group tasks by dependency level, launch each wave in parallel
3. **Fail fast** — if a critical agent fails, don't wait for others before reacting
4. **Context is key** — give each agent enough context to work independently (file paths, references, patterns to follow)
5. **Verify gates** — after each wave, run verification (cargo check, cargo test) before next wave
6. **Max 5 agents per wave** — practical limit for reliable parallel execution
7. **Descriptive prompts** — each agent should know: WHAT to do, WHERE (file paths), HOW (reference patterns), WHY (context)

### Prompt Template for Agents

```
Task(subagent_type="{AGENT_TYPE}", prompt="
## Task
{WHAT_TO_DO}

## Files
- Input: {FILES_TO_READ}
- Output: {FILES_TO_CREATE_OR_MODIFY}

## Reference
Follow patterns from: {REFERENCE_FILES}

## Verification
After completion, run: {VERIFY_COMMAND}
")
```

### Anti-Patterns

- **Don't over-decompose** — 2-3 agents for a simple feature is enough
- **Don't spawn agents for trivial work** — if it's a 5-line change, just do it
- **Don't use Teams for short tasks** — Teams overhead isn't worth it for < 10 min work
- **Don't forget verification gates** — always check compilation/tests between waves
