//! Infestor — agent-driven merge validator for the Hatchery swarm.
//!
//! The Infestor reviews code changes from Queens and decides whether to approve or reject merges.
//! It's a Claude Code subprocess with a reviewer role, not an implementer.

mod handle;
mod spawn_infestor;

pub use handle::*;
pub use spawn_infestor::*;

// Re-export InfestorId from core types for convenience
pub use crate::core::types::InfestorId;
