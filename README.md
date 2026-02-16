# Hatchery — Modular Swarm Orchestrator

**Multi-agent orchestration system for AI coding agents**. Named after StarCraft II Zerg units, Hatchery spawns, coordinates, and manages swarms of AI agents through a composable building-block architecture.

## What is Hatchery?

Hatchery orchestrates AI coding agents through a modular trait-based system. Instead of hardcoded coordination logic, you compose runtime behaviors from 50+ standalone modules across 8 categories: topology, decomposition, communication, memory, scheduling, resilience, and scaling. Each module implements a core trait, allowing mix-and-match composition via `PipelineBuilder`.

Existing components (Nydus, Queen, Overlord, Overmind, SwarmPool) remain fully functional and now integrate with the new modular architecture. You can use them standalone (classic mode) or compose them with new modules (pipeline mode).

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            HATCHERY V4                                       │
│                       Modular Swarm Orchestrator                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
        ┌───────────────────────────┴───────────────────────────┐
        │                                                        │
   CLASSIC MODE                                          PIPELINE MODE
   (Integrated)                                         (Composable)
        │                                                        │
        ▼                                                        ▼
┌──────────────────┐                              ┌────────────────────────┐
│   Nydus          │◄─────────────────────────────┤  PipelineBuilder       │
│   (Transport     │                              │  .topology(...)        │
│    Coordinator)  │                              │  .decomposition(...)   │
├──────────────────┤                              │  .communication(...)   │
│   Queen          │                              │  .memory(...)          │
│   (Agent Wrapper)│                              │  .scheduling(...)      │
├──────────────────┤                              │  .resilience(...)      │
│   Overlord       │                              │  .scaling(...)         │
│   (Merge Review) │                              │  .build()              │
├──────────────────┤                              └────────┬───────────────┘
│   Overmind       │                                       │
│   (Strategic LLM)│                                       │
├──────────────────┤                              ┌────────▼───────────────┐
│   SwarmPool      │                              │   Pipeline Runtime     │
│   (Spawn Logic)  │                              │   (Hot-swappable)      │
└──────────────────┘                              └────────────────────────┘
                                                            │
                ┌───────────────────────────────────────────┴────────────────┐
                │                                                            │
        ┌───────▼─────────┐   ┌──────────────┐   ┌──────────────┐   ┌──────▼──────┐
        │  7 TOPOLOGIES   │   │6 DECOMP      │   │8 COMM        │   │ 9 MEMORY    │
        │  - Centralized  │   │- DAG         │   │- Direct      │   │ - SharedState│
        │  - Hierarchical │   │- HTN         │   │- Broadcast   │   │ - MultiTier │
        │  - P2P          │   │- TDAG        │   │- Blackboard  │   │ - Document  │
        │  - Blackboard   │   │- Emergent    │   │- MessageBus  │   │ - Isolated  │
        │  - GraphDAG     │   │- RoleBased   │   │- Handoff     │   │ - Session   │
        │  - Conversational│   │- Capability  │   │- ContractNet │   │ - RAG       │
        │  - Hybrid       │   └──────────────┘   │- RippleEffect│   │ - Ontology  │
        └─────────────────┘                      │- Protocols   │   │ - Collab    │
                                                 └──────────────┘   └─────────────┘
        ┌─────────────────┐   ┌──────────────┐   ┌──────────────┐   ┌─────────────┐
        │ 7 SCHEDULING    │   │7 RESILIENCE  │   │6 SCALING     │   │3 PIPELINE   │
        │ - EventDriven   │   │- Retry       │   │- SmallScale  │   │- Builder    │
        │ - TimerBased    │   │- Recovery    │   │- MediumScale │   │- Presets    │
        │ - Hybrid        │   │- Degradation │   │- LargeScale  │   │- Runtime    │
        │ - LoadBalancing │   │- Consensus   │   │- VeryLarge   │   │             │
        │ - Priority      │   │- HITL        │   │- EdgeCloud   │   └─────────────┘
        │ - LLMRealtime   │   │- CircuitBreak│   │- ElasticPool │
        │ - WorkStealing  │   │- FailTrack   │   └──────────────┘
        └─────────────────┘   └──────────────┘
