# Hatchery V2: Swarm Orchestration Architecture

## Overview

Hatchery is a Rust-based swarm orchestration system that manages hierarchical teams of AI coding agents. It wraps existing agent infrastructure (Claude Code CLI, OpenAI Codex, arbitrary LLM APIs) and provides coordination, communication, task distribution, memory management, and git isolation.

### Key Principle

Hatchery controls the **OUTER contour** (Main → Senior → Junior assignment). The **INNER contour** (Junior ↔ their sub-workers) can be native to the wrapped agent system (Claude Code Teams mailbox) or custom-built by Hatchery.

### Three Levels

```
Level 3: BroodLord (strategic decomposition, operator channel)
  ↕ SwarmMailbox
Level 2: SwarmHost (tactical coordination, validation, git isolation)
  ↕ SwarmMailbox
Level 1: Queen (basic unit: AI sergeant + soldiers)
  ↕ Native transport OR custom transport
Workers (Claude Code / Codex / API-based)
```

---

## Level 1: Queen (Basic Cell)

### Concept

Queen is the smallest autonomous unit: an AI manager ("sergeant") that controls N workers ("soldiers"). This is **NOT** a dumb PRD iterator — it's an intelligent coordinator that:

- Receives a task from SwarmHost
- Decomposes it into sub-tasks for workers
- Monitors worker progress
- Handles failures and retries
- Reports results back up

### Queen Trait

The core abstraction that all Queen implementations must satisfy:

```rust
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;
use std::collections::VecDeque;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait Queen: Send + Sync {
    /// Unique identifier
    fn id(&self) -> QueenId;

    /// Backend type for logging/routing
    fn backend(&self) -> QueenBackend;

    /// Assign a task from SwarmHost
    async fn assign(&mut self, task: Task, context: TaskContext) -> Result<()>;

    /// Current status
    async fn status(&self) -> QueenStatus;

    /// Completed result (None if still working)
    async fn result(&self) -> Option<TaskResult>;

    /// Receive a message from SwarmHost or another Queen
    async fn send_message(&mut self, msg: SwarmMessage) -> Result<()>;

    /// Drain outgoing messages (status reports, escalations, knowledge)
    async fn drain_outbox(&mut self) -> Vec<SwarmMessage>;

    /// Check if Queen is still alive/responsive
    async fn is_alive(&self) -> bool;

    /// Graceful shutdown
    async fn shutdown(&mut self) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QueenId(pub String); // e.g., "Q0", "Q1", "L2.0.Q0"

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueenBackend {
    /// Wrapped Claude Code CLI with native Teams/mailbox
    ClaudeNative,
    /// Wrapped Claude Code CLI without Teams (raw pipe)
    ClaudeRaw,
    /// OpenAI Codex sandbox
    Codex,
    /// Generic HTTP API (any LLM provider)
    ApiGeneric { base_url: String, model: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueenStatus {
    Idle,
    Working {
        task_id: TaskId,
        progress: f32,
        sub_tasks: Vec<SubTaskStatus>
    },
    Blocked {
        task_id: TaskId,
        reason: String
    },
    Failed {
        task_id: TaskId,
        error: String
    },
    Completed {
        task_id: TaskId
    },
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskStatus {
    pub id: String,
    pub description: String,
    pub worker_id: Option<WorkerId>,
    pub status: SubTaskState,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubTaskState {
    Pending,
    Assigned,
    InProgress,
    Completed,
    Failed { error: String },
}
```

### NativeQueen (Claude Code Wrapped)

Uses Claude Code CLI with native Teams infrastructure:

- Spawns via `PipeProcess`
- If Teams available: uses internal `TeammateTool`, mailbox at `~/.claude/teams/`
- Queen prompt instructs Claude to use native `Task` tool for sub-workers
- Hatchery reads Queen's stdout for status reports via `@hatchery:` protocol
- Does **NOT** interfere with internal Claude ↔ worker communication

```rust
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::process::{Child, Command, ChildStdin, ChildStdout};
use tokio::io::{BufReader, AsyncBufReadExt, AsyncWriteExt};

pub struct NativeQueen {
    id: QueenId,
    process: PipeProcess,
    parser: Box<dyn NdjsonParser + Send>,
    outbox: VecDeque<SwarmMessage>,
    current_task: Option<TaskId>,
    last_activity: Instant,
    working_dir: PathBuf,
    config: NativeQueenConfig,
    alive: Arc<AtomicBool>,  // Cached alive status updated during poll_messages
}

pub struct NativeQueenConfig {
    /// Model to use: "sonnet", "opus", "haiku"
    pub model: String,
    /// Use native Claude Teams infrastructure
    pub use_teams: bool,
    /// How many sub-agents Queen can spawn
    pub max_workers: usize,
    /// Task timeout
    pub timeout: Duration,
    /// Prompt template for the Queen
    pub prompt_template: String,
}

impl NativeQueenConfig {
    pub fn default_prompt(&self) -> String {
        format!(
            r#"You are a Queen in the Hatchery swarm system.

Your role:
1. Receive a task from SwarmHost
2. Break it into {max_workers} or fewer sub-tasks
3. Assign sub-tasks to workers using the Task tool
4. Monitor progress and handle failures
5. Report status to SwarmHost via @hatchery: protocol

Communication protocol:
- Receive tasks via stdin (JSON)
- Report status: @hatchery:status:<json>
- Report completion: @hatchery:complete:<json>
- Report failures: @hatchery:error:<json>
- Ask questions: @hatchery:escalate:<json>

You have access to {max_workers} workers. Use them wisely."#,
            max_workers = self.max_workers
        )
    }
}

// NOTE: PipeProcess is reused from v1 (queen/mod.rs).
// V2 additions needed for NativeQueen:
// - Track alive status in an AtomicBool to avoid &mut self in is_alive()
// - See v1 implementation for full struct definition
//
// V2-specific requirements:
// struct PipeProcess {
//     child: Arc<Mutex<Child>>,  // Changed to Arc<Mutex> for shared access
//     stdin: ChildStdin,
//     stdout_reader: BufReader<ChildStdout>,
//     alive: Arc<AtomicBool>,     // New: cached alive status
// }
//
// The alive flag is updated during polling and checked via &self

pub trait NdjsonParser: Send {
    fn parse(&self, line: &str) -> Result<ParsedMessage>;
}

#[derive(Debug)]
pub enum ParsedMessage {
    StatusReport(QueenStatus),
    Completion(TaskResult),
    Error(String),
    Escalation { issue: String, severity: Severity },
    Knowledge { key: String, value: serde_json::Value },
    Unknown(String),
}

impl NativeQueen {
    pub async fn spawn(
        id: QueenId,
        working_dir: PathBuf,
        config: NativeQueenConfig,
    ) -> Result<Self> {
        let mut args = vec!["--model", &config.model];
        if config.use_teams {
            args.push("--enable-teams");
        }

        let process = PipeProcess::spawn("claude", &args, &working_dir).await?;

        let parser = Box::new(HatcheryProtocolParser);

        Ok(Self {
            id,
            process,
            parser,
            outbox: VecDeque::new(),
            current_task: None,
            last_activity: Instant::now(),
            working_dir,
            config,
        })
    }

    async fn poll_messages(&mut self) -> Result<()> {
        // Update alive status from the process
        let is_alive = self.process.check_alive().await;
        self.alive.store(is_alive, Ordering::Relaxed);

        while let Some(line) = self.process.read_line().await? {
            if let Some(hatchery_msg) = line.strip_prefix("@hatchery:") {
                match self.parser.parse(hatchery_msg)? {
                    ParsedMessage::StatusReport(status) => {
                        let msg = SwarmMessage::status_report(
                            AgentId::Queen(self.id.clone()),
                            AgentId::SwarmHost(SwarmHostId::default()),
                            status,
                        );
                        self.outbox.push_back(msg);
                    }
                    ParsedMessage::Completion(result) => {
                        let msg = SwarmMessage::task_result(
                            AgentId::Queen(self.id.clone()),
                            AgentId::SwarmHost(SwarmHostId::default()),
                            self.current_task.clone().unwrap(),
                            result,
                        );
                        self.outbox.push_back(msg);
                    }
                    ParsedMessage::Error(error) => {
                        let msg = SwarmMessage::escalation(
                            AgentId::Queen(self.id.clone()),
                            AgentId::SwarmHost(SwarmHostId::default()),
                            error,
                            Severity::High,
                        );
                        self.outbox.push_back(msg);
                    }
                    ParsedMessage::Escalation { issue, severity } => {
                        let msg = SwarmMessage::escalation(
                            AgentId::Queen(self.id.clone()),
                            AgentId::SwarmHost(SwarmHostId::default()),
                            issue,
                            severity,
                        );
                        self.outbox.push_back(msg);
                    }
                    ParsedMessage::Knowledge { key, value } => {
                        let msg = SwarmMessage::knowledge(
                            AgentId::Queen(self.id.clone()),
                            AgentId::SwarmHost(SwarmHostId::default()),
                            key,
                            value,
                        );
                        self.outbox.push_back(msg);
                    }
                    ParsedMessage::Unknown(_) => {
                        // Log and ignore
                    }
                }
            }
            self.last_activity = Instant::now();
        }
        Ok(())
    }
}

#[async_trait]
impl Queen for NativeQueen {
    fn id(&self) -> QueenId {
        self.id.clone()
    }

    fn backend(&self) -> QueenBackend {
        QueenBackend::ClaudeNative
    }

    async fn assign(&mut self, task: Task, context: TaskContext) -> Result<()> {
        self.current_task = Some(task.id.clone());

        let assignment = serde_json::json!({
            "type": "task_assignment",
            "task": task,
            "context": context,
        });

        self.process
            .send_message(&serde_json::to_string(&assignment)?)
            .await?;

        Ok(())
    }

    async fn status(&self) -> QueenStatus {
        // Poll latest status from outbox or return cached
        if let Some(task_id) = &self.current_task {
            QueenStatus::Working {
                task_id: task_id.clone(),
                progress: 0.0,
                sub_tasks: vec![],
            }
        } else {
            QueenStatus::Idle
        }
    }

    async fn result(&self) -> Option<TaskResult> {
        None // Results are sent via outbox
    }

    async fn send_message(&mut self, msg: SwarmMessage) -> Result<()> {
        let json = serde_json::to_string(&msg)?;
        self.process.send_message(&json).await
    }

    async fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.outbox.drain(..).collect()
    }

    async fn is_alive(&self) -> bool {
        // Read cached alive status (updated during poll_messages)
        self.alive.load(Ordering::Relaxed)
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Send graceful shutdown signal
        let _ = self.process.send_message("@hatchery:shutdown").await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        Ok(())
    }
}
```

