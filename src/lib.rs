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

/// Infestor: merge validator
pub mod infestor;

/// Nydus coordinator (L2): task scheduling, validation, merging
pub mod nydus;

/// Git safety: attribution, worktree isolation
pub mod safety;

/// Infestation pit: Claude Code session parsers (from zengeld-memory)
pub mod infestation_pit;
