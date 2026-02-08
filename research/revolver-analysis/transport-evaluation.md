# Transport & Communication Protocol Evaluation for Swarm Systems

**Date**: 2026-02-08
**Source**: Analysis of 22 research files from `research/revolver-research/transport-spec/`
**Scope**: Evaluation of IPC mechanisms, message formats, agent communication patterns for Hatchery swarm orchestration

## Executive Summary

Analyzed 14+ CLI coding agent projects to extract transport/mailbox patterns. Key finding: **No single canonical protocol exists**. Instead, successful systems combine 3-4 transport primitives matched to specific needs:

1. **Session-scoped mailbox** (not agent-named mailboxes)
2. **Correlation-ID multiplexing** for request/response over shared channels
3. **Durable-log + live-tail** pattern for reconnect/replay
4. **Typed event envelopes** with explicit protocol evolution

Best references for swarm transport design: **Goose**, **OpenCode**, **Plandex**, **AutoGen gRPC**.

---

## Core Primitives Catalog

### 1. Session-Scoped Mailbox Primitive

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Pattern**:
- Mailbox keyed by `session_id`/`task_id`, not static agent name
- Enables CLI agents that start/stop and reconnect
- Natural fit for ephemeral worker processes

**Evidence**:
- **Crush**: `session_id` + nested child sessions (`messageID$$toolCallID`)
- **OpenCode**: `SessionPrompt` busy/callback queue per session
- **Roo Code**: per-task message queue + IPC routing
- **Plandex**: `ActivePlan` subscription map by plan+branch
- **Cline**: per-task storage + correlation registry

**Why Essential for Swarms**:
- Hatchery agents are spawned on-demand and terminate after task completion
- Static agent names don't match dynamic spawn/death lifecycle
- Session-ID naturally represents work unit (task, delegation, sub-goal)