### CustomQueen (Full Control Stack)

For non-Claude backends or when native Teams not available:

- Manages workers via API calls or subprocess
- Has its own mailbox, task scheduler, compaction
- Full control over worker lifecycle

```rust
pub struct CustomQueen {
    id: QueenId,
    backend: QueenBackend,
    workers: Vec<CustomWorker>,
    mailbox: QueenMailbox,
    task_scheduler: TaskScheduler,
    memory: QueenMemory,
    outbox: VecDeque<SwarmMessage>,
    config: CustomQueenConfig,
}

pub struct CustomQueenConfig {
    pub backend: QueenBackend,
    pub max_workers: usize,
    pub timeout: Duration,
    pub coordinator_model: String,
}

pub struct CustomWorker {
    id: WorkerId,
    backend: WorkerBackend,
    status: WorkerStatus,
    current_task: Option<TaskId>,
    last_activity: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub String);

pub enum WorkerBackend {
    /// Subprocess (PipeProcess)
    Subprocess(PipeProcess),
    /// HTTP API (stateless calls)
    HttpApi {
        client: reqwest::Client,
        endpoint: String,
        model: String,
    },
    /// Codex sandbox
    CodexSandbox {
        session_id: String,
        client: reqwest::Client,
    },
}

#[derive(Debug, Clone)]
pub enum WorkerStatus {
    Idle,
    Busy { task_id: TaskId, progress: f32 },
    Failed { error: String },
    Dead,
}

pub struct QueenMailbox {
    inbox: VecDeque<SwarmMessage>,
    worker_inboxes: HashMap<WorkerId, VecDeque<SwarmMessage>>,
    event_log: Vec<SwarmMessage>,  // full audit trail
}

impl QueenMailbox {
    pub fn new() -> Self {
        Self {
            inbox: VecDeque::new(),
            worker_inboxes: HashMap::new(),
            event_log: Vec::new(),
        }
    }

    pub fn push_to_worker(&mut self, worker_id: &WorkerId, msg: SwarmMessage) {
        self.worker_inboxes
            .entry(worker_id.clone())
            .or_insert_with(VecDeque::new)
            .push_back(msg.clone());
        self.event_log.push(msg);
    }

    pub fn pop_from_worker(&mut self, worker_id: &WorkerId) -> Option<SwarmMessage> {
        self.worker_inboxes
            .get_mut(worker_id)
            .and_then(|q| q.pop_front())
    }

    pub fn push_to_queen(&mut self, msg: SwarmMessage) {
        self.inbox.push_back(msg.clone());
        self.event_log.push(msg);
    }

    pub fn pop_from_queen(&mut self) -> Option<SwarmMessage> {
        self.inbox.pop_front()
    }
}

pub struct TaskScheduler {
    pending: VecDeque<Task>,
    assigned: HashMap<WorkerId, Task>,
}

impl TaskScheduler {
    pub fn add_task(&mut self, task: Task) {
        self.pending.push_back(task);
    }

    pub fn assign_next(&mut self, worker_id: &WorkerId) -> Option<Task> {
        if let Some(task) = self.pending.pop_front() {
            self.assigned.insert(worker_id.clone(), task.clone());
            Some(task)
        } else {
            None
        }
    }

    pub fn complete_task(&mut self, worker_id: &WorkerId) -> Option<Task> {
        self.assigned.remove(worker_id)
    }

    pub fn all_complete(&self) -> bool {
        self.pending.is_empty() && self.assigned.is_empty()
    }
}

pub struct QueenMemory {
    knowledge: HashMap<String, serde_json::Value>,
    task_results: HashMap<TaskId, TaskResult>,
}

impl CustomQueen {
    pub async fn new(
        id: QueenId,
        config: CustomQueenConfig,
    ) -> Result<Self> {
        let workers = Vec::with_capacity(config.max_workers);

        Ok(Self {
            id,
            backend: config.backend.clone(),
            workers,
            mailbox: QueenMailbox::new(),
            task_scheduler: TaskScheduler {
                pending: VecDeque::new(),
                assigned: HashMap::new(),
            },
            memory: QueenMemory {
                knowledge: HashMap::new(),
                task_results: HashMap::new(),
            },
            outbox: VecDeque::new(),
            config,
        })
    }

    async fn spawn_worker(&mut self) -> Result<WorkerId> {
        let worker_id = WorkerId(format!("{}.W{}", self.id.0, self.workers.len()));

        let backend = match &self.backend {
            QueenBackend::ApiGeneric { base_url, model } => {
                WorkerBackend::HttpApi {
                    client: reqwest::Client::new(),
                    endpoint: base_url.clone(),
                    model: model.clone(),
                }
            }
            QueenBackend::Codex => {
                WorkerBackend::CodexSandbox {
                    session_id: uuid::Uuid::new_v4().to_string(),
                    client: reqwest::Client::new(),
                }
            }
            _ => return Err(anyhow!("Unsupported backend for CustomQueen")),
        };

        let worker = CustomWorker {
            id: worker_id.clone(),
            backend,
            status: WorkerStatus::Idle,
            current_task: None,
            last_activity: Instant::now(),
        };

        self.workers.push(worker);
        Ok(worker_id)
    }

    async fn assign_to_worker(&mut self, worker_id: &WorkerId, task: Task) -> Result<()> {
        let worker = self.workers
            .iter_mut()
            .find(|w| &w.id == worker_id)
            .ok_or_else(|| anyhow!("Worker not found"))?;

        match &mut worker.backend {
            WorkerBackend::HttpApi { client, endpoint, model } => {
                // Make API call to assign task
                let response = client
                    .post(endpoint)
                    .json(&serde_json::json!({
                        "model": model,
                        "task": task.description,
                    }))
                    .send()
                    .await?;

                // Handle response...
            }
            WorkerBackend::Subprocess(process) => {
                // Send task via stdin
                process.send_message(&serde_json::to_string(&task)?).await?;
            }
            WorkerBackend::CodexSandbox { session_id, client } => {
                // Codex-specific API call
            }
        }

        worker.status = WorkerStatus::Busy {
            task_id: task.id.clone(),
            progress: 0.0,
        };
        worker.current_task = Some(task.id);

        Ok(())
    }
}

#[async_trait]
impl Queen for CustomQueen {
    fn id(&self) -> QueenId {
        self.id.clone()
    }

    fn backend(&self) -> QueenBackend {
        self.backend.clone()
    }

    async fn assign(&mut self, task: Task, context: TaskContext) -> Result<()> {
        // 1. Decompose task into sub-tasks (could use LLM call here)
        let sub_tasks = self.decompose_task(&task, &context).await?;

        // 2. Add to scheduler
        for sub_task in sub_tasks {
            self.task_scheduler.add_task(sub_task);
        }

        // 3. Spawn workers if needed
        while self.workers.len() < self.config.max_workers
            && self.workers.len() < self.task_scheduler.pending.len() {
            self.spawn_worker().await?;
        }

        // 4. Assign tasks to idle workers (avoid borrow conflict by collecting IDs first)
        let idle_worker_ids: Vec<WorkerId> = self.workers
            .iter()
            .filter(|w| matches!(w.status, WorkerStatus::Idle))
            .map(|w| w.id.clone())
            .collect();

        for worker_id in idle_worker_ids {
            if let Some(sub_task) = self.task_scheduler.assign_next(&worker_id) {
                self.assign_to_worker(&worker_id, sub_task).await?;
            }
        }

        Ok(())
    }

    async fn status(&self) -> QueenStatus {
        if self.task_scheduler.all_complete() {
            QueenStatus::Idle
        } else {
            QueenStatus::Working {
                task_id: TaskId::default(),
                progress: 0.5,
                sub_tasks: vec![],
            }
        }
    }

    async fn result(&self) -> Option<TaskResult> {
        None
    }

    async fn send_message(&mut self, msg: SwarmMessage) -> Result<()> {
        self.mailbox.push_to_queen(msg);
        Ok(())
    }

    async fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.outbox.drain(..).collect()
    }

    async fn is_alive(&self) -> bool {
        true // Check worker health
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Shutdown all workers
        Ok(())
    }
}

impl CustomQueen {
    async fn decompose_task(&self, task: &Task, context: &TaskContext) -> Result<Vec<Task>> {
        // Use LLM to decompose task
        // For now, simple split
        Ok(vec![task.clone()])
    }
}
```

