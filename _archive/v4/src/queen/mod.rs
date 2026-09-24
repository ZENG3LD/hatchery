//! Queen module — Claude Code manager agents for Hatchery V3.
//!
//! A Queen is an autonomous AI manager that receives tasks from Nydus,
//! decomposes them into sub-tasks, and spawns worker agents via Claude Code's
//! native Task tool. Queens NEVER do implementation work themselves.
//!
//! Implementation:
//! - StreamQueen: long-lived subprocess (--input-format stream-json)

pub mod recovery;
pub mod spawn_mode;
pub mod handle;
pub mod completion;
pub mod stream_queen;
pub mod pipe_process;

// Re-export key types
pub use handle::{QueenCommand, QueenEvent, QueenHandle};
