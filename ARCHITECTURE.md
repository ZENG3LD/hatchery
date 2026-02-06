# Hatchery: Swarm Orchestration Architecture

**Version:** 1.0
**Last Updated:** 2026-02-06
**Status:** Design Document

## Table of Contents

1. [System Overview](#system-overview)
2. [Core Principles](#core-principles)
3. [Architecture Diagram](#architecture-diagram)
4. [Components](#components)
5. [Session Types](#session-types)
6. [Communication Channels](#communication-channels)
7. [Shared Memory Design](#shared-memory-design)
8. [Event Flow](#event-flow)
9. [PRD Format](#prd-format)
10. [Prompt Templates](#prompt-templates)
11. [Dependencies](#dependencies)
12. [CLI Interface](#cli-interface)
13. [Integration with Claude Code](#integration-with-claude-code)
14. [Implementation Roadmap](#implementation-roadmap)

---

## System Overview

Hatchery is a Rust-based swarm orchestration system that manages multiple concurrent Claude Code AI agent sessions to collaboratively execute complex, multi-step projects defined in PRD (Product Requirements Document) markdown files.

### Key Innovation

Unlike traditional task orchestrators, Hatchery's **coordinator is itself a Claude AI session** — not rule-based Rust logic. This enables:

- Intelligent task decomposition
- Dynamic response to blockers
- Natural language communication
- Adaptive replanning

### Design Philosophy

```
Traditional:           Hatchery:
Rule-based orchestrator    AI Coordinator (Claude session)
         ↓                        ↓
    Dumb workers          Smart workers (Claude sessions)
                               ↓
                       Autonomous + Collaborative
```

---

## Core Principles

### 1. Separate Crate
- Lives in `nemo/hatchery/` workspace member
- Depends on `zengeld-hub-core` for PTY/Pipe infrastructure
- Standalone binary: `hatchery`

### 2. AI-First Orchestration
- Coordinator is a Claude session, not hardcoded logic
- Workers are autonomous Claude sessions
- Human/Lead provides high-level goals only

### 3. Distributed PRD
- Single markdown file defines all tasks
- Shared across all sessions
- Workers update checkboxes + results in real-time
- File-backed for crash recovery

### 4. Smart vs Dumb Workers
- **Smart mode**: Claude sessions with autonomy, can subdivide tasks, collaborate
- **Dumb mode**: Ralph-style PRD iteration, no coordination (for simple batch tasks)

### 5. Swarm, Not Pyramid
- Horizontal communication between workers
- Shared knowledge base accessible to all
- Workers can help each other without coordinator mediation
- Feedback loops and emergent collaboration

### 6. Communication Hierarchy

```
LEAD (Human or Opus session)
  ↕ stdin/stdout
HATCHERY CLI
  ↕ manages
COORDINATOR (Sonnet AI session)
  ↕ task assignment / monitoring
WORKERS (Sonnet AI sessions)
  ↔ horizontal collaboration via SharedMemory
```

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│ LEAD (Human or Opus Claude Code session)                           │
│   - Provides PRD                                                    │
│   - Monitors progress                                               │
│   - Can override or message coordinator/workers                     │
└────────────────────────────┬────────────────────────────────────────┘
                             │ stdin/stdout (human-readable)
                             ↓
┌─────────────────────────────────────────────────────────────────────┐
│ HATCHERY BINARY (Rust CLI)                                         │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ HatcheryCore (orchestration engine)                           │ │
│  │                                                                │ │
│  │  ┌──────────────────────────────────────────────────────────┐ │ │
│  │  │ SessionManager                                           │ │ │
│  │  │   - Manages all Claude PipeProcess instances            │ │ │
│  │  │   - Polls for TerminalEvents                            │ │ │
│  │  │   - Routes messages between sessions                    │ │ │
│  │  │                                                          │ │ │
│  │  │   Sessions:                                             │ │ │
│  │  │   ┌────────────────────┐                                │ │ │
│  │  │   │ Coordinator        │ (PipeProcess, AI session)      │ │ │
│  │  │   │ - Decomposes PRD   │                                │ │ │
│  │  │   │ - Assigns tasks    │                                │ │ │
│  │  │   │ - Monitors workers │                                │ │ │
│  │  │   └────────────────────┘                                │ │ │
│  │  │                                                          │ │ │
│  │  │   ┌──────────┐  ┌──────────┐  ┌──────────┐             │ │ │
│  │  │   │ Worker 1 │  │ Worker 2 │  │ Worker N │ (AI)        │ │ │
│  │  │   │          │  │          │  │          │             │ │ │
│  │  │   └──────────┘  └──────────┘  └──────────┘             │ │ │
│  │  └──────────────────────────────────────────────────────────┘ │ │
│  │                                                                │ │
│  │  ┌──────────────────────────────────────────────────────────┐ │ │
│  │  │ SharedMemory (in-memory + file-backed)                   │ │ │
│  │  │   - tasks: Vec<Task>          (PRD items with status)    │ │ │
│  │  │   - knowledge: HashMap        (shared facts/discoveries) │ │ │
│  │  │   - results: HashMap          (per-worker outputs)       │ │ │
│  │  │   - messages: VecDeque        (inter-agent messages)     │ │ │
│  │  │                                                           │ │ │
│  │  │   File: hatchery/state/<project-id>/shared.json          │ │ │
│  │  └──────────────────────────────────────────────────────────┘ │ │
│  │                                                                │ │
│  │  ┌──────────────────────────────────────────────────────────┐ │ │
│  │  │ EventBus                                                  │ │ │
│  │  │   - Routes TerminalEvents from PipeProcesses             │ │ │
│  │  │   - Dispatches commands to sessions                      │ │ │
│  │  │   - Broadcasts state updates                             │ │ │
│  │  └──────────────────────────────────────────────────────────┘ │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ CLI Interface                                                  │ │
│  │   - spawn <prd> [--workers N]                                 │ │
│  │   - status                                                     │ │
│  │   - message <target> <text>                                   │ │
│  │   - kill [worker-id]                                          │ │
│  │   - logs [worker-id]                                          │ │
│  └───────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Components

### 1. HatcheryCore

Main orchestration engine. Responsibilities:

```rust
pub struct HatcheryCore {
    /// Manages all Claude sessions (coordinator + workers)
    session_manager: SessionManager,

    /// Shared state accessible to all sessions
    shared_memory: Arc<RwLock<SharedMemory>>,

    /// Event routing
    event_bus: EventBus,

    /// Project metadata
    project_id: String,
    prd_path: PathBuf,

    /// Configuration
    config: HatcheryConfig,
}

impl HatcheryCore {
    /// Spawn a new swarm from PRD file
    pub async fn spawn(prd_path: PathBuf, config: HatcheryConfig) -> Result<Self>;

    /// Main event loop
    pub async fn run(&mut self) -> Result<SwarmResult>;

    /// Send message to specific session
    pub async fn send_message(&mut self, target: SessionId, msg: String) -> Result<()>;

    /// Get current status
    pub fn status(&self) -> SwarmStatus;

    /// Graceful shutdown
    pub async fn shutdown(&mut self) -> Result<()>;
}
```

### 2. SessionManager

Manages all Claude PipeProcess instances:

```rust
pub struct SessionManager {
    /// Coordinator session (always present)
    coordinator: PipeProcess,

    /// Worker sessions
    workers: HashMap<WorkerId, WorkerSession>,

    /// Session configuration
    session_config: SessionConfig,
}

pub struct WorkerSession {
    process: PipeProcess,
    id: WorkerId,
    mode: WorkerMode,
    current_task: Option<TaskId>,
    status: WorkerStatus,
    last_activity: Instant,
}

#[derive(Debug, Clone)]
pub enum WorkerMode {
    /// Autonomous AI session with shared memory access
    Smart,

    /// Ralph-style PRD iterator (no coordination)
    Dumb,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerStatus {
    Idle,
    Working(TaskId),
    Blocked(String),
    Failed(String),
    Completed,
}

impl SessionManager {
    /// Spawn coordinator + N workers
    pub async fn new(
        coordinator_prompt: String,
        worker_count: usize,
        mode: WorkerMode,
        config: SessionConfig,
    ) -> Result<Self>;

    /// Poll all sessions for events
    pub fn poll_all(&mut self) -> Vec<(SessionId, TerminalEvent)>;

    /// Send prompt to specific session
    pub async fn send_prompt(&mut self, session_id: SessionId, prompt: String) -> Result<()>;

    /// Add worker at runtime
    pub async fn add_worker(&mut self, mode: WorkerMode) -> Result<WorkerId>;

    /// Remove worker
    pub async fn remove_worker(&mut self, worker_id: WorkerId) -> Result<()>;
}
```

### 3. EventBus

Routes events between sessions and core:

```rust
pub struct EventBus {
    /// Channel for session events
    events_tx: mpsc::UnboundedSender<SessionEvent>,
    events_rx: mpsc::UnboundedReceiver<SessionEvent>,

    /// Channel for commands to sessions
    commands_tx: mpsc::UnboundedSender<SessionCommand>,
    commands_rx: mpsc::UnboundedReceiver<SessionCommand>,
}

#[derive(Debug)]
pub enum SessionEvent {
    /// Session produced output
    Output { session_id: SessionId, text: String },

    /// Session ready for next prompt
    PromptReady { session_id: SessionId },

    /// Session reported error
    Error { session_id: SessionId, error: String },

    /// Session exited
    Exit { session_id: SessionId, code: i32 },

    /// Parsed structured output from session
    Parsed { session_id: SessionId, message: AgentMessage },
}

#[derive(Debug)]
pub enum SessionCommand {
    /// Send prompt to session
    SendPrompt { session_id: SessionId, prompt: String },

    /// Update session's view of shared memory
    SyncMemory { session_id: SessionId },

    /// Kill session
    Kill { session_id: SessionId },
}
```

### 4. HatcheryConfig

Configuration for swarm behavior:

```rust
#[derive(Debug, Clone)]
pub struct HatcheryConfig {
    /// Number of worker sessions
    pub worker_count: usize,

    /// Worker mode (smart or dumb)
    pub worker_mode: WorkerMode,

    /// Custom coordinator prompt file (optional)
    pub coordinator_prompt: Option<PathBuf>,

    /// Custom worker prompt file (optional)
    pub worker_prompt: Option<PathBuf>,

    /// Claude model for workers
    pub worker_model: String, // "sonnet", "opus", etc.

    /// Claude model for coordinator
    pub coordinator_model: String,

    /// Max runtime in minutes
    pub timeout_mins: u64,

    /// Worker stall detection timeout
    pub stall_timeout_secs: u64,

    /// Verbose mode (show all session output)
    pub verbose: bool,

    /// Auto-save interval for SharedMemory
    pub autosave_interval_secs: u64,
}

impl Default for HatcheryConfig {
    fn default() -> Self {
        Self {
            worker_count: 3,
            worker_mode: WorkerMode::Smart,
            coordinator_prompt: None,
            worker_prompt: None,
            worker_model: "sonnet".to_string(),
            coordinator_model: "sonnet".to_string(),
            timeout_mins: 60,
            stall_timeout_secs: 300, // 5 minutes
            verbose: false,
            autosave_interval_secs: 30,
        }
    }
}
```

---

## Session Types

### A. Coordinator Session (AI)

**Nature**: A Claude PipeProcess session with specialized system prompt.

**Responsibilities**:
- Parse and understand PRD structure
- Decompose high-level tasks into worker-assignable units
- Assign tasks to available workers
- Monitor worker progress via EventBus
- Detect stalls and reassign tasks
- Update SharedMemory with task status
- Report progress to Lead via structured output

**Communication**:
- **Input**: PRD file, SharedMemory state, worker status updates
- **Output**: Task assignments, status reports, escalations
- **Protocol**: Structured JSON output that Hatchery parses

**Example structured output**:

```json
{
  "type": "task_assignment",
  "worker_id": "worker-1",
  "task_id": 3,
  "prompt": "Research Bybit REST API authentication. Output to shared knowledge: auth_method, required_headers."
}

{
  "type": "status_report",
  "completed": [1, 2, 3],
  "in_progress": [4, 5],
  "blocked": [],
  "message": "Phase 1 complete. Auth implementation starting."
}

{
  "type": "escalation",
  "issue": "Worker-2 stalled on task 5 for 7 minutes",
  "recommendation": "reassign",
  "fallback_worker": "worker-3"
}
```

**Loop structure**:

```rust
// Coordinator's main loop (executed by Claude in the session)
loop {
    // 1. Read current SharedMemory state
    let state = read_shared_memory();

    // 2. Check task dependencies
    let ready_tasks = find_ready_tasks(&state.tasks);

    // 3. Assign to idle workers
    for task in ready_tasks {
        if let Some(worker) = find_idle_worker() {
            assign_task(worker, task);
        }
    }

    // 4. Check for stalls
    for worker in workers {
        if worker.stalled(timeout: 5min) {
            reassign_or_escalate(worker);
        }
    }

    // 5. Report to lead
    emit_status_report();

    // 6. Check completion
    if all_tasks_done() {
        emit_completion_report();
        break;
    }

    // 7. Wait for next event
    sleep(30sec);
}
```

**Prompt template**: See [Coordinator Prompt](#coordinator-prompt-template)

---

### B. Smart Worker Session (AI)

**Nature**: Autonomous Claude PipeProcess session.

**Responsibilities**:
- Receive task assignment from coordinator
- Execute task using full Claude Code capabilities (Read, Write, Edit, Bash, Grep, etc.)
- Read SharedMemory to leverage others' work
- Write results to SharedMemory
- Optionally send brief messages to other workers for coordination
- Mark PRD checkbox when complete
- Report blockers or request help from coordinator

**Autonomy**:
- Can subdivide tasks if needed
- Can ask coordinator for clarification
- Can collaborate with other workers via messages
- Can discover new sub-tasks and report them

**Communication**:
- **Input**: Task assignment from coordinator, SharedMemory state
- **Output**: Results to SharedMemory, checkbox updates, optional worker messages
- **Protocol**: Special commands that Hatchery intercepts

**Special commands** (parsed by Hatchery):

```
@hatchery:write-knowledge api_base_url=https://api.bybit.com/v5
@hatchery:write-result Task 3: Found auth uses HMAC-SHA256 with API key in header...
@hatchery:send-message worker-2 "Found rate limit is 100/min, adjust your test plan"
@hatchery:mark-complete task-id=3
@hatchery:report-blocker "Missing API credentials, need .env file"
```

**Example workflow**:

```
Coordinator assigns: "Research Bybit REST API authentication"

Worker-1:
  1. Read SharedMemory → sees worker-2 already researched endpoints
  2. Use Grep/Read to explore Bybit docs (if available locally)
  3. Use Bash to curl Bybit docs or API
  4. Parse authentication requirements
  5. @hatchery:write-knowledge auth_method=HMAC-SHA256
  6. @hatchery:write-knowledge required_headers=X-BAPI-API-KEY,X-BAPI-SIGN,X-BAPI-TIMESTAMP
  7. @hatchery:write-result "Auth analysis complete. See knowledge base."
  8. @hatchery:mark-complete task-id=1
  9. Report back to coordinator: "Task 1 complete, ready for next"
```

**Prompt template**: See [Worker Prompt](#worker-prompt-template)

---

### C. Dumb Worker Session (Ralph-style)

**Nature**: Simple PRD iterator, no coordination.

**Behavior**:
- Reads PRD file
- Finds first unchecked task
- Executes task
- Marks checkbox
- Moves to next task
- Repeats until all tasks done or timeout

**Use cases**:
- Simple batch research tasks
- Independent file generations
- No dependencies between tasks

**No access to**:
- SharedMemory
- Inter-worker messages
- Coordinator communication

**Implementation**: Just runs a loop with the standard Ralph prompt, no special Hatchery integration.

---

## Communication Channels

### 1. Lead ↔ Hatchery CLI

**Protocol**: stdin/stdout, human-readable text

**Lead → Hatchery**:
```bash
# Spawn swarm
hatchery spawn bybit-connector.md --workers 3

# Check status
hatchery status

# Send message to coordinator
hatchery message coordinator "Prioritize authentication task"

# Send message to specific worker
hatchery message worker-1 "Skip WebSocket for now"
```

**Hatchery → Lead**:
```
[HATCHERY] Spawned 3 workers + coordinator
[HATCHERY] Coordinator: Decomposed PRD into 12 tasks
[HATCHERY] Worker-1: Started task 1 (Research API auth)
[HATCHERY] Worker-2: Started task 2 (Research endpoints)
[HATCHERY] Worker-3: Idle, waiting for task 3 dependency
[HATCHERY] Progress: 2/12 tasks complete
[HATCHERY] Worker-1: Completed task 1 → auth_method=HMAC-SHA256
[HATCHERY] Coordinator: Assigned task 3 to worker-3
...
[HATCHERY] SWARM COMPLETE: 12/12 tasks done in 18 minutes
```

---

### 2. Coordinator ↔ Workers

**Protocol**: PipeProcess.send_prompt() + TerminalEvent parsing

**Coordinator → Worker**:
```rust
// Assign task
session_manager.send_prompt(
    worker_1,
    format!("Task assignment:\n{}\n\nContext from shared memory:\n{}", task_desc, context)
).await?;

// Request status update
session_manager.send_prompt(worker_1, "@hatchery:status").await?;

// Cancel task
session_manager.send_prompt(worker_1, "@hatchery:cancel").await?;
```

**Worker → Coordinator** (via EventBus):
```rust
SessionEvent::Parsed {
    session_id: worker_1,
    message: AgentMessage::TaskComplete {
        task_id: 3,
        result: "Auth module implemented at src/auth.rs",
    }
}

SessionEvent::Parsed {
    session_id: worker_1,
    message: AgentMessage::Blocker {
        task_id: 5,
        issue: "Missing API credentials",
        needs_help: true,
    }
}
```

---

### 3. Workers ↔ Workers (Horizontal)

**Protocol**: SharedMemory.messages (async queue)

**Use case**: Brief coordination messages, not long discussions.

**Examples**:

```rust
// Worker-1 discovers something useful
@hatchery:send-message worker-2 "Rate limit is 100/min, seen in response headers"

// Worker-2 warns about breaking change
@hatchery:send-message broadcast "API v2 is deprecated, use v5 endpoints"

// Worker-3 asks for help
@hatchery:send-message worker-1 "Did you find docs on WebSocket auth?"
```

**Implementation**:

```rust
pub struct AgentMessage {
    pub from: SessionId,
    pub to: MessageTarget,
    pub content: String, // Keep under 500 chars
    pub timestamp: Instant,
}

pub enum MessageTarget {
    Specific(SessionId),
    Coordinator,
    Broadcast, // All workers
}

// In SharedMemory
pub struct SharedMemory {
    messages: VecDeque<AgentMessage>, // Max 100 messages
    // ... other fields
}

impl SharedMemory {
    pub fn send_message(&mut self, msg: AgentMessage) {
        // Keep queue bounded
        if self.messages.len() >= 100 {
            self.messages.pop_front();
        }
        self.messages.push_back(msg);
    }

    pub fn get_messages_for(&self, session_id: SessionId) -> Vec<AgentMessage> {
        self.messages.iter()
            .filter(|m| {
                matches!(m.to, MessageTarget::Specific(id) if id == session_id)
                || matches!(m.to, MessageTarget::Broadcast)
            })
            .cloned()
            .collect()
    }
}
```

---

### 4. Workers → SharedMemory

**Protocol**: Special commands parsed by Hatchery

**Write knowledge**:
```
@hatchery:write-knowledge api_base_url=https://api.bybit.com/v5
@hatchery:write-knowledge auth_method=HMAC-SHA256
```

**Write result**:
```
@hatchery:write-result task-id=3 Implemented auth module at src/exchanges/bybit/auth.rs. Uses HMAC-SHA256 with timestamp nonce.
```

**Mark task complete**:
```
@hatchery:mark-complete task-id=3
```

**Read from SharedMemory**:
Workers receive SharedMemory snapshot on every task assignment as context:

```
=== Shared Knowledge ===
api_base_url: https://api.bybit.com/v5
auth_method: HMAC-SHA256
rate_limit: 100 req/min

=== Completed Tasks ===
[✓] Task 1: Research API auth (worker-1)
[✓] Task 2: Research endpoints (worker-2)

=== Your Task ===
Task 3: Implement auth module
Dependencies: Task 1
```

---

## Shared Memory Design

### Core Structure

```rust
use std::collections::{HashMap, VecDeque};
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use std::sync::Arc;

/// Shared state accessible to all sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMemory {
    /// Project identifier
    pub project_id: String,

    /// PRD tasks with status tracking
    pub tasks: Vec<Task>,

    /// Shared knowledge base (key-value store)
    /// Examples: api_base_url, auth_method, discovered_endpoints
    pub knowledge: HashMap<String, String>,

    /// Per-worker results
    pub results: HashMap<WorkerId, Vec<WorkResult>>,

    /// Inter-agent message queue
    pub messages: VecDeque<AgentMessage>,

    /// Last updated timestamp
    pub last_updated: std::time::SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub description: String,
    pub status: TaskStatus,
    pub assigned_to: Option<WorkerId>,
    pub depends_on: Vec<TaskId>,
    pub result: Option<String>,
    pub created_at: std::time::SystemTime,
    pub updated_at: std::time::SystemTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Assigned(WorkerId),
    InProgress,
    Completed,
    Failed(String),
    Blocked(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkResult {
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub content: String,
    pub artifacts: Vec<PathBuf>, // Files created/modified
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub from: SessionId,
    pub to: MessageTarget,
    pub content: String,
    pub timestamp: std::time::Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageTarget {
    Specific(SessionId),
    Coordinator,
    Broadcast,
}

pub type TaskId = usize;
pub type WorkerId = String; // e.g., "worker-1"
pub type SessionId = String; // "coordinator" or "worker-1"
```

### File-Backed Persistence

```rust
impl SharedMemory {
    /// Load from disk (on startup or crash recovery)
    pub fn load(project_id: &str) -> Result<Self> {
        let path = Self::get_path(project_id);
        if path.exists() {
            let json = std::fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&json)?)
        } else {
            Ok(Self::new(project_id))
        }
    }

    /// Save to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::get_path(&self.project_id);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Auto-save path: hatchery/state/<project-id>/shared.json
    fn get_path(project_id: &str) -> PathBuf {
        PathBuf::from("hatchery")
            .join("state")
            .join(project_id)
            .join("shared.json")
    }
}
```

### Knowledge Operations

```rust
impl SharedMemory {
    /// Write knowledge (idempotent)
    pub fn write_knowledge(&mut self, key: String, value: String) {
        self.knowledge.insert(key, value);
        self.last_updated = std::time::SystemTime::now();
    }

    /// Read knowledge
    pub fn read_knowledge(&self, key: &str) -> Option<&String> {
        self.knowledge.get(key)
    }

    /// Get all knowledge as formatted string
    pub fn format_knowledge(&self) -> String {
        self.knowledge.iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
```

### Task Operations

```rust
impl SharedMemory {
    /// Mark task as complete
    pub fn complete_task(&mut self, task_id: TaskId, result: String) -> Result<()> {
        let task = self.tasks.get_mut(task_id)
            .ok_or_else(|| anyhow!("Task {} not found", task_id))?;

        task.status = TaskStatus::Completed;
        task.result = Some(result);
        task.updated_at = std::time::SystemTime::now();
        Ok(())
    }

    /// Assign task to worker
    pub fn assign_task(&mut self, task_id: TaskId, worker_id: WorkerId) -> Result<()> {
        let task = self.tasks.get_mut(task_id)
            .ok_or_else(|| anyhow!("Task {} not found", task_id))?;

        task.status = TaskStatus::Assigned(worker_id.clone());
        task.assigned_to = Some(worker_id);
        task.updated_at = std::time::SystemTime::now();
        Ok(())
    }

    /// Get ready tasks (dependencies met, not assigned)
    pub fn get_ready_tasks(&self) -> Vec<&Task> {
        self.tasks.iter()
            .filter(|t| {
                matches!(t.status, TaskStatus::Pending)
                && t.depends_on.iter().all(|dep_id| {
                    self.tasks.get(*dep_id)
                        .map(|dep| matches!(dep.status, TaskStatus::Completed))
                        .unwrap_or(false)
                })
            })
            .collect()
    }

    /// Check if all tasks complete
    pub fn all_tasks_done(&self) -> bool {
        self.tasks.iter().all(|t| matches!(t.status, TaskStatus::Completed))
    }
}
```

### Message Operations

```rust
impl SharedMemory {
    /// Send message
    pub fn send_message(&mut self, from: SessionId, to: MessageTarget, content: String) {
        if self.messages.len() >= 100 {
            self.messages.pop_front();
        }

        self.messages.push_back(AgentMessage {
            from,
            to,
            content,
            timestamp: std::time::Instant::now(),
        });
    }

    /// Get messages for specific session
    pub fn get_messages_for(&self, session_id: &SessionId) -> Vec<&AgentMessage> {
        self.messages.iter()
            .filter(|m| {
                match &m.to {
                    MessageTarget::Specific(id) => id == session_id,
                    MessageTarget::Broadcast => m.from != *session_id, // Don't return own broadcasts
                    MessageTarget::Coordinator => session_id == "coordinator",
                }
            })
            .collect()
    }

    /// Clear old messages (older than 1 hour)
    pub fn prune_old_messages(&mut self) {
        let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(3600);
        self.messages.retain(|m| m.timestamp > cutoff);
    }
}
```

---

## Event Flow

### Main Event Loop

```rust
impl HatcheryCore {
    pub async fn run(&mut self) -> Result<SwarmResult> {
        let start_time = Instant::now();
        let mut autosave_timer = tokio::time::interval(
            Duration::from_secs(self.config.autosave_interval_secs)
        );

        loop {
            tokio::select! {
                // Poll all sessions
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    self.poll_sessions().await?;
                }

                // Auto-save SharedMemory
                _ = autosave_timer.tick() => {
                    self.shared_memory.write().await.save()?;
                }

                // Check timeout
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    if start_time.elapsed() > Duration::from_secs(self.config.timeout_mins * 60) {
                        return Err(anyhow!("Swarm timeout after {} minutes", self.config.timeout_mins));
                    }
                }
            }

            // Check completion
            if self.shared_memory.read().await.all_tasks_done() {
                return Ok(self.generate_result().await?);
            }

            // Check for stalls
            self.detect_stalls().await?;
        }
    }

    async fn poll_sessions(&mut self) -> Result<()> {
        let events = self.session_manager.poll_all();

        for (session_id, event) in events {
            match event {
                TerminalEvent::Output(text) => {
                    self.handle_output(session_id, text).await?;
                }

                TerminalEvent::PromptReady => {
                    self.handle_prompt_ready(session_id).await?;
                }

                TerminalEvent::Error(e) => {
                    self.handle_error(session_id, e).await?;
                }

                TerminalEvent::Exit(code) => {
                    self.handle_exit(session_id, code).await?;
                }

                TerminalEvent::Parsed(msg) => {
                    self.handle_parsed_message(session_id, msg).await?;
                }
            }
        }

        Ok(())
    }
}
```

### Event Handlers

```rust
impl HatcheryCore {
    /// Handle text output from session
    async fn handle_output(&mut self, session_id: SessionId, text: String) -> Result<()> {
        // Log to file
        self.log_session_output(&session_id, &text)?;

        // Parse for special @hatchery: commands
        if text.contains("@hatchery:") {
            self.parse_hatchery_command(&session_id, &text).await?;
        }

        // If verbose mode, print to stdout
        if self.config.verbose {
            println!("[{}] {}", session_id, text);
        }

        // Route to coordinator if from worker
        if session_id.starts_with("worker-") && text.contains("[REPORT]") {
            self.forward_to_coordinator(&session_id, &text).await?;
        }

        Ok(())
    }

    /// Handle session ready for next prompt
    async fn handle_prompt_ready(&mut self, session_id: SessionId) -> Result<()> {
        // Mark worker as idle
        if let Some(worker) = self.session_manager.workers.get_mut(&session_id) {
            worker.status = WorkerStatus::Idle;
            worker.last_activity = Instant::now();
        }

        // If coordinator, trigger next assignment cycle
        if session_id == "coordinator" {
            self.trigger_coordinator_cycle().await?;
        }

        Ok(())
    }

    /// Handle session error
    async fn handle_error(&mut self, session_id: SessionId, error: String) -> Result<()> {
        eprintln!("[HATCHERY] Error from {}: {}", session_id, error);

        // If worker error, mark task as failed and reassign
        if session_id.starts_with("worker-") {
            if let Some(worker) = self.session_manager.workers.get(&session_id) {
                if let Some(task_id) = worker.current_task {
                    self.reassign_task(task_id, &session_id).await?;
                }
            }
        }

        // If coordinator error, escalate to lead
        if session_id == "coordinator" {
            println!("[HATCHERY] CRITICAL: Coordinator error, manual intervention needed");
        }

        Ok(())
    }

    /// Handle session exit
    async fn handle_exit(&mut self, session_id: SessionId, code: i32) -> Result<()> {
        println!("[HATCHERY] {} exited with code {}", session_id, code);

        // If worker exited unexpectedly, respawn
        if session_id.starts_with("worker-") && code != 0 {
            self.respawn_worker(&session_id).await?;
        }

        Ok(())
    }

    /// Handle parsed structured message
    async fn handle_parsed_message(&mut self, session_id: SessionId, msg: AgentMessage) -> Result<()> {
        // Update SharedMemory based on message type
        match msg {
            AgentMessage::WriteKnowledge { key, value } => {
                self.shared_memory.write().await.write_knowledge(key, value);
            }

            AgentMessage::WriteResult { task_id, content } => {
                self.shared_memory.write().await.results
                    .entry(session_id.clone())
                    .or_default()
                    .push(WorkResult {
                        task_id,
                        worker_id: session_id,
                        content,
                        artifacts: vec![],
                        timestamp: std::time::SystemTime::now(),
                    });
            }

            AgentMessage::MarkComplete { task_id } => {
                self.shared_memory.write().await.complete_task(task_id, "".to_string())?;
            }

            AgentMessage::SendMessage { to, content } => {
                self.shared_memory.write().await.send_message(session_id, to, content);
            }

            // ... other message types
        }

        Ok(())
    }
}
```

### Stall Detection

```rust
impl HatcheryCore {
    async fn detect_stalls(&mut self) -> Result<()> {
        let stall_timeout = Duration::from_secs(self.config.stall_timeout_secs);
        let now = Instant::now();

        for (worker_id, worker) in &self.session_manager.workers {
            if matches!(worker.status, WorkerStatus::Working(_)) {
                if now.duration_since(worker.last_activity) > stall_timeout {
                    println!("[HATCHERY] Worker {} stalled, reassigning task", worker_id);

                    if let Some(task_id) = worker.current_task {
                        self.reassign_task(task_id, worker_id).await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn reassign_task(&mut self, task_id: TaskId, failed_worker: &str) -> Result<()> {
        // Find idle worker
        let idle_worker = self.session_manager.workers.iter()
            .find(|(id, w)| matches!(w.status, WorkerStatus::Idle) && id.as_str() != failed_worker)
            .map(|(id, _)| id.clone());

        if let Some(worker_id) = idle_worker {
            // Assign to new worker
            self.shared_memory.write().await.assign_task(task_id, worker_id.clone())?;

            // Send prompt
            let task = &self.shared_memory.read().await.tasks[task_id];
            let context = self.build_context_for_task(task).await?;

            self.session_manager.send_prompt(
                worker_id.clone(),
                format!("Task assignment (reassigned from {}):\n{}\n\nContext:\n{}",
                    failed_worker, task.description, context)
            ).await?;

            println!("[HATCHERY] Reassigned task {} from {} to {}",
                task_id, failed_worker, worker_id);
        } else {
            println!("[HATCHERY] No idle workers available for reassignment");
        }

        Ok(())
    }
}
```

### Command Parsing

```rust
impl HatcheryCore {
    async fn parse_hatchery_command(&mut self, session_id: &str, text: &str) -> Result<()> {
        for line in text.lines() {
            if !line.starts_with("@hatchery:") {
                continue;
            }

            let cmd = line.trim_start_matches("@hatchery:");
            let parts: Vec<&str> = cmd.splitn(2, ' ').collect();

            match parts[0] {
                "write-knowledge" => {
                    if let Some(kv) = parts.get(1) {
                        if let Some((k, v)) = kv.split_once('=') {
                            self.shared_memory.write().await.write_knowledge(
                                k.to_string(),
                                v.to_string()
                            );
                            println!("[HATCHERY] {} wrote knowledge: {} = {}", session_id, k, v);
                        }
                    }
                }

                "write-result" => {
                    if let Some(result) = parts.get(1) {
                        // Parse "task-id=X <content>"
                        if let Some((task_part, content)) = result.split_once(' ') {
                            if let Some(id_str) = task_part.strip_prefix("task-id=") {
                                if let Ok(task_id) = id_str.parse::<TaskId>() {
                                    self.shared_memory.write().await.results
                                        .entry(session_id.to_string())
                                        .or_default()
                                        .push(WorkResult {
                                            task_id,
                                            worker_id: session_id.to_string(),
                                            content: content.to_string(),
                                            artifacts: vec![],
                                            timestamp: std::time::SystemTime::now(),
                                        });
                                }
                            }
                        }
                    }
                }

                "mark-complete" => {
                    if let Some(task_part) = parts.get(1) {
                        if let Some(id_str) = task_part.strip_prefix("task-id=") {
                            if let Ok(task_id) = id_str.parse::<TaskId>() {
                                self.shared_memory.write().await.complete_task(task_id, "".to_string())?;
                                println!("[HATCHERY] {} marked task {} complete", session_id, task_id);
                            }
                        }
                    }
                }

                "send-message" => {
                    if let Some(msg_part) = parts.get(1) {
                        if let Some((target, content)) = msg_part.split_once(' ') {
                            let msg_target = match target {
                                "coordinator" => MessageTarget::Coordinator,
                                "broadcast" => MessageTarget::Broadcast,
                                worker_id => MessageTarget::Specific(worker_id.to_string()),
                            };

                            self.shared_memory.write().await.send_message(
                                session_id.to_string(),
                                msg_target,
                                content.to_string()
                            );
                        }
                    }
                }

                "report-blocker" => {
                    if let Some(issue) = parts.get(1) {
                        println!("[HATCHERY] {} BLOCKED: {}", session_id, issue);
                        // Update worker status
                        if let Some(worker) = self.session_manager.workers.get_mut(session_id) {
                            worker.status = WorkerStatus::Blocked(issue.to_string());
                        }
                    }
                }

                _ => {
                    eprintln!("[HATCHERY] Unknown command: {}", cmd);
                }
            }
        }

        Ok(())
    }
}
```

---

## PRD Format

### Structure

```markdown
# Hatchery PRD: <Project Name>

**Project ID:** `<unique-id>`
**Created:** <timestamp>
**Status:** In Progress / Complete

## Context

<High-level goal description>

Example:
> Create a Bybit exchange connector for the V5 connector architecture.
> Must support REST API (public + private), authentication, and WebSocket streams.

## Tasks

- [ ] **Task 1**: Research Bybit REST API authentication
  - **Worker**: any
  - **Depends on**: none
  - **Estimated**: 15min

- [ ] **Task 2**: Research Bybit REST API endpoints (public + private)
  - **Worker**: any
  - **Depends on**: none
  - **Estimated**: 20min

- [ ] **Task 3**: Implement authentication module (auth.rs)
  - **Worker**: any
  - **Depends on**: Task 1
  - **Estimated**: 30min

- [x] **Task 4**: Implement endpoint definitions (endpoints.rs)
  - **Worker**: worker-2
  - **Depends on**: Task 2
  - **Completed**: 2026-02-06 14:23
  - **Duration**: 18min

- [ ] **Task 5**: Implement JSON parser (parser.rs)
  - **Worker**: any
  - **Depends on**: Task 2
  - **Estimated**: 25min

- [ ] **Task 6**: Implement connector trait (connector.rs)
  - **Worker**: any
  - **Depends on**: Task 3, Task 4, Task 5
  - **Estimated**: 45min

- [ ] **Task 7**: Write integration tests
  - **Worker**: any
  - **Depends on**: Task 6
  - **Estimated**: 30min

- [ ] **Task 8**: Debug tests until real data
  - **Worker**: any
  - **Depends on**: Task 7
  - **Estimated**: variable

## Shared Knowledge

<!-- Auto-updated by workers via @hatchery:write-knowledge -->

- **api_base_url**: `https://api.bybit.com/v5`
- **auth_method**: `HMAC-SHA256`
- **required_headers**: `X-BAPI-API-KEY`, `X-BAPI-SIGN`, `X-BAPI-TIMESTAMP`
- **rate_limit**: `100 req/min`
- **websocket_url**: `wss://stream.bybit.com/v5/public/linear`

## Results

<!-- Auto-updated by workers via @hatchery:write-result -->

### Task 1 (worker-1, 15min)

Found Bybit uses HMAC-SHA256 authentication with:
- API key in `X-BAPI-API-KEY` header
- Signature in `X-BAPI-SIGN` header
- Timestamp in `X-BAPI-TIMESTAMP` header
- Signature payload: `timestamp + apiKey + recvWindow + queryString + body`

Reference: https://bybit-exchange.github.io/docs/v5/guide#authentication

### Task 2 (worker-2, 20min)

Documented 23 endpoints across:
- Market data (8 public endpoints)
- Trading (7 private endpoints)
- Account (5 private endpoints)
- Wallet (3 private endpoints)

All use REST with JSON responses. See `endpoints.rs`.

### Task 4 (worker-2, 18min)

Implemented endpoint definitions at `src/exchanges/bybit/endpoints.rs`:
- Created `BybitEndpoint` enum with all 23 endpoints
- Implemented URL formatting
- Symbol normalization (BTC/USDT → BTCUSDT)

---

## Metadata

**Total tasks**: 8
**Completed**: 3
**In progress**: 2
**Blocked**: 0
**Failed**: 0

**Workers active**: 3
**Total runtime**: 47 minutes
**Est. completion**: 2026-02-06 15:30
```

### Parsing Logic

```rust
pub struct PrdParser;

impl PrdParser {
    pub fn parse(prd_path: &Path) -> Result<Vec<Task>> {
        let content = std::fs::read_to_string(prd_path)?;
        let mut tasks = Vec::new();
        let mut current_id = 0;

        for line in content.lines() {
            // Parse task line: "- [ ] **Task 1**: Description"
            if let Some(task_line) = line.strip_prefix("- [ ] **Task ") {
                if let Some((id_part, desc)) = task_line.split_once("**: ") {
                    let id = id_part.trim_end_matches("**").parse::<usize>()?;

                    tasks.push(Task {
                        id,
                        description: desc.to_string(),
                        status: TaskStatus::Pending,
                        assigned_to: None,
                        depends_on: vec![],
                        result: None,
                        created_at: std::time::SystemTime::now(),
                        updated_at: std::time::SystemTime::now(),
                    });
                    current_id = id;
                }
            }

            // Parse dependencies: "  - **Depends on**: Task 1, Task 2"
            if line.contains("**Depends on**:") {
                if let Some(deps_str) = line.split("**Depends on**:").nth(1) {
                    let deps = deps_str.split(',')
                        .filter_map(|s| {
                            s.trim()
                                .strip_prefix("Task ")
                                .and_then(|n| n.parse::<usize>().ok())
                        })
                        .collect::<Vec<_>>();

                    if let Some(task) = tasks.last_mut() {
                        task.depends_on = deps;
                    }
                }
            }
        }

        Ok(tasks)
    }

    /// Update PRD file with task completion
    pub fn mark_complete(prd_path: &Path, task_id: TaskId) -> Result<()> {
        let content = std::fs::read_to_string(prd_path)?;
        let updated = content.replace(
            &format!("- [ ] **Task {}**:", task_id),
            &format!("- [x] **Task {}**:", task_id)
        );
        std::fs::write(prd_path, updated)?;
        Ok(())
    }
}
```

---

## Prompt Templates

### Coordinator Prompt Template

```markdown
# Hatchery Coordinator System Prompt

You are the coordinator AI for a Hatchery swarm. Your role is to orchestrate multiple worker Claude sessions to complete a project defined in a PRD (Product Requirements Document).

## Your Responsibilities

1. **Parse the PRD**: Understand all tasks, dependencies, and context
2. **Decompose work**: Break down complex tasks if needed
3. **Assign tasks**: Distribute work to available workers based on:
   - Task dependencies (only assign when dependencies complete)
   - Worker availability (prefer idle workers)
   - Worker specialization (if any patterns emerge)
4. **Monitor progress**: Track worker status via their reports
5. **Detect stalls**: Identify workers stuck for >5 minutes
6. **Reassign failures**: Move tasks from failed/stalled workers to idle ones
7. **Report to Lead**: Provide regular status updates

## Available Workers

You manage {N} worker sessions:
- worker-1 (status: {status})
- worker-2 (status: {status})
- worker-N (status: {status})

Each worker is a full Claude Code session with all tools (Read, Write, Edit, Bash, Grep, etc.).

## Communication Protocol

### To assign a task to a worker:

```json
{
  "type": "task_assignment",
  "worker_id": "worker-1",
  "task_id": 3,
  "prompt": "Implement authentication module at src/exchanges/bybit/auth.rs. Use HMAC-SHA256 from shared knowledge. Reference: kucoin/auth.rs"
}
```

### To report status to Lead:

```json
{
  "type": "status_report",
  "completed": [1, 2, 3],
  "in_progress": [4, 5],
  "blocked": [],
  "pending": [6, 7, 8],
  "message": "Phase 1 (research) complete. Implementation phase started."
}
```

### To handle a stalled worker:

```json
{
  "type": "reassignment",
  "from_worker": "worker-2",
  "to_worker": "worker-3",
  "task_id": 5,
  "reason": "Worker-2 stalled for 7 minutes on task 5"
}
```

### To escalate an issue:

```json
{
  "type": "escalation",
  "issue": "All workers blocked on missing API credentials",
  "recommendation": "Lead needs to provide .env file with BYBIT_API_KEY and BYBIT_SECRET"
}
```

## Shared Memory Access

You can read SharedMemory state to see:
- **knowledge**: Key-value store updated by workers (e.g., api_base_url, auth_method)
- **results**: Per-worker outputs for completed tasks
- **messages**: Inter-worker messages (for context)

Workers automatically have access to SharedMemory context when you assign tasks.

## Decision Framework

When deciding what to do next:

1. Check if any dependencies are now satisfied → assign newly ready tasks
2. Check if any workers are idle → find tasks to assign them
3. Check if any workers reported blockers → escalate or provide guidance
4. Check if all tasks done → emit completion report

## Example Coordination Loop

```
1. Parse PRD → identify 8 tasks
2. Tasks 1 & 2 have no dependencies → assign to worker-1 and worker-2
3. Wait for completion signals
4. Worker-1 completes task 1 → task 3 now ready (depends on 1)
5. Assign task 3 to worker-1
6. Worker-2 completes task 2 → tasks 4 & 5 now ready
7. Assign task 4 to worker-2, task 5 to worker-3
8. Continue until all tasks complete
9. Emit final status report to Lead
```

## Current PRD

```
{PRD_CONTENT}
```

## Shared Memory State

```
{SHARED_MEMORY_SNAPSHOT}
```

## Instructions

Begin coordinating. Start by analyzing the PRD and assigning initial tasks to idle workers.
```

---

### Worker Prompt Template

```markdown
# Hatchery Worker System Prompt

You are a worker AI in a Hatchery swarm. You work on tasks assigned by the coordinator alongside other workers to complete a project.

## Your Role

- **Execute tasks** assigned by coordinator using all your tools (Read, Write, Edit, Bash, Grep, etc.)
- **Access shared knowledge** written by other workers to avoid duplicate work
- **Write results** back to shared memory for others to use
- **Collaborate** with other workers via brief messages when needed
- **Report completion** when task is done
- **Report blockers** if you get stuck

## Shared Memory Access

You have access to a shared knowledge base that all workers can read and write:

### Read from shared knowledge:

The coordinator includes relevant shared knowledge in your task assignment. Example:

```
=== Shared Knowledge ===
api_base_url: https://api.bybit.com/v5
auth_method: HMAC-SHA256
rate_limit: 100 req/min
```

### Write to shared knowledge:

Use the special `@hatchery:write-knowledge` command:

```
@hatchery:write-knowledge api_base_url=https://api.bybit.com/v5
@hatchery:write-knowledge auth_method=HMAC-SHA256
```

Keys should be snake_case, values can be any string.

## Reporting Results

### Write your result:

```
@hatchery:write-result task-id=3 Implemented auth module at src/exchanges/bybit/auth.rs. Uses HMAC-SHA256 with timestamp nonce. All unit tests passing.
```

### Mark task complete:

```
@hatchery:mark-complete task-id=3
```

### Report blocker:

```
@hatchery:report-blocker Missing API credentials, need .env file with BYBIT_API_KEY
```

## Collaborating with Other Workers

You can send brief messages to other workers for coordination:

### To specific worker:

```
@hatchery:send-message worker-2 Found rate limit is 100/min in headers, adjust your test plan
```

### Broadcast to all workers:

```
@hatchery:send-message broadcast API v2 is deprecated, everyone use v5 endpoints
```

### To coordinator:

```
@hatchery:send-message coordinator Need clarification: should WebSocket support private streams or just public?
```

**Keep messages under 500 characters. Use shared knowledge for longer content.**

## Autonomy Guidelines

You have autonomy to:

- **Subdivide tasks**: If a task is complex, break it into sub-steps
- **Ask for help**: Message coordinator or other workers
- **Skip inefficient work**: If another worker already did something, reference their result
- **Discover new tasks**: If you find missing requirements, report them

You should NOT:

- Wait indefinitely for responses (report blocker after 2 minutes)
- Duplicate work already in shared knowledge
- Write overly verbose messages (use shared knowledge for details)

## Example Workflow

**Coordinator assigns:**
> Task 3: Implement authentication module at src/exchanges/bybit/auth.rs

**You do:**

1. Read shared knowledge → see worker-1 documented auth_method=HMAC-SHA256
2. Check if kucoin/auth.rs exists → use as reference
3. Implement auth.rs using HMAC-SHA256
4. Run `cargo check` to verify it compiles
5. Write knowledge: `@hatchery:write-knowledge auth_implementation=complete`
6. Write result: `@hatchery:write-result task-id=3 Implemented auth module at src/exchanges/bybit/auth.rs`
7. Mark complete: `@hatchery:mark-complete task-id=3`
8. Report to coordinator: "Task 3 complete, ready for next assignment"

## Current Task Assignment

{TASK_DESCRIPTION}

## Relevant Shared Knowledge

{SHARED_KNOWLEDGE_CONTEXT}

## Other Workers' Recent Results

{RECENT_RESULTS}

## Instructions

Execute the assigned task. Use shared knowledge to avoid duplicate work. Report your result when done.
```

---

## Dependencies

### From zengeld-hub-core

Hatchery requires these components from `zengeld-hub-core`:

```rust
// Core process management
use zengeld_hub_core::{
    pipe_process::PipeProcess,        // Headless Claude sessions
    pty_process::PtyProcess,          // (optional, for local dev)
    cli_tool::{CliTool, CliEvent},    // Output parsing
    session::Session,                 // Session state management
};

// Event classification
use zengeld_hub_core::classification::{
    ClassificationPipeline,           // Understand session output
    OutputClassifier,                 // Categorize messages
};

// Rate limiting
use zengeld_hub_core::rate_limit::{
    RateLimitDetector,                // Detect API rate limits
    RateLimitHandler,                 // Auto-retry logic
};

// Terminal events
use zengeld_hub_core::events::{
    TerminalEvent,                    // Output, error, exit, etc.
    EventStream,                      // Async event polling
};
```

### Hatchery-Specific Additions

```rust
// In hatchery/src/lib.rs

mod core;           // HatcheryCore
mod session;        // SessionManager, WorkerSession
mod memory;         // SharedMemory
mod events;         // EventBus, custom events
mod parser;         // PRD parsing
mod cli;            // CLI interface

pub use core::HatcheryCore;
pub use session::{SessionManager, WorkerSession, WorkerMode};
pub use memory::{SharedMemory, Task, TaskStatus};
pub use cli::HatcheryCli;
```

### Cargo.toml

```toml
[package]
name = "hatchery"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "hatchery"
path = "src/main.rs"

[dependencies]
zengeld-hub-core = { path = "../zengeld-hub/zengeld-hub-core" }

tokio = { version = "1.35", features = ["full"] }
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
clap = { version = "4.4", features = ["derive"] }

[dev-dependencies]
tempfile = "3.8"
```

---

## CLI Interface

### Commands

#### 1. `spawn` - Start a new swarm

```bash
hatchery spawn <prd.md> [OPTIONS]

OPTIONS:
  --workers <N>              Number of worker sessions [default: 3]
  --coordinator <FILE>       Custom coordinator prompt file
  --worker-prompt <FILE>     Custom worker prompt file
  --mode <smart|dumb>        Worker mode [default: smart]
  --model <MODEL>            Claude model for workers [default: sonnet]
  --coordinator-model <MODEL> Claude model for coordinator [default: sonnet]
  --timeout <MINS>           Max runtime in minutes [default: 60]
  --stall-timeout <SECS>     Worker stall timeout [default: 300]
  --verbose                  Show all session output
  --project-id <ID>          Custom project ID [default: auto-generated]

EXAMPLES:
  # Basic: 3 smart workers, default settings
  hatchery spawn bybit-connector.md

  # Custom: 5 workers, extended timeout
  hatchery spawn bybit-connector.md --workers 5 --timeout 120

  # Dumb mode (Ralph-style)
  hatchery spawn research-tasks.md --mode dumb --workers 10

  # Custom prompts
  hatchery spawn project.md --coordinator my-coordinator.md --worker-prompt my-worker.md
```

#### 2. `status` - Show swarm status

```bash
hatchery status [PROJECT_ID]

OUTPUT:
  Project: bybit-connector (ID: bybit-20260206-1423)
  Runtime: 47 minutes

  Tasks: 3/8 complete, 2 in progress, 3 pending

  Workers:
    worker-1: Working on task 5 (23min)
    worker-2: Idle (last active: 2min ago)
    worker-3: Working on task 6 (11min)

  Recent completions:
    [14:45] Task 3: Implement auth module (worker-1, 18min)
    [14:52] Task 4: Implement endpoints (worker-2, 15min)
    [15:03] Task 2: Research endpoints (worker-3, 12min)

  Shared knowledge: 7 entries
  Messages: 12 (3 unread)
```

#### 3. `message` - Send message to coordinator or worker

```bash
hatchery message <TARGET> <TEXT>

TARGETS:
  coordinator      Send to coordinator
  worker-1         Send to specific worker
  broadcast        Send to all workers

EXAMPLES:
  hatchery message coordinator "Prioritize WebSocket implementation"
  hatchery message worker-1 "Skip unit tests for now, focus on integration"
  hatchery message broadcast "Use API v5, v2 is deprecated"
```

#### 4. `kill` - Stop swarm or specific worker

```bash
hatchery kill [WORKER_ID]

# Kill entire swarm
hatchery kill

# Kill specific worker (will be respawned)
hatchery kill worker-2
```

#### 5. `logs` - Show session logs

```bash
hatchery logs [WORKER_ID] [OPTIONS]

OPTIONS:
  --tail <N>      Show last N lines [default: 50]
  --follow        Follow log output (like tail -f)
  --all           Show all sessions

EXAMPLES:
  # Show coordinator logs
  hatchery logs coordinator

  # Show worker-1 logs
  hatchery logs worker-1 --tail 100

  # Follow all logs
  hatchery logs --all --follow
```

#### 6. `resume` - Resume crashed swarm

```bash
hatchery resume <PROJECT_ID>

# Loads SharedMemory from hatchery/state/<project-id>/shared.json
# Respawns coordinator + workers
# Continues from last saved state
```

---

## Integration with Claude Code

### 1. As `/hatchery` Skill

Define in `.claude/skills/`:

```markdown
# Hatchery Skill

## Trigger
/hatchery

## Description
Spawn a swarm of Claude worker sessions to execute a multi-step project in parallel.

## Usage
/hatchery <prd-file> [--workers N]

## Example
User: /hatchery research/bybit-api-research.md --workers 5
Assistant: *spawns hatchery swarm via Bash*
```

### 2. As Task Tool Subagent

```python
# In Claude Code coordinator session

Task(name="hatchery-bybit", tool="bash", background=False, prompt="""
Run hatchery swarm for Bybit connector:
hatchery spawn bybit-connector.md --workers 3 --timeout 90
""")
```

### 3. Opus Lead Spawning Hatchery

**Scenario**: Opus receives large project from user

```
User: Create connectors for Bybit, OKX, and Kraken

Opus (Lead):
  1. Creates 3 PRD files (bybit.md, okx.md, kraken.md)
  2. Spawns 3 hatchery swarms in parallel:

     Bash: hatchery spawn bybit.md --workers 3 &
     Bash: hatchery spawn okx.md --workers 3 &
     Bash: hatchery spawn kraken.md --workers 3 &

  3. Monitors stdout from all 3 hatchery instances
  4. Aggregates final results
  5. Reports to user
```

**Communication flow**:

```
USER
  ↓
OPUS (Lead Claude session)
  ↓ (spawns via Bash)
  ├─→ hatchery (bybit) → coordinator → 3 workers
  ├─→ hatchery (okx) → coordinator → 3 workers
  └─→ hatchery (kraken) → coordinator → 3 workers
  ↓ (reads stdout)
OPUS aggregates results
  ↓
USER
```

### 4. Bidirectional Communication

**Lead → Hatchery**:
```bash
# Send command via stdin
echo "message coordinator Prioritize task 5" | hatchery control <project-id>
```

**Hatchery → Lead**:
```
[HATCHERY] Worker-1 completed task 3
[HATCHERY] Progress: 5/12 tasks done
[HATCHERY] BLOCKER: Worker-2 needs API credentials
```

Opus parses stdout and can respond by sending commands.

---

## Implementation Roadmap

### Phase 1: Core Infrastructure (Week 1)

**Goal**: Basic swarm with dumb workers

- [ ] Create `hatchery/` crate in nemo workspace
- [ ] Implement `SharedMemory` with file-backed persistence
- [ ] Implement `SessionManager` (spawn coordinator + workers as PipeProcesses)
- [ ] Implement `EventBus` (route TerminalEvents)
- [ ] Implement `PrdParser` (parse markdown tasks)
- [ ] CLI: `hatchery spawn` with dumb mode
- [ ] Test: Spawn 3 dumb workers on simple PRD

**Deliverable**: Hatchery can spawn N workers that iterate through PRD checkboxes independently.

---

### Phase 2: Coordinator AI (Week 2)

**Goal**: AI coordinator managing workers

- [ ] Design coordinator prompt template
- [ ] Implement coordinator → worker task assignment (via PipeProcess.send_prompt)
- [ ] Implement worker → coordinator status reporting (via EventBus)
- [ ] Implement dependency tracking (only assign when dependencies met)
- [ ] Implement stall detection (reassign if worker idle >5min)
- [ ] CLI: `hatchery status`
- [ ] Test: Coordinator assigns 5 tasks to 3 workers with dependencies

**Deliverable**: Coordinator AI intelligently assigns tasks based on dependencies and worker availability.

---

### Phase 3: Smart Workers (Week 3)

**Goal**: Workers with autonomy and shared memory

- [ ] Design worker prompt template
- [ ] Implement `@hatchery:write-knowledge` command parsing
- [ ] Implement `@hatchery:write-result` command parsing
- [ ] Implement `@hatchery:mark-complete` command parsing
- [ ] Implement SharedMemory context injection (workers see others' knowledge)
- [ ] CLI: `hatchery logs`
- [ ] Test: Worker-1 writes knowledge, worker-2 reads it, avoids duplicate work

**Deliverable**: Workers collaborate via shared knowledge base.

---

### Phase 4: Horizontal Communication (Week 4)

**Goal**: Workers can message each other

- [ ] Implement `@hatchery:send-message` command parsing
- [ ] Implement message queue in SharedMemory
- [ ] Implement message delivery (coordinator forwards to target worker)
- [ ] Implement message pruning (keep last 100, or 1 hour)
- [ ] CLI: `hatchery message`
- [ ] Test: Worker-1 messages worker-2, worker-2 receives and responds

**Deliverable**: Workers can coordinate without coordinator mediation.

---

### Phase 5: Reliability & Recovery (Week 5)

**Goal**: Handle failures gracefully

- [ ] Implement worker crash detection (via TerminalEvent::Exit)
- [ ] Implement worker respawn logic
- [ ] Implement task reassignment on failure
- [ ] Implement auto-save (SharedMemory persists every 30s)
- [ ] CLI: `hatchery resume`
- [ ] Test: Kill worker mid-task, hatchery reassigns to another worker

**Deliverable**: Swarm survives worker crashes and can resume from saved state.

---

### Phase 6: Integration & Polish (Week 6)

**Goal**: Production-ready

- [ ] Integrate with Claude Code as `/hatchery` skill
- [ ] Implement `hatchery kill` command
- [ ] Implement verbose mode (show all session output)
- [ ] Improve status reporting (rich formatting)
- [ ] Add metrics (tasks/min, worker utilization)
- [ ] Documentation: Write user guide
- [ ] Test: Full connector creation (Bybit) with 3 workers

**Deliverable**: Hatchery ready for real-world use.

---

## ASCII Diagrams

### Swarm Lifecycle

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. SPAWN                                                        │
│    Lead runs: hatchery spawn bybit.md --workers 3              │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ↓
┌─────────────────────────────────────────────────────────────────┐
│ 2. INITIALIZATION                                               │
│    - Parse PRD → extract 8 tasks                               │
│    - Create SharedMemory (in-memory + file-backed)             │
│    - Spawn PipeProcess for coordinator (Sonnet)                │
│    - Spawn 3 PipeProcesses for workers (Sonnet)                │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ↓
┌─────────────────────────────────────────────────────────────────┐
│ 3. COORDINATION LOOP                                            │
│    Coordinator:                                                 │
│      - Reads PRD + SharedMemory                                │
│      - Finds tasks 1 & 2 (no dependencies)                     │
│      - Assigns task 1 to worker-1                              │
│      - Assigns task 2 to worker-2                              │
│      - Waits for completion signals                            │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ↓
┌─────────────────────────────────────────────────────────────────┐
│ 4. WORKER EXECUTION                                             │
│    Worker-1:                                                    │
│      - Executes task 1 (research auth)                         │
│      - Writes to SharedMemory: auth_method=HMAC-SHA256         │
│      - Marks task 1 complete                                   │
│                                                                 │
│    Worker-2:                                                    │
│      - Executes task 2 (research endpoints)                    │
│      - Writes to SharedMemory: api_base_url=...                │
│      - Marks task 2 complete                                   │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ↓
┌─────────────────────────────────────────────────────────────────┐
│ 5. NEXT ROUND                                                   │
│    Coordinator:                                                 │
│      - Sees tasks 1 & 2 complete                               │
│      - Task 3 now ready (depends on task 1)                    │
│      - Assigns task 3 to worker-1                              │
│      - Tasks 4 & 5 now ready (depend on task 2)                │
│      - Assigns task 4 to worker-2, task 5 to worker-3          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ↓
                          (repeat)
                             │
                             ↓
┌─────────────────────────────────────────────────────────────────┐
│ 6. COMPLETION                                                   │
│    - All 8 tasks marked complete                               │
│    - Coordinator emits final report                            │
│    - Hatchery saves SharedMemory                               │
│    - All sessions exit gracefully                              │
│    - Returns SwarmResult to Lead                               │
└─────────────────────────────────────────────────────────────────┘
```

---

### Message Flow Example

```
SCENARIO: Worker-1 discovers rate limit info and shares with worker-2

┌──────────┐
│ Worker-1 │ Executes task: "Research Bybit API"
└────┬─────┘
     │ (discovers rate limit in response headers)
     │
     ↓
  Outputs: @hatchery:write-knowledge rate_limit=100/min
     │
     ↓
┌─────────────┐
│  Hatchery   │ Parses command, updates SharedMemory.knowledge
└─────┬───────┘
      │
      ↓
  Outputs: @hatchery:send-message worker-2 "Found rate limit 100/min"
      │
      ↓
┌─────────────┐
│  Hatchery   │ Adds message to SharedMemory.messages queue
└─────┬───────┘
      │
      ↓ (next poll cycle)
┌─────────────┐
│  Hatchery   │ Delivers message to worker-2's next prompt
└─────┬───────┘
      │
      ↓
┌──────────┐
│ Worker-2 │ Receives: "Message from worker-1: Found rate limit 100/min"
└────┬─────┘
     │ (adjusts test plan to respect 100 req/min)
     │
     ↓
  Continues execution...
```

---

## Conclusion

Hatchery represents a paradigm shift in AI-assisted development:

- **Traditional**: Single AI agent executes tasks sequentially
- **Hatchery**: Swarm of AI agents collaborate in parallel

**Key advantages**:
1. **Parallelism**: N workers execute N tasks simultaneously
2. **Specialization**: Workers can develop expertise in sub-areas
3. **Resilience**: Worker failures don't halt entire swarm
4. **Scalability**: Add more workers for larger projects
5. **Collaboration**: Shared knowledge prevents duplicate work

**Use cases**:
- Multi-exchange connector creation (3 exchanges × 3 workers each)
- Large-scale refactoring (partition codebase, assign regions to workers)
- Batch research (10 topics × 10 workers)
- Comprehensive testing (unit, integration, e2e in parallel)

**Future directions**:
- **Dynamic scaling**: Auto-spawn workers based on task queue depth
- **Worker specialization**: Tag workers with skills (rust-expert, api-research)
- **Cross-project memory**: Workers remember patterns across swarms
- **Multi-model swarms**: Mix Opus (complex tasks) + Sonnet (simple tasks)

Hatchery turns Claude from a solo developer into a development team.

---

**End of Architecture Document**
