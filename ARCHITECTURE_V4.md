# Hatchery V4: Modular Building-Block Architecture

## Evolution History

### V1: Monolithic (Early 2026)
Single `Nydus` coordinator handled everything: task assignment, spawn heuristics, merge validation, strategic decisions. Simple but rigid.

### V2: Nydus + Queen Split (Mid Jan 2026)
Extracted Queen (AI agent wrapper) from Nydus. Nydus became transport/scheduling layer. Introduced git worktree isolation via `Safety` module.

### V3: Overmind Split (Late Jan 2026)
Split Nydus into 4 specialized entities:
- **Nydus**: Transport + mechanical scheduling
- **SwarmPool**: Spawn heuristics (Zerg Rush, elastic pool, retry)
- **Overlord**: Hybrid merge validator (parsers → checks → optional LLM)
- **Overmind**: Strategic LLM coordinator (escalation handler)

Still hardcoded orchestration logic, but clear separation of concerns.

### V4: Modular Building Blocks (Feb 2026) — **CURRENT**
Complete rewrite into composable trait-based architecture. 50+ standalone modules across 8 categories. Runtime composition via `PipelineBuilder`. Existing V3 components remain functional and integrate with new modules.

## Core Design Principles

### 1. Trait-Based Composability
Every orchestration concern is a trait:
```rust
Topology        // HOW agents organize
Decomposition   // HOW tasks break down
Communication   // HOW agents talk
Memory          // HOW state persists
Scheduling      // WHEN things run
Resilience      // ERROR handling
Scaling         // SCALE management
```

Each trait has 3-10 production implementations. All `Send + Sync` for async/parallel execution.

### 2. Runtime Composition
No compile-time coupling. Modules compose at runtime via `PipelineBuilder`:

```rust
let pipeline = PipelineBuilder::new()
    .topology(CentralizedTopology::new(config))
    .decomposition(DagDecomposition::new(config))
    .communication(MessageBusCommunication::new(config))
    .memory(MultiTierMemory::new())
    .scheduling(HybridScheduling::new(config))
    .resilience(RetryResilience::new(config))
    .scaling(ElasticPoolScaling::new(config))
    .build()?;
```

No trait objects in user code — builder handles boxing internally.

### 3. Hot-Swap Capability
Components swap at runtime without restart:

```rust
pipeline.swap_topology(PeerToPeerTopology::new(config))?;
pipeline.swap_memory(RagMemory::new(config))?;
```

Useful for A/B testing, adaptive workflows, research experiments.

### 4. Preset Pipelines
Common patterns pre-configured as presets:

```rust
carousel_preset()   // Structured multi-phase (research → implement → test → debug)
ralph_preset()      // Autonomous iterative (PRD checkboxes)
blackboard_preset() // Exploratory research (agent self-selection)
consensus_preset()  // High-stakes decisions (multi-agent agreement)
swarm_preset()      // Embarrassingly parallel (work stealing)
minimal_preset()    // Simple single-agent
```

Presets are starting points — customize via builder methods.

## The 8 Module Categories

### 1. Topology (7 modules)

**Trait**: How agents are organized and tasks assigned.

```rust
trait Topology: Send + Sync {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>>;
    fn handle_completion(&mut self, agent: AgentId, task: TaskId, result: TaskResult);
    fn agents(&self) -> Vec<AgentId>;
    fn add_agent(&mut self, agent: AgentId) -> Result<()>;
    fn remove_agent(&mut self, agent: AgentId) -> Result<()>;
}
```

| Module | Strategy | Best For |
|--------|----------|----------|
| **Centralized** | Single coordinator, round-robin assignment | 3-8 agents, simple coordination |
| **Hierarchical** | Tree structure, auto-rebalance on load | 10-100 agents, region/team organization |
| **PeerToPeer** | Flat swarm, consensus voting | Research, democratic decisions |
| **Blackboard** | Shared semantic space, self-selection | Expertise matching, exploratory tasks |
| **GraphDAG** | DAG-based with Kahn's algorithm | Explicit dependencies, ordered execution |
| **Conversational** | Multi-round debate before assignment | Critical decisions, design reviews |
| **Hybrid** | Compose multiple topologies, task-based routing | Mixed workloads |

