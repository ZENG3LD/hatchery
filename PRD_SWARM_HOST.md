# Hatchery PRD: Swarm Host Mode

**Version:** 1.0
**Status:** Draft
**Priority:** P1

## Overview

Swarm Host mode introduces intelligent coordination to the Hatchery swarm. Unlike Queen's independent workers, Swarm Host uses a dedicated AI Coordinator (Sonnet) that manages a pool of Worker agents (Sonnet), maintaining a SharedMemory system for knowledge exchange and task dependencies.

The mode is named after StarCraft's Swarm Host: a unit that burrows and spawns waves of Locusts (smart workers). The Coordinator builds a task dependency graph (DAG), dynamically assigns tasks to workers based on readiness, detects stalls and reassigns work, and facilitates horizontal communication between workers through a message queue.

This architecture enables complex multi-task projects where tasks have dependencies (e.g., "implement API client" must complete before "write integration tests"), and where workers can share learnings (e.g., "I found that endpoint X requires auth header Y").

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                       Hatchery Swarm Host                           │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                    Main Orchestrator                          │ │
│  │  - Spawns Coordinator PipeProcess                             │ │
│  │  - Spawns N Worker PipeProcesses                              │ │
│  │  - Maintains SharedMemory (in-mem + file-backed)              │ │
│  │  - Routes events between Coordinator and Workers              │ │
│  │  - Displays unified dashboard                                 │ │
│  └────────────────────────────┬──────────────────────────────────┘ │
│                               │                                    │
│         ┌─────────────────────┼─────────────────────┐              │
│         ▼                     ▼                     ▼              │
│  ┌──────────────┐      ┌─────────────┐      ┌─────────────┐       │
│  │ Coordinator  │      │  Worker 1   │      │  Worker N   │       │
│  │  (Sonnet)    │      │  (Sonnet)   │      │  (Sonnet)   │       │
│  │              │      │             │      │             │       │
│  │ - Reads PRD  │      │ - Executes  │      │ - Executes  │       │
│  │ - Builds DAG │      │   tasks     │      │   tasks     │       │
│  │ - Assigns    │◄────►│ - Writes to │◄────►│ - Writes to │       │
│  │   tasks      │      │   SharedMem │      │   SharedMem │       │
│  │ - Monitors   │      │ - Reads     │      │ - Reads     │       │
│  │   progress   │      │   knowledge │      │   knowledge │       │
│  │ - Reassigns  │      │ - Sends     │      │ - Sends     │       │
│  │   stalled    │      │   messages  │      │   messages  │       │
│  └──────────────┘      └─────────────┘      └─────────────┘       │
│         │                     │                     │              │
│         └─────────────────────┴─────────────────────┘              │
│                               │                                    │
│                               ▼                                    │
│              ┌────────────────────────────────┐                    │
│              │       SharedMemory             │                    │
│              │  ┌──────────────────────────┐  │                    │
│              │  │ Knowledge HashMap        │  │                    │
│              │  │ - "api_auth": "Bearer X" │  │                    │
│              │  │ - "test_setup": "..."    │  │                    │
│              │  └──────────────────────────┘  │                    │
│              │  ┌──────────────────────────┐  │                    │
│              │  │ Task List (DAG)          │  │                    │
│              │  │ - T1: ready → assigned   │  │                    │
│              │  │ - T2: blocked (deps T1)  │  │                    │
│              │  └──────────────────────────┘  │                    │
│              │  ┌──────────────────────────┐  │                    │
│              │  │ Message Queue            │  │                    │
│              │  │ - W1→W2: "endpoint docs" │  │                    │
│              │  │ - W2→Coord: "task done"  │  │                    │
│              │  └──────────────────────────┘  │                    │
│              │  ┌──────────────────────────┐  │                    │
│              │  │ Results Log              │  │                    │
│              │  │ - T1: success, commit SHA│  │                    │
│              │  │ - T3: failed, error msg  │  │                    │
│              │  └──────────────────────────┘  │                    │
│              └────────────────────────────────┘                    │
│                           │                                        │
│                           ▼                                        │
│              File: shared_memory.json (persisted)                  │
└─────────────────────────────────────────────────────────────────────┘

Communication Protocol:
  Coordinator → Workers: Task assignments via SharedMemory.task_list updates
  Workers → Coordinator: Status updates via @hatchery:result commands in output
  Workers ↔ Workers: Messages via @hatchery:message commands
  All → SharedMemory: Knowledge via @hatchery:knowledge commands
