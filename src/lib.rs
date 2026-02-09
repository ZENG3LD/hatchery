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

/// Queen agents (L1): StreamQueen, SpawnQueen, NativeQueen (deprecated)
pub mod queen;

/// Nydus coordinator (L2): task scheduling, validation, merging
pub mod nydus;

/// Message routing and event logging
pub mod mailbox;

/// Git safety: attribution, worktree isolation
pub mod safety;

/// IPC server for CLI ↔ Nydus communication
pub mod ipc;