---

## Level 2: SwarmHost (Tactical Coordinator)

### Concept

SwarmHost receives a complex task, decomposes it, spawns Queens, assigns sub-tasks, validates results, manages git isolation, and reports up to BroodLord or directly to operator.

### Core Structure

```rust
use parking_lot::RwLock;
use std::sync::Arc;

pub struct SwarmHost {
    id: SwarmHostId,

    /// AI coordinator (Claude session that makes decisions)
    coordinator: CoordinatorSession,

    /// Queens managed by this SwarmHost
    queens: HashMap<QueenId, Box<dyn Queen>>,

    /// Validator (separate Queen or simple verify command)
    validator: Validator,

    /// Communication bus
    mailbox: SwarmMailbox,

    /// Shared knowledge store
    memory: SharedMemory,

    /// Git isolation
    worktree_manager: WorktreeManager,

    /// Task dependency graph
    task_dag: TaskDag,

    /// Context compression
    compaction: CompactionStrategy,

    /// Configuration
    config: SwarmHostConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SwarmHostId(pub String);

pub struct SwarmHostConfig {
    pub max_queens: usize,
    pub validator_mode: ValidatorMode,
    pub compaction_threshold: f32,
    pub git_isolation: bool,
    pub autosave_interval: Duration,
    pub coordinator_model: String,
}

#[derive(Debug, Clone)]
pub enum ValidatorMode {
    Command(String),
    Queen,
    Pipeline(Vec<String>),
}

pub struct CoordinatorSession {
    process: PipeProcess,
    parser: Box<dyn NdjsonParser + Send>,
    last_activity: Instant,
}
```

### SwarmMailbox (Communication Hub)

```rust
use tokio::sync::mpsc::{UnboundedSender, UnboundedReceiver};
use uuid::Uuid;

pub struct SwarmMailbox {
    /// Inbox for SwarmHost (messages from Queens, from BroodLord)
    host_inbox: VecDeque<SwarmMessage>,

    /// Per-queen inboxes (SwarmHost → specific Queen)
    queen_inboxes: HashMap<QueenId, VecDeque<SwarmMessage>>,

    /// Validator inbox
    validator_inbox: VecDeque<SwarmMessage>,

    /// Outbox (SwarmHost → BroodLord / Operator)
    outbox: VecDeque<SwarmMessage>,

    /// Event log (ALL messages, durable, for audit and replay)
    event_log: Arc<SqliteEventLog>,

    /// Subscribers for real-time notifications
    subscribers: Vec<UnboundedSender<SwarmMessage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmMessage {
    pub id: Uuid,
    pub from: AgentId,
    pub to: AgentId,
    pub msg_type: MessageType,
    pub payload: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub correlation_id: Option<Uuid>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    // Task lifecycle
    TaskAssignment { task: Task, context: TaskContext },
    TaskResult { task_id: TaskId, result: TaskResult },
    TaskProgress { task_id: TaskId, progress: f32, detail: String },

    // Status
    StatusRequest,
    StatusReport { status: QueenStatus },

    // Knowledge sharing
    Knowledge { key: String, value: serde_json::Value },
    KnowledgeQuery { pattern: String },

    // Control
    Escalation { issue: String, severity: Severity },
    Shutdown,

    // Custom
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Visibility {
    pub agent_visible: bool,
    pub coordinator_visible: bool,
    pub user_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentId {
    SwarmHost(SwarmHostId),
    Queen(QueenId),
    Validator,
    BroodLord,
    Operator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl SwarmMessage {
    pub fn task_assignment(from: AgentId, to: AgentId, task: Task, context: TaskContext) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            msg_type: MessageType::TaskAssignment { task, context },
            payload: serde_json::json!({}),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility {
                agent_visible: true,
                coordinator_visible: true,
                user_visible: false,
            },
        }
    }

    pub fn status_report(from: AgentId, to: AgentId, status: QueenStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            msg_type: MessageType::StatusReport { status },
            payload: serde_json::json!({}),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility {
                agent_visible: false,
                coordinator_visible: true,
                user_visible: false,
            },
        }
    }

    pub fn task_result(from: AgentId, to: AgentId, task_id: TaskId, result: TaskResult) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            msg_type: MessageType::TaskResult { task_id, result },
            payload: serde_json::json!({}),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility {
                agent_visible: true,
                coordinator_visible: true,
                user_visible: true,
            },
        }
    }

    pub fn escalation(from: AgentId, to: AgentId, issue: String, severity: Severity) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            msg_type: MessageType::Escalation { issue, severity },
            payload: serde_json::json!({}),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility {
                agent_visible: false,
                coordinator_visible: true,
                user_visible: true,
            },
        }
    }

    pub fn knowledge(from: AgentId, to: AgentId, key: String, value: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            msg_type: MessageType::Knowledge { key, value },
            payload: serde_json::json!({}),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility {
                agent_visible: true,
                coordinator_visible: true,
                user_visible: false,
            },
        }
    }
}

impl SwarmMailbox {
    pub fn new(event_log: Arc<SqliteEventLog>) -> Self {
        Self {
            host_inbox: VecDeque::new(),
            queen_inboxes: HashMap::new(),
            validator_inbox: VecDeque::new(),
            outbox: VecDeque::new(),
            event_log,
            subscribers: Vec::new(),
        }
    }

    pub fn push_to_host(&mut self, msg: SwarmMessage) {
        self.event_log.log(&msg);
        self.host_inbox.push_back(msg.clone());
        self.notify_subscribers(&msg);
    }

    pub fn push_to_queen(&mut self, queen_id: &QueenId, msg: SwarmMessage) {
        self.event_log.log(&msg);
        self.queen_inboxes
            .entry(queen_id.clone())
            .or_insert_with(VecDeque::new)
            .push_back(msg.clone());
        self.notify_subscribers(&msg);
    }

    pub fn push_to_outbox(&mut self, msg: SwarmMessage) {
        self.event_log.log(&msg);
        self.outbox.push_back(msg.clone());
        self.notify_subscribers(&msg);
    }

    pub fn pop_host_message(&mut self) -> Option<SwarmMessage> {
        self.host_inbox.pop_front()
    }

    pub fn pop_queen_message(&mut self, queen_id: &QueenId) -> Option<SwarmMessage> {
        self.queen_inboxes
            .get_mut(queen_id)
            .and_then(|q| q.pop_front())
    }

    pub fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.outbox.drain(..).collect()
    }

    pub fn subscribe(&mut self) -> UnboundedReceiver<SwarmMessage> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.subscribers.push(tx);
        rx
    }

    fn notify_subscribers(&mut self, msg: &SwarmMessage) {
        self.subscribers.retain(|tx| tx.send(msg.clone()).is_ok());
    }
}
```

### SqliteEventLog (Durable Storage)

