# Hatchery V2: Implementation PRD

**Project ID:** hatchery-v2
**Created:** 2026-02-08
**Status:** In Progress
**Estimated Duration:** 7 weeks

## Context

Hatchery v2 is a major refactor of the swarm orchestration system. The current v1 has three modes (Queen, SwarmHost, BroodLord) that work but lack:
- Abstraction over different agent backends (only Claude Code PipeProcess)
- Proper mailbox/communication bus between levels
- Context compression for long-running coordinators
- AI-driven coordination (SwarmHost coordinator is still Rust-deterministic)
- Proper Queen as AI manager (currently just PRD iterator)
- Event-driven communication with operator
- Durable event logging

V2 introduces:
- `trait Queen` with NativeQueen (Claude Code) and CustomQueen (any backend) implementations
- `SwarmMailbox` with per-agent inboxes, SQLite event log, tokio channels
- `CompactionStrategy` for context management
- Refactored SwarmHost using Queens instead of raw PipeProcesses
- `OperatorChannel` for BroodLord ↔ human/parent-agent communication
- `TaskDag` with dependency tracking, auto-unblocking, critical path analysis
- Git coordination with per-Queen worktrees and validated merges

## Phase 1: Queen Trait + Core Types (Week 1-2)

### 1.1 Core Types & Messages

- [x] Define `SwarmMessage` struct with id, from, to, msg_type, payload, timestamp, correlation_id, visibility
- [x] Define `MessageType` enum: TaskAssignment, TaskResult, TaskProgress, StatusRequest, StatusReport, Knowledge, KnowledgeQuery, Escalation, Shutdown, Custom
- [x] Define `Visibility` struct with agent_visible, coordinator_visible, user_visible flags
- [x] Define `AgentId` enum: SwarmHost, Queen, Validator, BroodLord, Operator
- [x] Define `TaskId`, `QueenId`, `SwarmHostId` newtypes
- [x] Define `TaskStatus` enum: Blocked, Ready, Assigned, InProgress, Validating, Completed, Failed
- [x] Define `QueenStatus` enum: Idle, Working, Blocked, Failed, Completed, Dead
- [x] Define `TaskContext` struct (knowledge snapshot, recent messages, shared state)
- [x] Define `TaskResult` struct (status, output, artifacts, duration, git_sha)
- [x] Create `hatchery/src/v2/types.rs` with all types
- [x] Add serde Serialize/Deserialize for all types
- [ ] Add unit tests for serialization roundtrip

### 1.2 Queen Trait

- [x] Define `trait Queen` with async methods: assign, status, result, send_message, drain_outbox, is_alive, shutdown
- [x] Define `QueenBackend` enum: ClaudeNative, ClaudeRaw, Codex, ApiGeneric
- [x] Define `QueenConfig` struct with common config (timeout, max_workers, model)
- [x] Create `hatchery/src/v2/queen/mod.rs` with trait definition
- [ ] Add documentation with usage examples

### 1.3 NativeQueen Implementation

- [x] Create `hatchery/src/v2/queen/native.rs`
- [x] Implement NativeQueen struct wrapping PipeProcess
- [x] Implement `trait Queen` for NativeQueen
- [x] Design NativeQueen prompt template (AI sergeant, not dumb iterator)
  - Must instruct Claude to decompose task into sub-tasks
  - Must instruct Claude to use Task tool for sub-workers (if teams enabled)
  - Must instruct Claude to report progress via @hatchery: protocol
  - Must instruct Claude to report completion with result summary
- [x] Implement @hatchery: command parsing from NativeQueen stdout
- [x] Implement status detection from Claude output (working, blocked, completed)
- [x] Implement timeout and stall detection
- [x] Implement graceful shutdown (send /exit or kill process)
- [ ] Write unit tests with mock PipeProcess
- [ ] Write integration test: spawn NativeQueen, assign simple task, verify completion

### 1.4 CustomQueen Implementation (Basic)

- [x] Create `hatchery/src/v2/queen/custom.rs`
- [x] Implement CustomQueen struct with configurable backend
- [ ] Implement HTTP API backend (WorkerBackend::HttpApi)
  - Build prompt from task + context
  - Send to API endpoint
  - Parse response
  - Track worker state