**Implementation details**:
- Centralized: `VecDeque` for round-robin, `HashMap<AgentId, TaskId>` for tracking
- Hierarchical: Tree with `parent_id`, auto-split when coordinator > threshold
- PeerToPeer: Consensus via voting (`ConsensusProtocol` enum: Majority/Approval/Unanimity)
- Blackboard: Semantic matching via TF-IDF similarity (from RAG memory)
- GraphDAG: Kahn's topological sort, cycle detection via DFS
- Conversational: Multi-round voting with configurable rounds (default 3)
- Hybrid: Task property matcher → route to appropriate topology

### 2. Decomposition (6 modules)

**Trait**: How tasks are broken down into subtasks.

```rust
trait Decomposition: Send + Sync {
    fn decompose(&mut self, task: Task) -> Result<Vec<Task>>;
    fn can_decompose(&self, task: &Task) -> bool;
    fn add_dynamic_task(&mut self, parent: &TaskId, task: Task) -> Result<()>;
    fn ready_tasks(&self) -> Vec<TaskId>;
    fn mark_complete(&mut self, task_id: &TaskId) -> Result<()>;
}
```

| Module | Strategy | Best For |
|--------|----------|----------|
| **DAG** | Static dependency graph, cycle detection | Known dependencies, build systems |
| **HTN** | Hierarchical Task Networks (methods + operators) | Planning, robotics, complex domains |
| **TDAG** | Dynamic DAG with runtime task addition + budget tracking | Adaptive workflows, research |
| **Emergent** | LLM-driven runtime task creation with guard rails | Exploratory tasks, unknown domain |
| **RoleBased** | Skill matching (Research/Implement/Test/Debug) | Multi-stage pipelines (Carousel) |
| **Capability** | Agent card matching, proficiency scoring | Expertise-based assignment |

**Implementation details**:
- DAG: `HashMap<TaskId, Vec<TaskId>>` for deps, DFS cycle detection, Kahn's ready queue
- HTN: Methods (abstract) → Operators (primitive). Recursive decomposition.
- TDAG: Extends DAG with `add_dynamic_task()`, budget tracking per task
- Emergent: LLM generates subtasks, guard rails via max depth (5), max tasks (50), budget limits
- RoleBased: Tags (research/implement/test/debug) → match agent skills
- Capability: Agent cards with proficiency scores (0.0-1.0), cosine similarity matching

### 3. Communication (8+4 modules)

**Trait**: How agents exchange messages.

```rust
trait Communication: Send + Sync {
    fn send(&mut self, from: AgentId, to: AgentId, msg: Message) -> Result<()>;
    fn broadcast(&mut self, from: AgentId, msg: Message) -> Result<()>;
    fn subscribe(&mut self, agent: AgentId, topic: String) -> Result<()>;
    fn publish(&mut self, from: AgentId, topic: String, msg: Message) -> Result<()>;
    fn receiver(&self, agent: AgentId) -> Option<Receiver<Message>>;
}
```

| Module | Strategy | Best For |
|--------|----------|----------|
| **Direct** | Point-to-point via `tokio::mpsc` | Simple coordination, low overhead |
| **Broadcast** | Topic-based pub/sub via `tokio::broadcast` | Event notifications, status updates |
| **Blackboard** | Shared knowledge space (write/read/query) | Collaborative research, semantic matching |
| **MessageBus** | Central routing hub with audit logging | Production systems, debugging |
| **Handoff** | Task ownership transfer with chain tracking | Multi-stage workflows, responsibility tracking |
| **ContractNet** | Call-for-proposals + bidding protocol | Market-based allocation, load balancing |
| **RippleEffect** | Signal propagation with strength attenuation | Cascading updates, influence spreading |
| **Protocols** | MCP/A2A/ACP/ANP adapters | Interop with external systems |

