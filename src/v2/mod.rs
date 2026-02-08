//! Hatchery V2 — Next-generation swarm orchestration architecture.
//!
//! V2 introduces:
//! - Queen trait: Pluggable agent backends (Claude Native, Raw, Codex, API)
//! - SwarmHost: AI coordinator with shared memory and smart task routing
//! - Message-based communication between agents
//! - Hierarchical task breakdown and dependency management

pub mod types;
pub mod queen;
pub mod mailbox;
pub mod task_dag;
pub mod compaction;
pub mod validator;
pub mod shared_memory;
pub mod worktree;
pub mod swarm_host;
pub mod operator;
pub mod brood_lord;
pub mod config;
pub mod prompts;
