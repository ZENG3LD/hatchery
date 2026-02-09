//! SpawnInfestor: Per-review Claude Code subprocess for merge validation.
//!
//! Each review spawns a fresh Claude Code process to analyze diffs and make merge decisions.

use crate::core::types::{QueenId, InfestorId};
use crate::infestor::handle::*;
use tokio::sync::{mpsc, watch, Notify};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Configuration for SpawnInfestor.
#[derive(Debug, Clone)]
pub struct SpawnInfestorConfig {
    pub id: InfestorId,
    pub model: String,
    pub working_dir: PathBuf,
    /// Notify handle to wake Nydus when review completes.
    pub wakeup_notify: Option<Arc<Notify>>,
}

impl Default for SpawnInfestorConfig {
    fn default() -> Self {
        Self {
            id: InfestorId("infestor-0".to_string()),
            model: "sonnet".to_string(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            wakeup_notify: None,
        }
    }
}

/// Spawn an Infestor actor.
///
/// Returns a cloneable handle for sending review commands.
pub fn spawn_infestor_actor(
    config: SpawnInfestorConfig,
    event_tx: mpsc::UnboundedSender<(InfestorId, InfestorEvent)>,
) -> InfestorHandle {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InfestorCommand>(32);
    let (status_tx, status_rx) = watch::channel(InfestorStatus::Idle);

    let id = config.id.clone();
    let handle = InfestorHandle {
        id: id.clone(),
        cmd_tx,
        status_rx,
    };

    tokio::spawn(async move {
        eprintln!("[Infestor {}] Started", id.0);

        loop {
            match cmd_rx.recv().await {
                Some(InfestorCommand::Review {
                    queen_id,
                    task_id,
                    branch_name,
                    worktree_path,
                }) => {
                    let _ = status_tx.send(InfestorStatus::Reviewing {
                        queen_id: queen_id.clone(),
                        task_id: task_id.clone(),
                    });

                    let result = run_review(
                        &config,
                        &queen_id,
                        &task_id,
                        &branch_name,
                        &worktree_path,
                    )
                    .await;

                    match result {
                        Ok(ReviewVerdict::Approved { summary }) => {
                            let _ = event_tx.send((
                                id.clone(),
                                InfestorEvent::ReviewApproved {
                                    queen_id: queen_id.clone(),
                                    task_id: task_id.clone(),
                                    summary,
                                },
                            ));
                        }
                        Ok(ReviewVerdict::Rejected { reason }) => {
                            let _ = event_tx.send((
                                id.clone(),
                                InfestorEvent::ReviewRejected {
                                    queen_id: queen_id.clone(),
                                    task_id: task_id.clone(),
                                    reason,
                                },
                            ));
                        }
                        Err(e) => {
                            let _ = event_tx.send((
                                id.clone(),
                                InfestorEvent::ReviewRejected {
                                    queen_id: queen_id.clone(),
                                    task_id: task_id.clone(),
                                    reason: format!("Review process error: {}", e),
                                },
                            ));
                        }
                    }

                    let _ = status_tx.send(InfestorStatus::Idle);

                    // Wake Nydus for next scheduling cycle
                    if let Some(ref notify) = config.wakeup_notify {
                        notify.notify_one();
                    }
                }
                Some(InfestorCommand::Shutdown) => {
                    eprintln!("[Infestor {}] Shutdown requested", id.0);
                    let _ = status_tx.send(InfestorStatus::Dead);
                    break;
                }
                None => {
                    let _ = status_tx.send(InfestorStatus::Dead);
                    let _ = event_tx.send((id.clone(), InfestorEvent::ProcessDied("Channel closed".into())));
                    break;
                }
            }
        }

        eprintln!("[Infestor {}] Actor loop exited", id.0);
    });

    handle
}

// ============================================================================
// Review Logic
// ============================================================================

/// Review verdict from the Infestor.
enum ReviewVerdict {
    Approved { summary: String },
    Rejected { reason: String },
}

/// Execute a review by spawning Claude Code to analyze the diff.
async fn run_review(
    config: &SpawnInfestorConfig,
    queen_id: &QueenId,
    task_id: &str,
    branch_name: &str,
    worktree_path: &Path,
) -> Result<ReviewVerdict, anyhow::Error> {
    eprintln!(
        "[Infestor {}] Reviewing task {} from {} in branch {}",
        config.id.0, task_id, queen_id.0, branch_name
    );

    // 1. Get diff stats from the Queen's branch
    let diff_output = tokio::process::Command::new("git")
        .args(["diff", "HEAD~1..HEAD", "--stat"])
        .current_dir(worktree_path)
        .output()
        .await?;

    let diff_stat = String::from_utf8_lossy(&diff_output.stdout).to_string();

    // Get full diff
    let full_diff = tokio::process::Command::new("git")
        .args(["diff", "HEAD~1..HEAD"])
        .current_dir(worktree_path)
        .output()
        .await?;
    let full_diff_text = String::from_utf8_lossy(&full_diff.stdout).to_string();

    // Get commit log
    let log_output = tokio::process::Command::new("git")
        .args(["log", "--oneline", "-5"])
        .current_dir(worktree_path)
        .output()
        .await?;
    let commit_log = String::from_utf8_lossy(&log_output.stdout).to_string();

    // 2. Run cargo check in the worktree
    let check_output = tokio::process::Command::new("cargo")
        .args(["check", "--message-format=short"])
        .current_dir(worktree_path)
        .env(
            "CARGO_TARGET_DIR",
            worktree_path.join(".hatchery_target"),
        )
        .output()
        .await?;
    let check_stderr = String::from_utf8_lossy(&check_output.stderr).to_string();
    let check_passed = check_output.status.success();

    // 3. If cargo check fails, auto-reject
    if !check_passed {
        return Ok(ReviewVerdict::Rejected {
            reason: format!("cargo check failed:\n{}", check_stderr),
        });
    }

    // 4. Spawn Claude Code for intelligent review
    let system_prompt = crate::core::prompts::infestor_system_prompt();
    let review_prompt = format_review_prompt(
        queen_id,
        task_id,
        branch_name,
        &diff_stat,
        &full_diff_text,
        &commit_log,
        &check_stderr,
    );

    // Use claude CLI in non-interactive mode for the review
    let mut child = tokio::process::Command::new("claude")
        .args([
            "--print",
            "--model",
            &config.model,
            "--system-prompt",
            &system_prompt,
            "--output-format",
            "text",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(worktree_path)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn Claude for review: {}", e))?;

    // Write prompt to stdin
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(review_prompt.as_bytes()).await;
        drop(stdin); // Close stdin to signal EOF
    }

    // Wait for response with timeout
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120), // 2 min timeout for review
        child.wait_with_output(),
    )
    .await??;