**Protocol adapters** (4 modules in `communication/protocols/`):
- **MCP** (Model Context Protocol): Anthropic's standard for tool/resource sharing
- **A2A** (Agent-to-Agent Protocol): OpenAI's agent communication spec
- **ACP** (Agent Communication Protocol): Generic FIPA-style ACL
- **ANP** (Agent Negotiation Protocol): Contract Net Protocol implementation

**Implementation details**:
- Direct: `HashMap<AgentId, Sender<Message>>`, 1:1 channels
- Broadcast: `tokio::broadcast::channel(1000)`, clone receivers per subscriber
- Blackboard: Semantic space via TF-IDF, concurrent reads via `RwLock`
- MessageBus: Audit log (SQLite), message routing table, priority queues
- Handoff: Chain tracking via `Vec<AgentId>`, timestamps, status (Pending/InProgress/Complete)
- ContractNet: Bid collection, winner selection (lowest cost/highest quality/weighted), timeout handling
- RippleEffect: Attenuation function `strength * 0.7^distance`, flood control

### 4. Memory (9 modules)

**Trait**: How state is persisted and queried.

```rust
trait Memory: Send + Sync {
    fn insert(&mut self, key: String, value: MemoryValue) -> Result<()>;
    fn get(&self, key: &str) -> Option<MemoryValue>;
    fn query(&self, query: MemoryQuery) -> Vec<MemoryValue>;
    fn evict(&mut self, key: &str) -> Result<()>;
    fn snapshot(&self) -> Result<MemorySnapshot>;
    fn restore(&mut self, snapshot: MemorySnapshot) -> Result<()>;
}
```

| Module | Strategy | Best For |
|--------|----------|----------|
| **SharedState** | Simple KV store with versioning (RwLock) | Single-agent, small state |
| **Conversation** | Per-agent history with auto-summarization | Multi-round dialogues, LLM context |
| **MultiTier** | 4-tier: short/long/entity/contextual | Complex workflows, mixed access patterns |
| **Document** | PRD/design docs storage with versioning | Structured workflows (Carousel) |
| **Isolated** | Per-agent namespaced (no cross-contamination) | Parallel tasks, security |
| **Session** | Session tracking for recovery (heartbeat) | Long-running tasks, crash recovery |
| **Collaborative** | Dual-tier with access control (private/shared) | Team coordination, privacy |
| **Ontology** | In-memory knowledge graph (entities + relations) | Semantic reasoning, domain modeling |
| **RAG** | TF-IDF similarity search with embedding cache | Research, codebase search |

**Implementation details**:
- SharedState: `HashMap<String, MemoryValue>`, version counter, `RwLock` for concurrent reads
- Conversation: Per-agent `Vec<Message>`, auto-summarize when > 100 messages (via LLM or extractive)
- MultiTier: Short (last 10), Long (summarized old), Entity (key facts), Contextual (task context)
- Document: `HashMap<DocId, Document>`, versioning via git-style snapshots
- Isolated: `HashMap<AgentId, HashMap<String, MemoryValue>>`, zero sharing
- Session: Heartbeat tracking, stale detection (no update > 60s), recovery via snapshot
- Collaborative: Private (per-agent) + Shared (all agents), access control via ACL
- Ontology: `HashMap<Entity, Vec<Relation>>`, SPARQL-like query API
- RAG: TF-IDF via `HashMap<term, Vec<(doc_id, freq)>>`, cosine similarity

### 5. Scheduling (7 modules)

**Trait**: When tasks are executed.

```rust
trait Scheduling: Send + Sync {
    fn next_task(&mut self) -> Option<TaskId>;
    fn notify_completion(&mut self, task_id: TaskId);
    fn next_interval(&self) -> Option<Duration>;
    fn should_terminate(&self) -> bool;
}
```