```

## User Stories

### US-1: SharedMemory Data Structure
**As a** Swarm Host orchestrator, **I want** a SharedMemory structure that all agents can access, **so that** knowledge is shared across the swarm.

#### Acceptance Criteria
- [ ] AC-1.1: SharedMemory stored in-memory as `Arc<RwLock<SharedMemoryState>>`
- [ ] AC-1.2: SharedMemory persisted to `shared_memory.json` after each write
- [ ] AC-1.3: SharedMemory contains `knowledge: HashMap<String, serde_json::Value>`
- [ ] AC-1.4: SharedMemory contains `tasks: Vec<Task>` with status and dependencies
- [ ] AC-1.5: SharedMemory contains `messages: VecDeque<Message>` (FIFO queue)
- [ ] AC-1.6: SharedMemory contains `results: HashMap<TaskId, TaskResult>`
- [ ] AC-1.7: Read operations use `RwLock::read()` for concurrent access
- [ ] AC-1.8: Write operations use `RwLock::write()` for exclusive access
- [ ] AC-1.9: File persistence is atomic (write to temp file, rename on success)
- [ ] AC-1.10: File load on startup validates schema version (error if incompatible)

### US-2: Knowledge HashMap CRUD
**As a** worker, **I want** to write knowledge to SharedMemory, **so that** other workers can benefit from my discoveries.

#### Acceptance Criteria
- [ ] AC-2.1: Workers write knowledge via `@hatchery:knowledge key=value` in output
- [ ] AC-2.2: Orchestrator parses `@hatchery:knowledge` commands from NDJSON stream
- [ ] AC-2.3: Orchestrator updates `SharedMemory.knowledge[key] = value`
- [ ] AC-2.4: Knowledge values support JSON types: string, number, object, array
- [ ] AC-2.5: Knowledge keys are namespaced by category (e.g., `api.auth.header`, `test.setup.steps`)
- [ ] AC-2.6: Workers read knowledge via prompt injection: coordinator includes relevant knowledge in task prompt
- [ ] AC-2.7: Knowledge query supports glob patterns: `api.*` returns all API-related knowledge
- [ ] AC-2.8: Knowledge updates are logged to orchestrator output: `[W1] Knowledge: api.auth.header = Bearer X`
- [ ] AC-2.9: Knowledge has TTL (time-to-live): entries expire after 1 hour (configurable)
- [ ] AC-2.10: Expired knowledge is pruned on next SharedMemory persist

### US-3: Task List with Dependencies (DAG)
**As a** Coordinator, **I want** to build a task dependency graph from the PRD, **so that** tasks execute in correct order.

#### Acceptance Criteria
- [ ] AC-3.1: Coordinator parses PRD and extracts tasks with dependencies
- [ ] AC-3.2: Dependencies inferred from indentation (nested = depends on parent)
- [ ] AC-3.3: Explicit dependencies supported via syntax: `- [ ] Task B @depends(Task A)`
- [ ] AC-3.4: DAG builder validates no cycles (error if circular dependencies detected)
- [ ] AC-3.5: Task status: `Blocked`, `Ready`, `Assigned`, `InProgress`, `Completed`, `Failed`
- [ ] AC-3.6: Task transitions: `Blocked` → `Ready` when all dependencies complete
- [ ] AC-3.7: Coordinator finds ready tasks via `get_ready_tasks() -> Vec<TaskId>`
- [ ] AC-3.8: Task assignment updates status: `Ready` → `Assigned` → `InProgress`
- [ ] AC-3.9: Failed tasks block dependent tasks (cascade failure)
- [ ] AC-3.10: DAG visualization logged to coordinator output (Graphviz DOT format)

### US-4: Coordinator Session Spawning
**As a** Hatchery orchestrator, **I want** to spawn a Coordinator PipeProcess with specialized prompt, **so that** it manages the swarm.

#### Acceptance Criteria
- [ ] AC-4.1: Coordinator spawned with `PipeProcess::new(CliTool::ClaudeCode, working_dir, coordinator_prompt)`
- [ ] AC-4.2: Coordinator prompt loaded from `prompts/swarm_host_coordinator.md`
- [ ] AC-4.3: Coordinator prompt includes: PRD content, DAG structure, worker count, SharedMemory schema
- [ ] AC-4.4: Coordinator prompt instructs on JSON command format: `{"cmd": "assign_task", "task_id": 1, "worker_id": 0}`
- [ ] AC-4.5: Coordinator outputs JSON commands in NDJSON format (one command per line)
- [ ] AC-4.6: Coordinator model configurable via `--coordinator-model` flag (default: Sonnet)
- [ ] AC-4.7: Coordinator session uses `--output-format stream-json --verbose`
- [ ] AC-4.8: Coordinator has higher context limit (200K tokens for Claude)
- [ ] AC-4.9: Coordinator receives periodic status updates from orchestrator (every 10s)
- [ ] AC-4.10: Coordinator can request worker status via `{"cmd": "get_worker_status", "worker_id": 0}`

### US-5: Worker Session Spawning
**As a** Hatchery orchestrator, **I want** to spawn N Worker PipeProcesses with specialized prompts, **so that** they execute tasks.

#### Acceptance Criteria
- [ ] AC-5.1: Workers spawned with `PipeProcess::new(CliTool::ClaudeCode, working_dir, worker_prompt)`
- [ ] AC-5.2: Worker prompt loaded from `prompts/swarm_host_worker.md`
- [ ] AC-5.3: Worker prompt includes: `@hatchery:` command reference, SharedMemory access pattern, task format
- [ ] AC-5.4: Worker prompt instructs to read knowledge before starting task: check `SharedMemory.knowledge` for relevant keys
- [ ] AC-5.5: Worker prompt instructs to write results after task: `@hatchery:result task_id=X status=success`
- [ ] AC-5.6: Worker ID included in prompt: `You are Worker {id}. Your role is to execute assigned tasks.`
- [ ] AC-5.7: Workers spawned sequentially with 1-second delay (avoid API rate limit spike)
- [ ] AC-5.8: Worker spawn failures logged and retried up to 3 times
- [ ] AC-5.9: Workers use same model as Coordinator (Sonnet by default)
- [ ] AC-5.10: Worker count configurable via `--workers N` flag (default: 4, max: 16)

### US-6: @hatchery Command Parsing
**As a** orchestrator, **I want** to parse `@hatchery:` commands from worker output, **so that** I can update SharedMemory.

#### Acceptance Criteria
- [ ] AC-6.1: Detect `@hatchery:knowledge key=value` and call `SharedMemory::set_knowledge(key, value)`
- [ ] AC-6.2: Detect `@hatchery:result task_id=X status=Y` and call `SharedMemory::set_result(task_id, status)`
- [ ] AC-6.3: Detect `@hatchery:message to=W2 text="..."` and call `SharedMemory::send_message(from, to, text)`
- [ ] AC-6.4: Detect `@hatchery:query key=pattern` and inject matching knowledge into next worker prompt
- [ ] AC-6.5: Commands parsed from `CliEvent::AssistantText` (scan for `@hatchery:` prefix)
- [ ] AC-6.6: Command parsing is case-insensitive (`@HATCHERY:` works)
- [ ] AC-6.7: Invalid commands logged as warnings but do not crash worker
- [ ] AC-6.8: Command execution is synchronous (update SharedMemory before processing next event)
- [ ] AC-6.9: Commands support JSON values: `@hatchery:knowledge api.config='{"timeout": 30}'`
- [ ] AC-6.10: Command parsing handles multi-line values: `@hatchery:knowledge setup="""line1\nline2"""`

### US-7: Task Assignment Algorithm
**As a** Coordinator, **I want** to assign ready tasks to idle workers, **so that** work is distributed efficiently.

#### Acceptance Criteria
- [ ] AC-7.1: Coordinator polls `SharedMemory.get_ready_tasks()` every 5 seconds
- [ ] AC-7.2: Coordinator finds idle workers (status = Idle, no assigned task)
- [ ] AC-7.3: Coordinator assigns task via JSON command: `{"cmd": "assign_task", "task_id": X, "worker_id": Y}`
- [ ] AC-7.4: Orchestrator parses command and updates `SharedMemory.tasks[X].status = Assigned`
- [ ] AC-7.5: Orchestrator sends task prompt to worker via `worker.process.write(prompt)`
- [ ] AC-7.6: Task prompt includes: task text, PRD context, verification command, relevant knowledge
- [ ] AC-7.7: Assignment algorithm prioritizes tasks with most dependencies satisfied
- [ ] AC-7.8: Assignment algorithm balances load (assign to least-busy worker)
- [ ] AC-7.9: Assignment updates logged: `[Coordinator] Assigned task 3 to Worker 1`
- [ ] AC-7.10: Failed assignments (no idle workers) are retried on next poll cycle

### US-8: Horizontal Messaging Between Workers
**As a** worker, **I want** to send messages to other workers, **so that** I can share discoveries or ask questions.

#### Acceptance Criteria
- [ ] AC-8.1: Worker sends message via `@hatchery:message to=W2 text="Found auth pattern in code"`
- [ ] AC-8.2: Orchestrator parses message command and appends to `SharedMemory.messages`
- [ ] AC-8.3: Message structure: `{ from: WorkerId, to: WorkerId, text: String, timestamp: DateTime }`
- [ ] AC-8.4: Target worker receives message in next task prompt: `Message from W1: "..."`
- [ ] AC-8.5: Broadcast messages supported: `@hatchery:message to=all text="..."`
- [ ] AC-8.6: Message queue is FIFO, max 100 messages (oldest pruned when limit reached)
- [ ] AC-8.7: Messages expire after 10 minutes (not delivered if recipient hasn't polled)
- [ ] AC-8.8: Coordinator can send messages to workers: `{"cmd": "send_message", "to": 1, "text": "..."}`
- [ ] AC-8.9: Message delivery logged: `[W1→W2] Found auth pattern in code`
- [ ] AC-8.10: Workers can reply to messages using same mechanism (no special reply syntax)

### US-9: Stall Detection and Reassignment
**As a** Coordinator, **I want** to detect stalled workers and reassign their tasks, **so that** progress continues.

#### Acceptance Criteria
- [ ] AC-9.1: Coordinator tracks last activity timestamp per worker (updated on any output)
- [ ] AC-9.2: Worker stalled if no activity for 5 minutes (configurable via `--stall-timeout`)
- [ ] AC-9.3: On stall detection, Coordinator sends `{"cmd": "kill_worker", "worker_id": X}`
- [ ] AC-9.4: Orchestrator kills worker process and marks worker as `Stalled`
- [ ] AC-9.5: Stalled worker's assigned task status reset: `InProgress` → `Ready`
- [ ] AC-9.6: Ready task becomes available for reassignment to another worker
- [ ] AC-9.7: Stall detection logged: `[Coordinator] Worker 2 stalled (5m idle), reassigning task 7`
- [ ] AC-9.8: Stalled workers are restarted if worker pool size < N (auto-heal)
- [ ] AC-9.9: Restarted workers get new ID and start fresh (no state carried over)
- [ ] AC-9.10: Max 3 reassignments per task (mark as failed after 3 stalls)

### US-10: Progress Reporting to Orchestrator
**As a** Coordinator, **I want** to report progress to the orchestrator, **so that** users see swarm status.

#### Acceptance Criteria
- [ ] AC-10.1: Coordinator emits progress via `@hatchery:progress completed=X total=Y workers=[...]`
- [ ] AC-10.2: Orchestrator parses progress command and updates dashboard
- [ ] AC-10.3: Dashboard shows: total tasks, completed, failed, in-progress, blocked
- [ ] AC-10.4: Dashboard shows per-worker status: `W1: Task 3 (2m elapsed)`, `W2: Idle`
- [ ] AC-10.5: Dashboard shows recent messages (last 5 in scrolling log)
- [ ] AC-10.6: Dashboard updates every 1 second (non-blocking)
- [ ] AC-10.7: Progress includes estimated completion time based on task rate
- [ ] AC-10.8: Progress includes token usage and cost estimate
- [ ] AC-10.9: Dashboard uses colored output: green (active), yellow (stalled), red (failed)
- [ ] AC-10.10: Dashboard saves snapshot to `swarm_host_progress.json` on Ctrl+C

### US-11: Coordinator Prompt Engineering
**As a** Hatchery developer, **I want** a well-crafted Coordinator prompt, **so that** Claude manages the swarm effectively.

#### Acceptance Criteria
- [ ] AC-11.1: Prompt defines Coordinator role: "You are a swarm coordinator managing N workers"
- [ ] AC-11.2: Prompt includes JSON command reference with examples
- [ ] AC-11.3: Prompt includes DAG structure and task dependencies
- [ ] AC-11.4: Prompt instructs Coordinator to assign tasks in dependency order
- [ ] AC-11.5: Prompt instructs Coordinator to monitor worker output for `@hatchery:result` commands
- [ ] AC-11.6: Prompt instructs Coordinator to reassign stalled tasks
- [ ] AC-11.7: Prompt instructs Coordinator to report progress every 30 seconds
- [ ] AC-11.8: Prompt includes examples of good coordination strategies (e.g., parallel tasks, critical path)
- [ ] AC-11.9: Prompt warns against over-assigning (don't assign more tasks than workers can handle)
- [ ] AC-11.10: Prompt template supports variable substitution: `{PRD}`, `{DAG}`, `{WORKERS}`, `{SHARED_MEMORY_SCHEMA}`

### US-12: Worker Prompt Engineering
**As a** Hatchery developer, **I want** a well-crafted Worker prompt, **so that** Claude executes tasks correctly and communicates.

#### Acceptance Criteria
- [ ] AC-12.1: Prompt defines Worker role: "You are Worker {id}, part of a coordinated swarm"
- [ ] AC-12.2: Prompt includes `@hatchery:` command reference with examples
- [ ] AC-12.3: Prompt instructs Worker to check SharedMemory knowledge before starting task
- [ ] AC-12.4: Prompt instructs Worker to write discoveries to knowledge: `@hatchery:knowledge api.auth.method=bearer`
- [ ] AC-12.5: Prompt instructs Worker to report result after task: `@hatchery:result task_id=X status=success`
- [ ] AC-12.6: Prompt instructs Worker to run verification command and include output in result
- [ ] AC-12.7: Prompt instructs Worker to send messages if blocked: `@hatchery:message to=coordinator text="Need info on X"`
- [ ] AC-12.8: Prompt includes examples of good task implementations
- [ ] AC-12.9: Prompt warns against marking tasks complete without verification
- [ ] AC-12.10: Prompt template supports variable substitution: `{WORKER_ID}`, `{TASK}`, `{KNOWLEDGE}`, `{MESSAGES}`

### US-13: EventBus for Message Routing
**As a** orchestrator, **I want** an EventBus to route messages between Coordinator and Workers, **so that** communication is organized.

#### Acceptance Criteria
- [ ] AC-13.1: EventBus receives events from all PipeProcess instances (Coordinator + Workers)
- [ ] AC-13.2: EventBus parses NDJSON output and extracts CliEvents
- [ ] AC-13.3: EventBus routes `@hatchery:` commands to appropriate handlers
- [ ] AC-13.4: EventBus routes Coordinator JSON commands to command executor
- [ ] AC-13.5: EventBus maintains event log (last 1000 events) for debugging
- [ ] AC-13.6: EventBus supports event filtering (e.g., only route AssistantText with `@hatchery:` prefix)
- [ ] AC-13.7: EventBus is async (uses tokio channels for event passing)
- [ ] AC-13.8: EventBus handles backpressure (drop oldest events if queue full)
- [ ] AC-13.9: EventBus logs routing decisions at TRACE level: `[EventBus] Routed @hatchery:knowledge from W1 to SharedMemory`
- [ ] AC-13.10: EventBus exposes metrics: events processed, commands executed, messages routed

### US-14: SharedMemory Persistence
**As a** orchestrator, **I want** SharedMemory to persist to disk, **so that** state survives crashes.

#### Acceptance Criteria
- [ ] AC-14.1: SharedMemory serialized to `shared_memory.json` after each write
- [ ] AC-14.2: Persistence is async (uses tokio spawn_blocking for file I/O)
- [ ] AC-14.3: Persistence is debounced (max 1 write per second)
- [ ] AC-14.4: Persistence is atomic (write to `shared_memory.json.tmp`, rename on success)
- [ ] AC-14.5: Persistence includes schema version field (for compatibility checks)
- [ ] AC-14.6: On startup, load SharedMemory from file if exists
- [ ] AC-14.7: File corruption detected and handled (log error, start with empty state)
- [ ] AC-14.8: Persistence failures logged but do not crash orchestrator
- [ ] AC-14.9: Support `--no-persist` flag to disable file persistence (in-memory only)
- [ ] AC-14.10: Persistence includes timestamp and metadata (worker count, coordinator model, etc.)

### US-15: CLI Interface
**As a** user, **I want** a clean CLI interface to launch Swarm Host mode, **so that** I can easily start coordinated swarms.

#### Acceptance Criteria
- [ ] AC-15.1: Command syntax: `hatchery swarm_host <prd.md> [OPTIONS]`
- [ ] AC-15.2: Support `--workers N` flag (default: 4, range: 1-16)
- [ ] AC-15.3: Support `--coordinator-model MODEL` flag (default: "claude-sonnet-4.5")
- [ ] AC-15.4: Support `--worker-model MODEL` flag (default: same as coordinator)
- [ ] AC-15.5: Support `--stall-timeout SECONDS` flag (default: 300)
- [ ] AC-15.6: Support `--verify "command"` flag (default: "cargo check")
- [ ] AC-15.7: Support `--working-dir PATH` flag
- [ ] AC-15.8: Support `--no-persist` flag
- [ ] AC-15.9: Support `--resume` flag to load from `shared_memory.json`
- [ ] AC-15.10: Validate PRD file exists and DAG has no cycles before spawning

### US-16: Error Recovery
**As a** orchestrator, **I want** robust error recovery, **so that** swarm continues after failures.

#### Acceptance Criteria
- [ ] AC-16.1: Worker crashes detected via `PipeProcess::is_running()` polling
- [ ] AC-16.2: Crashed workers auto-restart with exponential backoff (1s, 2s, 4s, max 3 retries)
- [ ] AC-16.3: Crashed worker's task status reset: `InProgress` → `Ready` for reassignment
- [ ] AC-16.4: Coordinator crashes trigger graceful shutdown (all workers killed, state saved)
- [ ] AC-16.5: API rate limits detected and handled (pause worker, resume after reset time)
- [ ] AC-16.6: SharedMemory write failures retry up to 3 times with backoff
- [ ] AC-16.7: Parsing errors (malformed JSON, invalid commands) logged and ignored
- [ ] AC-16.8: Network errors (API timeouts) logged and retried per worker
- [ ] AC-16.9: All errors logged to `swarm_host_errors.log` with full context
- [ ] AC-16.10: Orchestrator exits with code 1 if Coordinator crashes, code 0 if all tasks complete

### US-17: Cross-Worker Knowledge Sharing
**As a** worker, **I want** to access knowledge written by other workers, **so that** I can avoid duplicate work.

#### Acceptance Criteria
- [ ] AC-17.1: Worker prompts include knowledge section: `Knowledge available: api.auth.method=bearer, test.setup.steps=[...]`
- [ ] AC-17.2: Knowledge section updated before each task assignment
- [ ] AC-17.3: Knowledge filtered by relevance (only include keys matching task keywords)
- [ ] AC-17.4: Workers can query specific knowledge: `@hatchery:query api.*` (response in next prompt)
- [ ] AC-17.5: Knowledge queries logged: `[W1] Queried knowledge: api.* (3 entries found)`
- [ ] AC-17.6: Knowledge updates visible to all workers within 5 seconds (SharedMemory refresh rate)
- [ ] AC-17.7: Knowledge namespacing prevents conflicts: `W1.findings.auth` vs `W2.findings.auth`
- [ ] AC-17.8: Knowledge includes metadata: author (worker ID), timestamp, TTL
- [ ] AC-17.9: Expired knowledge pruned from SharedMemory every 60 seconds
- [ ] AC-17.10: Knowledge export to markdown: `hatchery export-knowledge shared_memory.json > knowledge.md`

### US-18: DAG Visualization
**As a** user, **I want** to visualize the task dependency graph, **so that** I understand task ordering.

#### Acceptance Criteria
- [ ] AC-18.1: DAG exported to Graphviz DOT format: `dag.dot`
- [ ] AC-18.2: Nodes colored by status: green (completed), yellow (in-progress), red (failed), gray (blocked)
- [ ] AC-18.3: Edges labeled with dependency type (explicit, inferred from indentation)
- [ ] AC-18.4: Critical path highlighted (longest dependency chain)
- [ ] AC-18.5: Generate PNG image with `dot -Tpng dag.dot -o dag.png` (if Graphviz installed)
- [ ] AC-18.6: DAG updates in real-time (regenerated every 30 seconds)
- [ ] AC-18.7: Support `--dag-only` flag to export DAG and exit (no execution)
- [ ] AC-18.8: DAG includes task text as node labels (truncated to 40 chars)
- [ ] AC-18.9: DAG includes estimated completion percentage per node
- [ ] AC-18.10: DAG viewer in terminal using ASCII art (optional, via `--dag-ascii`)

### US-19: Status Aggregation
**As a** Coordinator, **I want** to aggregate worker status, **so that** I can make informed decisions.

#### Acceptance Criteria
- [ ] AC-19.1: Coordinator queries worker status via `{"cmd": "get_worker_status", "worker_id": X}`
- [ ] AC-19.2: Orchestrator responds with JSON: `{"worker_id": X, "status": "busy", "current_task": Y, "elapsed": 120}`
- [ ] AC-19.3: Worker status updated on every CliEvent (activity timestamp)
- [ ] AC-19.4: Aggregated status includes: total tasks, completed, failed, workers idle/busy/stalled
- [ ] AC-19.5: Aggregated status includes token usage per worker
- [ ] AC-19.6: Aggregated status includes cost estimate per worker
- [ ] AC-19.7: Status query responses cached for 5 seconds (avoid excessive computation)
- [ ] AC-19.8: Coordinator uses status to decide task priority (assign to least-busy worker)
- [ ] AC-19.9: Status logged to coordinator output: `[Coordinator] Status: 3/10 tasks done, 2 workers busy, 2 idle`
- [ ] AC-19.10: Status exported to JSON: `hatchery status shared_memory.json > status.json`

### US-20: Graceful Shutdown
**As a** user, **I want** graceful shutdown on Ctrl+C, **so that** swarm state is saved.

#### Acceptance Criteria
- [ ] AC-20.1: Register signal handler for SIGINT and SIGTERM
- [ ] AC-20.2: On signal, send shutdown command to Coordinator: `{"cmd": "shutdown", "reason": "user_interrupt"}`
- [ ] AC-20.3: Coordinator sends shutdown message to all workers: `@hatchery:message to=all text="Shutting down"`
- [ ] AC-20.4: Workers finish current tool call (max 5 seconds), then exit
- [ ] AC-20.5: Orchestrator waits for all workers to exit (max 10 seconds total)
- [ ] AC-20.6: After timeout, forcefully kill remaining workers
- [ ] AC-20.7: Save SharedMemory to file before exit
- [ ] AC-20.8: Generate summary report with partial results
- [ ] AC-20.9: Display shutdown message: "Shutting down gracefully... (Ctrl+C again to force)"
- [ ] AC-20.10: Exit with code 130 (standard for SIGINT)

### US-21: Resume from SharedMemory
**As a** user, **I want** to resume a Swarm Host session from saved state, **so that** I can recover from crashes.

#### Acceptance Criteria
- [ ] AC-21.1: Support `hatchery swarm_host --resume <shared_memory.json>`
- [ ] AC-21.2: Load SharedMemory from file and validate schema version
- [ ] AC-21.3: Resume rebuilds DAG from SharedMemory task list
- [ ] AC-21.4: Resume spawns Coordinator and Workers with restored state
- [ ] AC-21.5: Coordinator prompt includes resume context: "Resuming from previous session, X tasks completed"
- [ ] AC-21.6: Resume skips completed tasks (status = Completed)
- [ ] AC-21.7: Resume resets in-progress tasks to ready (status = InProgress → Ready)
- [ ] AC-21.8: Resume preserves knowledge HashMap (no reset)
- [ ] AC-21.9: Resume appends to existing logs (no overwrite)
- [ ] AC-21.10: Resume mode logged in summary: "Resumed from {timestamp}, {completed} tasks already done"

### US-22: Logging and Observability
**As a** developer, **I want** detailed logging from Swarm Host, **so that** I can debug coordination issues.

#### Acceptance Criteria
- [ ] AC-22.1: Use `tracing` crate for structured logging
- [ ] AC-22.2: Log levels: TRACE (events), DEBUG (state changes), INFO (assignments), WARN (stalls), ERROR (failures)
- [ ] AC-22.3: Default log level: INFO (configurable via `RUST_LOG`)
- [ ] AC-22.4: Log to stdout with colored output
- [ ] AC-22.5: Log to file `swarm_host_{timestamp}.log` (optional, via `--log-file`)
- [ ] AC-22.6: Logs include entity prefix: `[Coordinator]`, `[W1]`, `[EventBus]`, `[SharedMem]`
- [ ] AC-22.7: Logs include span context for tracing message flow
- [ ] AC-22.8: Log SharedMemory state changes: `[SharedMem] Knowledge updated: api.auth.method=bearer`
- [ ] AC-22.9: Log task state transitions: `[DAG] Task 3: Ready → Assigned → InProgress`
- [ ] AC-22.10: Support `--quiet` flag to suppress INFO logs

## Technical Design

### Core Data Structures

```rust
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Shared memory state (in-memory + file-backed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMemoryState {
    pub version: u32,
    pub knowledge: HashMap<String, KnowledgeEntry>,
    pub tasks: Vec<Task>,
    pub messages: VecDeque<Message>,
    pub results: HashMap<TaskId, TaskResult>,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub key: String,
    pub value: serde_json::Value,
    pub author: WorkerId,
    pub timestamp: DateTime<Utc>,
    pub ttl_seconds: u64,
}

type TaskId = usize;
type WorkerId = usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub text: String,
    pub status: TaskStatus,
    pub dependencies: Vec<TaskId>,
    pub assigned_to: Option<WorkerId>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Blocked,
    Ready,
    Assigned,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub from: WorkerId,
    pub to: Target,
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Target {
    Worker(WorkerId),
    Coordinator,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub status: TaskStatus,
    pub verification_output: String,
    pub error_message: Option<String>,
    pub duration: std::time::Duration,
    pub git_commit_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub worker_count: usize,
    pub coordinator_model: String,
    pub worker_model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Thread-safe SharedMemory wrapper.
pub struct SharedMemory {
    state: Arc<RwLock<SharedMemoryState>>,
    persist_path: PathBuf,
}

impl SharedMemory {
    pub fn new(persist_path: PathBuf, worker_count: usize) -> Self {
        let state = SharedMemoryState {
            version: 1,
            knowledge: HashMap::new(),
            tasks: Vec::new(),
            messages: VecDeque::new(),
            results: HashMap::new(),
            metadata: Metadata {
                worker_count,
                coordinator_model: "claude-sonnet-4.5".to_string(),
                worker_model: "claude-sonnet-4.5".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        };
        Self {
            state: Arc::new(RwLock::new(state)),
            persist_path,
        }
    }

    pub fn set_knowledge(&self, key: String, value: serde_json::Value, author: WorkerId) {
        let mut state = self.state.write().unwrap();
        state.knowledge.insert(key.clone(), KnowledgeEntry {
            key,
            value,
            author,
            timestamp: Utc::now(),
            ttl_seconds: 3600,
        });
        drop(state);
        self.persist();
    }

    pub fn get_knowledge(&self, pattern: &str) -> HashMap<String, serde_json::Value> {
        let state = self.state.read().unwrap();
        // Glob pattern matching (simplified)
        state.knowledge.iter()
            .filter(|(k, _)| k.starts_with(pattern.trim_end_matches('*')))
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect()
    }

    pub fn send_message(&self, from: WorkerId, to: Target, text: String) {
        let mut state = self.state.write().unwrap();
        state.messages.push_back(Message {
            from,
            to,
            text,
            timestamp: Utc::now(),
        });
        if state.messages.len() > 100 {
            state.messages.pop_front();
        }
        drop(state);
        self.persist();
    }

    pub fn get_messages(&self, worker_id: WorkerId) -> Vec<Message> {
        let state = self.state.read().unwrap();
        state.messages.iter()
            .filter(|m| match m.to {
                Target::Worker(id) => id == worker_id,
                Target::All => true,
                Target::Coordinator => false,
            })
            .cloned()
            .collect()
    }

    pub fn get_ready_tasks(&self) -> Vec<TaskId> {
        let state = self.state.read().unwrap();
        state.tasks.iter()
            .filter(|t| t.status == TaskStatus::Ready)
            .map(|t| t.id)
            .collect()
    }

    fn persist(&self) {
        let state = self.state.read().unwrap();
        let json = serde_json::to_string_pretty(&*state).unwrap();
        let tmp_path = self.persist_path.with_extension("tmp");
        std::fs::write(&tmp_path, json).ok();
        std::fs::rename(tmp_path, &self.persist_path).ok();
    }
}
```

### Coordinator Command Protocol

```json
// Assign task to worker
{"cmd": "assign_task", "task_id": 3, "worker_id": 1}

// Get worker status
{"cmd": "get_worker_status", "worker_id": 1}

// Send message to worker
{"cmd": "send_message", "to": 1, "text": "Check knowledge.api.auth before starting"}

// Kill stalled worker
{"cmd": "kill_worker", "worker_id": 2}

// Report progress
{"cmd": "report_progress", "completed": 5, "total": 20, "workers": [{"id": 0, "status": "busy"}, ...]}

// Shutdown
{"cmd": "shutdown", "reason": "user_interrupt"}
```

### Worker @hatchery Commands

```
// Write knowledge
@hatchery:knowledge api.auth.method=bearer

// Write JSON knowledge
@hatchery:knowledge api.config='{"timeout": 30, "retries": 3}'

// Report task result
@hatchery:result task_id=3 status=success

// Send message
@hatchery:message to=W2 text="Found endpoint docs at docs/api.md"

// Broadcast message
@hatchery:message to=all text="API auth requires X-Api-Key header"

// Query knowledge
@hatchery:query api.*
```

### DAG Builder Algorithm

```rust
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::is_cyclic_directed;

pub fn build_dag(prd: &Prd) -> Result<DiGraph<Task, ()>, Error> {
    let mut graph = DiGraph::new();
    let mut nodes: HashMap<TaskId, NodeIndex> = HashMap::new();

    // Add nodes
    for task in &prd.tasks {
        let idx = graph.add_node(task.clone());
        nodes.insert(task.id, idx);
    }

    // Add edges (dependencies)
    for task in &prd.tasks {
        for dep_id in &task.dependencies {
            let from = nodes[dep_id];
            let to = nodes[&task.id];
            graph.add_edge(from, to, ());
        }
    }

    // Check for cycles
    if is_cyclic_directed(&graph) {
        return Err(Error::CyclicDependency);
    }

    Ok(graph)
}
```

## Dependencies

```toml
[dependencies]
zengeld-hub-core = { path = "../zengeld-hub/crates/core" }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
regex = "1"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
petgraph = "0.6" # DAG
parking_lot = "0.12" # RwLock
```

## Verification

### US-1-2: SharedMemory CRUD
```rust
#[test]
fn test_shared_memory_knowledge() {
    let mem = SharedMemory::new("test.json".into(), 4);
    mem.set_knowledge("api.auth".into(), json!("bearer"), 0);
    let result = mem.get_knowledge("api.*");
    assert_eq!(result.get("api.auth").unwrap(), "bearer");
}
```

### US-3: DAG Building
```bash
cat > test.md <<EOF
- [ ] Task A
  - [ ] Task B (depends on A)
  - [ ] Task C (depends on A)
- [ ] Task D
EOF

cargo run -- swarm_host test.md --dag-only
# Expected: dag.dot with 4 nodes, edges A→B, A→C, no cycles
```

### US-4-5: Session Spawning
```bash
cargo run -- swarm_host test.md --workers 2
# Expected: 1 Coordinator + 2 Workers spawned, logged to stdout
```

### US-7-8: Task Assignment and Messaging
```bash
# Run with verbose logging
RUST_LOG=debug cargo run -- swarm_host test.md --workers 2
# Expected: Coordinator assigns tasks, workers send @hatchery:result, messages routed
```

### US-17: Knowledge Sharing
```bash
# Worker 1 writes knowledge, Worker 2 reads it
# Check shared_memory.json after run
cat shared_memory.json | jq '.knowledge'
# Expected: Contains entries from multiple workers
```

### US-21: Resume
```bash
# Start session, kill mid-execution
cargo run -- swarm_host test.md --workers 2
# ^C

# Resume
cargo run -- swarm_host --resume shared_memory.json
# Expected: Loads state, continues from last checkpoint
```

## Open Questions

1. **Coordinator Model**: Should Coordinator run on Opus for better strategic thinking, or Sonnet for cost efficiency?
   - **Proposal**: Default Sonnet, optional `--coordinator-model opus` for complex PRDs.

2. **Message Queue Size**: 100 messages may be too small for large swarms. Dynamic sizing?
   - **Proposal**: v1 fixed 100, v2 add `--message-queue-size` flag.

3. **Knowledge Conflicts**: If two workers write different values to same key, which wins?
   - **Proposal**: Last write wins, log warning if overwrite detected.

4. **DAG Parallelism**: Should Coordinator assign multiple ready tasks to one worker (parallel execution)?
   - **Proposal**: v1 one task per worker, v2 add task batching.

5. **Worker Affinity**: Should certain workers specialize in certain task types?
   - **Proposal**: v1 no specialization, v2 add task→worker affinity hints.

6. **Cost Budgets**: Should SharedMemory track and enforce token/cost budgets?
   - **Proposal**: v1 no budgets (rely on API limits), v2 add budget enforcement.

7. **Verification Parallelism**: Should verification commands run in parallel across workers?
   - **Proposal**: Yes, each worker runs verification independently (no global lock).

8. **Message Priority**: Should urgent messages jump the queue?
   - **Proposal**: v1 FIFO only, v2 add priority field.

9. **DAG Mutations**: Should Coordinator be able to add new tasks mid-execution?
   - **Proposal**: v1 static DAG, v2 add `{"cmd": "add_task", ...}` command.

10. **Session Isolation**: Should workers have separate working directories to avoid conflicts?
    - **Proposal**: v1 shared working dir (assumes tasks are disjoint), v2 add per-worker isolation.