```rust
use rusqlite::{Connection, params};

pub struct SqliteEventLog {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteEventLog {
    pub fn new(path: &PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                from_agent TEXT NOT NULL,
                to_agent TEXT NOT NULL,
                msg_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                correlation_id TEXT,
                visibility_agent BOOLEAN DEFAULT TRUE,
                visibility_coordinator BOOLEAN DEFAULT TRUE,
                visibility_user BOOLEAN DEFAULT TRUE
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id)",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn log(&self, msg: &SwarmMessage) {
        let conn = self.conn.lock();
        let _ = conn.execute(
            "INSERT INTO events (
                id, timestamp, from_agent, to_agent, msg_type, payload,
                correlation_id, visibility_agent, visibility_coordinator, visibility_user
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.id.to_string(),
                msg.timestamp.to_rfc3339(),
                serde_json::to_string(&msg.from).unwrap(),
                serde_json::to_string(&msg.to).unwrap(),
                serde_json::to_string(&msg.msg_type).unwrap(),
                serde_json::to_string(&msg.payload).unwrap(),
                msg.correlation_id.map(|id| id.to_string()),
                msg.visibility.agent_visible,
                msg.visibility.coordinator_visible,
                msg.visibility.user_visible,
            ],
        );
    }

    pub fn query_by_correlation(&self, correlation_id: &Uuid) -> Result<Vec<SwarmMessage>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id,
                    visibility_agent, visibility_coordinator, visibility_user
             FROM events WHERE correlation_id = ?1 ORDER BY timestamp"
        )?;

        let rows = stmt.query_map(params![correlation_id.to_string()], |row| {
            // Deserialize row into SwarmMessage
            Ok(SwarmMessage {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                    .unwrap()
                    .with_timezone(&Utc),
                from: serde_json::from_str(&row.get::<_, String>(2)?).unwrap(),
                to: serde_json::from_str(&row.get::<_, String>(3)?).unwrap(),
                msg_type: serde_json::from_str(&row.get::<_, String>(4)?).unwrap(),
                payload: serde_json::from_str(&row.get::<_, String>(5)?).unwrap(),
                correlation_id: row.get::<_, Option<String>>(6)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                visibility: Visibility {
                    agent_visible: row.get(7)?,
                    coordinator_visible: row.get(8)?,
                    user_visible: row.get(9)?,
                },
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn query_recent(&self, limit: usize) -> Result<Vec<SwarmMessage>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id,
                    visibility_agent, visibility_coordinator, visibility_user
             FROM events ORDER BY timestamp DESC LIMIT ?1"
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            Ok(SwarmMessage {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                    .unwrap()
                    .with_timezone(&Utc),
                from: serde_json::from_str(&row.get::<_, String>(2)?).unwrap(),
                to: serde_json::from_str(&row.get::<_, String>(3)?).unwrap(),
                msg_type: serde_json::from_str(&row.get::<_, String>(4)?).unwrap(),
                payload: serde_json::from_str(&row.get::<_, String>(5)?).unwrap(),
                correlation_id: row.get::<_, Option<String>>(6)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                visibility: Visibility {
                    agent_visible: row.get(7)?,
                    coordinator_visible: row.get(8)?,
                    user_visible: row.get(9)?,
                },
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
```

### TaskDag (Dependency-Aware Scheduler)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl Default for TaskId {
    fn default() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub description: String,
    pub context: serde_json::Value,
    pub priority: Priority,
    pub complexity: Complexity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContext {
    pub shared_memory_key: Option<String>,
    pub files: Vec<PathBuf>,
    pub dependencies: Vec<TaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: TaskId,
    pub success: bool,
    pub output: serde_json::Value,
    pub files_modified: Vec<PathBuf>,
    pub knowledge: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Complexity {
    Trivial,
    Simple,
    Medium,
    Complex,
    VeryComplex,
}

pub struct TaskDag {
    tasks: HashMap<TaskId, DagTask>,
}

#[derive(Debug, Clone)]
pub struct DagTask {
    pub id: TaskId,
    pub description: String,
    pub status: TaskStatus,
    pub assigned_to: Option<QueenId>,
    pub blocked_by: Vec<TaskId>,
    pub blocks: Vec<TaskId>,
    pub priority: Priority,
    pub estimated_complexity: Complexity,
    pub result: Option<TaskResult>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Blocked,
    Ready,
    Assigned(QueenId),
    InProgress,
    Validating,
    Completed,
    Failed { error: String, attempts: usize },
}

impl TaskDag {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    pub fn add_task(&mut self, task: DagTask) {
        // Update reverse dependencies
        for dep_id in &task.blocked_by {
            if let Some(dep_task) = self.tasks.get_mut(dep_id) {
                dep_task.blocks.push(task.id.clone());
            }
        }
        self.tasks.insert(task.id.clone(), task);
    }

    /// Get tasks ready for assignment (deps met, not assigned)
    pub fn ready_tasks(&self) -> Vec<&DagTask> {
        self.tasks
            .values()
            .filter(|t| {
                matches!(t.status, TaskStatus::Ready)
                    && t.blocked_by.iter().all(|dep_id| {
                        self.tasks
                            .get(dep_id)
                            .map(|dep| matches!(dep.status, TaskStatus::Completed))
                            .unwrap_or(false)
                    })
            })
            .collect()
    }

    /// Auto-unblock tasks whose dependencies completed
    pub fn refresh_readiness(&mut self) {
        let completed_ids: Vec<TaskId> = self
            .tasks
            .values()
            .filter(|t| matches!(t.status, TaskStatus::Completed))
            .map(|t| t.id.clone())
            .collect();

        for task in self.tasks.values_mut() {
            if matches!(task.status, TaskStatus::Blocked) {
                let all_deps_met = task.blocked_by.iter().all(|dep_id| {
                    completed_ids.contains(dep_id)
                });
                if all_deps_met {
                    task.status = TaskStatus::Ready;
                }
            }
        }
    }

    /// Critical path analysis (which tasks are bottlenecks)
    /// Uses iterative topological sort to find the longest path through the DAG
    pub fn critical_path(&self) -> Vec<TaskId> {
        use std::collections::VecDeque;

        // Calculate longest path using topological sort
        let mut in_degree: HashMap<TaskId, usize> = HashMap::new();
        let mut longest_path: HashMap<TaskId, usize> = HashMap::new();
        let mut predecessor: HashMap<TaskId, Option<TaskId>> = HashMap::new();

        // Initialize
        for (id, task) in &self.tasks {
            in_degree.insert(id.clone(), task.blocked_by.len());
            longest_path.insert(id.clone(), 0);
            predecessor.insert(id.clone(), None);
        }

        // Find tasks with no dependencies (start nodes)
        let mut queue: VecDeque<TaskId> = self.tasks.iter()
            .filter(|(_, task)| task.blocked_by.is_empty())
            .map(|(id, _)| id.clone())
            .collect();

        // Process topologically
        while let Some(task_id) = queue.pop_front() {
            let task = &self.tasks[&task_id];
            let current_length = longest_path[&task_id];

            // Update successors
            for successor_id in &task.blocks {
                let new_length = current_length + 1;
                if new_length > longest_path[successor_id] {
                    longest_path.insert(successor_id.clone(), new_length);
                    predecessor.insert(successor_id.clone(), Some(task_id.clone()));
                }

                // Decrease in-degree
                if let Some(degree) = in_degree.get_mut(successor_id) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(successor_id.clone());
                    }
                }
            }
        }

        // Backtrack from the task with the longest path
        let mut path = Vec::new();
        if let Some((&end_id, _)) = longest_path.iter().max_by_key(|(_, &len)| len) {
            let mut current = Some(end_id);
            while let Some(id) = current {
                path.push(id.clone());
                current = predecessor[&id].clone();
            }
            path.reverse();
        }

        path
    }

    pub fn mark_completed(&mut self, task_id: &TaskId, result: TaskResult) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::Completed;
            task.completed_at = Some(Utc::now());
            task.result = Some(result);
        }
        self.refresh_readiness();
    }

    pub fn mark_failed(&mut self, task_id: &TaskId, error: String) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            let attempts = if let TaskStatus::Failed { attempts, .. } = task.status {
                attempts + 1
            } else {
                1
            };
            task.status = TaskStatus::Failed { error, attempts };
        }
    }
}
```

### CompactionStrategy (Context Management)

```rust
pub struct CompactionStrategy {
    /// Trigger compaction at this % of context window
    threshold_pct: f32,  // 0.80 = 80%

    /// What to preserve (never compact)
    protected: Vec<CompactionScope>,

    /// Progressive removal levels
    levels: Vec<CompactionLevel>,
}

#[derive(Debug, Clone)]
pub enum CompactionScope {
    SystemPrompt,
    TaskAssignments,
    RecentMessages(usize),  // last N messages
    Knowledge,
    ActiveTaskContext,
}

#[derive(Debug, Clone)]
pub struct CompactionLevel {
    /// At what % to apply this level
    trigger_pct: f32,
    /// What to remove
    action: CompactionAction,
}

#[derive(Debug, Clone)]
pub enum CompactionAction {
    /// Remove tool responses older than N turns
    RemoveToolResponses { older_than_turns: usize },
    /// Summarize conversation up to N turns ago
    SummarizeOlderThan { turns: usize },
    /// Drop thinking blocks
    DropThinkingBlocks,
    /// Fresh start with summary
    FreshStart { carry_over: Vec<CompactionScope> },
}