| Module | Strategy | Best For |
|--------|----------|----------|
| **EventDriven** | Instant dispatch on task ready | Latency-sensitive, reactive systems |
| **TimerBased** | Fixed-interval heartbeat (default 1s) | Periodic maintenance, monitoring |
| **Hybrid** | Event-driven + periodic combined | Production systems (react + maintain) |
| **LoadBalancing** | RoundRobin/LeastLoaded/WeightedRandom/PowerOfTwo | Load distribution, fairness |
| **Priority** | BinaryHeap + starvation detection | SLA-based, urgent tasks |
| **LLMRealtime** | Preemption + SLA monitoring (p50/p95/p99) | LLM inference, GPU sharing |
| **WorkStealing** | Global + local queues, peer stealing | Parallel tasks, CPU-bound |

**Implementation details**:
- EventDriven: `VecDeque` for ready tasks, zero polling
- TimerBased: `tokio::time::interval(1s)`, periodic `try_schedule()` calls
- Hybrid: Event queue + timer, drain events first, fallback to timer
- LoadBalancing: Strategy enum → algorithm impl, agent load tracking
- Priority: `BinaryHeap<(priority, TaskId)>`, starvation via age threshold (5 ticks → boost priority)
- LLMRealtime: Preemption via `tokio::select!`, SLA tracking via histograms
- WorkStealing: Per-agent local queue (LIFO), global queue (FIFO), random peer selection

### 6. Resilience (7 modules)

**Trait**: How errors are handled.

```rust
trait Resilience: Send + Sync {
    fn handle_failure(&mut self, task: &TaskId, error: &str) -> Result<RecoveryAction>;
    fn check_health(&self, agent: &AgentId) -> HealthStatus;
    fn plan_recovery(&mut self, agents: &[AgentId]) -> Vec<RecoveryAction>;
    fn record_failure(&mut self, task: &TaskId, error: String);
}
```

| Module | Strategy | Best For |
|--------|----------|----------|
| **Retry** | 4 backoff strategies (Immediate/Linear/Exponential/Jitter) | Transient failures, rate limits |
| **Recovery** | Session-based, heartbeat monitoring, stall detection | Long-running tasks, crash recovery |
| **Degradation** | 5 fallback strategies per task (retry/skip/partial/alternative/escalate) | SLA-based workflows |
| **Consensus** | Voting (Majority/Approval/Unanimity/SuperMajority) | Byzantine failures, critical decisions |
| **HITL** | Human-in-the-loop with auto-approve threshold | High-stakes tasks, compliance |
| **CircuitBreaker** | State machine: Closed→Open→HalfOpen | Cascading failures, dependency protection |
| **FailureTracking** | Analytics, MTBF, categorization (transient/permanent/unknown) | Observability, debugging |

**Implementation details**:
- Retry: Backoff strategies via enum, max attempts (default 3), exponential base (2.0), jitter (±20%)
- Recovery: Heartbeat via `Instant::now()`, stale threshold (60s), recovery via snapshot restore
- Degradation: Strategy per task, fallback chain (primary → alternative → escalate)
- Consensus: Voting protocol (weighted votes, quorum), Byzantine fault tolerance (f < n/3)
- HITL: Auto-approve if confidence > threshold (0.9), escalate to human otherwise, timeout (5min)
- CircuitBreaker: Failure threshold (5), reset timeout (30s), half-open trial (1 task)
- FailureTracking: Categorization via error message patterns, MTBF via exponential moving average

### 7. Scaling (6 modules)

**Trait**: How the agent pool grows/shrinks.

```rust
trait Scaling: Send + Sync {
    fn should_scale(&self, metrics: &ScalingMetrics) -> ScalingDecision;
    fn execute_scale(&mut self, decision: ScalingDecision) -> Result<Vec<ScalingAction>>;
}
```