**Implementation Notes**:
- Use `session_id` as primary mailbox key
- Support parent-child session linkage (`parent_session_id`)
- Deterministic child IDs from parent context (like Crush's `messageID$$toolCallID`)

---

### 2. Correlation-ID Multiplexing Primitive

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Pattern**:
- Every request/response carries correlation ID (`request_id`, `messageId`, `tool_call_id`)
- Enables many logical conversations over one transport channel
- Maps to pending-response tables for RPC semantics

**Evidence**:
- **Cline**: `request_id` UUID per gRPC request + stream registry
- **Roo Code**: IPC `messageId` correlation
- **Continue**: message-id correlation maps + typed listeners
- **AutoGen gRPC**: `RpcRequest.request_id` + pending-response Future map
- **Goose ACP**: request correlation in JSON-RPC protocol

**Why Essential for Swarms**:
- Single orchestrator communicates with N workers over shared channel
- Avoids N separate socket connections (resource overhead)
- Simplifies reconnect logic (one channel, many logical streams)

**Implementation Notes**:
- Use UUID v7 or snowflake IDs for ordering + uniqueness
- Maintain `pending_requests[correlation_id] -> oneshot::Sender<Response>`
- Timeout logic per pending request, not per connection

---

### 3. Durable-Log + Live-Tail Primitive

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Pattern**:
- Persist every event/message first (or near-first)
- Fan out as live stream to connected clients
- Reconnect replays from durable state using cursor/offset

**Evidence**:
- **OpenHands**: `EventStream` = file-backed event store + in-memory queue + fanout
- **Plandex**: DB/repo durable state + `connectActive` snapshot for reconnect
- **AutoGPT**: Redis Streams with `XREAD` from `last_message_id` + blocking tail
- **Crush**: SQLite messages + in-memory pubsub for live updates
- **Goose**: SQLite sessions DB + SSE stream for live delivery

**Why Essential for Swarms**:
- Network partitions and process crashes are common
- Prevents state loss when transport disconnects
- Enables "catch-up" without re-running entire task

**Implementation Notes**:
- Write-ahead pattern: persist, then enqueue for live delivery
- Store sequence number or event ID for resume cursors
- Separate durable store from live channel (SQLite + in-memory bus)

---

### 4. Typed Event Envelope Primitive

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Pattern**:
- Explicit typed enums for all events/messages
- Shared schema between producer/consumer
- Enables protocol versioning and evolution

**Evidence**:
- **Plandex**: `StreamMessageType` enum with explicit variants
- **Roo Code**: `TaskEvent` + `RooCodeEventName` typed schemas
- **Goose**: `AgentEvent` / `ReplyEvent` enums
- **OpenHands**: typed event hierarchy with `event_name` discriminator
- **AutoGen**: envelope types (`SendMessageEnvelope`, `PublishMessageEnvelope`, `ResponseMessageEnvelope`)

**Why Essential for Swarms**:
- Prevents message schema drift across agent versions
- Enables forward/backward compatibility checks
- Documents protocol contracts explicitly

**Implementation Notes**:
- Use Rust enums with serde `#[serde(tag = "type")]`
- Version envelope schema separately from payload
- Consider protobuf for cross-language compatibility

---

### 5. Fanout Subscription Primitive

**Rating**: GOOD IDEA — worth considering

**Pattern**:
- Multiple consumers subscribe to one producer stream
- Each has isolated queue/buffer
- Producer doesn't block on slow consumer

**Evidence**:
- **Plandex**: per-subscription FIFO queue + condition variable
- **OpenHands**: `EventStream` subscribers with per-subscriber worker thread
- **AutoGPT**: Redis Pub/Sub fanout + WS connection manager
- **Cline**: subscription sets for state/UI/checkpoint streams

**Why Useful for Swarms**:
- Orchestrator broadcasts state to multiple workers
- UI observers don't block execution workers
- Enables "observe-only" clients

**Cautions**:
- Requires explicit backpressure policy (drop, buffer, block?)
- Memory overhead grows with subscriber count
- Consider fanout only for broadcast events, not targeted messages

---

### 6. Backpressure Strategy Primitive

**Rating**: GOOD IDEA — worth considering

**Pattern**:
- Explicit policy when consumer is slow
- Strategies: drop-on-full, buffer+coalesce, replayable stream

**Evidence**:
- **Crush**: drop event if subscriber channel full (bufferSize=64)
- **Plandex**: buffer + coalesce into `multi` frames, rate-limited at 70ms
- **AutoGPT**: Redis Streams as durable buffer with replay
- **OpenHands**: per-subscriber worker serializes delivery

**Why Useful for Swarms**:
- Slow UI client shouldn't block fast worker execution
- Network hiccups shouldn't halt entire swarm
- Enables correctness vs latency tradeoffs

**Implementation Notes**:
- For critical messages (task assignments, results): block or fail-fast
- For telemetry/logs: drop or coalesce
- For state updates: latest-value semantics (skip intermediate)

---

### 7. Child-Session Delegation Primitive

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Pattern**:
- Subagent call creates child session/task with parent linkage
- Durable audit trail and resumability
- Hierarchical task tree

**Evidence**:
- **Crush**: `messageID$$toolCallID` deterministic child session ID
- **OpenCode**: `task` tool creates child session with `parentID`
- **Roo Code**: parent/child task metadata + delegation lifecycle events
- **Goose**: subagent tool creates dedicated session with `SubAgent` type

**Why Essential for Swarms**:
- Swarms naturally form delegation trees (coordinator -> workers -> sub-workers)
- Parent-child linkage enables cost tracking, error propagation, cancellation
- Durable hierarchy supports resume after crashes

**Implementation Notes**:
- Store `parent_session_id` in session metadata
- Generate deterministic child IDs: `{parent_id}/{tool_call_id}` or hash
- Propagate cancellation signals down tree
- Aggregate results/costs up tree

---

### 8. Runtime Router Primitive

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Pattern**:
- Single orchestrator accepts inbound events/commands
- Routes to sessions/tasks/workers by session_id
- Minimum layer required for live coordination without direct agent-to-agent sockets

**Evidence**:
- **OpenCode**: `Bus` + server routing
- **Plandex**: `ActivePlan` registry + subscribe/unsubscribe
- **Roo Code**: API event emitter + IPC broadcast
- **Cline**: gRPC handler + request registry
- **AutoGen**: runtime envelope queue + host-worker gRPC router

**Why Essential for Swarms**:
- Central router simplifies topology (star vs mesh)
- Enables dynamic agent spawn/death without peer discovery
- Natural bottleneck for observability/metrics

**Implementation Notes**:
- Use concurrent hashmap: `active_sessions: DashMap<SessionId, SessionHandle>`
- Route by session_id extracted from envelope
- Handle session-not-found errors gracefully (reconnect case)

---

### 9. Heartbeat + Liveness Primitive

**Rating**: GOOD IDEA — worth considering

**Pattern**:
- Keepalive frames and timeout-based disconnect detection
- Enables dead-connection cleanup

**Evidence**:
- **Plandex**: 5s heartbeat / 16s timeout on HTTP chunked stream
- **OpenCode**: SSE heartbeat every 30s
- **Goose**: SSE `Ping` frames + `CancellationToken`
- **Cline**: implicit via gRPC stream health

**Why Useful for Swarms**:
- Detects zombie workers without waiting for next message
- Enables timely task reassignment
- Prevents resource leaks from abandoned connections

**Cautions**:
- Adds complexity to protocol state machine
- Requires timeout tuning (too short = false positives, too long = slow detection)

---

### 10. Transport/State Split Primitive

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Pattern**:
- Live channel is ephemeral
- Authoritative state is separate durable store
- Can restart transport without losing conversation truth

**Evidence**:
- Seen in almost all serious implementations
- **Goose**: SQLite session store + SSE transport
- **OpenHands**: file-backed event log + Socket.IO transport
- **Crush**: SQLite + in-memory pubsub
- **LangGraph**: checkpointer abstraction + SSE API

**Why Essential for Swarms**:
- Orchestrator restart shouldn't lose in-flight work
- Transport failures are common (network, OOM, crashes)
- Enables blue/green orchestrator deployments

**Implementation Notes**:
- SQLite with WAL mode for single-writer durability
- Or PostgreSQL for multi-writer deployments
- Transport reconnect fetches latest state from DB
- Use event sourcing pattern for audit trail

---

## Transport Technology Evaluation

### HTTP SSE (Server-Sent Events)

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Used By**: Goose, OpenCode, Plandex (custom framing), LangGraph SDK, OpenHands v1

**Advantages**:
- Simple: one-way push over HTTP
- Built-in reconnect with `Last-Event-ID`
- Works through HTTP proxies and firewalls
- Standard `text/event-stream` content type

**Disadvantages**:
- Unidirectional (need separate POST for commands)
- No built-in backpressure signal
- Browser connection limits (6 per domain)

**Best For**:
- Orchestrator -> UI telemetry/logs
- Long-running task progress updates
- Read-only observers

**Example from Goose**:
```
GET /reply HTTP/1.1
Accept: text/event-stream

event: Message
data: {"content": "..."}
id: 42

event: Ping
data: {}
```

---

### WebSocket

**Rating**: GOOD IDEA — worth considering

**Used By**: AutoGPT, OpenHands v1, Goose ACP (optional), Continue (TCP variant)

**Advantages**:
- Bidirectional: full-duplex communication
- Lower overhead than HTTP request/response
- Native browser support

**Disadvantages**:
- More complex state machine (handshake, pings, close frames)
- Requires explicit heartbeat implementation
- Harder to debug than HTTP

**Best For**:
- Interactive UI with frequent bidirectional messages
- Real-time collaboration features
- High message rate scenarios

---

### gRPC Bidirectional Streams

**Rating**: GOOD IDEA — worth considering (but adds complexity)

**Used By**: AutoGen (distributed runtime), Cline (standalone mode)

**Advantages**:
- Strong typing via protobuf schemas
- Efficient binary encoding
- Built-in load balancing and retry semantics
- Excellent for distributed polyglot systems

**Disadvantages**:
- Heavy dependency (protoc, generated code)
- Poor browser support (requires grpc-web proxy)
- Debugging more complex than HTTP

**Best For**:
- Distributed multi-language orchestrator/worker architecture
- High throughput inter-service communication
- When protobuf schema governance is already in place

**Example from AutoGen**:
```protobuf
service AgentRpc {
  rpc OpenChannel(stream Message) returns (stream Message);
}

message Message {
  oneof msg {
    RpcRequest request = 1;
    RpcResponse response = 2;
    CloudEvent event = 3;
  }
}
```

---

### Local IPC (Unix domain sockets / node-ipc)

**Rating**: GOOD IDEA — worth considering (for local-only swarms)

**Used By**: Roo Code (node-ipc), Continue (stdio/TCP JSONL)

**Advantages**:
- Lowest latency for same-machine communication
- No network stack overhead
- Good security boundary (filesystem permissions)

**Disadvantages**:
- Platform-specific (Unix sockets on Linux/Mac, named pipes on Windows)
- Doesn't support remote workers
- Harder to inspect/debug than HTTP

**Best For**:
- Single-machine swarms (all agents on localhost)
- Extension <-> language server patterns
- Low-latency coordination

---

### HTTP Chunked Custom Framing

**Rating**: BAD IDEA — avoid this (use SSE instead)

**Used By**: Plandex (`@@PX@@` separator)

**Why Avoid**:
- Reinvents SSE without standardization
- Client must implement custom frame parser
- No standard tooling support
- Harder to debug than SSE

**Only Use If**:
- You need custom binary framing
- SSE event-id reconnect semantics don't fit
- You're already invested in this pattern

---

### RabbitMQ / Redis Pub-Sub / Redis Streams

**Rating**: IRRELEVANT — not applicable to single-orchestrator swarms

**Used By**: AutoGPT (RabbitMQ commands, Redis Pub/Sub events, Redis Streams chat)

**Why Irrelevant**:
- Adds external service dependency (operational complexity)
- Overkill for single-orchestrator architecture
- Useful for multi-datacenter or polyglot distributed systems
- Hatchery is Rust-only, single-orchestrator, shared-memory friendly

**Consider Only If**:
- You need true distributed orchestration (multiple orchestrator instances)
- You're building multi-language agent ecosystem
- You need durable work queues surviving orchestrator death

---

### Stdin/Stdout JSONL (ACP-style)

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Used By**: OpenCode ACP, Goose ACP, Continue (stdio mode)

**Advantages**:
- Simple: line-delimited JSON over stdio
- Works with any subprocess
- Easy to test: pipe files through command
- No network config required

**Disadvantages**:
- Unidirectional per stream (need separate stdin/stdout)
- Buffering issues if not flushed
- No built-in session recovery

**Best For**:
- Agent subprocess protocol (orchestrator spawns worker)
- Command-line agent invocation
- Test/debug harness

**Implementation**:
```rust
// Orchestrator writes to worker stdin
writeln!(worker.stdin, "{{\"type\":\"task\",\"id\":\"123\",...}}")?;

// Orchestrator reads from worker stdout
for line in BufReader::new(worker.stdout).lines() {
    let event: TaskEvent = serde_json::from_str(&line?)?;
    handle_event(event)?;
}
```

---

## Mailbox Architecture Patterns

### Pattern A: In-Process Async Channels

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Used By**: MetaGPT, CAMEL, LangGraph (core), AutoGen (single-threaded runtime)

**Description**:
- Each agent/worker has `tokio::sync::mpsc` or `async_channel` mailbox
- Orchestrator routes messages by looking up agent handle
- Pure in-process communication (no serialization)

**Advantages**:
- Zero-copy message passing
- Type-safe Rust enums
- Natural fit for tokio async runtime
- Extremely low latency

**Disadvantages**:
- Doesn't support multi-process workers
- No durability (lost on crash)

**Best For**:
- Hatchery **Swarm Host** mode (shared-memory workers)
- Fast coordination between Sonnet workers in same process

**Implementation**:
```rust
struct WorkerHandle {
    inbox: mpsc::UnboundedSender<WorkerMessage>,
    task: JoinHandle<()>,
}

struct Orchestrator {
    workers: DashMap<SessionId, WorkerHandle>,
}

impl Orchestrator {
    async fn route_message(&self, session_id: SessionId, msg: WorkerMessage) {
        if let Some(worker) = self.workers.get(&session_id) {
            worker.inbox.send(msg).ok();
        }
    }
}
```

---

### Pattern B: Session-Keyed Routing + Durable State

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Used By**: Goose, Crush, OpenHands, Roo Code

**Description**:
- Orchestrator maintains `active_sessions: HashMap<SessionId, SessionState>`
- Messages route by extracting session_id from envelope
- Session state persisted to DB (SQLite/Postgres)
- Live transport separate from durable state

**Advantages**:
- Survives orchestrator restart
- Natural multi-tenant isolation
- Clean separation of concerns

**Disadvantages**:
- DB write overhead on every message (mitigate with write-batching)
- More complex than pure in-memory

**Best For**:
- Hatchery **Brood Lord** mode (orchestrator of orchestrators)
- Long-running tasks that outlive orchestrator lifetime

---

### Pattern C: Event-Sourcing with Replay

**Rating**: GOOD IDEA — worth considering (for audit/debug heavy scenarios)

**Used By**: OpenHands, AutoGPT (Redis Streams), Plandex (implicit via DB log)

**Description**:
- Every message/event stored in append-only log
- Session state derived by replaying events
- Reconnect provides last_event_id, replays from there

**Advantages**:
- Perfect audit trail
- Time-travel debugging
- Idempotent replay (crashes are harmless)

**Disadvantages**:
- Storage grows unbounded (need compaction/snapshots)
- Replay latency on long sessions
- More complex than state-snapshot pattern

**Best For**:
- Compliance/audit requirements
- Complex debugging scenarios
- Research/analysis workflows

---

### Pattern D: Per-Role Mailbox (MetaGPT/CAMEL style)

**Rating**: IRRELEVANT — not applicable to ephemeral worker swarms

**Used By**: MetaGPT, CAMEL workforce

**Description**:
- Each role has named mailbox (e.g., "ProductManager", "Engineer")
- Messages routed by role tags
- Round-based scheduling (all roles process, then next round)

**Why Irrelevant**:
- Assumes static set of roles known at design-time
- Hatchery spawns dynamic anonymous workers
- Round-based scheduling conflicts with async execution

**Consider Only If**:
- Building multi-role simulation (not task execution)
- Roles are stable and well-defined

---

## Message Format Evaluation

### JSON-RPC 2.0

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Used By**: Goose ACP, Continue, OpenCode ACP

**Advantages**:
- Standard specification (JSON-RPC 2.0)
- Explicit request/response correlation via `id`
- Notification support (one-way messages with `id: null`)
- Wide tooling support

**Disadvantages**:
- Verbose compared to binary formats
- Requires JSON parsing overhead

**Example**:
```json
// Request
{"jsonrpc": "2.0", "method": "execute_task", "params": {"task": "..."}, "id": 1}

// Response
{"jsonrpc": "2.0", "result": {"status": "completed"}, "id": 1}

// Notification (no response expected)
{"jsonrpc": "2.0", "method": "progress_update", "params": {"percent": 50}}
```

**Best For**:
- Agent subprocess protocol (stdin/stdout JSONL)
- HTTP API endpoints

---

### Custom Typed Envelopes (Rust serde)

**Rating**: EXCELLENT IDEA — must adopt for swarm

**Used By**: Internal message passing in most Rust projects

**Advantages**:
- Type-safe at compile time
- Zero-copy deserialization with `serde`
- Pattern matching on message type
- Easy to version with `#[serde(tag = "type")]`

**Example**:
```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum WorkerMessage {
    TaskAssignment { task_id: TaskId, prompt: String },
    TaskResult { task_id: TaskId, result: String },
    Heartbeat { timestamp: u64 },
}
```

**Best For**:
- In-process async channel messages
- Shared-memory worker communication

---

### Protobuf (gRPC)

**Rating**: IRRELEVANT — overkill for single-language Rust swarm

**Used By**: AutoGen distributed runtime, Cline standalone

**Why Irrelevant**:
- Adds build complexity (protoc codegen)
- No advantage over serde for Rust-to-Rust communication
- Useful only for polyglot systems

---

## Anti-Patterns to Avoid

### 1. Background Run Mode (run_in_background: true)

**Rating**: BAD IDEA — avoid this

**Why Avoid**:
- Requires constant polling to check completion
- Hides progress from user
- Complicates error handling (when to check?)
- Loses ordering guarantees

**Seen In**: Task/agent systems that misuse background execution

**Instead**: Use async tasks with proper completion signaling

---

### 2. Direct Agent-to-Agent Sockets

**Rating**: BAD IDEA — avoid this (use central router)

**Why Avoid**:
- N^2 connection scaling problem
- Complex peer discovery
- Hard to observe/debug (no central visibility)
- Race conditions on connection failures

**Instead**: Use central orchestrator as message router (star topology)

---

### 3. Polling Filesystem for State

**Rating**: BAD IDEA — avoid this (use event streams)

**Why Avoid**:
- High latency (seconds until poll)
- Wasted CPU on empty polls
- Doesn't scale to many agents
- Filesystem locks become bottleneck

**Seen In**: Early versions of some CLI agents

**Instead**: Use push-based event streams (SSE, WebSocket, async channels)

---

### 4. Global Shared Mutable State

**Rating**: BAD IDEA — avoid this (use message passing)

**Why Avoid**:
- Race conditions and deadlocks
- Hard to reason about ordering
- Prevents multi-process scaling

**Instead**: Message passing with actor model (each agent owns its state)

---

### 5. Synchronous RPC Everywhere

**Rating**: BAD IDEA — avoid this (use async + timeouts)

**Why Avoid**:
- Blocks caller thread
- No way to cancel in-flight requests
- Cascading failures (slow worker blocks orchestrator)

**Instead**: Async requests with explicit timeouts and cancellation

---

## Recommended Architecture for Hatchery

### Transport Stack

```
┌─────────────────────────────────────────────────────┐
│ UI/External Clients (SSE /events/{session_id})     │
└─────────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────┐
│ HTTP Server (axum)                                  │
│  - SSE endpoint for live events                     │
│  - REST API for commands (POST /sessions/{id}/task) │
│  - Correlation-ID per request                       │
└─────────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────┐
│ Orchestrator (Runtime Router)                       │
│  - active_sessions: DashMap<SessionId, SessionState>│
│  - Routes by session_id from envelope               │
│  - Spawns workers on-demand                         │
└─────────────────────────────────────────────────────┘
                        │
          ┌─────────────┴─────────────┐
          ▼                           ▼
┌──────────────────┐       ┌──────────────────┐
│ Worker (Sonnet)  │       │ Worker (Sonnet)  │
│  - Stdin/Stdout  │       │  - In-process    │
│    JSONL (ACP)   │       │    mpsc channel  │
└──────────────────┘       └──────────────────┘
          │                           │
          └─────────────┬─────────────┘
                        ▼
┌─────────────────────────────────────────────────────┐
│ Durable State (SQLite WAL)                          │
│  - sessions (id, parent_id, status, metadata)       │
│  - events (session_id, sequence, type, payload)     │
└─────────────────────────────────────────────────────┘
```

### Message Format

**External Protocol (HTTP/SSE)**: JSON-RPC 2.0 over SSE data frames

```json
// Inbound command (HTTP POST)
{
  "jsonrpc": "2.0",
  "method": "execute_task",
  "params": {
    "session_id": "abc-123",
    "prompt": "Research topic X"
  },
  "id": 42
}

// Outbound event (SSE data frame)
event: task_progress
data: {"jsonrpc": "2.0", "method": "task_progress", "params": {"session_id": "abc-123", "percent": 50}}
id: 100
```

**Internal Protocol (Rust)**: Typed enums with serde

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum OrchestratorMessage {
    TaskAssignment {
        session_id: SessionId,
        task: Task,
        correlation_id: u64,
    },
    TaskResult {
        session_id: SessionId,
        result: String,
        correlation_id: u64,
    },
    Heartbeat {
        session_id: SessionId,
    },
    CancelTask {
        session_id: SessionId,
    },
}
```

### Core Primitives to Implement

1. **Session-scoped routing**: `active_sessions: DashMap<SessionId, SessionHandle>`
2. **Correlation IDs**: Track pending requests with `pending: DashMap<CorrelationId, oneshot::Sender<Response>>`
3. **Durable log**: Write all events to SQLite before emitting to live channels
4. **Child-session delegation**: Store `parent_session_id` + deterministic child ID generation
5. **Typed envelopes**: Use Rust enums with `#[serde(tag = "type")]`
6. **SSE transport**: For UI/observer clients (read-only telemetry)
7. **Stdin/stdout JSONL**: For subprocess worker agents (ACP protocol)
8. **In-process channels**: For shared-memory worker agents (tokio mpsc)

---

## References & Evidence

All findings source-grounded in:
- `research/revolver-research/transport-spec/projects-v2/*.md` (14 project specs)
- `research/revolver-research/transport-spec/protocol-primitives-catalog-v2.md`
- `research/revolver-research/transport-spec/transport-mailbox-matrix-v2.md`
- `research/revolver-research/transport-spec/deep-dive-queue-v2.md`

Top references for implementation details:
1. **Goose**: `crates/goose/src/agents/agent.rs`, `crates/goose-server/src/routes/reply.rs`
2. **OpenCode**: `src/bus/index.ts`, `src/session/prompt.ts`, `src/acp/agent.ts`
3. **Plandex**: `app/server/types/active_plan.go`, `app/shared/stream.go`
4. **AutoGen**: `python/packages/autogen-ext/src/autogen_ext/runtimes/grpc/`
5. **Crush**: `internal/pubsub/broker.go`, `internal/agent/agent_tool.go`

---

## Conclusion

No need to invent a new protocol. The winning pattern is:

1. **HTTP + SSE** for external clients (UI, CLI observers)
2. **Stdin/stdout JSONL** for subprocess workers (ACP protocol)
3. **In-process async channels** for shared-memory workers
4. **Session-scoped routing** + **correlation IDs** for multiplexing
5. **SQLite write-ahead log** for durability
6. **Typed Rust enums** for internal messages, **JSON-RPC** for wire protocol

This combination is battle-tested across Goose, OpenCode, Plandex, and gives Hatchery the right balance of simplicity, durability, and performance.