impl CompactionStrategy {
    pub fn default_swarm_host() -> Self {
        Self {
            threshold_pct: 0.80,
            protected: vec![
                CompactionScope::SystemPrompt,
                CompactionScope::TaskAssignments,
                CompactionScope::Knowledge,
                CompactionScope::RecentMessages(5),
            ],
            levels: vec![
                CompactionLevel {
                    trigger_pct: 0.80,
                    action: CompactionAction::RemoveToolResponses {
                        older_than_turns: 20,
                    },
                },
                CompactionLevel {
                    trigger_pct: 0.85,
                    action: CompactionAction::RemoveToolResponses {
                        older_than_turns: 10,
                    },
                },
                CompactionLevel {
                    trigger_pct: 0.90,
                    action: CompactionAction::SummarizeOlderThan { turns: 5 },
                },
                CompactionLevel {
                    trigger_pct: 0.95,
                    action: CompactionAction::FreshStart {
                        carry_over: vec![
                            CompactionScope::SystemPrompt,
                            CompactionScope::TaskAssignments,
                            CompactionScope::Knowledge,
                            CompactionScope::RecentMessages(3),
                        ],
                    },
                },
            ],
        }
    }

    pub fn should_compact(&self, context_usage_pct: f32) -> bool {
        context_usage_pct >= self.threshold_pct
    }

    pub fn get_action(&self, context_usage_pct: f32) -> Option<&CompactionAction> {
        self.levels
            .iter()
            .filter(|level| context_usage_pct >= level.trigger_pct)
            .max_by(|a, b| a.trigger_pct.partial_cmp(&b.trigger_pct).unwrap())
            .map(|level| &level.action)
    }
}
```

### Validator

```rust
pub enum Validator {
    /// Simple command (e.g., "cargo check", "cargo test")
    Command { cmd: String, working_dir: PathBuf },

    /// Queen acting as validator (AI review)
    Queen(Box<dyn Queen>),

    /// Multi-stage: command first, then AI review if passes
    Pipeline(Vec<ValidationStage>),
}

pub struct ValidationStage {
    pub name: String,
    pub validator: Validator,
    pub required: bool,  // must pass to proceed
}

pub struct ValidationResult {
    pub passed: bool,
    pub stage_results: Vec<StageResult>,
    pub feedback: Option<String>,  // for Queen to fix issues
}

pub struct StageResult {
    pub stage_name: String,
    pub passed: bool,
    pub output: String,
    pub duration: Duration,
}

impl Validator {
    pub async fn validate(&self, files: &[PathBuf]) -> Result<ValidationResult> {
        match self {
            Validator::Command { cmd, working_dir } => {
                let start = Instant::now();
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .current_dir(working_dir)
                    .output()
                    .await?;

                let passed = output.status.success();
                let output_str = String::from_utf8_lossy(&output.stdout).to_string();

                Ok(ValidationResult {
                    passed,
                    stage_results: vec![StageResult {
                        stage_name: "command".to_string(),
                        passed,
                        output: output_str.clone(),
                        duration: start.elapsed(),
                    }],
                    feedback: if !passed { Some(output_str) } else { None },
                })
            }
            Validator::Queen(queen) => {
                // Assign validation task to Queen
                // Queen returns review comments
                todo!()
            }
            Validator::Pipeline(stages) => {
                let mut stage_results = Vec::new();
                let mut all_passed = true;

                for stage in stages {
                    let result = stage.validator.validate(files).await?;
                    stage_results.extend(result.stage_results);

                    if !result.passed && stage.required {
                        all_passed = false;
                        break;
                    }
                }

                Ok(ValidationResult {
                    passed: all_passed,
                    stage_results,
                    feedback: None,
                })
            }
        }
    }
}
```

### WorktreeManager (Git Isolation)

```rust
use std::process::Command;

pub struct WorktreeManager {
    base_dir: PathBuf,
    worktrees: HashMap<QueenId, WorktreeInfo>,
}

pub struct WorktreeInfo {
    pub queen_id: QueenId,
    pub path: PathBuf,
    pub branch: String,
    pub created_at: DateTime<Utc>,
}

impl WorktreeManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            worktrees: HashMap::new(),
        }
    }

    /// Create isolated worktree for a Queen
    pub fn create(&mut self, queen_id: &QueenId) -> Result<PathBuf> {
        let branch_name = format!("hatchery/{}", queen_id.0);
        let worktree_path = self.base_dir.join(format!(".hatchery/worktrees/{}", queen_id.0));

        // Create branch
        Command::new("git")
            .current_dir(&self.base_dir)
            .args(&["branch", &branch_name])
            .output()?;

        // Create worktree
        Command::new("git")
            .current_dir(&self.base_dir)
            .args(&["worktree", "add", worktree_path.to_str().unwrap(), &branch_name])
            .output()?;

        let info = WorktreeInfo {
            queen_id: queen_id.clone(),
            path: worktree_path.clone(),
            branch: branch_name,
            created_at: Utc::now(),
        };

        self.worktrees.insert(queen_id.clone(), info);

        Ok(worktree_path)
    }

    /// Merge Queen's worktree back to base branch
    pub fn merge(&mut self, queen_id: &QueenId) -> Result<MergeResult> {
        let info = self.worktrees.get(queen_id)
            .ok_or_else(|| anyhow!("Worktree not found"))?;

        // Switch to main
        Command::new("git")
            .current_dir(&self.base_dir)
            .args(&["checkout", "main"])
            .output()?;

        // Merge
        let output = Command::new("git")
            .current_dir(&self.base_dir)
            .args(&["merge", &info.branch, "--no-ff"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("CONFLICT") {
                // Extract conflicted files
                let files = self.get_conflicted_files()?;
                return Ok(MergeResult::Conflict { files });
            }
            return Err(anyhow!("Merge failed: {}", stderr));
        }

        // Get commit SHA
        let sha_output = Command::new("git")
            .current_dir(&self.base_dir)
            .args(&["rev-parse", "HEAD"])
            .output()?;

        let commit_sha = String::from_utf8_lossy(&sha_output.stdout)
            .trim()
            .to_string();

        Ok(MergeResult::Success { commit_sha })
    }

    /// Handle merge conflict (auto-resolve or escalate)
    pub fn resolve_conflict(&mut self, queen_id: &QueenId, strategy: ConflictStrategy) -> Result<()> {
        match strategy {
            ConflictStrategy::TakeTheirs => {
                Command::new("git")
                    .current_dir(&self.base_dir)
                    .args(&["checkout", "--theirs", "."])
                    .output()?;
                Command::new("git")
                    .current_dir(&self.base_dir)
                    .args(&["add", "."])
                    .output()?;
            }
            ConflictStrategy::TakeOurs => {
                Command::new("git")
                    .current_dir(&self.base_dir)
                    .args(&["checkout", "--ours", "."])
                    .output()?;
                Command::new("git")
                    .current_dir(&self.base_dir)
                    .args(&["add", "."])
                    .output()?;
            }
            ConflictStrategy::Escalate => {
                return Err(anyhow!("Conflict requires manual resolution"));
            }
        }
        Ok(())
    }

    /// Cleanup worktree
    pub fn cleanup(&mut self, queen_id: &QueenId) -> Result<()> {
        let info = self.worktrees.remove(queen_id)
            .ok_or_else(|| anyhow!("Worktree not found"))?;

        Command::new("git")
            .current_dir(&self.base_dir)
            .args(&["worktree", "remove", info.path.to_str().unwrap()])
            .output()?;

        Command::new("git")
            .current_dir(&self.base_dir)
            .args(&["branch", "-D", &info.branch])
            .output()?;

        Ok(())
    }

    fn get_conflicted_files(&self) -> Result<Vec<PathBuf>> {
        let output = Command::new("git")
            .current_dir(&self.base_dir)
            .args(&["diff", "--name-only", "--diff-filter=U"])
            .output()?;

        let files = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| PathBuf::from(line.trim()))
            .collect();

        Ok(files)
    }
}

#[derive(Debug)]
pub enum MergeResult {
    Success { commit_sha: String },
    Conflict { files: Vec<PathBuf> },
    NoChanges,
}

#[derive(Debug, Clone)]
pub enum ConflictStrategy {
    /// Take Queen's version
    TakeTheirs,
    /// Keep base version
    TakeOurs,
    /// Escalate to SwarmHost for AI resolution
    Escalate,
}
```

### SharedMemory (Knowledge Store)

```rust
pub struct SharedMemory {
    state: Arc<RwLock<MemoryState>>,
    persist_path: PathBuf,
}

pub struct MemoryState {
    pub version: u64,
    pub knowledge: HashMap<String, KnowledgeEntry>,
    pub task_results: HashMap<TaskId, TaskResult>,
    pub metadata: MemoryMetadata,
}

pub struct KnowledgeEntry {
    pub key: String,
    pub value: serde_json::Value,
    pub author: AgentId,
    pub timestamp: DateTime<Utc>,
    pub visibility: Visibility,
    pub ttl: Option<Duration>,
}

pub struct MemoryMetadata {
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub swarm_id: SwarmHostId,
}

impl SharedMemory {
    pub fn new(swarm_id: SwarmHostId, persist_path: PathBuf) -> Self {
        Self {
            state: Arc::new(RwLock::new(MemoryState {
                version: 0,
                knowledge: HashMap::new(),
                task_results: HashMap::new(),
                metadata: MemoryMetadata {
                    created_at: Utc::now(),
                    last_updated: Utc::now(),
                    swarm_id,
                },
            })),
            persist_path,
        }
    }