```

## Quick Start

### Classic Mode (Integrated Components)

```rust
use hatchery::nydus::Nydus;
use hatchery::cli::HatcheryConfig;

// Use Nydus + Queen + Overlord + SwarmPool as integrated system
let config = HatcheryConfig::default();
let nydus = Nydus::new(config).await?;
nydus.run().await?;
```

### Pipeline Mode (Composable Modules)

```rust
use hatchery::pipeline::PipelineBuilder;
use hatchery::topology::CentralizedTopology;
use hatchery::decomposition::DagDecomposition;
// ... import other modules

let pipeline = PipelineBuilder::new()
    .topology(CentralizedTopology::new(config))
    .decomposition(DagDecomposition::new(config))
    .communication(MessageBusCommunication::new(config))
    .memory(MultiTierMemory::new())
    .scheduling(HybridScheduling::new(config))
    .resilience(RetryResilience::new(config))
    .scaling(ElasticPoolScaling::new(config))
    .build()?;

pipeline.run().await?;
```

### Using Presets

```rust
use hatchery::pipeline::presets::*;

// Structured multi-phase workflows (research → implement → test → debug)
let pipeline = carousel_preset().build()?;

// Autonomous iterative tasks with PRD checkboxes
let pipeline = ralph_preset().build()?;

// Exploratory research with agent self-selection
let pipeline = blackboard_preset().build()?;

// High-stakes decisions with multi-agent consensus
let pipeline = consensus_preset().build()?;

// Embarrassingly parallel tasks with work stealing
let pipeline = swarm_preset().build()?;

// Simple single-agent or small-team tasks
let pipeline = minimal_preset().build()?;
```

## Module Overview

| Category | Modules | Purpose |
|----------|---------|---------|
| **Topology** | 7 | HOW agents are organized: Centralized, Hierarchical, P2P, Blackboard, GraphDAG, Conversational, Hybrid |
| **Decomposition** | 6 | HOW tasks are broken down: DAG, HTN, TDAG, Emergent, RoleBased, Capability |
| **Communication** | 8+4 | HOW agents talk: Direct, Broadcast, Blackboard, MessageBus, Handoff, ContractNet, RippleEffect, Protocols (MCP/A2A/ACP/ANP) |
| **Memory** | 9 | HOW state is stored: SharedState, Conversation, MultiTier, Document, Isolated, Session, Collaborative, Ontology, RAG |
| **Scheduling** | 7 | WHEN things run: EventDriven, TimerBased, Hybrid, LoadBalancing, Priority, LLMRealtime, WorkStealing |
| **Resilience** | 7 | ERROR handling: Retry, Recovery, Degradation, Consensus, HITL, CircuitBreaker, FailureTracking |
| **Scaling** | 6 | SCALE management: SmallScale (3-8), MediumScale (10-100), LargeScale (100-1K), VeryLarge (1K+), EdgeCloud, ElasticPool |
| **Pipeline** | 3 | RUNTIME composition: Builder, Presets, Runtime (hot-swap) |

**Total: 50+ modules**, all trait-based, composable at runtime.

## The 7 Core Traits

Each category defines a trait that all modules implement:

```rust
/// Topology: HOW agents are organized and tasks assigned
trait Topology: Send + Sync {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>>;
    fn handle_completion(&mut self, agent: AgentId, task: TaskId, result: TaskResult);
    fn agents(&self) -> Vec<AgentId>;
    fn add_agent(&mut self, agent: AgentId) -> Result<()>;
    fn remove_agent(&mut self, agent: AgentId) -> Result<()>;
}