- [x] Implement basic QueenMailbox (in-memory VecDeque per worker)
- [x] Implement basic TaskScheduler (round-robin assignment, no DAG yet)
- [x] Implement `trait Queen` for CustomQueen
- [ ] Write unit tests with mock HTTP server
- [ ] Write integration test with real API key (skip in CI)

### 1.5 Backward Compatibility

- [ ] Refactor current Queen mode to use NativeQueen internally
- [ ] Ensure `hatchery spawn prd.md --mode queen` still works as before
- [ ] Ensure `hatchery spawn prd.md --mode queen --workers 3` still works
- [ ] Add `--backend` flag: `--backend claude-native` (default), `--backend api --api-url ... --api-model ...`
- [ ] Run existing tests, verify nothing breaks

## Phase 2: SwarmMailbox + Message Router (Week 2-3)

### 2.1 SwarmMailbox

- [ ] Create `hatchery/src/v2/mailbox/mod.rs`
- [ ] Implement `SwarmMailbox` struct with host_inbox, queen_inboxes, validator_inbox, outbox
- [ ] Implement `send()` method — routes message to correct inbox based on `to` field
- [ ] Implement `recv()` methods — per-agent inbox draining
- [ ] Implement broadcast — sends to all queen inboxes
- [ ] Implement message TTL expiration
- [ ] Implement max message cap (configurable, default 1000)
- [ ] Write unit tests: send, receive, broadcast, TTL, cap

### 2.2 SqliteEventLog

- [ ] Create `hatchery/src/v2/mailbox/event_log.rs`
- [ ] Implement SqliteEventLog with rusqlite
- [ ] Design schema: events table with id, timestamp, from, to, msg_type, payload, correlation_id, visibility fields
- [ ] Implement `log()` method — insert event
- [ ] Implement `query()` method — filter by time range, agent, type
- [ ] Implement `replay()` method — replay events from cursor
- [ ] Implement `count()` and `stats()` methods
- [ ] Auto-create database and tables on first use
- [ ] Write unit tests with tempfile database

### 2.3 Message Router

- [ ] Create `hatchery/src/v2/mailbox/router.rs`
- [ ] Implement MessageRouter with tokio::mpsc channels
- [ ] Implement `register()` — add agent's channel to route table
- [ ] Implement `unregister()` — remove agent
- [ ] Implement `route()` — send message to correct channel, log to event log
- [ ] Implement default route (escalate to parent if target unknown)
- [ ] Implement correlation ID tracking for request-response pairs
- [ ] Write unit tests: register, route, unregister, unknown target
- [ ] Write integration test: spawn 3 agents, route messages between them

## Phase 3: SwarmHost Refactor (Week 3-4)

### 3.1 TaskDag

- [ ] Create `hatchery/src/v2/task_dag.rs`
- [ ] Implement DagTask struct with blocked_by, blocks, priority, complexity
- [ ] Implement TaskDag with HashMap storage
- [ ] Implement `add_task()` with automatic reverse-link (blocked_by ↔ blocks)
- [ ] Implement `ready_tasks()` — return tasks with all deps satisfied and status Ready
- [ ] Implement `refresh_readiness()` — Blocked → Ready when deps complete
- [ ] Implement `assign()` — mark task as Assigned to QueenId
- [ ] Implement `complete()` — mark as Completed, trigger refresh
- [ ] Implement `fail()` — mark as Failed, optionally reset to Ready for retry
- [ ] Implement `critical_path()` — identify bottleneck tasks
- [ ] Implement `from_prd()` — build DAG from parsed PRD (infer deps from indentation/keywords)
- [ ] Write unit tests for all operations including dependency chains

### 3.2 CompactionStrategy

- [ ] Create `hatchery/src/v2/compaction.rs`
- [ ] Implement CompactionStrategy struct with threshold, protected scopes, levels
- [ ] Implement `should_compact()` — check if current context size exceeds threshold
- [ ] Implement `compact()` — apply progressive compaction levels
- [ ] Implement compaction levels:
  - Level 1 (80%): Remove tool responses older than 20 turns
  - Level 2 (85%): Remove tool responses older than 10 turns
  - Level 3 (90%): Summarize conversation older than 5 turns
  - Level 4 (95%): Fresh start with system prompt + task DAG + knowledge + last 3 messages
