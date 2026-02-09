//! Core infrastructure for Hatchery swarm orchestration.

pub mod types;
pub mod config;
pub mod prompts;
pub mod task_dag;
pub mod dag_generator;
pub mod shared_memory;
pub mod compaction;
pub mod validator;
pub mod operator;