/// Decomposition: HOW tasks are broken down
trait Decomposition: Send + Sync {
    fn decompose(&mut self, task: Task) -> Result<Vec<Task>>;
    fn can_decompose(&self, task: &Task) -> bool;
    fn add_dynamic_task(&mut self, parent: &TaskId, task: Task) -> Result<()>;
    fn ready_tasks(&self) -> Vec<TaskId>;
    fn mark_complete(&mut self, task_id: &TaskId) -> Result<()>;
}

/// Communication: HOW agents exchange messages
trait Communication: Send + Sync {
    fn send(&mut self, from: AgentId, to: AgentId, msg: Message) -> Result<()>;
    fn broadcast(&mut self, from: AgentId, msg: Message) -> Result<()>;
    fn subscribe(&mut self, agent: AgentId, topic: String) -> Result<()>;
    fn publish(&mut self, from: AgentId, topic: String, msg: Message) -> Result<()>;
    fn receiver(&self, agent: AgentId) -> Option<Receiver<Message>>;
}

/// Memory: HOW state is persisted and queried
trait Memory: Send + Sync {
    fn insert(&mut self, key: String, value: MemoryValue) -> Result<()>;
    fn get(&self, key: &str) -> Option<MemoryValue>;
    fn query(&self, query: MemoryQuery) -> Vec<MemoryValue>;
    fn evict(&mut self, key: &str) -> Result<()>;
    fn snapshot(&self) -> Result<MemorySnapshot>;
    fn restore(&mut self, snapshot: MemorySnapshot) -> Result<()>;
}

/// Scheduling: WHEN tasks are executed
trait Scheduling: Send + Sync {
    fn next_task(&mut self) -> Option<TaskId>;
    fn notify_completion(&mut self, task_id: TaskId);
    fn next_interval(&self) -> Option<Duration>;
    fn should_terminate(&self) -> bool;
}

/// Resilience: HOW errors are handled
trait Resilience: Send + Sync {
    fn handle_failure(&mut self, task: &TaskId, error: &str) -> Result<RecoveryAction>;
    fn check_health(&self, agent: &AgentId) -> HealthStatus;
    fn plan_recovery(&mut self, agents: &[AgentId]) -> Vec<RecoveryAction>;
    fn record_failure(&mut self, task: &TaskId, error: String);
}