- [ ] Implement protected scope preservation (system prompt, active task context, knowledge)
- [ ] Implement Goose-style dual-visibility: only compact agent-invisible messages
- [ ] Write unit tests with mock context data

### 3.3 Validator

- [ ] Create `hatchery/src/v2/validator.rs`
- [ ] Implement Validator enum: Command, Queen, Pipeline
- [ ] Implement Command validator (run shell command, check exit code)
- [ ] Implement Queen validator (spawn AI agent to review code)
- [ ] Implement Pipeline validator (command first, then AI if passes)
- [ ] Implement ValidationResult with stage results and feedback
- [ ] Write unit tests for Command validator

### 3.4 SwarmHost Refactor

- [ ] Refactor `hatchery/src/v2/swarm_host.rs` (new file, keep old for reference)
- [ ] SwarmHost uses `HashMap<QueenId, Box<dyn Queen>>` instead of raw PipeProcess
- [ ] SwarmHost uses SwarmMailbox for all communication
- [ ] SwarmHost uses TaskDag for task scheduling
- [ ] SwarmHost uses Validator for result checking
- [ ] SwarmHost uses CompactionStrategy for context management
- [ ] Implement AI Coordinator session (Claude prompt for intelligent task assignment)
  - Coordinator reads task DAG state
  - Coordinator decides which Queen gets which task
  - Coordinator monitors progress and handles escalations
  - Coordinator adjusts plan based on results
- [ ] Implement main orchestration loop with tokio::select!
- [ ] Implement Queen spawn/shutdown lifecycle
- [ ] Implement SharedMemory integration (knowledge injection into Queen prompts)
- [ ] Implement progress reporting (to outbox for BroodLord/Operator)
- [ ] Write integration test: SwarmHost with 2 NativeQueens, 3 tasks with deps

### 3.5 Backward Compatibility

- [ ] Ensure `hatchery spawn prd.md --mode swarm_host` still works
- [ ] Map old SharedMemory to new SwarmMailbox + SharedMemory
- [ ] Map old @hatchery: commands to SwarmMessage types
- [ ] Run existing SwarmHost tests

## Phase 4: Git Coordination (Week 4-5)

### 4.1 WorktreeManager Refactor

- [ ] Refactor `hatchery/src/v2/git/worktree.rs`
- [ ] Implement per-Queen worktree creation with branch naming: `hatchery/swarm-{id}/queen-{id}`
- [ ] Implement merge with validation gate (merge only after Validator passes)
- [ ] Implement conflict detection and strategy selection (TakeTheirs, TakeOurs, Escalate)
- [ ] Implement conflict escalation to SwarmHost for AI resolution
- [ ] Implement worktree cleanup
- [ ] Implement branch cleanup after merge

### 4.2 Merge Flow

- [ ] Implement Queen → SwarmHost branch merge
- [ ] Implement SwarmHost branch → main merge (for BroodLord level)
- [ ] Implement merge commit attribution (Co-Authored-By: Queen-0)
- [ ] Implement merge verification (run validator after merge)
- [ ] Write integration test: two Queens edit same file, merge, verify

## Phase 5: BroodLord + OperatorChannel (Week 5-6)

### 5.1 OperatorChannel

- [ ] Create `hatchery/src/v2/operator/mod.rs`
- [ ] Define `trait OperatorChannel` with emit, recv, is_connected
- [ ] Define `OperatorEvent` enum: Progress, GlobalProgress, Escalation, Question, SwarmCompleted, AllComplete, Error
- [ ] Define `OperatorCommand` enum: Reprioritize, Cancel, Answer, Message, Scale, ShutdownAll
- [ ] Implement `StdoutChannel` (print events to stdout, read commands from stdin)
- [ ] Implement `PipeChannel` (for when BroodLord is spawned by parent Opus agent)
- [ ] Write unit tests for both channels

### 5.2 BroodLord Refactor

- [ ] Refactor `hatchery/src/v2/brood_lord.rs` (new file)
- [ ] BroodLord uses SwarmHosts instead of raw L2 processes
- [ ] Implement continuous monitoring loop (not one-shot decomposition)
  - Periodically check SwarmHost status
  - React to escalations
  - Adjust resource allocation
  - Report to Operator
