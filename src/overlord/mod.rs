//! Overlord — agent-driven merge validator for the Hatchery swarm.
//!
//! The Overlord is a StreamQueen with a reviewer role. It reviews code changes
//! from Queens and decides whether to approve or reject merges.

mod handle;
mod spawn_overlord;
pub mod parsers;
pub mod code_checks;

pub use handle::OverlordHandle;
pub use spawn_overlord::{OverlordConfig, spawn_overlord};

// Re-export OverlordId from core types for convenience
pub use crate::core::types::OverlordId;
