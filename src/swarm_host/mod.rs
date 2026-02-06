//! Swarm Host mode — AI Coordinator + smart workers with shared memory.
//!
//! The Coordinator is a Claude session that:
//! 1. Reads the PRD and decomposes work
//! 2. Spawns N worker sessions via PipeProcess
//! 3. Distributes tasks intelligently (not round-robin)
//! 4. Monitors worker output in real-time
//! 5. Manages shared memory (cross-worker knowledge)
//! 6. Handles failures and reassignment

use crate::types::{HatcheryConfig, SwarmResult};
use anyhow::Result;

// Prompts will live in prompts/ next to this file.
// const COORDINATOR_PROMPT: &str = include_str!("prompts/coordinator.md");
// const WORKER_PROMPT: &str = include_str!("prompts/worker.md");

/// Run Swarm Host mode.
pub fn run(config: &HatcheryConfig) -> Result<SwarmResult> {
    let _ = config;
    anyhow::bail!("Swarm Host mode not yet implemented")
}