| Module | Strategy | Best For |
|--------|----------|----------|
| **SmallScale** | 3-8 agents, simple thresholds | Current Hatchery setup |
| **MediumScale** | 10-100 agents, cooldown-based (30s) | Team-level coordination |
| **LargeScale** | 100-1K agents, hierarchical coordinators | Multi-team, regional |
| **VeryLargeScale** | 1K+ agents, P2P mesh with partitions | Datacenter-scale |
| **EdgeCloud** | Hybrid edge/cloud deployment | IoT, distributed inference |
| **ElasticPool** | Dynamic spawn/teardown, zerg rush mode | Burst workloads, cost optimization |

**Implementation details**:
- SmallScale: Fixed pool (3-8), scale up when `idle == 0 && ready > 0`, scale down when `idle > ready * 2`
- MediumScale: Cooldown timer (30s), gradual scaling (±2 agents/tick)
- LargeScale: Hierarchical coordinators (tree), auto-partition when coordinator > 20 agents
- VeryLargeScale: P2P mesh with consistent hashing, partition tolerance via gossip protocol
- EdgeCloud: Location-aware routing, bandwidth constraints, latency-based scheduling
- ElasticPool: Zerg rush (instant spawn 3-5 agents), teardown after idle timeout (300s)

### 8. Pipeline (3 modules)

**Runtime composition and execution.**

| Module | Purpose |
|--------|---------|
| **Builder** | Fluent API for composing pipelines |
| **Presets** | 6 pre-built configurations (carousel, ralph, blackboard, consensus, swarm, minimal) |
| **Runtime** | Execution engine with hot-swap capability |

**Builder API**:
```rust
impl PipelineBuilder {
    fn new() -> Self;
    fn topology(self, t: impl Topology + 'static) -> Self;
    fn decomposition(self, d: impl Decomposition + 'static) -> Self;
    fn communication(self, c: impl Communication + 'static) -> Self;
    fn memory(self, m: impl Memory + 'static) -> Self;
    fn scheduling(self, s: impl Scheduling + 'static) -> Self;
    fn resilience(self, r: impl Resilience + 'static) -> Self;
    fn scaling(self, s: impl Scaling + 'static) -> Self;
    fn name(self, name: impl Into<String>) -> Self;
    fn description(self, desc: impl Into<String>) -> Self;
    fn build(self) -> Result<Pipeline>;
    fn build_partial(self) -> Result<Pipeline>; // Uses defaults for missing
}
```

**Runtime hot-swap**:
```rust
impl Pipeline {
    async fn run(&mut self) -> Result<()>;
    fn swap_topology(&mut self, t: impl Topology + 'static) -> Result<()>;
    fn swap_decomposition(&mut self, d: impl Decomposition + 'static) -> Result<()>;
    fn swap_communication(&mut self, c: impl Communication + 'static) -> Result<()>;
    fn swap_memory(&mut self, m: impl Memory + 'static) -> Result<()>;
    fn swap_scheduling(&mut self, s: impl Scheduling + 'static) -> Result<()>;
    fn swap_resilience(&mut self, r: impl Resilience + 'static) -> Result<()>;
    fn swap_scaling(&mut self, s: impl Scaling + 'static) -> Result<()>;
}
```

Hot-swap is **non-destructive**: existing tasks continue, only new tasks use new module.

## Integration with Existing Components

### How Nydus Integrates

**Classic Mode** (unchanged):
```rust
let nydus = Nydus::new(config).await?;
nydus.run().await?; // Uses integrated Overlord + SwarmPool + Overmind
```

**Pipeline Mode** (new):
```rust
// Nydus acts as a Pipeline runtime executor
let pipeline = carousel_preset().build()?;
let nydus = Nydus::from_pipeline(pipeline).await?;
nydus.run().await?;
```

Nydus becomes a transport layer for Pipeline. Task assignment → `pipeline.topology.assign_task()`, decomposition → `pipeline.decomposition.decompose()`, etc.

### How Queen Integrates

Queen unchanged — it's the agent executor, not orchestrator. Both modes spawn Queens identically:

```rust
// Spawn Queen (classic or pipeline mode)
let queen_handle = spawn_queen(queen_id, task, config).await?;
```

