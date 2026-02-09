# PRD: Hatchery Transport Layer — Operator↔Queen Communication

## Problem Statement

Hatchery has a working DAG scheduler that assigns tasks to Queens and receives completion events. However, several critical issues prevent true interactive operation:

1. **MailboxSend is fake** — `hatchery mailbox send` writes to SharedMemory as `msg:{uuid}` KnowledgeEntry with 1h TTL. Queens NEVER receive these. No routing, no notification, no delivery.

2. **No operator visibility** — CLI cannot query queen status (idle/working/dead). Must infer from event log.

3. **No runtime task injection** — All tasks must be in DAG before `nydus.run()`. Operator cannot add new tasks to running swarm.

4. **One-shot lifecycle** — Swarm runs DAG to completion and exits. No interactive loop where operator reviews results and gives new instructions.

## Goal

Enable a true interactive loop:
```
Queen finishes task → reports result to outbox → goes idle
Operator reads outbox → sees results/questions → sends new instruction
Queen receives instruction → works on it → reports back
... repeat until operator says stop
```

## Architecture Overview

Current transport layers:
- **EventBus** (tokio mpsc): Queen→Nydus events (TaskCompleted, Progress, etc.) — **WORKS**
- **QueenHandle** (tokio mpsc): Nydus→Queen commands (Assign, Message, Shutdown) — **WORKS**
- **TCP IPC** (JSON over TCP): CLI→Nydus requests — **WORKS** but limited commands
- **SwarmMailbox** (VecDeque): Per-agent inboxes + outbox — **EXISTS** but disconnected from IPC
- **SharedMemory** (HashMap): Knowledge store — **WORKS** but used as fake mailbox

## Phase 1: Fix Mailbox — True Message Delivery

### 1.1 Wire IPC MailboxSend to SwarmMailbox
- [x] Change `MailboxSend` handler in `nydus/ipc/mod.rs` to route through `SwarmMailbox::send()` instead of SharedMemory
- [x] Parse `to` field: "queen:Q0" → `AgentId::Queen(QueenId("Q0"))`, "nydus" → `AgentId::Nydus(...)`, "operator" → `AgentId::Operator`
- [x] Parse `queen_id` (sender): present → `AgentId::Queen(...)`, absent → `AgentId::Operator`
- [x] Create proper `SwarmMessage` with UUID, timestamp, correlation_id
- [x] Return `{sent: true, message_id}` in response

### 1.2 Wire IPC MailboxRead to SwarmMailbox
- [x] Change `MailboxRead` handler to read from `SwarmMailbox` instead of SharedMemory
- [x] `from: None` → read operator's outbox via `drain_outbox()`
- [x] `from: Some("queen:Q0")` → read that queen's inbox via `recv_queen()`
- [x] Support `inbox: "outbox"` param to explicitly read outbox (operator sees queen results)
- [x] Return messages as JSON array with id, from, to, msg_type, payload, timestamp

### 1.3 Give Nydus mutable mailbox access from IPC handler
- [x] Currently IPC handler only has `Arc<RwLock<MemoryState>>`. Need to also pass `Arc<Mutex<SwarmMailbox>>` to `start_ipc_listener()`
- [x] Or: add a channel from IPC handler to Nydus main loop that forwards mailbox operations
- [x] Preferred approach: `Arc<Mutex<SwarmMailbox>>` since mailbox operations are simple and fast

### 1.4 Auto-push TaskCompleted results to outbox
- [x] In `Nydus::handle_event()` for `TaskCompleted`: create SwarmMessage with result_text and push to `mailbox.send(to: Operator)`
- [x] Include queen_id, task_id, cost, duration in payload
- [x] For `TaskFailed`: same but with error info
- [x] Operator can then read these via `hatchery mailbox read`

## Phase 2: Queen Status Query

### 2.1 Add IpcRequest::QueenStatus
- [x] New variant: `QueenStatus { queen_id: Option<String> }`
- [x] If queen_id is None: return ALL queens' status
- [x] If queen_id is Some: return that specific queen
- [x] Response: `{queens: [{id, status, task_id, progress, spawn_mode, is_alive}]}`

### 2.2 Implement handler
- [x] IPC handler needs access to `HashMap<QueenId, QueenHandle>` (read-only)
- [x] Pass `Arc<RwLock<Vec<QueenStatusSnapshot>>>` that Nydus periodically updates
- [x] Or: pass handles directly (QueenHandle.status() is non-blocking watch read)
- [x] Preferred: snapshot approach — Nydus updates snapshot every tick, IPC reads it

### 2.3 CLI command
- [x] `hatchery queen-status [--queen Q0]` — shows queen status table
- [x] Format: `Q0: Idle | Q1: Working(T3, 45%) | Q2: Dead`

## Phase 3: Runtime Task Injection

### 3.1 Add IpcRequest::InjectTask
- [x] New variant: `InjectTask { queen_id: Option<String>, prompt: String, priority: Option<u8>, task_id: Option<String> }`
- [x] If queen_id specified: create task and assign directly to that queen (must be idle)
- [x] If queen_id is None: add task to DAG, scheduler assigns to next idle queen
- [x] Auto-generate task_id if not provided: `"injected-{uuid}"`

