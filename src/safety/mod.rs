//! Safety module — git attribution, worktree isolation, and safe-mode policies.
//!
//! Three levels:
//! - **L1 (always-on):** Git commit attribution with worker ID and Co-Authored-By
//! - **L2 (`--worktree`):** Git worktree isolation per worker
//! - **L3 (`--safe-mode`):** Prompt-level command restrictions

pub mod policy;
pub mod worktree;

use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Git commit with worker attribution (Level 1).
///
/// Commit format: `feat({mode_tag}/{worker_tag}): {task_desc}`
/// Includes `Co-Authored-By: Hatchery-{worker_tag} <hatchery@nemo>` trailer.
///
/// `mode_tag`: "queen", "swarm", "brood"
/// `worker_tag`: "W0", "W1", "L2.0.W0", etc.
/// `working_dir`: where to run git (may be a worktree path).
pub fn git_commit_task(
    working_dir: &Path,
    mode_tag: &str,
    worker_tag: &str,
    task_desc: &str,
) -> Result<bool> {
    // Truncate task description for commit subject line
    let desc = if task_desc.len() > 60 {
        let mut end = 57;
        while end > 0 && !task_desc.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &task_desc[..end])
    } else {
        task_desc.to_string()
    };

    let subject = format!("feat({}/{}): {}", mode_tag, worker_tag, desc);
    let co_author = format!("Co-Authored-By: Hatchery-{} <hatchery@nemo>", worker_tag);
    let message = format!("{}\n\n{}", subject, co_author);

    // git add -A
    let add_output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(working_dir)
        .output();

    if let Err(e) = add_output {
        eprintln!("[SAFETY] git add failed: {}", e);
        return Ok(false);
    }

    // Check if there's anything to commit
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(working_dir)
        .output();

    if let Ok(out) = &status_output {
        let status = String::from_utf8_lossy(&out.stdout);
        if status.trim().is_empty() {
            // Nothing to commit
            return Ok(false);
        }
    }

    // git commit
    let output = Command::new("git")
        .args(["commit", "-m", &message])
        .current_dir(working_dir)
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                println!("[SAFETY] Committed: {}", subject);
                Ok(true)
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                eprintln!("[SAFETY] git commit failed: {}", stderr.trim());
                Ok(false)
            }
        }
        Err(e) => {
            eprintln!("[SAFETY] git commit error: {}", e);
            Ok(false)
        }
    }
}

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

/// Build a safe-mode prompt suffix. Returns empty string if safe_mode is false.
pub fn safe_mode_suffix(safe_mode: bool) -> &'static str {
    if safe_mode {
        policy::safe_mode_prompt()
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_mode_suffix_disabled() {
        assert!(safe_mode_suffix(false).is_empty());
    }

    #[test]
    fn test_safe_mode_suffix_enabled() {
        let suffix = safe_mode_suffix(true);
        assert!(!suffix.is_empty());
        assert!(suffix.contains("FORBIDDEN"));
    }
}
