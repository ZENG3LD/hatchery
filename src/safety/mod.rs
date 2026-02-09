//! Safety module — git attribution, worktree isolation, and safe-mode policies.
//!
//! Two levels:
//! - **L1 (`--worktree`):** Git worktree isolation per worker
//! - **L2 (`--safe-mode`):** Prompt-level command restrictions

pub mod worktree;

use std::path::Path;
use std::process::Command;

/// Get the SHA of the latest commit (for TaskResult tracking).
pub fn get_head_sha(working_dir: &Path) -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(working_dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}
