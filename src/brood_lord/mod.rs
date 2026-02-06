//! Brood Lord mode — full hierarchy with Opus manager, L2 coordinators, workers.
//!
//! Three-tier architecture:
//! 1. **Brood Lord** (Opus) — strategic decisions, PRD decomposition, coordinator oversight
//! 2. **L2 Coordinators** (Sonnet) — manage worker groups, handle failures
//! 3. **Workers** (Sonnet/Haiku) — execute individual tasks

use crate::types::{HatcheryConfig, SwarmResult};
use anyhow::Result;

// Prompts will live in prompts/ next to this file.
// const BROOD_LORD_PROMPT: &str = include_str!("prompts/brood_lord.md");
// const COORDINATOR_PROMPT: &str = include_str!("prompts/coordinator.md");
// const WORKER_PROMPT: &str = include_str!("prompts/worker.md");

/// Run Brood Lord mode.
pub fn run(config: &HatcheryConfig) -> Result<SwarmResult> {
    let _ = config;
    anyhow::bail!("Brood Lord mode not yet implemented")
}
