//! Hatchery — swarm orchestration for AI coding agents.
//!
//! Named after StarCraft II Zerg units:
//! - **Nydus**: transport & scheduling node — assigns tasks, validates, merges
//! - **Queen**: AI manager — spawns and coordinates worker agents

/// CLI types: HatcheryConfig, Mode, SwarmResult
pub mod cli;

/// PRD parser (markdown checkbox parsing)
pub mod prd;

/// Progress display utilities
pub mod progress;

/// Core infrastructure: types, task DAG, shared memory, etc.
pub mod core;

/// Queen agents (L1): StreamQueen, SpawnQueen
pub mod queen;

/// Overlord: merge validator
pub mod overlord;

/// Nydus coordinator (L2): task scheduling, validation, merging
pub mod nydus;

/// Git safety: attribution, worktree isolation
pub mod safety;

/// Overseer: Claude Code session parsers (from zengeld-memory)
pub mod overseer;

/// SwarmPool: deterministic spawn heuristics (zerg rush, elastic pool, retry)
pub mod swarm_pool;

/// Overmind: strategic LLM coordinator (escalation handler)
pub mod overmind;

/// Topology: defines how agents are organized and how tasks are assigned
pub mod topology;

/// Communication: agent message passing (Direct, Broadcast, Blackboard, MessageBus, Handoff, ContractNet, RippleEffect, Protocols)
pub mod communication;

/// Decomposition: task breakdown strategies (DAG, HTN, TDAG, Emergent, Role-based, Capability)
pub mod decomposition;

/// Resilience: failure handling, recovery, degradation, consensus, HITL, circuit breaker
pub mod resilience;

/// Scheduling: task execution scheduling (Event-driven, Timer-based, Hybrid, Load balancing, Priority, LLM realtime, Work stealing)
pub mod scheduling;

/// Memory: multiple memory implementations (SharedState, Conversation, MultiTier, Document, Isolated, Session, Collaborative, Ontology, RAG)
pub mod memory;

/// Scaling: dynamic agent pool management (SmallScale, MediumScale, LargeScale, VeryLargeScale, EdgeCloud, ElasticPool)
pub mod scaling;

/// Pipeline: composable orchestration workflows (builder, presets, runtime)
pub mod pipeline;