    pub fn insert(&self, key: String, value: serde_json::Value, author: AgentId) {
        let mut state = self.state.write();
        state.knowledge.insert(
            key.clone(),
            KnowledgeEntry {
                key,
                value,
                author,
                timestamp: Utc::now(),
                visibility: Visibility {
                    agent_visible: true,
                    coordinator_visible: true,
                    user_visible: false,
                },
                ttl: None,
            },
        );
        state.version += 1;
        state.metadata.last_updated = Utc::now();
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        let state = self.state.read();
        state.knowledge.get(key).map(|entry| entry.value.clone())
    }

    pub fn query(&self, pattern: &str) -> Vec<KnowledgeEntry> {
        let state = self.state.read();
        state
            .knowledge
            .values()
            .filter(|entry| entry.key.contains(pattern))
            .cloned()
            .collect()
    }

    pub fn save(&self) -> Result<()> {
        let state = self.state.read();
        let json = serde_json::to_string_pretty(&*state)?;
        std::fs::write(&self.persist_path, json)?;
        Ok(())
    }

    pub fn load(persist_path: PathBuf) -> Result<Self> {
        let json = std::fs::read_to_string(&persist_path)?;
        let state: MemoryState = serde_json::from_str(&json)?;
        Ok(Self {
            state: Arc::new(RwLock::new(state)),
            persist_path,
        })
    }
}
```

---

## Level 3: BroodLord (Strategic Orchestrator)

### Concept

BroodLord is the top-level orchestrator. It:

1. Receives a large goal from the Operator (human or parent agent)
2. Decomposes into sub-projects
3. Spawns SwarmHosts for each sub-project
4. Monitors progress across all SwarmHosts
5. Handles cross-SwarmHost dependencies
6. Reports to Operator via EventStream
7. Responds to Operator commands (reprioritize, cancel, add resources)

### Core Structure

```rust
pub struct BroodLord {
    /// AI strategist (Opus-level model)
    strategist: StrategistSession,

    /// SwarmHosts managed
    swarm_hosts: HashMap<SwarmHostId, SwarmHostHandle>,

    /// Global mailbox (BroodLord ↔ SwarmHosts)
    mailbox: SwarmMailbox,

    /// Global memory (cross-SwarmHost knowledge)
    global_memory: GlobalMemory,

    /// Operator channel (events up, commands down)
    operator_channel: Box<dyn OperatorChannel>,

    /// Global task DAG (cross-SwarmHost dependencies)
    global_dag: TaskDag,
}

pub struct StrategistSession {
    process: PipeProcess,
    parser: Box<dyn NdjsonParser + Send>,
    last_activity: Instant,
    model: String,
}

pub struct SwarmHostHandle {
    id: SwarmHostId,
    sender: UnboundedSender<SwarmMessage>,
    receiver: UnboundedReceiver<SwarmMessage>,
    status: SwarmHostStatus,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwarmHostStatus {
    Initializing,
    Running { tasks_done: usize, tasks_total: usize },
    Blocked { reason: String },
    Completed { result: SwarmResult },
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmResult {
    pub swarm_id: SwarmHostId,
    pub success: bool,
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub duration: Duration,
    pub output: serde_json::Value,
}
```

### OperatorChannel (Bidirectional Communication)

```rust
#[async_trait]
pub trait OperatorChannel: Send + Sync {
    /// Emit event to operator (progress, question, escalation)
    async fn emit(&self, event: OperatorEvent) -> Result<()>;

    /// Receive command from operator (non-blocking)
    async fn recv(&mut self) -> Option<OperatorCommand>;

    /// Check if operator is connected
    fn is_connected(&self) -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperatorEvent {
    // Progress
    Progress {
        swarm_id: SwarmHostId,
        done: usize,
        total: usize
    },
    GlobalProgress {
        done: usize,
        total: usize,
        elapsed: Duration
    },

    // Escalation
    Escalation {
        source: AgentId,
        issue: String,
        severity: Severity
    },

    // Questions (need operator input)
    Question {
        id: Uuid,
        text: String,
        options: Vec<String>
    },

    // Results
    SwarmCompleted {
        swarm_id: SwarmHostId,
        result: SwarmResult
    },
    AllComplete {
        results: Vec<SwarmResult>,
        total_duration: Duration
    },

    // Errors
    Error {
        source: AgentId,
        error: String
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperatorCommand {
    /// Change priority of a SwarmHost
    Reprioritize {
        swarm_id: SwarmHostId,
        priority: Priority
    },
    /// Cancel a SwarmHost
    Cancel {
        swarm_id: SwarmHostId
    },
    /// Answer a question
    Answer {
        question_id: Uuid,
        answer: String
    },
    /// Send message to specific agent
    Message {
        target: AgentId,
        text: String
    },
    /// Add workers to a SwarmHost
    Scale {
        swarm_id: SwarmHostId,
        queens: usize
    },
    /// Shutdown everything
    ShutdownAll,
}
```

### OperatorChannel Implementations

```rust
/// Stdout/stdin implementation
pub struct StdoutChannel {
    stdin_receiver: UnboundedReceiver<OperatorCommand>,
}

impl StdoutChannel {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Spawn stdin reader task
        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(cmd) = serde_json::from_str::<OperatorCommand>(&line) {
                    let _ = tx.send(cmd);
                }
            }
        });

        Self {
            stdin_receiver: rx,
        }
    }
}

#[async_trait]
impl OperatorChannel for StdoutChannel {
    async fn emit(&self, event: OperatorEvent) -> Result<()> {
        let json = serde_json::to_string(&event)?;
        println!("@operator:{}", json);
        Ok(())
    }

    async fn recv(&mut self) -> Option<OperatorCommand> {
        self.stdin_receiver.recv().await
    }

    fn is_connected(&self) -> bool {
        true
    }
}

/// SSE implementation for web UI
use tokio::sync::broadcast;

pub struct SseChannel {
    event_sender: broadcast::Sender<OperatorEvent>,
    command_receiver: UnboundedReceiver<OperatorCommand>,
    command_sender: UnboundedSender<OperatorCommand>,
}

impl SseChannel {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(100);
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        Self {
            event_sender: event_tx,
            command_receiver: cmd_rx,
            command_sender: cmd_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<OperatorEvent> {
        self.event_sender.subscribe()
    }

    pub fn command_sender(&self) -> UnboundedSender<OperatorCommand> {
        self.command_sender.clone()
    }
}

#[async_trait]
impl OperatorChannel for SseChannel {
    async fn emit(&self, event: OperatorEvent) -> Result<()> {
        let _ = self.event_sender.send(event);
        Ok(())
    }

    async fn recv(&mut self) -> Option<OperatorCommand> {
        self.command_receiver.recv().await
    }

    fn is_connected(&self) -> bool {
        self.event_sender.receiver_count() > 0
    }
}

/// Pipe implementation (for nested BroodLords)
pub struct PipeChannel {
    process: PipeProcess,
    connected: Arc<AtomicBool>,  // Cached connection status
}

#[async_trait]
impl OperatorChannel for PipeChannel {
    async fn emit(&self, event: OperatorEvent) -> Result<()> {
        let json = serde_json::to_string(&event)?;
        // Send via process stdin
        Ok(())
    }

    async fn recv(&mut self) -> Option<OperatorCommand> {
        // Read from process stdout, update connected flag
        let is_alive = self.process.check_alive().await;
        self.connected.store(is_alive, Ordering::Relaxed);
        None
    }

    fn is_connected(&self) -> bool {
        // Read cached connection status
        self.connected.load(Ordering::Relaxed)
    }
}
```

### GlobalMemory (Cross-SwarmHost Knowledge)

```rust
pub struct GlobalMemory {
    state: Arc<RwLock<GlobalState>>,
    persist_path: PathBuf,
}

pub struct GlobalState {
    pub version: u64,
    /// Namespaced: "swarm.0.auth.method", "global.api_version"
    pub knowledge: HashMap<String, KnowledgeEntry>,
    /// Per-SwarmHost status
    pub swarm_status: HashMap<SwarmHostId, SwarmHostStatus>,
    /// Cross-swarm messages
    pub messages: VecDeque<SwarmMessage>,
}

impl GlobalMemory {
    pub fn new(persist_path: PathBuf) -> Self {
        Self {
            state: Arc::new(RwLock::new(GlobalState {
                version: 0,
                knowledge: HashMap::new(),
                swarm_status: HashMap::new(),
                messages: VecDeque::new(),
            })),
            persist_path,
        }
    }

    pub fn insert(&self, namespace: &str, key: &str, value: serde_json::Value, author: AgentId) {
        let mut state = self.state.write();
        let full_key = format!("{}.{}", namespace, key);
        state.knowledge.insert(
            full_key.clone(),
            KnowledgeEntry {
                key: full_key,
                value,
                author,
                timestamp: Utc::now(),
                visibility: Visibility {
                    agent_visible: true,
                    coordinator_visible: true,
                    user_visible: false,
                },
                ttl: None,
            },
        );
        state.version += 1;
    }