- [ ] Implement AI strategist session (Opus-level)
  - Decompose master task into SwarmHost assignments
  - Monitor cross-SwarmHost dependencies
  - Handle escalations with intelligent decisions
- [ ] Implement GlobalMemory with namespaced knowledge
- [ ] Implement global TaskDag for cross-SwarmHost dependencies
- [ ] Implement dynamic scaling (add/remove Queens from SwarmHosts)
- [ ] Wire up OperatorChannel for bidirectional communication
- [ ] Write integration test: BroodLord with 2 SwarmHosts, operator interaction

### 5.3 Backward Compatibility

- [ ] Ensure `hatchery spawn prd.md --mode brood_lord` still works
- [ ] Map old BroodLord to new architecture

## Phase 6: CustomQueen Full Stack (Week 6-7)

### 6.1 CustomQueen Enhanced

- [ ] Implement full QueenMailbox with per-worker inboxes and event log
- [ ] Implement TaskScheduler with DAG support (not just round-robin)
- [ ] Implement worker pool management (spawn, monitor, restart dead workers)
- [ ] Implement context tracking per worker (token count, compaction)

### 6.2 Codex Backend

- [ ] Implement WorkerBackend::CodexSandbox
- [ ] Research Codex API for sandbox creation/management
- [ ] Implement session management (create, send task, poll result, cleanup)
- [ ] Write integration test (skip in CI, needs Codex access)

### 6.3 Generic API Backend

- [ ] Implement WorkerBackend::HttpApi with configurable model/endpoint
- [ ] Support OpenAI-compatible API format
- [ ] Support Anthropic API format
- [ ] Implement conversation history management (for multi-turn)
- [ ] Implement streaming response handling
- [ ] Write unit tests with mock HTTP server

### 6.4 Mixed Backend SwarmHost

- [ ] Test: SwarmHost with NativeQueen + CustomQueen (API backend) working together
- [ ] Verify mailbox routing works across backend types
- [ ] Verify SharedMemory accessible to both backend types
- [ ] Verify git worktrees work for both backend types

## Phase 7: CLI + Integration (Week 7)

### 7.1 CLI Updates

- [ ] Add `--backend` flag to `hatchery spawn`
- [ ] Add `--api-url` and `--api-model` flags for custom backend
- [ ] Add `--validator` flag (command string or "ai" for Queen validator)
- [ ] Add `--compaction-threshold` flag
- [ ] Add `--event-log` flag for SQLite path
- [ ] Add `hatchery events` command to query event log
- [ ] Add `hatchery message <target> <text>` for operator → agent messaging
- [ ] Update `hatchery status` to show Queen-level details

### 7.2 Configuration File

- [ ] Implement TOML config loading from `.hatchery/config.toml`
- [ ] Config precedence: CLI flags > config file > defaults
- [ ] Document all config options

### 7.3 Prompt Templates

- [ ] Design NativeQueen sergeant prompt (AI manager, not iterator)
- [ ] Design SwarmHost coordinator prompt (tactical coordinator)
- [ ] Design BroodLord strategist prompt (strategic decomposer)
- [ ] Design Validator AI reviewer prompt
- [ ] Store prompts in `hatchery/src/v2/prompts/`
- [ ] Support user override via `--prompt` flag or config

### 7.4 Final Integration Tests

- [ ] End-to-end: Queen mode with NativeQueen, simple 3-task PRD
- [ ] End-to-end: SwarmHost with 3 NativeQueens, 10-task PRD with dependencies
- [ ] End-to-end: BroodLord with 2 SwarmHosts, master PRD decomposed into 2 sub-PRDs
- [ ] End-to-end: SwarmHost with mixed backends (1 NativeQueen + 1 CustomQueen)
- [ ] Crash recovery: kill Queen mid-task, verify SwarmHost reassigns
- [ ] Git isolation: 2 Queens edit overlapping files, verify clean merge
- [ ] Compaction: long-running SwarmHost triggers compaction, continues working

## Shared Knowledge

<!-- Auto-updated by workers via @hatchery:write-knowledge -->

## Results

<!-- Auto-updated by workers via @hatchery:write-result -->

## Metadata

**Total tasks**: 120
**Architecture doc**: ARCHITECTURE_V2.md
**Reference code**: hatchery/src/ (v1), research/revolver-research/_tmp_src/
**Research**: hatchery/research/major-swarm-research/
