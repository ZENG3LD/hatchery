//! Hatchery — swarm orchestration for AI coding agents.
//!
//! Three modes named after StarCraft II Zerg units:
//! - **Queen**: Simple iteration (NativeQueen wraps Claude Code CLI)
//! - **Swarm Host**: AI Coordinator + smart workers with shared memory
//! - **Brood Lord**: Full hierarchy with Opus manager, L2 coordinators, workers

pub mod types;
pub mod prd;
pub mod progress;
pub mod safety;

/// V2 architecture with Queen trait and message-based coordination
pub mod v2;