    pub fn get(&self, namespace: &str, key: &str) -> Option<serde_json::Value> {
        let state = self.state.read();
        let full_key = format!("{}.{}", namespace, key);
        state.knowledge.get(&full_key).map(|e| e.value.clone())
    }

    pub fn update_swarm_status(&self, swarm_id: SwarmHostId, status: SwarmHostStatus) {
        let mut state = self.state.write();
        state.swarm_status.insert(swarm_id, status);
        state.version += 1;
    }
}
```

---

## Communication Architecture

### Message Flow

```
Operator
  ↕ OperatorChannel (stdout/SSE/pipe)
BroodLord
  ↕ SwarmMailbox (SwarmMessage over tokio channels + SQLite log)
SwarmHost
  ↕ SwarmMailbox (SwarmMessage over tokio channels + SQLite log)
Queen (native)
  ↕ Claude Code native transport (TeammateTool, ~/.claude/teams/ inbox)
  Workers

Queen (custom)
  ↕ QueenMailbox (SwarmMessage over our channels)
  Workers (API/subprocess)
```

### Transport Stack

| Level | Transport | Durable Storage |
|-------|-----------|-----------------|
| Operator ↔ BroodLord | stdout/SSE/pipe | SQLite event log |
| BroodLord ↔ SwarmHost | tokio::mpsc channels | SQLite event log |
| SwarmHost ↔ Queen | tokio::mpsc channels | SQLite event log |
| Queen (native) ↔ Workers | Claude Code native | Claude Code manages |
| Queen (custom) ↔ Workers | HTTP/stdin-stdout | QueenMailbox in-memory |

### Message Router

```rust
pub struct MessageRouter {
    /// Route table: AgentId → channel sender
    routes: HashMap<AgentId, UnboundedSender<SwarmMessage>>,

    /// Default route for unknown targets (escalate to parent)
    default_route: Option<AgentId>,

    /// Event log for ALL routed messages
    event_log: Arc<SqliteEventLog>,
}

impl MessageRouter {
    pub fn new(event_log: Arc<SqliteEventLog>) -> Self {
        Self {
            routes: HashMap::new(),
            default_route: None,
            event_log,
        }
    }

    pub async fn route(&self, msg: SwarmMessage) -> Result<()> {
        self.event_log.log(&msg);

        if let Some(sender) = self.routes.get(&msg.to) {
            sender.send(msg)?;
        } else if let Some(default_id) = &self.default_route {
            if let Some(sender) = self.routes.get(default_id) {
                sender.send(msg)?;
            }
        } else {
            return Err(anyhow!("No route for {:?}", msg.to));
        }

        Ok(())
    }

    pub fn register(&mut self, agent: AgentId, sender: UnboundedSender<SwarmMessage>) {
        self.routes.insert(agent, sender);
    }

    pub fn unregister(&mut self, agent: &AgentId) {
        self.routes.remove(agent);
    }

