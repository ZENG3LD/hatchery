//! Hatchery — swarm orchestration for AI coding agents.
//!
//! Three modes named after StarCraft II Zerg units:
//! - **Queen**: Simple iteration (NativeQueen wraps Claude Code CLI)
//! - **Swarm Host**: AI Coordinator + smart workers with shared memory
//! - **Brood Lord**: Full hierarchy with Opus manager, L2 coordinators, workers

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

/// SwarmHost coordinator (L2): task scheduling, validation, merging
pub mod swarm_host;

/// BroodLord strategist (L3): multi-SwarmHost orchestration
pub mod brood_lord;

/// Message routing and event logging
pub mod mailbox;

/// Git safety: attribution, worktree isolation
pub mod safety;
