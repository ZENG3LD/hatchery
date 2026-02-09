//! Infestor — agent-driven merge validator for the Hatchery swarm.
//!
//! The Infestor is a StreamQueen with a reviewer role. It reviews code changes
//! from Queens and decides whether to approve or reject merges.

mod handle;
mod spawn_infestor;

pub use handle::InfestorHandle;
pub use spawn_infestor::{InfestorConfig, spawn_infestor};

// Re-export InfestorId from core types for convenience
pub use crate::core::types::InfestorId;
