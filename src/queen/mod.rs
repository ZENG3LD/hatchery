//! Queen module — Claude Code manager agents for Hatchery V3.
//!
//! A Queen is an autonomous AI manager that receives tasks from Nydus,
//! decomposes them into sub-tasks, and spawns worker agents via Claude Code's
//! native Task tool. Queens NEVER do implementation work themselves.
//!
//! Implementations:
//! - StreamQueen: long-lived subprocess (--input-format stream-json)
//! - SpawnQueen: spawn-per-task with session resume (--resume)
//! - NativeQueen: DEPRECATED legacy wrapper

pub mod native;
pub mod recovery;
pub mod spawn_mode;
pub mod handle;
pub mod completion;
pub mod stream_queen;
pub mod spawn_queen;
pub mod pipe_process;

// Re-export key types
#[allow(deprecated)]
pub use native::NativeQueen;
pub use spawn_mode::SpawnMode;
pub use handle::{QueenCommand, QueenEvent, QueenHandle};
pub use spawn_queen::{spawn, SpawnQueenConfig};

use async_trait::async_trait;
use anyhow::Result;
use crate::core::types::*;

/// QueenConfig — common configuration for all Queen implementations.
#[derive(Debug, Clone)]
pub struct QueenConfig {
    pub timeout: std::time::Duration,
    pub max_workers: usize,
    pub model: String,
}

/// The Queen trait — core abstraction for AI manager agents.
///
/// A Queen receives tasks from Nydus, decomposes them, and
/// spawns worker agents via Claude Code's Task tool.
/// Queens are managers — they NEVER do implementation work themselves.
///
/// V3 implementations (StreamQueen, SpawnQueen) use actor model
/// with QueenHandle instead of this trait directly.
/// This trait is kept for backward compatibility with NativeQueen.
#[async_trait]
pub trait Queen: Send + Sync {
    /// Unique identifier for this Queen
    fn id(&self) -> QueenId;

    /// Backend type (for routing and logging)
    fn backend(&self) -> QueenBackend;

    /// Assign a task from Nydus
    async fn assign(&mut self, task: Task, context: TaskContext) -> Result<()>;

    /// Get current status
    async fn status(&self) -> QueenStatus;

    /// Get completed result (None if still working)
    async fn result(&self) -> Option<TaskResult>;

    /// Receive a message from Nydus or another Queen
    async fn send_message(&mut self, msg: SwarmMessage) -> Result<()>;

    /// Drain outgoing messages (status reports, escalations, knowledge)
    async fn drain_outbox(&mut self) -> Vec<SwarmMessage>;

    /// Check if Queen is still alive/responsive
    async fn is_alive(&self) -> bool;

    /// Graceful shutdown
    async fn shutdown(&mut self) -> Result<()>;
}
