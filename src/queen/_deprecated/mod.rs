//! DEPRECATED: Per-task spawn mode implementation.
//!
//! This module contains the deprecated SpawnQueen implementation (per-task spawning).
//! Hatchery now uses only stream mode (StreamQueen) for all Queens.
//!
//! This code is kept for archival purposes but should not be used in new code.

pub mod spawn_queen;

// Re-export for backward compatibility (if anyone imports it directly)
pub use spawn_queen::{spawn, SpawnQueenConfig};