/// Scaling: HOW the agent pool grows/shrinks
trait Scaling: Send + Sync {
    fn should_scale(&self, metrics: &ScalingMetrics) -> ScalingDecision;
    fn execute_scale(&mut self, decision: ScalingDecision) -> Result<Vec<ScalingAction>>;
}
```

## Preset Pipelines

Pre-built configurations for common workflow patterns:

| Preset | Best For | Key Modules |
|--------|----------|-------------|
| **carousel** | Structured multi-phase workflows (research → implement → test → debug) | Centralized + DAG + MessageBus + MultiTier |
| **ralph** | Autonomous iterative tasks with PRD checkboxes | Centralized + Emergent + MessageBus + Conversation |
| **blackboard** | Exploratory research with agent self-selection | Blackboard + Capability + Blackboard + Document |
| **consensus** | High-stakes decisions requiring agreement | Conversational + RoleBased + Broadcast + Collaborative |
| **swarm** | Embarrassingly parallel tasks | P2P + DAG + Direct + Isolated + WorkStealing |
| **minimal** | Simple single-agent or small-team tasks | Centralized + DAG + Direct + SharedState + EventDriven |

## Existing Components (Still Fully Functional)

The following integrated components continue to work in classic mode:

- **Nydus**: Transport/scheduling coordinator. Routes messages, manages worktrees, assigns tasks to Queens, validates results via Overlord.
- **Queen**: AI agent wrapper. Spawns Claude Code subprocesses via NDJSON pipe. Two variants: `StreamQueen` (long-lived) and `SpawnQueen` (one-shot).
- **Overlord**: Merge validator. Hybrid pipeline: Rust parsers (diff/tests/quality) → deterministic checks → optional LLM review. MERGE/DECLINE verdict.
- **Overmind**: Strategic LLM coordinator. Escalation handler for deadlocks, repeated failures, merge conflicts. Commands Nydus/SwarmPool.
- **SwarmPool**: Spawn heuristics. Pure Rust (no LLM): Zerg Rush (auto-spawn on bottleneck), elastic pool (min/max Queens), retry policy (1st decline → retry, 2nd → escalate).
- **Safety**: Git worktree isolation, attribution, safe merging.
- **Overseer**: Claude Code session parsers (from zengeld-memory).

These components now integrate with the modular architecture — you can use them standalone or compose them with new pipeline modules.

## Directory Structure

```
hatchery/
├── src/
│   ├── main.rs               # CLI entry point
│   ├── lib.rs                # Re-exports
│   ├── cli.rs                # CLI config
│   ├── prd.rs                # PRD parser
│   ├── progress.rs           # Progress display
│   │
│   ├── core/                 # Core types + TaskDAG
│   │   ├── types.rs
│   │   ├── task_dag.rs
│   │   └── shared_memory.rs
│   │
│   ├── topology/             # 7 topology modules
│   ├── decomposition/        # 6 decomposition modules
│   ├── communication/        # 8+4 communication modules
│   ├── memory/               # 9 memory modules
│   ├── scheduling/           # 7 scheduling modules
│   ├── resilience/           # 7 resilience modules
│   ├── scaling/              # 6 scaling modules
│   │
│   ├── pipeline/             # Pipeline builder + presets + runtime
│   │   ├── builder.rs
│   │   ├── presets.rs
│   │   └── runtime.rs
│   │
│   ├── nydus/                # Classic Nydus coordinator
│   ├── queen/                # Queen agent wrapper
│   ├── overlord/             # Merge validator
│   ├── overmind/             # Strategic LLM coordinator
│   ├── swarm_pool/           # Spawn heuristics
│   ├── safety/               # Git worktree isolation
│   └── overseer/             # Session parsers
│
├── tests/                    # Integration tests
├── examples/                 # Usage examples
├── ARCHITECTURE_V3.md        # (will be ARCHITECTURE_V4.md)
├── PRD_*.md                  # Design docs
└── README.md                 # This file
```

## Common Commands

```bash
# Check compilation
cargo check --package hatchery

# Run unit tests
cargo test --package hatchery

# Run integration tests
cargo test --package hatchery --test pipeline_integration

# Build release binary
cargo build --release --package hatchery

# Run hatchery CLI
cargo run --release --package hatchery -- --help

# Run with a preset pipeline
cargo run --release --package hatchery -- --preset carousel --prd path/to/PRD.md

# Run classic mode (Nydus)
cargo run --release --package hatchery -- --classic --prd path/to/PRD.md
```

## Runtime Hot-Swap

Pipeline components can be swapped at runtime without restarting:

```rust
// Start with carousel preset
let mut pipeline = carousel_preset().build()?;

// Later, hot-swap topology from Centralized to P2P
pipeline.swap_topology(PeerToPeerTopology::new(config))?;

// Hot-swap memory from MultiTier to RAG
pipeline.swap_memory(RagMemory::new(config))?;
```

See `ARCHITECTURE_V4.md` for full architectural details and migration guide.

## Code Style

- Use `Result<T, E>` for fallible operations
- Prefer `&str` over `String` in function params
- Use `eprintln!` for logging with `[Component]` prefix
- All modules implement `Send + Sync` for async/parallel execution
- Follow existing patterns in the codebase
- Traits over concrete types for composability

## Language & Stack

- **Rust** (2021 edition)
- **Async Runtime**: Tokio
- **Key Crates**: tokio, serde, serde_json, reqwest, clap, anyhow

## License

See project root for license details.
