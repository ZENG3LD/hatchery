//! Core types for Hatchery swarm orchestration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single task parsed from a PRD markdown file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Task number (1-based, from PRD ordering).
    pub id: usize,
    /// Full description text after the checkbox.
    pub description: String,
    /// Whether the checkbox is checked.
    pub done: bool,
    /// Line number in the PRD file (0-based).
    pub line_number: usize,
}

/// Swarm operation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    /// Simple Ralph-style: N workers iterate PRD checkboxes independently.
    Queen,
    /// AI Coordinator + smart workers with shared memory.
    SwarmHost,
    /// Full hierarchy: Opus manager → L2 coordinators → workers.
    BroodLord,
}

/// Configuration for a hatchery run.
#[derive(Debug, Clone)]
pub struct HatcheryConfig {
    /// Path to the PRD markdown file.
    pub prd_path: PathBuf,
    /// Number of worker sessions.
    pub workers: usize,
    /// Operation mode.
    pub mode: Mode,
    /// Working directory for workers.
    pub working_dir: PathBuf,
    /// Verification command (e.g. "cargo check").
    pub verify_cmd: Option<String>,
    /// Maximum iterations per worker.
    pub max_iterations: usize,
    /// Stall threshold: consecutive iterations without progress before pause.
    pub stall_threshold: usize,
    /// Progress file path (auto-generated if None).
    pub progress_path: Option<PathBuf>,
    /// Show verbose output from worker sessions.
    pub verbose: bool,
    /// Enable git worktree isolation per worker (Level 2 safety).
    pub worktree_isolation: bool,
    /// Enable safe-mode prompt restrictions (Level 3 safety).
    pub safe_mode: bool,
    /// Backend type: "claude-native" (default), "api"
    pub backend: String,
    /// API URL for custom backend
    pub api_url: Option<String>,
    /// API model for custom backend
    pub api_model: Option<String>,
    /// Validation command for v2
    pub validator_cmd: Option<String>,
    /// Compaction threshold (0.0-1.0)
    pub compaction_threshold: f32,
    /// Event log path
    pub event_log_path: Option<PathBuf>,
    /// Spawn mode for Queen actors: "stream" or "per-task"
    pub spawn_mode: String,
}

impl Default for HatcheryConfig {
    fn default() -> Self {
        Self {
            prd_path: PathBuf::from("PRD.md"),
            workers: 1,
            mode: Mode::Queen,
            working_dir: std::env::current_dir().unwrap_or_default(),
            verify_cmd: None,
            max_iterations: 100,
            stall_threshold: 3,
            progress_path: None,
            verbose: false,
            worktree_isolation: false,
            safe_mode: false,
            backend: "claude-native".to_string(),
            api_url: None,
            api_model: None,
            validator_cmd: None,
            compaction_threshold: 0.8,
            event_log_path: None,
            spawn_mode: "per-task".to_string(),
        }
    }
}

/// Result of a single iteration.
#[derive(Debug, Clone)]
pub enum IterationResult {
    /// A task was completed.
    Progress { task_id: usize, description: String },
    /// Worker ran but no checkbox was flipped.
    NoProgress { reason: String },
    /// Worker encountered an error.
    Error { message: String },
    /// All tasks are done.
    AllDone,
}

/// Summary of a completed swarm run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmResult {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub total_iterations: usize,
    pub duration_secs: u64,
    pub workers_used: usize,
}