    let response = String::from_utf8_lossy(&output.stdout).to_string();

    // 5. Parse verdict from Claude's response
    parse_verdict(&response)
}

/// Format the review prompt with diff and context.
fn format_review_prompt(
    queen_id: &QueenId,
    task_id: &str,
    branch_name: &str,
    diff_stat: &str,
    full_diff: &str,
    commit_log: &str,
    cargo_check: &str,
) -> String {
    // Truncate diff if too large (Claude context limit)
    let diff_display = if full_diff.len() > 50_000 {
        format!(
            "{}...\n\n[DIFF TRUNCATED — {} bytes total, showing first 50000]",
            &full_diff[..50_000],
            full_diff.len()
        )
    } else {
        full_diff.to_string()
    };

    format!(
        r#"## Review Request

**Queen**: {queen_id}
**Task**: {task_id}
**Branch**: {branch_name}

### Commit Log
```
{commit_log}
```

### Diff Statistics
```
{diff_stat}
```

### Cargo Check Output
```
{cargo_check}
```

### Full Diff
```diff
{diff_display}
```

## Your Task

Review this diff and decide: APPROVE or REJECT.

**APPROVE** if:
- Code compiles (cargo check passed above)
- Changes are reasonable and related to the task
- No obvious bugs, security issues, or broken logic
- Code follows existing patterns in the codebase

**REJECT** if:
- Code has logic errors or bugs
- Changes are unrelated to the task (scope creep)
- Code quality is poor (hardcoded values, missing error handling for critical paths)
- Changes could break other parts of the system

## Response Format

You MUST end your response with exactly one of these lines:

VERDICT: APPROVE
<summary>Brief description of what was changed and why it's acceptable</summary>

OR

VERDICT: REJECT
<reason>Specific explanation of what's wrong and what needs to be fixed</reason>
"#,
        queen_id = queen_id.0,
        task_id = task_id,
        branch_name = branch_name,
        commit_log = commit_log,
        diff_stat = diff_stat,
        cargo_check = cargo_check,
        diff_display = diff_display
    )
}

/// Parse verdict from Claude's response.
fn parse_verdict(response: &str) -> Result<ReviewVerdict, anyhow::Error> {
    // Look for VERDICT: APPROVE or VERDICT: REJECT
    let lines: Vec<&str> = response.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("VERDICT: APPROVE") {
            // Extract summary from <summary> tag
            let summary = extract_tag(response, "summary")
                .unwrap_or_else(|| "Approved by Infestor".to_string());
            return Ok(ReviewVerdict::Approved { summary });
        }
        if trimmed.starts_with("VERDICT: REJECT") {
            let reason = extract_tag(response, "reason").unwrap_or_else(|| {
                // Try to get remaining lines as reason
                lines[i + 1..].join("\n").trim().to_string()
            });
            return Ok(ReviewVerdict::Rejected { reason });
        }
    }

    // If no clear verdict, treat as rejection (safety)
    Ok(ReviewVerdict::Rejected {
        reason: format!(
            "Infestor did not provide a clear verdict. Raw response:\n{}",
            &response[..response.len().min(1000)]
        ),
    })
}

/// Extract content from XML-like tags.
fn extract_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = text.find(&open) {
        if let Some(end) = text.find(&close) {
            let content = &text[start + open.len()..end];
            return Some(content.trim().to_string());
        }
    }
    None
}