Pipeline modules assign tasks to Queens, but Queen execution logic is identical.

### How Overlord Integrates

**Classic Mode**: Nydus calls Overlord directly after task completion.

**Pipeline Mode**: Overlord is a `Resilience` module variant. Validation happens via `resilience.handle_failure()`:

```rust
// In Pipeline runtime
let result = agent.complete_task(task_id).await?;
if result.needs_validation {
    let action = pipeline.resilience.handle_failure(&task_id, &result.error)?;
    match action {
        RecoveryAction::Merge => pipeline.nydus.merge_task(task_id)?,
        RecoveryAction::Retry => pipeline.decomposition.add_dynamic_task(parent, task)?,
        RecoveryAction::Escalate => pipeline.overmind.handle_escalation(task_id)?,
    }
}
```

### How Overmind Integrates

**Classic Mode**: SwarmPool escalates to Overmind via Nydus event bus.

**Pipeline Mode**: Overmind is a specialized `Resilience` module for strategic decisions:

```rust
// Overmind as Resilience module
impl Resilience for OvermindResilience {
    fn handle_failure(&mut self, task: &TaskId, error: &str) -> Result<RecoveryAction> {
        // LLM call: analyze error, return strategic decision
        let command = self.overmind_handle.analyze(task, error).await?;
        match command {
            OvermindCommand::ZergRush { num } => Ok(RecoveryAction::ZergRush(num)),
            OvermindCommand::Redecompose { subtasks } => Ok(RecoveryAction::Redecompose(subtasks)),
            // ...
        }
    }
}
```

### How SwarmPool Integrates

**Classic Mode**: Nydus calls SwarmPool on events (decline, DAG change, maintenance).

**Pipeline Mode**: SwarmPool functionality split across modules:
- Spawn heuristics → `ElasticPoolScaling` (scaling module)
- Retry policy → `RetryResilience` (resilience module)
- Zerg Rush → `ElasticPoolScaling` with `zerg_rush_mode: true`

```rust
// SwarmPool as Scaling + Resilience composition
let scaling = ElasticPoolScaling::new(config.with_zerg_rush(true));
let resilience = RetryResilience::new(config.with_escalation(true));

let pipeline = PipelineBuilder::new()
    .scaling(scaling)
    .resilience(resilience)
    .build()?;
```

SwarmPool remains as an integrated implementation for classic mode, but its logic is now available as composable modules.

## Migration Path

### Phase 1: Classic Mode (Current, Unchanged)
```rust
let nydus = Nydus::new(config).await?;
nydus.run().await?;
```

### Phase 2: Hybrid Mode (Classic + Partial Pipeline)
```rust
let pipeline = PipelineBuilder::new()
    .memory(MultiTierMemory::new())      // Use new memory
    .scheduling(HybridScheduling::new()) // Use new scheduler
    .build_partial()?;                   // Defaults for rest

let nydus = Nydus::from_pipeline(pipeline).await?;
nydus.run().await?;
```

### Phase 3: Full Pipeline Mode (All New Modules)
```rust
let pipeline = carousel_preset().build()?;
pipeline.run().await?; // No Nydus, pure Pipeline runtime
```

### Phase 4: Custom Composition
```rust
let pipeline = PipelineBuilder::new()
    .topology(HybridTopology::new()         // Custom topology
        .add_selector(TaskSelector::ByComplexity, CentralizedTopology::new(config))
        .add_selector(TaskSelector::ByUrgency, PeerToPeerTopology::new(config)))
    .decomposition(TdagDecomposition::new(config)) // Dynamic DAG
    .communication(MessageBusCommunication::new(config)) // Audit logging
    .memory(RagMemory::new(config))         // Semantic search
    .scheduling(LLMRealtimeScheduling::new(config)) // Preemption
    .resilience(HITLResilience::new(config)) // Human-in-the-loop
    .scaling(ElasticPoolScaling::new(config)) // Burst scaling
    .build()?;

pipeline.run().await?;
```