### 3.2 Implement handler
- [x] IPC handler sends `InjectTask` request to Nydus via channel (can't directly modify DAG from IPC handler)
- [x] Add `inject_tx: mpsc::Sender<InjectRequest>` to IPC handler
- [x] Nydus main loop: `tokio::select!` branch for inject_rx
- [x] On receive: add task to DAG, call try_schedule()

### 3.3 Direct queen assignment
- [x] If queen_id specified and queen is idle: bypass DAG, call `handle.assign()` directly
- [x] Create ad-hoc Task object with injected prompt as description
- [x] TaskContext populated from current SharedMemory

### 3.4 CLI command
- [x] `hatchery inject "do X" [--queen Q0] [--priority 5]`
- [x] Returns: `{task_id, assigned_to (if immediate), status}`

## Phase 4: Persistent Interactive Loop

### 4.1 Keep-alive mode for Nydus
- [x] Currently `is_complete()` returns true when all DAG tasks done → exits
- [x] Add `--keep-alive` flag to `hatchery spawn`
- [x] When keep-alive: don't exit after DAG completion, keep event loop running
- [x] Wait for: new injected tasks OR operator shutdown command

### 4.2 Add IpcRequest::Shutdown
- [x] Clean shutdown via IPC: `hatchery shutdown`
- [x] Triggers `Nydus::shutdown()` gracefully

### 4.3 Add IpcRequest::SwarmStatus
- [x] Returns overall swarm state: total/completed/failed/in-progress tasks, queens summary, uptime
- [x] `hatchery swarm-status`

## Phase 5: Message Delivery to Queens

### 5.1 Deliver operator messages to idle queens
- [x] When operator sends message via `MailboxSend` to queen:
- [x] If queen is idle: Nydus creates synthetic task from message and assigns via `handle.assign()`
- [x] If queen is working: queue in SwarmMailbox queen inbox, deliver after current task completes

### 5.2 After task completion, check queen inbox
- [x] In `handle_event(TaskCompleted)`: after marking DAG complete, check `mailbox.recv_queen(queen_id)`
- [x] If messages waiting: create synthetic task from messages, assign immediately
- [x] If no messages and no DAG tasks ready: queen stays idle

### 5.3 Synthetic task format
- [x] Task description = operator's message text
- [x] TaskContext includes recent SharedMemory + previous task result
- [x] task_id = `"operator-msg-{uuid}"`

## Non-Goals (out of scope)

- Queen-to-Queen direct messaging (queens communicate via SharedMemory knowledge)
- WebSocket/SSE streaming to CLI (CLI polls via IPC)
- Authentication/authorization on IPC (localhost only)
- Multi-operator support (single operator assumed)

## Implementation Order

**Phase 1** (mailbox fix) → **Phase 2** (status) → **Phase 3** (inject) → **Phase 4** (keep-alive) → **Phase 5** (delivery)

Each phase is independently useful:
- **After Phase 1**: operator can read queen results via CLI
- **After Phase 2**: operator can see who's idle
- **After Phase 3**: operator can give new work to running swarm
- **After Phase 4**: swarm doesn't exit, waits for operator
- **After Phase 5**: full interactive loop

## Files to Modify

| File | Changes |
|------|---------|
| `src/nydus/ipc/protocol.rs` | Add QueenStatus, InjectTask, Shutdown, SwarmStatus variants |
| `src/nydus/ipc/mod.rs` | Fix MailboxSend/Read handlers, add new handlers, pass mailbox+handles |
| `src/nydus/mod.rs` | Add inject channel handling in run(), auto-push to outbox, keep-alive mode |
| `src/nydus/mailbox/mod.rs` | No changes needed (SwarmMailbox already works correctly) |
| `src/core/types.rs` | Possibly add InjectRequest type |
| `src/main.rs` | Add CLI commands: queen-status, inject, shutdown, swarm-status; add --keep-alive flag |

## Testing Plan

- [x] Unit test: MailboxSend routes to SwarmMailbox correctly
- [x] Unit test: MailboxRead reads from outbox/queen inbox
- [x] Unit test: QueenStatus returns correct status for all queens
- [x] Unit test: InjectTask creates task in DAG
- [x] Integration test: full loop — spawn swarm, inject task, read result, inject another, shutdown
- [x] Manual test: `hatchery spawn --keep-alive`, then in another terminal: `hatchery queen-status`, `hatchery inject "task"`, `hatchery mailbox read`, `hatchery shutdown`

## Success Criteria

1. **True message delivery**: `hatchery mailbox send --to queen:Q0 "message"` → queen receives message in next task assignment
2. **Real-time visibility**: `hatchery queen-status` shows accurate idle/working/dead state for all queens
3. **Dynamic task injection**: `hatchery inject "new task"` assigns to idle queen within 1 second
4. **Persistent operation**: Swarm with `--keep-alive` continues running after DAG completion, accepts new tasks
5. **Interactive loop**: Operator reads results → gives new instruction → reads results → repeat without restarting swarm

## Timeline Estimate

- **Phase 1**: 4-6 hours (core mailbox wiring)
- **Phase 2**: 2-3 hours (status query)
- **Phase 3**: 3-4 hours (task injection)
- **Phase 4**: 2-3 hours (keep-alive mode)
- **Phase 5**: 3-4 hours (message delivery to queens)

**Total**: 14-20 hours for complete implementation and testing