    pub fn set_default_route(&mut self, agent: AgentId) {
        self.default_route = Some(agent);
    }
}
```

---

## Git Coordination

### Strategy per Level

| Level | Git Strategy | Merge Frequency |
|-------|-------------|-----------------|
| BroodLord | One branch per SwarmHost | On SwarmHost completion |
| SwarmHost | One worktree per Queen | On task completion |
| Queen (native) | Claude Code manages internally | N/A |
| Queen (custom) | Single working dir or sub-worktrees | Per sub-task |

### Branch Naming Convention

```
main
├── hatchery/swarm-0/auth-module
│   ├── hatchery/swarm-0/queen-0
│   └── hatchery/swarm-0/queen-1
├── hatchery/swarm-1/api-endpoints
│   ├── hatchery/swarm-1/queen-0
│   └── hatchery/swarm-1/queen-1
└── hatchery/swarm-2/tests
```

### Merge Flow

```
Queen worktree → merge to SwarmHost branch → merge to main
                    ↑                            ↑
              Validator checks              BroodLord approves
```

### Example Workflow

1. **BroodLord** creates branch `hatchery/swarm-0/auth-module` for SwarmHost-0
2. **SwarmHost-0** creates worktrees:
   - `hatchery/swarm-0/queen-0` for Queen-0 (implements OAuth)
   - `hatchery/swarm-0/queen-1` for Queen-1 (implements API keys)
3. **Queen-0** completes task → Validator runs `cargo check` → merge to `hatchery/swarm-0/auth-module`
4. **Queen-1** completes task → Validator runs → merge to `hatchery/swarm-0/auth-module`
5. **SwarmHost-0** completes all tasks → BroodLord reviews → merge `hatchery/swarm-0/auth-module` to `main`

---

## Context Compression

### Per-Level Strategy

| Level | Context Owner | Compression Strategy |
|-------|--------------|----------------------|
| BroodLord | Long-lived strategist | Fresh-start every N hours with summary carry-over |
| SwarmHost | Medium-lived coordinator | 80% threshold, progressive tool response removal |
| Queen (native) | Claude Code manages | Claude's built-in /compact |
| Queen (custom) | Our code manages | Configurable per-backend |

### Progressive Compaction (SwarmHost)

```
80% context used → Remove tool responses > 20 turns old
85% context used → Remove tool responses > 10 turns old
90% context used → Summarize all conversation > 5 turns old
95% context used → Fresh start with: system prompt + task DAG + knowledge + last 3 messages
```

### Implementation Example

```rust
impl SwarmHost {
    async fn check_and_compact(&mut self) -> Result<()> {
        let context_usage = self.coordinator.context_usage_pct().await?;

        if self.compaction.should_compact(context_usage) {
            if let Some(action) = self.compaction.get_action(context_usage) {
                match action {
                    CompactionAction::RemoveToolResponses { older_than_turns } => {
                        self.coordinator.remove_tool_responses(*older_than_turns).await?;
                    }
                    CompactionAction::SummarizeOlderThan { turns } => {
                        let summary = self.coordinator.summarize_history(*turns).await?;
                        self.coordinator.fresh_start_with_summary(&summary).await?;
                    }
                    CompactionAction::DropThinkingBlocks => {
                        self.coordinator.drop_thinking_blocks().await?;
                    }
                    CompactionAction::FreshStart { carry_over } => {
                        let preserved = self.extract_preserved_context(carry_over).await?;
                        self.coordinator.fresh_start(&preserved).await?;
                    }
                }
            }
        }

        Ok(())
    }
}
```

---

## Configuration

### TOML Config

```toml
[hatchery]
default_mode = "swarm_host"
default_queen_backend = "claude_native"

[queen.native]
model = "sonnet"
use_teams = true
max_workers = 5
timeout_secs = 600

[queen.custom]
model = "gpt-4o"
api_base = "https://api.openai.com/v1"
max_workers = 3

[swarm_host]
max_queens = 5
validator = "cargo check"
compaction_threshold = 0.80
git_isolation = true
autosave_interval_secs = 30

[brood_lord]
strategist_model = "opus"
max_swarm_hosts = 5
operator_channel = "stdout"

[mailbox]
max_messages = 1000
event_log_path = ".hatchery/events.db"
message_ttl_secs = 3600

[git]
branch_prefix = "hatchery"
auto_merge = true
conflict_strategy = "escalate"
```

### Config Loading

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HatcheryConfig {
    pub hatchery: HatcherySettings,
    pub queen: QueenSettings,
    pub swarm_host: SwarmHostSettings,
    pub brood_lord: BroodLordSettings,
    pub mailbox: MailboxSettings,
    pub git: GitSettings,
}

#[derive(Debug, Deserialize)]
pub struct HatcherySettings {
    pub default_mode: String,
    pub default_queen_backend: String,
}

#[derive(Debug, Deserialize)]
pub struct QueenSettings {
    pub native: NativeQueenConfig,
    pub custom: CustomQueenConfig,
}

#[derive(Debug, Deserialize)]
pub struct SwarmHostSettings {
    pub max_queens: usize,
    pub validator: String,
    pub compaction_threshold: f32,
    pub git_isolation: bool,
    pub autosave_interval_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct BroodLordSettings {
    pub strategist_model: String,
    pub max_swarm_hosts: usize,
    pub operator_channel: String,
}

#[derive(Debug, Deserialize)]
pub struct MailboxSettings {
    pub max_messages: usize,
    pub event_log_path: String,
    pub message_ttl_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct GitSettings {
    pub branch_prefix: String,
    pub auto_merge: bool,
    pub conflict_strategy: String,
}

impl HatcheryConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: HatcheryConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn default() -> Self {
        toml::from_str(include_str!("../hatchery.toml")).unwrap()
    }
}
```

---

## Implementation Roadmap

### Phase 1: Queen Trait + Refactor (Week 1-2)
**Goal**: Establish core abstraction layer

- [x] Define `trait Queen` with full interface
- [ ] Implement `NativeQueen` (refactor current PipeProcess wrapper)
- [ ] Implement basic `CustomQueen` (HTTP API backend)
- [ ] Refactor current Queen mode to use new trait
- [ ] Unit tests for both implementations
- [ ] Documentation for Queen trait

**Deliverables**:
- `src/queen/mod.rs` - trait definition
- `src/queen/native.rs` - NativeQueen implementation
- `src/queen/custom.rs` - CustomQueen implementation
- `tests/queen_trait_tests.rs` - comprehensive tests

### Phase 2: SwarmMailbox + Message Router (Week 2-3)
**Goal**: Establish reliable communication layer

- [ ] Implement `SwarmMessage` types with full serialization
- [ ] Implement `SwarmMailbox` with per-agent inboxes
- [ ] Implement `SqliteEventLog` for durable storage
- [ ] Implement `MessageRouter` with tokio channels
- [ ] Integration tests: send/receive between SwarmHost and Queens
- [ ] Replay mechanism from event log
- [ ] Message filtering and querying

**Deliverables**:
- `src/mailbox/message.rs` - message types
- `src/mailbox/mailbox.rs` - SwarmMailbox
- `src/mailbox/event_log.rs` - SQLite persistence
- `src/mailbox/router.rs` - MessageRouter
- `tests/mailbox_integration_tests.rs`

### Phase 3: SwarmHost Refactor (Week 3-4)
**Goal**: Upgrade SwarmHost to use new abstractions

- [ ] Refactor SwarmHost to use `trait Queen` instead of raw PipeProcess
- [ ] Implement `TaskDag` with dependency tracking and auto-unblocking
- [ ] Implement `Validator` (command + AI modes)
- [ ] Implement `CompactionStrategy`
- [ ] Wire up SwarmMailbox as communication layer
- [ ] Autosave mechanism for SharedMemory
- [ ] Integration tests with multiple Queens

**Deliverables**:
- `src/swarm_host/mod.rs` - refactored SwarmHost
- `src/swarm_host/task_dag.rs` - TaskDag implementation
- `src/swarm_host/validator.rs` - Validator
- `src/swarm_host/compaction.rs` - CompactionStrategy
- `src/swarm_host/memory.rs` - SharedMemory
- `tests/swarm_host_integration_tests.rs`

### Phase 4: Git Coordination (Week 4-5)
**Goal**: Safe parallel development with git isolation

- [ ] Refactor `WorktreeManager` with per-Queen isolation
- [ ] Implement merge flow with Validator checks
- [ ] Branch naming convention enforcement
- [ ] Conflict detection and escalation
- [ ] Automatic cleanup of merged worktrees
- [ ] Integration tests with real git operations
- [ ] Safety mechanisms (prevent force-push, etc.)

**Deliverables**:
- `src/git/worktree_manager.rs` - WorktreeManager v2
- `src/git/merge_strategy.rs` - merge and conflict handling
- `tests/git_integration_tests.rs`

### Phase 5: BroodLord + OperatorChannel (Week 5-6)
**Goal**: Top-level orchestration with operator interface

- [ ] Implement `OperatorChannel` trait + stdout implementation
- [ ] Implement BroodLord with continuous monitoring (not one-shot decomposition)
- [ ] Global TaskDag for cross-SwarmHost dependencies
- [ ] SSE OperatorChannel for web UI (optional)
- [ ] Graceful shutdown and cleanup
- [ ] Recovery from crashes (load from event log)
- [ ] End-to-end integration tests

**Deliverables**:
- `src/brood_lord/mod.rs` - BroodLord implementation
- `src/brood_lord/operator_channel.rs` - OperatorChannel trait + impls
- `src/brood_lord/global_memory.rs` - GlobalMemory
- `tests/brood_lord_integration_tests.rs`
- `examples/brood_lord_stdout.rs` - CLI example

### Phase 6: CustomQueen Full Stack (Week 6-7)
**Goal**: Support non-Claude backends

- [ ] Full CustomQueen with own mailbox, task scheduler, compaction
- [ ] Codex backend implementation
- [ ] Generic HTTP API backend
- [ ] Worker lifecycle management
- [ ] Test: SwarmHost with mixed Queen backends
- [ ] Performance benchmarks
- [ ] Documentation and examples

**Deliverables**:
- `src/queen/custom/scheduler.rs` - TaskScheduler
- `src/queen/custom/backends/codex.rs` - Codex integration
- `src/queen/custom/backends/http_api.rs` - Generic API
- `tests/custom_queen_integration_tests.rs`
- `examples/mixed_backends.rs`

### Phase 7: CLI + Integration (Week 7)
**Goal**: Command-line interface and end-to-end integration

- [ ] CLI for launching BroodLord with configuration
- [ ] CLI commands: start, stop, status, logs, replay
- [ ] Integration tests covering full BroodLord → SwarmHost → Queen flow
- [ ] Performance benchmarks for large task DAGs
- [ ] Documentation: architecture guide, API reference, examples
- [ ] Migration guide from V1 to V2

**Deliverables**:
- `src/cli/mod.rs` - CLI implementation
- `tests/integration/e2e_tests.rs` - end-to-end tests
- `benches/task_dag_bench.rs` - performance benchmarks
- `docs/ARCHITECTURE.md` - architecture documentation
- `docs/MIGRATION.md` - V1 to V2 migration guide

### Phase 8: Web UI (Optional)
**Goal**: Visual monitoring and control

- [ ] SSE endpoint for event stream
- [ ] React-based dashboard
- [ ] Real-time task DAG visualization
- [ ] Manual intervention UI (answer questions, resolve conflicts)
- [ ] Historical replay from event log
- [ ] Agent chat interface

**Deliverables**:
- `hatchery-web/` - standalone web UI crate
- WebSocket/SSE server
- Dashboard UI

---

## Dependencies

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
async-trait = "0.1"
parking_lot = "0.12"
reqwest = { version = "0.12", features = ["json"] }
clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
toml = "0.8"

# From workspace (if applicable)
# zengeld-hub-core = { path = "../zengeld-hub/crates/core" }

[dev-dependencies]
tempfile = "3"
```

---

## Appendix: Example Usage

### Example 1: SwarmHost with NativeQueens

```rust
use hatchery::*;

#[tokio::main]
async fn main() -> Result<()> {
    let config = HatcheryConfig::load(Path::new("hatchery.toml"))?;

    let event_log = Arc::new(SqliteEventLog::new(
        &PathBuf::from(".hatchery/events.db")
    )?);

    let mut swarm_host = SwarmHost::new(
        SwarmHostId("swarm-0".to_string()),
        config.swarm_host,
        event_log.clone(),
    ).await?;

    // Spawn Queens
    for i in 0..3 {
        let queen = NativeQueen::spawn(
            QueenId(format!("Q{}", i)),
            PathBuf::from("."),
            config.queen.native.clone(),
        ).await?;
        swarm_host.add_queen(Box::new(queen)).await?;
    }

    // Assign task
    let task = Task {
        id: TaskId::default(),
        description: "Implement user authentication".to_string(),
        context: serde_json::json!({}),
        priority: Priority::High,
        complexity: Complexity::Complex,
    };

    swarm_host.assign_task(task, TaskContext::default()).await?;

    // Monitor
    while !swarm_host.is_complete().await {
        swarm_host.tick().await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let results = swarm_host.get_results().await?;
    println!("Results: {:?}", results);

    Ok(())
}
```

### Example 2: BroodLord with Operator Commands

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let config = HatcheryConfig::load(Path::new("hatchery.toml"))?;

    let operator_channel = Box::new(StdoutChannel::new());

    let mut brood_lord = BroodLord::new(
        config.brood_lord,
        operator_channel,
    ).await?;

    brood_lord.assign_goal(
        "Build a complete exchange connector for Binance"
    ).await?;

    // Main loop
    loop {
        brood_lord.tick().await?;

        // Check for operator commands
        if let Some(cmd) = brood_lord.recv_operator_command().await {
            match cmd {
                OperatorCommand::ShutdownAll => {
                    brood_lord.shutdown().await?;
                    break;
                }
                _ => brood_lord.handle_command(cmd).await?,
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}
```

---

## Summary

Hatchery V2 provides a robust 3-level architecture for orchestrating AI coding agents:

- **Level 1 (Queen)**: Basic unit with pluggable backends (Claude native, custom API, Codex)
- **Level 2 (SwarmHost)**: Tactical coordinator with git isolation, validation, and task DAG
- **Level 3 (BroodLord)**: Strategic orchestrator with operator communication and cross-swarm coordination

Key features:
- Durable event log (SQLite) for audit and replay
- Git worktree isolation per Queen
- Progressive context compression
- Flexible validation pipeline
- Shared memory for knowledge sharing
- Operator commands for runtime control

This architecture enables scaling from a single Queen to hundreds of agents working in parallel on complex codebases.
