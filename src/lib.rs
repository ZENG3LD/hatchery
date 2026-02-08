//! Hatchery — swarm orchestration for AI coding agents.
//!
//! Three modes named after StarCraft II Zerg units:
//! - **Queen**: Simple Ralph-style iteration (inject larvae → workers iterate PRD)
//! - **Swarm Host**: AI Coordinator + smart workers with shared memory
//! - **Brood Lord**: Full hierarchy with Opus manager, L2 coordinators, workers

pub mod types;
pub mod prd;
pub mod progress;
pub mod safety;
pub mod queen;
pub mod swarm_host;
pub mod brood_lord;
