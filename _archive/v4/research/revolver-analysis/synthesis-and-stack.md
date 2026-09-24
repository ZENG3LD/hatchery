# Swarm Orchestration: Synthesis & Technology Stack Recommendation

**Date**: 2026-02-08
**Purpose**: Map CLI agent features to swarm needs, recommend Rust-based technology stack, identify reusable components
**Sources**: Revolver research (compression, prompting, transport, storage specs) + Open source analysis (Parts 1-3)

---

## Table of Contents

1. [CLI Agent Features → Swarm Needs Mapping](#1-cli-agent-features--swarm-needs-mapping)
2. [Technology Stack for Rust-Based Swarm](#2-technology-stack-for-rust-based-swarm)
3. [Reuse vs Build from Scratch](#3-reuse-vs-build-from-scratch)
4. [Top 5 Repos to Study](#4-top-5-repos-to-study-deeply)
5. [Architecture Blueprint](#5-architecture-blueprint)

---

## 1. CLI Agent Features → Swarm Needs Mapping

### 1.1 Context Management

| CLI Feature | Example | Swarm Need | Mapping |
|-------------|---------|------------|---------|
| **Auto-compaction** | Goose: 80% threshold triggers summary | Agent memory reset | Use Goose's `GOOSE_AUTO_COMPACT_THRESHOLD` + summary prompt pattern |
| **Manual compact** | Claude Code `/compact` | Coordinator-triggered cleanup | Expose as swarm command: `compact_agent(agent_id)` |
| **Tool-pair summarization** | Goose: Summarize old tool calls | Reduce context pollution | Per-agent background task summarizing tool history |
| **Context window limits** | LangGraph: `RemoveMessage(id=...)` | Prevent context overflow | Implement state reducer pattern with explicit message removal |
| **Progressive disclosure** | Agent Skills: 3-level loading (metadata → instructions → resources) | Load only relevant skills per task | Adopt Agent Skills YAML frontmatter + lazy load full content |

**Key Insight**: Claude Code's `/compact` and Goose's auto-threshold are **direct swarm primitives**. For swarm:
- **Per-agent context budget**: Each agent tracks token usage independently
- **Coordinator-triggered reset**: After task completion, coordinator issues `compact_agent` command
- **Shared memory condensation**: Multi-agent shared state summarized at coordinator level (not per-agent)

---

### 1.2 Prompting Systems

| CLI Feature | Example | Swarm Need | Mapping |
|-------------|---------|------------|---------|
| **Template registry** | Goose: `PromptManager` with registered templates (`system.md`, `subagent_system.md`, `compaction.md`) | Role-specific prompts | Swarm maintains template catalog keyed by role (`coordinator.md`, `researcher.md`, `implementer.md`) |
| **User overrides** | Goose: `~/.config/goose/prompts/system.md` overrides built-in | Custom agent definitions | Support `~/.hatchery/prompts/{role}.md` overrides |
| **Runtime composition** | Goose: `PromptManager::builder()` injects mode flags, tool counts, hints | Dynamic prompt assembly | Coordinator assembles agent prompt from: base template + task context + available tools + agent state |
| **Subagent specialization** | Goose: `subagent_system.md` with `max_turns`, `task_instructions` | Worker agent prompts | Template: `swarm_worker.md` injected with: `task_description`, `delegation_chain`, `coordinator_id`, `max_steps` |
| **Multi-channel prompts** | Goose: Separate `plan.md`, `permission_judge.md`, `apps_create.md` | Specialized agent modes | Implement prompt channel routing: planning agent → `plan.md`, safety judge → `permission_judge.md` |

**Key Insight**: Goose's **compositional prompt runtime** is perfect for swarms. Cursor's `.cursorrules` and Agent Skills YAML frontmatter are **swarm worker prompt templates**.

**Swarm Pattern**:
```rust
struct PromptManager {
    templates: HashMap<String, String>,        // role → base template
    overrides: HashMap<String, String>,        // user custom templates
    context: HashMap<String, serde_json::Value> // runtime injection vars
}

fn build_agent_prompt(role: &str, task: &Task, tools: &[Tool]) -> String {
    let base = templates.get(role).or(overrides.get(role));
    let context = json!({
        "task": task,
        "tools": tools,
        "mode": "autonomous",
        "max_steps": 50
    });
    render_template(base, context)
}
```

---

### 1.3 Transport & Communication

| CLI Feature | Example | Swarm Need | Mapping |
|-------------|---------|------------|---------|
| **Session-scoped mailbox** | OpenCode: `SessionPrompt` queue per session | Agent inboxes | Each agent has `HashMap<SessionId, VecDeque<Message>>` |
| **Correlation IDs** | Continue/Cline: `messageId` on every frame | Track request-response pairs | All messages carry `correlation_id` + `parent_id` (for delegation chains) |
| **Durable log + live tail** | OpenHands: Event store + live stream | Swarm audit trail | Persist all events to SQLite, fan out live updates via channels |
| **Event envelope** | Plandex: `StreamMessageType` enum | Typed messages | Define `SwarmEvent` enum (Rust) with serde serialization |
| **Fanout subscription** | Plandex: Multiple subscribers per plan | Coordinator → N agents broadcast | Use `tokio::sync::broadcast` channel for coordinator updates |
| **Heartbeat + liveness** | Plandex: 5s heartbeat, 16s timeout | Dead agent detection | Each agent sends heartbeat every 3s, coordinator marks dead after 10s silence |
| **Child session delegation** | Crush: `messageID$$toolCallID` | Subagent spawning | New agent session links to parent via `parent_session_id` field |
| **SSE/WebSocket** | LangGraph SDK: SSE over HTTP, `Last-Event-ID` resume | Coordinator-agent communication | Use SSE for coordinator → UI, use in-process channels for agent-agent (same process swarm) |

**Key Insight**: LangGraph's **channel-based internal coordination + SSE external transport** is the gold standard. For Rust swarm:
- **Internal (same-process)**: `tokio::sync::mpsc` channels for agent-to-agent
- **External (multi-process/networked)**: SSE or gRPC (if needed later)
- **Durable state**: SQLite for audit + recovery (following LangGraph checkpoint pattern)

**Swarm Transport Architecture**:
```
┌─────────────┐
│ Coordinator │──┬──[mpsc channel]──> Agent 1
└─────────────┘  ├──[mpsc channel]──> Agent 2
                 └──[mpsc channel]──> Agent N
                         │
                         └──[SQLite]──> Event Store (durable log)
```

---

### 1.4 Memory & State Management

| CLI Feature | Example | Swarm Need | Mapping |
|-------------|---------|------------|---------|
| **Dual-memory architecture** | LangGraph: Checkpointer (short-term state) + Store (long-term KV + semantic search) | Swarm execution state vs shared knowledge | Adopt same split: SQLite checkpoints for agent state, PostgreSQL/vector DB for shared memory |
| **Thread-based state** | LangGraph: `thread_id` as primary key | Session continuity | Swarm uses `swarm_id` + `agent_id` + `session_id` as composite key |
| **Namespace hierarchy** | LangGraph Store: `tuple[str, ...]` namespaces | Multi-agent memory partitioning | Namespace: `("swarm", swarm_id, "agents", agent_id, "memory")` |
| **TTL + sweeper** | Postgres/SQLite stores: `supports_ttl = True` + background sweeper | Expire old agent memory | Implement TTL cleanup for agent state after task completion |
| **Vector search** | LangGraph: Optional `pgvector` / `sqlite_vec` | Semantic retrieval across agent memories | Use `qdrant` or `pgvector` for swarm-wide knowledge base |
| **Granular indexing** | LangGraph: Per-item `index` field controls embedding | Control what gets vectorized | Only index agent outputs, not internal logs |

**Key Insight**: LangGraph's **checkpointer + store split** prevents conflating execution state (ephemeral, versioned) with knowledge (durable, searchable). For swarm:
- **Checkpointer**: SQLite table `agent_checkpoints` (columns: `swarm_id`, `agent_id`, `step`, `state_json`)
- **Store**: Qdrant or pgvector for shared knowledge base (`namespace` = agent role, `payload` = condensed outputs)

---

### 1.5 Validation & Safety

| CLI Feature | Example | Swarm Need | Mapping |
|-------------|---------|------------|---------|
| **Approval gates** | Replit Agent 3: Plan approval mode | High-impact action review | Coordinator pauses execution, sends plan to user, waits for approval before proceeding |
| **Environment segregation** | Replit: Dev/prod isolation after DB deletion incident | Prevent production damage | Swarm runs in isolated Docker containers per environment |
| **Read-only by default** | Replit: Agents get read-only unless escalated | Minimize risk | Default agent tools are read-only; write operations require coordinator approval |
| **Static analysis guards** | Replit: Hybrid LLM + deterministic checks | Catch dangerous commands | Run `shellcheck` / `clippy` before executing agent-generated code |
| **Git safety** | FastRender: Worktrees + file locks | Prevent conflicts | Each agent works in separate git worktree, coordinator merges |
| **Optimistic locking** | Anthropic Compiler: Git's atomic ops for task claiming | Avoid race conditions | Use file locks or DB transactions for task assignment |

**Key Insight**: Replit's **multiple safety layers** (approval gates, read-only default, JIT access) prevent catastrophic errors. For swarm:
- **Approval gate**: Coordinator sends `ApprovalRequest` event to user before destructive operations
- **Worktree isolation**: Each agent clones task into separate `worktrees/{agent_id}` directory
- **Action audit log**: All agent actions logged to `audit.jsonl` with timestamp, agent_id, action type

---

## 2. Technology Stack for Rust-Based Swarm

### 2.1 Transport Layer

**Question**: Files vs sockets vs channels?

**Answer**: **Hybrid approach** (in-process channels + file-based audit log)

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| **Agent-to-coordinator** | `tokio::sync::mpsc::channel` | Low-latency, type-safe, backpressure-aware |
| **Coordinator-to-agents broadcast** | `tokio::sync::broadcast::channel` | Efficient 1-to-N (e.g., "all agents pause") |
| **Durable event log** | SQLite (`rusqlite`) | Fast, embeddable, ACID guarantees |
| **External UI streaming** | SSE (`axum` + `tokio_stream`) | Standard web protocol, auto-reconnect support |
| **Optional distributed mode** | gRPC (`tonic`) | If swarm spans multiple machines (future) |

**Why not files-only?**
- Files are too slow for live coordination (100ms+ latency per read/write)
- Files work for audit trail, not for real-time orchestration

**Why not sockets-only?**
- In-process channels are 100-1000x faster (microseconds vs milliseconds)
- Sockets add complexity without benefit for single-machine swarms

**Recommended**: Start with **in-process channels**, log all events to SQLite, add gRPC only if multi-machine needed.

---

### 2.2 Memory Management

**Question**: How to handle context per agent?

**Answer**: **Per-agent context budget + coordinator-level shared memory**

| Memory Type | Storage | Scope | Cleanup Policy |
|-------------|---------|-------|----------------|
| **Agent execution state** | In-memory `HashMap<AgentId, AgentState>` | Per-agent, ephemeral | Reset after task completion |
| **Agent conversation history** | SQLite table `agent_conversations` | Per-agent, durable | Auto-compact at 80% context window |
| **Shared knowledge base** | Qdrant vector DB | Swarm-wide, durable | TTL-based expiration (7 days) |
| **Task DAG** | SQLite table `tasks` | Swarm-wide, durable | Persist until swarm completes |
| **Checkpoints** | SQLite table `agent_checkpoints` | Per-agent, versioned | Keep last 10 checkpoints per agent |

**Context Budget Algorithm** (Goose-inspired):
```rust
const AUTO_COMPACT_THRESHOLD: f64 = 0.8;

fn check_compaction_needed(agent: &Agent) -> bool {
    let usage = agent.total_tokens as f64 / agent.context_window as f64;
    usage >= AUTO_COMPACT_THRESHOLD
}

async fn auto_compact(agent: &mut Agent, provider: &LLMProvider) {
    let summary = provider.summarize(&agent.conversation_history).await;
    agent.conversation_history = vec![Message::system(summary)];
    agent.total_tokens = count_tokens(&summary);
}
```

---

### 2.3 Task Distribution

**Question**: DAG vs queue vs pull-based?

**Answer**: **DAG-based with priority queue** (LangGraph + Claude Code pattern)

**Why DAG?**
- Tasks have dependencies (`blockedBy` / `blocks` arrays)
- Enables parallel execution of independent tasks
- Supports dynamic task creation (agent spawns subtasks)

**Implementation**:
```rust
struct Task {
    id: TaskId,
    description: String,
    assigned_to: Option<AgentId>,
    status: TaskStatus, // Pending | InProgress | Completed | Failed
    blocked_by: Vec<TaskId>,
    blocks: Vec<TaskId>,
    priority: u8,
    created_at: DateTime<Utc>,
}

struct TaskScheduler {
    dag: HashMap<TaskId, Task>,
    ready_queue: BinaryHeap<Task>, // Priority queue of unblocked tasks
}

impl TaskScheduler {
    fn get_ready_tasks(&self) -> Vec<&Task> {
        self.dag.values()
            .filter(|t| t.status == TaskStatus::Pending && t.blocked_by.is_empty())
            .collect()
    }

    fn assign_task(&mut self, task_id: TaskId, agent_id: AgentId) {
        if let Some(task) = self.dag.get_mut(&task_id) {
            task.assigned_to = Some(agent_id);
            task.status = TaskStatus::InProgress;
        }
    }

    fn complete_task(&mut self, task_id: TaskId) {
        if let Some(task) = self.dag.get_mut(&task_id) {
            task.status = TaskStatus::Completed;
            // Unblock dependent tasks
            for blocked_id in &task.blocks {
                if let Some(blocked_task) = self.dag.get_mut(blocked_id) {
                    blocked_task.blocked_by.retain(|id| id != &task_id);
                }
            }
        }
    }
}
```

**Scheduling Policy** (GitHub Agent HQ pattern):
- **Parallel**: Tasks with no shared dependencies → dispatch to N agents simultaneously
- **Sequential**: Tasks with data dependencies → wait for completion before next
- **Dynamic priority**: User-requested tasks get `priority = 10`, agent-created subtasks get `priority = 5`

---

### 2.4 Git Coordination

**Question**: Worktrees vs branches vs separate repos?

**Answer**: **Git worktrees** (FastRender + Anthropic Compiler + ccswarm pattern)

**Why worktrees?**
- **Conflict-free**: Each agent works in isolated directory
- **Atomic merges**: Coordinator reviews and merges sequentially
- **Shared history**: All worktrees share `.git` directory (no duplication)
- **Fast**: `git worktree add` is instant (no full clone)

**Implementation**:
```bash
# Coordinator sets up swarm
git worktree add worktrees/agent-1 -b task-1
git worktree add worktrees/agent-2 -b task-2

# Agent 1 works
cd worktrees/agent-1
# ... makes changes ...
git add .
git commit -m "Implement feature X"

# Coordinator merges
git checkout main
git merge task-1 --no-ff
git worktree remove worktrees/agent-1
```

**Rust Integration**:
```rust
use std::process::Command;

fn create_agent_worktree(agent_id: &str, task_id: &str) -> Result<PathBuf> {
    let worktree_path = PathBuf::from(format!("worktrees/{}", agent_id));
    let branch_name = format!("task-{}", task_id);

    Command::new("git")
        .args(&["worktree", "add", worktree_path.to_str().unwrap(), "-b", &branch_name])
        .status()?;

    Ok(worktree_path)
}

fn merge_agent_work(agent_id: &str, task_id: &str) -> Result<()> {
    let branch_name = format!("task-{}", task_id);

    Command::new("git")
        .args(&["checkout", "main"])
        .status()?;

    Command::new("git")
        .args(&["merge", &branch_name, "--no-ff", "-m", &format!("Merge task {}", task_id)])
        .status()?;

    Command::new("git")
        .args(&["worktree", "remove", &format!("worktrees/{}", agent_id)])
        .status()?;

    Ok(())
}
```

**Safety**: Follow Hatchery's existing git safety rules (never force push, never skip hooks, prefer specific files over `git add .`)

---

### 2.5 Prompting

**Question**: How to template agent prompts?

**Answer**: **Agent Skills specification + MiniJinja templates** (industry standard + Goose pattern)

**Agent Skills YAML Frontmatter** (adopted by Microsoft, OpenAI, Anthropic, Cursor, GitHub):
```yaml
---
name: rust-implementer
description: Implements Rust code following project patterns
license: Apache-2.0
compatibility: hatchery-swarm-v1
metadata:
  author: nemo
  version: "1.0"
allowed-tools: ["read", "write", "bash", "grep"]
---

# Rust Implementer Agent

You are a specialized Rust coding agent. Your task: {task_description}

## Guidelines
- Follow existing patterns in codebase
- Use `Result<T, E>` for fallible operations
- Prefer `&str` over `String` in function params
- Run `cargo check` after every change

## Available Tools
{{% for tool in tools %}}
- `{{tool.name}}`: {{tool.description}}
{{% endfor %}}

## Context
Working directory: {{working_dir}}
Task priority: {{priority}}
Max steps: {{max_steps}}
```

**Template Rendering** (MiniJinja):
```rust
use minijinja::Environment;

struct PromptRenderer {
    env: Environment<'static>,
}

impl PromptRenderer {
    fn render(&self, template_name: &str, context: &serde_json::Value) -> String {
        self.env.get_template(template_name).unwrap()
            .render(context).unwrap()
    }
}

// Usage
let context = json!({
    "task_description": "Implement exchange connector",
    "tools": vec![...],
    "working_dir": "/workspace",
    "priority": 5,
    "max_steps": 50
});

let prompt = renderer.render("rust-implementer.md", &context);
```

**Template Storage**:
- Built-in: `src/prompts/{role}.md` (embedded via `include_dir!`)
- User overrides: `~/.hatchery/prompts/{role}.md`
- Lookup order: User override → Built-in → Error

---

### 2.6 Validation

**Question**: Compilation checks, test gates, judge agents?

**Answer**: **All three** (layered validation)

| Validation Layer | Technology | When | Blocker? |
|-----------------|------------|------|----------|
| **Syntax check** | `cargo check` | After every code change | Yes (retry if fails) |
| **Linting** | `cargo clippy` | Before commit | No (warning only) |
| **Tests** | `cargo test` | After implementation | Yes (debug loop if fails) |
| **Judge agent** | Dedicated LLM call with validation prompt | Before merge | No (advisory review) |
| **Manual approval** | User confirmation for high-impact actions | Before production deployment | Yes (hard gate) |

**Judge Agent Pattern** (Goose `permission_judge.md` + Replit verifier pattern):
```rust
struct JudgeAgent {
    provider: LLMProvider,
}

impl JudgeAgent {
    async fn review_code(&self, diff: &str) -> JudgeResult {
        let prompt = format!(
            "Review this code change for:\n\
             - Safety issues\n\
             - Code quality\n\
             - Adherence to project patterns\n\n\
             Diff:\n{}\n\n\
             Respond with JSON: {{\"approved\": bool, \"issues\": [str], \"suggestions\": [str]}}",
            diff
        );

        let response = self.provider.complete(&prompt).await?;
        serde_json::from_str(&response)
    }
}

// Integration
let judge = JudgeAgent::new(provider);
let diff = git_diff("task-1")?;
let result = judge.review_code(&diff).await?;

if !result.approved {
    return Err(format!("Judge rejected: {:?}", result.issues));
}
```

**Test Gate Loop** (Carousel Phase 4 pattern):
```rust
const MAX_DEBUG_ITERATIONS: usize = 10;

async fn debug_until_tests_pass(agent: &Agent, task: &Task) -> Result<()> {
    for iteration in 1..=MAX_DEBUG_ITERATIONS {
        let test_result = run_tests()?;

        if test_result.passed {
            return Ok(());
        }

        let debug_prompt = format!(
            "Tests failed (iteration {}/{}):\n{}\n\nFix the code.",
            iteration, MAX_DEBUG_ITERATIONS, test_result.output
        );

        agent.execute(&debug_prompt).await?;
    }

    Err("Max debug iterations exceeded".into())
}
```

---

## 3. Reuse vs Build from Scratch

### 3.1 Component-by-Component Analysis

| Component | Recommendation | Justification |
|-----------|----------------|---------------|
| **Orchestration runtime** | **BUILD custom** (but study LangGraph patterns) | LangGraph is Python; porting 100k+ LOC to Rust is infeasible. But copy architecture: channels, checkpointer, DAG scheduler |
| **Prompt templating** | **USE minijinja** (https://github.com/mitsuhiko/minijinja) | Production-ready, Jinja2-compatible, used by Goose |
| **Agent Skills format** | **USE agentskills spec** (https://github.com/agentskills/agentskills) | Industry standard (Microsoft, OpenAI, Anthropic), Apache 2.0 license |
| **Event store (SQLite)** | **USE rusqlite** (https://github.com/rusqlite/rusqlite) | De-facto Rust SQLite library, 4.2k stars, active |
| **Async runtime** | **USE tokio** (existing in project) | Already in use, ecosystem standard |
| **Channels** | **USE tokio::sync** (mpsc, broadcast) | Built-in, zero-cost, type-safe |
| **SSE streaming** | **USE axum + tokio_stream** | Axum is fast, tokio_stream handles backpressure |
| **Vector DB** | **USE qdrant-client** (https://github.com/qdrant/rust-client) | Rust-native client, self-hostable, Apache 2.0 |
| **Git operations** | **USE git2-rs** (https://github.com/rust-lang/git2-rs) | Official Rust bindings for libgit2 |
| **LLM provider** | **USE existing in project** (reqwest + serde) | Already functional for Anthropic/OpenAI APIs |
| **MCP servers** | **USE existing ecosystem** (Playwright, Sequential Thinking) | Don't reinvent; integrate via JSON-RPC |
| **Task scheduler** | **BUILD custom** (DAG + priority queue) | No existing Rust library fits swarm needs (LangGraph's is Python-only) |
| **Validation gates** | **USE existing Rust tooling** (`cargo check`, `clippy`, `test`) | Standard Rust ecosystem |
| **File locks** | **USE fs2** (https://github.com/danburkert/fs2-rs) | Cross-platform file locking |
| **Worktree management** | **BUILD thin wrapper** around `git worktree` CLI | Git CLI is stable, no need for low-level API |

---

### 3.2 Detailed Recommendations

#### **USE: minijinja**
- **URL**: https://github.com/mitsuhiko/minijinja
- **Stars**: 2.1k
- **License**: Apache 2.0
- **Why**: Goose uses it, Jinja2-compatible, actively maintained by Armin Ronacher (Flask author)
- **Integration**:
```toml
[dependencies]
minijinja = { version = "2", features = ["loader"] }
```

#### **USE: agentskills specification**
- **URL**: https://github.com/agentskills/agentskills
- **License**: Apache 2.0 (code), CC-BY-4.0 (docs)
- **Why**: Industry-wide adoption (Microsoft, OpenAI, Anthropic, Cursor, GitHub)
- **Integration**: Parse YAML frontmatter with `serde_yaml`, follow 3-level loading (metadata → instructions → resources)

#### **USE: rusqlite**
- **URL**: https://github.com/rusqlite/rusqlite
- **Stars**: 4.2k
- **License**: MIT
- **Why**: De-facto standard, used by major Rust projects
- **Integration**:
```toml
[dependencies]
rusqlite = { version = "0.36", features = ["bundled"] }
```

#### **USE: qdrant-client**
- **URL**: https://github.com/qdrant/rust-client
- **License**: Apache 2.0
- **Why**: Rust-native, self-hostable vector DB, used in production
- **Integration**:
```toml
[dependencies]
qdrant-client = "1.13"
```

#### **USE: git2-rs**
- **URL**: https://github.com/rust-lang/git2-rs
- **Stars**: 1.7k
- **License**: MIT/Apache 2.0
- **Why**: Official Rust bindings, comprehensive API
- **Integration**:
```toml
[dependencies]
git2 = "0.19"
```

#### **USE: fs2**
- **URL**: https://github.com/danburkert/fs2-rs
- **License**: MIT/Apache 2.0
- **Why**: Cross-platform file locking (Windows + Unix)
- **Integration**:
```toml
[dependencies]
fs2 = "0.4"
```

#### **BUILD: Orchestration Runtime**
- **Why**: LangGraph is Python (100k+ LOC), porting is infeasible
- **Pattern to copy**: Channel-based coordination, checkpointer pattern, DAG scheduler, SSE streaming
- **Rust equivalent**: `tokio::sync::mpsc` + SQLite checkpointer + custom DAG scheduler

#### **BUILD: Task Scheduler**
- **Why**: No Rust library for agent-oriented DAG scheduling
- **Implementation**: `HashMap<TaskId, Task>` + `BinaryHeap<Task>` (priority queue) + dependency resolution

#### **BUILD: Worktree Manager**
- **Why**: Git worktree CLI is simple, no need for low-level API
- **Implementation**: Thin wrapper around `std::process::Command` for `git worktree add/remove`

---

## 4. Top 5 Repos to Study Deeply

### Ranking Criteria
1. **Swarm-relevant code volume**: How much orchestration/coordination code?
2. **Architecture clarity**: How easy to extract patterns?
3. **Production-readiness**: Proven in real deployments?
4. **License**: Can we legally study/reuse patterns?
5. **Language**: Rust > Python > TypeScript (for ease of porting)

---

### **#1: langchain-ai/langgraph** (24.4k stars, MIT)
- **URL**: https://github.com/langchain-ai/langgraph
- **Language**: Python (but architecture is language-agnostic)
- **Why study**:
  - **Gold standard for multi-agent orchestration** (used by Replit Agent 3, Klarna, Elastic)
  - **Channel-based coordination**: `TASKS` topic + state channels (directly maps to Rust `tokio::sync`)
  - **Checkpointer pattern**: Clean separation of execution state vs durable storage (copy for SQLite)
  - **DAG scheduler**: `_algo.py` has task dependency resolution (port to Rust)
  - **SSE streaming**: `sdk-py` shows how to expose graph execution over HTTP (copy for axum)
- **Key files to read**:
  - `libs/langgraph/langgraph/pregel/main.py` - Core execution loop
  - `libs/langgraph/langgraph/pregel/_algo.py` - Task scheduling algorithm
  - `libs/langgraph/langgraph/channels/topic.py` - Pub/sub channel implementation
  - `libs/checkpoint/langgraph/checkpoint/base/__init__.py` - Checkpointer contract
  - `libs/sdk-py/langgraph_sdk/client.py` - SSE streaming client
- **What to extract**: Channel patterns, checkpointer interface, DAG scheduler logic, SSE protocol

---

### **#2: elizaOS/eliza** (17.5k stars, MIT)
- **URL**: https://github.com/elizaOS/eliza
- **Language**: TypeScript
- **Why study**:
  - **Most complete open-source swarm** (production deployments: The Org, Agent Hub marketplace)
  - **Worlds & Rooms architecture**: Agent isolation + cross-agent signaling (can port to Rust with channels)
  - **Self-consistency voting**: Swarm of homogeneous agents vote via majority (useful for validation)
  - **Agent economy**: Machine-to-machine payments (future feature inspiration)
  - **Plugin ecosystem**: 200+ plugins show how to structure swarm extensibility
- **Key files to read**:
  - `packages/core/src/` - Core runtime
  - `packages/plugin-bootstrap/` - Unified message bus
  - `elizaOS/the-org` - Multi-agent system example
  - `elizaOS/agentmemory` - Vector memory with clustering
- **What to extract**: Agent isolation patterns, voting mechanism, plugin architecture, shared memory design

---

### **#3: block/goose** (Apache 2.0)
- **URL**: https://github.com/block/goose
- **Language**: **Rust** (!!!!)
- **Why study**:
  - **Only major Rust agent runtime in our dataset**
  - **Compositional prompting**: Template registry + runtime assembly + user overrides (direct copy)
  - **Auto-compaction**: 80% threshold + summary prompt (copy exact algorithm)
  - **Subagent support**: `subagent_system.md` template + task delegation (proven pattern)
  - **MCP integration**: Shows how to integrate MCP servers in Rust
- **Key files to read**:
  - `crates/goose/src/agents/prompt_manager.rs` - Prompt composition system
  - `crates/goose/src/context_mgmt/mod.rs` - Auto-compaction logic
  - `crates/goose/src/agents/subagent_handler.rs` - Subagent spawning
  - `crates/goose/src/prompt_template.rs` - Template registry
  - `crates/goose/src/session/session_manager.rs` - Session state management
- **What to extract**: Prompt manager Rust code (can copy directly), compaction algorithm, subagent pattern

---

### **#4: anthropics/claude-code** (65k stars, Proprietary with OSS components)
- **URL**: https://github.com/anthropics/claude-code
- **Language**: TypeScript/Python SDKs (CLI binary is closed)
- **Why study**:
  - **Agent Teams feature**: Lead + teammates with task DAG (proven at scale: 16 agents → 100K LOC compiler)
  - **TeammateTool**: 13 operations for coordination (spawn, join, write, broadcast, shutdown)
  - **Task DAG**: `blockedBy` / `blocks` arrays (copy exact schema)
  - **JSON inbox system**: `~/.claude/teams/{name}/inboxes/{agent}.json` (can port to SQLite)
  - **Git worktrees**: Used in compiler project (validates worktree approach)
- **Key resources**:
  - [Building a C Compiler](https://www.anthropic.com/engineering/building-c-compiler)
  - [Agent Teams Docs](https://code.claude.com/docs/en/agent-teams)
  - [Kieran Klaassen Gist](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea) - Full TeammateTool spec
- **What to extract**: Task DAG schema, inbox pattern, coordinator-agent protocol, git worktree usage

---

### **#5: wilsonzlin/fastrender** (1.4k stars, NOT SPECIFIED)
- **URL**: https://github.com/wilsonzlin/fastrender
- **Language**: Rust (browser engine, not orchestration harness)
- **Why study**:
  - **Largest successful swarm deployment** (~2,000 parallel agents → 1.6M LOC in 1 week)
  - **Hierarchical architecture**: Root Planner → Sub-Planners → Workers → Judge (scales to 2k agents)
  - **Git worktrees + file locks**: Conflict-free parallelism at massive scale
  - **Optimistic concurrency**: Error tolerance philosophy (critical for swarms)
  - **Constraints over instructions**: Prompting strategy that worked at scale
  - **AGENTS.md**: Master prompt governing all agents (copy pattern)
- **Key files to read**:
  - `AGENTS.md` - Master prompt and constraints
  - `instructions/` - Workstream-specific guides (shows how to partition work)
  - `progress/pages/` - Progress tracking system
  - `scripts/cargo_agent.sh` - Safety wrappers (resource limits, timeout enforcement)
  - `docs/philosophy.md` - "Correct pixels are the product" (outcome-driven validation)
- **What to extract**: Hierarchical swarm architecture, file-based task locks, error tolerance patterns, AGENTS.md format
- **Note**: Orchestration harness is NOT open sourced (this is the output, not the orchestrator)

---

## 5. Architecture Blueprint

### 5.1 System Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                          HATCHERY SWARM                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  ┌───────────────────┐                                               │
│  │  Coordinator      │────[broadcast]───> All Agents (pause/resume) │
│  │  (Brood Lord)     │                                               │
│  └─────────┬─────────┘                                               │
│            │                                                          │
│            ├──[mpsc]──> Agent 1 (rust-implementer)                   │
│            │               ├─> Worktree: task-1/                     │
│            │               └─> Tools: [read, write, bash]            │
│            │                                                          │
│            ├──[mpsc]──> Agent 2 (research-agent)                     │
│            │               ├─> Worktree: task-2/                     │
│            │               └─> Tools: [websearch, webfetch]          │
│            │                                                          │
│            └──[mpsc]──> Agent 3 (judge-agent)                        │
│                            └─> Tools: [read] (read-only)             │
│                                                                       │
├─────────────────────────────────────────────────────────────────────┤
│                          SHARED STATE                                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  ┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐│
│  │  Task DAG       │    │  Event Store     │    │  Knowledge Base ││
│  │  (SQLite)       │    │  (SQLite)        │    │  (Qdrant)       ││
│  ├─────────────────┤    ├──────────────────┤    ├─────────────────┤│
│  │ - task_id       │    │ - event_id       │    │ Namespace:      ││
│  │ - description   │    │ - timestamp      │    │  ("swarm",      ││
│  │ - assigned_to   │    │ - agent_id       │    │   swarm_id,     ││
│  │ - blocked_by    │    │ - event_type     │    │   "agents",     ││
│  │ - blocks        │    │ - payload_json   │    │   agent_id)     ││
│  │ - priority      │    └──────────────────┘    └─────────────────┘│
│  │ - status        │                                                 │
│  └─────────────────┘                                                 │
│                                                                       │
│  ┌─────────────────────────────────────┐                            │
│  │  Agent Checkpoints (SQLite)         │                            │
│  ├─────────────────────────────────────┤                            │
│  │ - swarm_id, agent_id, step          │                            │
│  │ - state_json (conversation history) │                            │
│  │ - created_at                        │                            │
│  └─────────────────────────────────────┘                            │
│                                                                       │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 5.2 Core Data Structures (Rust)

```rust
use std::collections::{HashMap, BinaryHeap};
use tokio::sync::{mpsc, broadcast};
use serde::{Serialize, Deserialize};

// ==================== Task DAG ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    id: TaskId,
    description: String,
    assigned_to: Option<AgentId>,
    status: TaskStatus,
    blocked_by: Vec<TaskId>,
    blocks: Vec<TaskId>,
    priority: u8,
    created_at: DateTime<Utc>,
}

#[derive(Debug, PartialEq, Eq)]
enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

// ==================== Agent ====================

struct Agent {
    id: AgentId,
    role: String, // "rust-implementer", "research-agent", "judge-agent"
    inbox: mpsc::Receiver<SwarmMessage>,
    outbox: mpsc::Sender<SwarmMessage>,
    context_window: usize,
    total_tokens: usize,
    conversation_history: Vec<Message>,
    working_dir: PathBuf, // Git worktree path
    tools: Vec<ToolName>,
}

impl Agent {
    async fn run(&mut self) {
        while let Some(msg) = self.inbox.recv().await {
            match msg {
                SwarmMessage::Task(task) => self.execute_task(task).await,
                SwarmMessage::Pause => self.pause().await,
                SwarmMessage::Compact => self.auto_compact().await,
                _ => {}
            }

            // Check context budget
            if self.needs_compaction() {
                self.auto_compact().await;
            }
        }
    }

    fn needs_compaction(&self) -> bool {
        (self.total_tokens as f64 / self.context_window as f64) >= 0.8
    }

    async fn auto_compact(&mut self) {
        let summary = self.provider.summarize(&self.conversation_history).await;
        self.conversation_history = vec![Message::system(summary)];
        self.total_tokens = count_tokens(&summary);
    }
}

// ==================== Coordinator ====================

struct Coordinator {
    agents: HashMap<AgentId, mpsc::Sender<SwarmMessage>>,
    broadcast: broadcast::Sender<SwarmMessage>,
    task_scheduler: TaskScheduler,
    event_store: EventStore,
}

impl Coordinator {
    async fn spawn_swarm(&mut self, tasks: Vec<Task>) {
        // Create agents
        for role in ["rust-implementer", "research-agent", "judge-agent"] {
            let (tx, rx) = mpsc::channel(100);
            let agent_id = AgentId::new();

            // Set up git worktree
            let worktree = self.create_worktree(&agent_id).await.unwrap();

            let agent = Agent::new(agent_id.clone(), role, rx, self.broadcast.clone(), worktree);

            tokio::spawn(async move {
                agent.run().await;
            });

            self.agents.insert(agent_id, tx);
        }

        // Schedule tasks
        for task in tasks {
            self.task_scheduler.add_task(task);
        }

        // Start orchestration loop
        self.orchestration_loop().await;
    }

    async fn orchestration_loop(&mut self) {
        loop {
            // Get ready tasks
            let ready_tasks = self.task_scheduler.get_ready_tasks();

            // Assign to available agents
            for task in ready_tasks {
                if let Some(agent_id) = self.find_available_agent(&task) {
                    self.assign_task(task.id, agent_id).await;
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn assign_task(&mut self, task_id: TaskId, agent_id: AgentId) {
        let task = self.task_scheduler.get_task(task_id).unwrap();

        if let Some(agent_tx) = self.agents.get(&agent_id) {
            agent_tx.send(SwarmMessage::Task(task.clone())).await.unwrap();
            self.task_scheduler.assign_task(task_id, agent_id);

            // Log event
            self.event_store.log(SwarmEvent::TaskAssigned {
                task_id,
                agent_id,
                timestamp: Utc::now(),
            }).await;
        }
    }

    fn find_available_agent(&self, task: &Task) -> Option<AgentId> {
        // Simple strategy: round-robin
        // Advanced: match task requirements to agent skills
        self.agents.keys().next().cloned()
    }

    async fn create_worktree(&self, agent_id: &AgentId) -> Result<PathBuf> {
        let worktree_path = PathBuf::from(format!("worktrees/{}", agent_id));
        let branch_name = format!("agent-{}", agent_id);

        Command::new("git")
            .args(&["worktree", "add", worktree_path.to_str().unwrap(), "-b", &branch_name])
            .status()?;

        Ok(worktree_path)
    }
}

// ==================== Messages ====================

#[derive(Debug, Clone)]
enum SwarmMessage {
    Task(Task),
    Pause,
    Resume,
    Compact,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
enum SwarmEvent {
    TaskCreated { task_id: TaskId, timestamp: DateTime<Utc> },
    TaskAssigned { task_id: TaskId, agent_id: AgentId, timestamp: DateTime<Utc> },
    TaskCompleted { task_id: TaskId, agent_id: AgentId, timestamp: DateTime<Utc> },
    AgentSpawned { agent_id: AgentId, role: String, timestamp: DateTime<Utc> },
    AgentCompacted { agent_id: AgentId, timestamp: DateTime<Utc> },
}

// ==================== Event Store ====================

struct EventStore {
    conn: rusqlite::Connection,
}

impl EventStore {
    async fn log(&self, event: SwarmEvent) {
        let json = serde_json::to_string(&event).unwrap();
        self.conn.execute(
            "INSERT INTO events (timestamp, event_type, payload_json) VALUES (?1, ?2, ?3)",
            params![Utc::now().to_rfc3339(), event.type_name(), json],
        ).unwrap();
    }
}
```

---

### 5.3 Prompt Template System

**Directory Structure**:
```
~/.hatchery/
├── prompts/
│   ├── coordinator.md         # Brood Lord prompt
│   ├── rust-implementer.md    # Code generation agent
│   ├── research-agent.md      # Web research agent
│   ├── judge-agent.md         # Code review agent
│   └── compaction.md          # Context summarization prompt
└── config.toml
```

**Example: `rust-implementer.md`** (Agent Skills format):
```markdown
---
name: rust-implementer
description: Implements Rust code following NEMO project patterns
license: Apache-2.0
metadata:
  author: nemo
  version: "1.0"
allowed-tools: ["read", "write", "bash", "grep"]
---

# Rust Implementer Agent

You are a specialized Rust coding agent working as part of a swarm.

## Your Mission
{task_description}

## Constraints
- MUST run `cargo check` after every file edit
- MUST follow existing code patterns (see CLAUDE.md)
- MUST use `Result<T, ExchangeError>` for fallible operations
- MUST prefer `&str` over `String` in function parameters
- MUST NOT use `unwrap()` in production code

## Your Role in the Swarm
- Role: Worker agent (implementation track)
- Coordinator: {coordinator_id}
- Working directory: {working_dir}
- Max steps: {max_steps}
- Priority: {priority}

## Available Tools
{% for tool in tools %}
- `{{tool.name}}`: {{tool.description}}
{% endfor %}

## Workflow
1. Read task requirements carefully
2. Identify relevant files using `grep` or `read`
3. Implement changes incrementally
4. Run `cargo check` after each change
5. Report completion to coordinator

## Communication Protocol
When you complete a subtask, output:
```json
{
  "status": "completed",
  "task_id": "{task_id}",
  "files_changed": ["path/to/file.rs"],
  "next_steps": "Optional suggestions for coordinator"
}
```

When you encounter a blocker, output:
```json
{
  "status": "blocked",
  "task_id": "{task_id}",
  "reason": "Description of blocker",
  "needs_help_from": "judge-agent" // or null
}
```
```

---

### 5.4 Safety Layers

**Multi-level validation** (Replit pattern):

```rust
struct SafetyCoordinator {
    approval_gate: ApprovalGate,
    static_analyzer: StaticAnalyzer,
    judge_agent: JudgeAgent,
}

impl SafetyCoordinator {
    async fn validate_action(&self, action: &AgentAction) -> Result<()> {
        // Layer 1: Static analysis (fast, deterministic)
        if action.is_code_change() {
            self.static_analyzer.check(&action.files)?;
        }

        // Layer 2: Judge agent review (LLM-based, advisory)
        let review = self.judge_agent.review(action).await?;
        if !review.approved {
            log::warn!("Judge flagged issues: {:?}", review.issues);
        }

        // Layer 3: Human approval for high-impact actions
        if action.is_destructive() {
            self.approval_gate.request_approval(action).await?;
        }

        Ok(())
    }
}

struct ApprovalGate {
    pending: HashMap<ActionId, oneshot::Sender<bool>>,
}

impl ApprovalGate {
    async fn request_approval(&self, action: &AgentAction) -> Result<()> {
        let (tx, rx) = oneshot::channel();

        // Send approval request to UI
        self.send_to_ui(ApprovalRequest {
            action_id: action.id,
            description: action.describe(),
            risk_level: action.risk_level(),
        });

        self.pending.insert(action.id, tx);

        // Wait for user response (with timeout)
        let approved = tokio::time::timeout(
            Duration::from_secs(300), // 5 min timeout
            rx
        ).await??;

        if !approved {
            return Err("User rejected action".into());
        }

        Ok(())
    }
}
```

---

### 5.5 Context Management

**Auto-compaction** (Goose pattern):

```rust
const AUTO_COMPACT_THRESHOLD: f64 = 0.8;

impl Agent {
    fn needs_compaction(&self) -> bool {
        let usage = self.total_tokens as f64 / self.context_window as f64;
        usage >= AUTO_COMPACT_THRESHOLD
    }

    async fn auto_compact(&mut self) {
        // Load compaction prompt template
        let template = self.prompt_manager.get_template("compaction.md");

        let context = json!({
            "conversation_history": self.conversation_history,
            "current_task": self.current_task,
        });

        let prompt = self.prompt_manager.render(template, context);

        // Request summary from fast model
        let summary = self.provider.complete_fast(&prompt).await.unwrap();

        // Replace history with summary
        self.conversation_history = vec![
            Message::system("Previous context (summarized):"),
            Message::assistant(summary),
        ];

        self.total_tokens = count_tokens(&self.conversation_history);

        // Persist checkpoint
        self.checkpoint().await;

        log::info!("Agent {} compacted context: {} → {} tokens",
                   self.id, self.total_tokens, count_tokens(&self.conversation_history));
    }
}
```

---

## Summary

### Key Decisions

1. **Transport**: In-process `tokio` channels + SQLite event log + SSE for UI (NO files, NO sockets for agent-agent)
2. **Memory**: Dual-memory (checkpointer for execution state, Qdrant for shared knowledge)
3. **Task Distribution**: DAG-based with priority queue (copy LangGraph pattern)
4. **Git**: Worktrees per agent (proven by FastRender, Anthropic, ccswarm)
5. **Prompting**: Agent Skills YAML + MiniJinja (industry standard + Goose pattern)
6. **Validation**: 3 layers (static analysis, judge agent, human approval)

### Technology Stack

| Layer | Technology | License | Stars | Justification |
|-------|-----------|---------|-------|---------------|
| Async runtime | tokio | MIT | - | Already in use |
| Channels | tokio::sync | MIT | - | Built-in, zero-cost |
| Event store | rusqlite | MIT | 4.2k | De-facto standard |
| Vector DB | qdrant-client | Apache 2.0 | - | Rust-native |
| Templating | minijinja | Apache 2.0 | 2.1k | Goose uses it |
| Git operations | git2-rs | MIT/Apache | 1.7k | Official bindings |
| File locks | fs2 | MIT/Apache | - | Cross-platform |
| Prompts | agentskills spec | Apache/CC-BY | - | Industry standard |

### Reuse Matrix

- **USE**: minijinja, rusqlite, qdrant-client, git2-rs, fs2, agentskills spec, tokio
- **BUILD**: Orchestration runtime (copy LangGraph patterns), Task scheduler (DAG + priority queue), Worktree manager (thin CLI wrapper)

### Top 5 Repos to Clone

1. **langchain-ai/langgraph** - Channel patterns, checkpointer, DAG scheduler, SSE
2. **elizaOS/eliza** - Worlds/Rooms isolation, voting, plugin architecture
3. **block/goose** - Rust prompt manager (copy directly), compaction, subagents
4. **anthropics/claude-code** - Task DAG schema, TeammateTool, worktree usage
5. **wilsonzlin/fastrender** - Hierarchical swarm, file locks, AGENTS.md format

---

**Next Steps**:
1. Clone top 5 repos locally
2. Extract key patterns from each (prompting, scheduling, coordination)
3. Implement minimal orchestration runtime in Rust (coordinator + 3 agents)
4. Validate with simple task (e.g., "research + implement + review" pipeline)
5. Iterate toward full Hatchery swarm