## Performance Characteristics

| Pipeline | Agents | Tasks/sec | Latency (p99) | Memory (MB) |
|----------|--------|-----------|---------------|-------------|
| **minimal** | 1-3 | 10-20 | 50ms | 50 |
| **carousel** | 3-8 | 5-10 | 200ms | 100 |
| **ralph** | 3-8 | 8-15 | 150ms | 120 |
| **blackboard** | 10-30 | 15-25 | 100ms | 200 |
| **consensus** | 5-15 | 2-5 | 500ms | 150 |
| **swarm** | 20-100 | 50-100 | 20ms | 300 |

Benchmarks: 2024 M3 MacBook Pro, Rust 1.75, Tokio 1.35.

## Testing Strategy

### Trait Contract Tests
Each trait has a contract test suite that all implementations must pass:

```rust
// tests/trait_contracts/topology_contract.rs
fn test_topology_contract<T: Topology>(mut topology: T) {
    // Add agent
    let agent = AgentId::Queen(QueenId(0));
    topology.add_agent(agent.clone()).unwrap();
    assert_eq!(topology.agents().len(), 1);

    // Assign task
    let task = TaskId::new("task-1");
    let assigned = topology.assign_task(&task).unwrap();
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0], agent);

    // Handle completion
    topology.handle_completion(agent, task, TaskResult::Success);
    // ... more contract assertions
}
```

Run contract tests for all modules:
```bash
cargo test --package hatchery --test trait_contracts
```

### Pipeline Integration Tests
End-to-end tests for preset pipelines:

```bash
cargo test --package hatchery --test pipeline_integration
cargo test --package hatchery --test carousel_integration
cargo test --package hatchery --test swarm_integration
```

### Hot-Swap Tests
Runtime component replacement tests:

```bash
cargo test --package hatchery --test hot_swap
```

### Regression Tests
Ensure classic mode (Nydus + SwarmPool + Overlord) still works:

```bash
cargo test --package hatchery --test nydus_classic
```

## Future Extensions

### V5 Roadmap (2026 Q2-Q3)
- **Distributed Runtime**: Pipeline across multiple machines (gRPC/QUIC)
- **GPU Scheduling**: First-class GPU resource management
- **Streaming Decomposition**: Incremental task breakdown (vs all-at-once)
- **Federated Learning**: Cross-agent knowledge sharing without central memory
- **Observability**: Prometheus metrics, Jaeger tracing, structured logging
- **WebAssembly Sandboxing**: Run untrusted agent code safely

### Research Ideas
- **Reinforcement Learning for Topology Selection**: Learn optimal topology per task type
- **Self-Optimizing Pipelines**: Automatically tune module configs based on metrics
- **Causal Reasoning**: Why did this task fail? What would have prevented it?
- **Multi-Objective Scheduling**: Optimize for latency + cost + quality simultaneously

## Related Documents

- `README.md` — Quick start, module overview, CLI usage
- `PRD_OVERMIND.md` — Overmind design rationale (V3)
- `PRD_QUEEN.md` — Queen agent wrapper design (V2)
- `PRD_ZERG_RUSH.md` — Zerg Rush spawn heuristics (V3)
- `src/overmind/README.md` — Overmind integration with V4
- `src/swarm_pool/README.md` — SwarmPool integration with V4
- `tests/README.md` — Testing V4 modules

## Code Examples

See `examples/` directory:
- `examples/carousel_workflow.rs` — Multi-phase connector development
- `examples/ralph_iterative.rs` — Autonomous PRD checkbox tasks
- `examples/blackboard_research.rs` — Self-selecting research agents
- `examples/consensus_decision.rs` — Multi-agent voting
- `examples/swarm_parallel.rs` — Embarrassingly parallel tasks
- `examples/hot_swap_demo.rs` — Runtime module replacement
- `examples/custom_pipeline.rs` — Build your own composition

## License

See project root for license details.
