//! Queen Recovery — session-based resurrection for crashed Queens.
//!
//! When a NativeQueen (Claude Code CLI) crashes, its session is persisted
//! at `~/.claude/projects/{project}/{session_id}.jsonl`.
//!
//! Recovery protocol (2 steps):
//! 1. SwarmHost detects dead Queen, reads stored session_id
//! 2. SwarmHost spawns new Queen with `--resume {session_id}` —
//!    Claude Code restores full conversation context automatically

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow, Context};
use crate::v2::types::*;

// ============================================================================
// Session Tracker
// ============================================================================

/// Tracks Claude Code session IDs for each Queen.
/// Session IDs come from the NDJSON `system` event when PipeProcess starts.
#[derive(Debug, Clone, Default)]
pub struct SessionTracker {
    /// queen_id → session metadata
    sessions: HashMap<QueenId, SessionRecord>,
}

/// Metadata about a Queen's Claude Code session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Claude Code session UUID (from NDJSON `system` event)
    pub session_id: String,
    /// When the session was started
    pub started_at: DateTime<Utc>,
    /// Which task was assigned when session started
    pub task_id: Option<TaskId>,
    /// Working directory for this Queen
    pub working_dir: PathBuf,
    /// Last known status before death
    pub last_status: Option<String>,
    /// Last status update timestamp
    pub last_seen: DateTime<Utc>,
    /// How many times this Queen has been recovered
    pub recovery_count: u32,
}

impl SessionTracker {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Register a session for a Queen. Called after PipeProcess emits session_id.
    pub fn register(
        &mut self,
        queen_id: QueenId,
        session_id: String,
        working_dir: PathBuf,
    ) {
        let record = SessionRecord {
            session_id,
            started_at: Utc::now(),
            task_id: None,
            working_dir,
            last_status: None,
            last_seen: Utc::now(),
            recovery_count: 0,
        };
        self.sessions.insert(queen_id, record);
    }

    /// Update the task assignment for a Queen.
    pub fn set_task(&mut self, queen_id: &QueenId, task_id: TaskId) {
        if let Some(record) = self.sessions.get_mut(queen_id) {
            record.task_id = Some(task_id);
        }
    }

    /// Update last known status (called during poll).
    pub fn update_status(&mut self, queen_id: &QueenId, status: &str) {
        if let Some(record) = self.sessions.get_mut(queen_id) {
            record.last_status = Some(status.to_string());
            record.last_seen = Utc::now();
        }
    }

    /// Get session record for a Queen.
    pub fn get(&self, queen_id: &QueenId) -> Option<&SessionRecord> {
        self.sessions.get(queen_id)
    }

    /// Remove a Queen's session (after successful completion or permanent failure).
    pub fn remove(&mut self, queen_id: &QueenId) -> Option<SessionRecord> {
        self.sessions.remove(queen_id)
    }

    /// Increment recovery count for a Queen's session.
    /// Returns the new count, or None if queen not tracked.
    pub fn increment_recovery(&mut self, queen_id: &QueenId) -> Option<u32> {
        if let Some(record) = self.sessions.get_mut(queen_id) {
            record.recovery_count += 1;
            Some(record.recovery_count)
        } else {
            None
        }
    }

    /// Get all tracked Queen IDs.
    pub fn tracked_queens(&self) -> Vec<QueenId> {
        self.sessions.keys().cloned().collect()
    }

    /// Number of tracked sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

// ============================================================================
// Recovery Manager
// ============================================================================

/// Configuration for recovery behavior.
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// Maximum recovery attempts per Queen before giving up
    pub max_recoveries: u32,
    /// How long to wait before considering a Queen stalled (no status updates)
    pub stall_timeout: Duration,
    /// Minimum time between recovery attempts (prevent thrashing)
    pub recovery_cooldown: Duration,
    /// Whether to use --fork-session (new session ID) or --resume (same ID)
    pub fork_on_resume: bool,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_recoveries: 3,
            stall_timeout: Duration::from_secs(300), // 5 minutes
            recovery_cooldown: Duration::from_secs(10),
            fork_on_resume: true, // safer: creates new session, preserves old
        }
    }
}

/// Reason why a Queen needs recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryReason {
    /// Process died (is_alive() returned false)
    ProcessDied,
    /// No status updates for stall_timeout duration
    Stalled,
    /// Queen reported an unrecoverable error
    FatalError(String),
}

/// Instructions for SwarmHost on how to respawn a Queen.
#[derive(Debug, Clone)]
pub struct RecoveryPlan {
    /// Which Queen to recover
    pub queen_id: QueenId,
    /// Why recovery is needed
    pub reason: RecoveryReason,
    /// The session ID to resume from
    pub session_id: String,
    /// Working directory for the new Queen
    pub working_dir: PathBuf,
    /// Task to reassign after recovery
    pub task_id: Option<TaskId>,
    /// How many times this Queen has been recovered before
    pub attempt: u32,
    /// Whether to use --fork-session
    pub fork_session: bool,
    /// Recovery prompt to inject context
    pub recovery_prompt: String,
}

/// Manages Queen health checks and recovery planning.
pub struct RecoveryManager {
    config: RecoveryConfig,
    /// Tracks when each Queen was last checked
    last_check: HashMap<QueenId, Instant>,
    /// Tracks when each Queen was last recovered (cooldown)
    last_recovery: HashMap<QueenId, Instant>,
    /// Queens that have exceeded max_recoveries (permanently failed)
    abandoned: Vec<QueenId>,
}

impl RecoveryManager {
    pub fn new(config: RecoveryConfig) -> Self {
        Self {
            config,
            last_check: HashMap::new(),
            last_recovery: HashMap::new(),
            abandoned: Vec::new(),
        }
    }

    /// Check a Queen's health and decide if recovery is needed.
    /// Returns a RecoveryPlan if recovery should be attempted.
    pub fn check_health(
        &mut self,
        queen_id: &QueenId,
        is_alive: bool,
        session_tracker: &SessionTracker,
    ) -> Option<RecoveryPlan> {
        // Skip abandoned queens
        if self.abandoned.contains(queen_id) {
            return None;
        }

        // Get session record
        let record = session_tracker.get(queen_id)?;

        // Check cooldown
        if let Some(last_recovery_time) = self.last_recovery.get(queen_id) {
            if last_recovery_time.elapsed() < self.config.recovery_cooldown {
                return None; // Too soon to recover again
            }
        }

        // Determine recovery reason
        let reason = if !is_alive {
            Some(RecoveryReason::ProcessDied)
        } else {
            // Check for stall
            let time_since_seen = Utc::now()
                .signed_duration_since(record.last_seen)
                .to_std()
                .unwrap_or(Duration::ZERO);

            if time_since_seen > self.config.stall_timeout {
                Some(RecoveryReason::Stalled)
            } else {
                None
            }
        };

        let reason = reason?;

        // Check max recoveries
        if record.recovery_count >= self.config.max_recoveries {
            self.abandoned.push(queen_id.clone());
            eprintln!(
                "[RecoveryManager] Queen {} abandoned after {} recovery attempts",
                queen_id.0, record.recovery_count
            );
            return None;
        }

        // Build recovery prompt
        let recovery_prompt = build_recovery_prompt(queen_id, record, &reason);

        Some(RecoveryPlan {
            queen_id: queen_id.clone(),
            reason,
            session_id: record.session_id.clone(),
            working_dir: record.working_dir.clone(),
            task_id: record.task_id.clone(),
            attempt: record.recovery_count + 1,
            fork_session: self.config.fork_on_resume,
            recovery_prompt,
        })
    }

    /// Mark a recovery as attempted (updates cooldown timer).
    pub fn mark_recovery_attempted(&mut self, queen_id: &QueenId) {
        self.last_recovery.insert(queen_id.clone(), Instant::now());
    }

    /// Check if a Queen has been permanently abandoned.
    pub fn is_abandoned(&self, queen_id: &QueenId) -> bool {
        self.abandoned.contains(queen_id)
    }

    /// Get list of abandoned Queens.
    pub fn abandoned_queens(&self) -> &[QueenId] {
        &self.abandoned
    }

    /// Reset abandonment for a Queen (e.g., operator override).
    pub fn reset_abandoned(&mut self, queen_id: &QueenId) {
        self.abandoned.retain(|id| id != queen_id);
    }

    /// Get the recovery config.
    pub fn config(&self) -> &RecoveryConfig {
        &self.config
    }
}

// ============================================================================
// Recovery Prompt Builder
// ============================================================================

/// Build a recovery prompt that gives the new Queen context about what happened.
fn build_recovery_prompt(
    queen_id: &QueenId,
    record: &SessionRecord,
    reason: &RecoveryReason,
) -> String {
    let reason_str = match reason {
        RecoveryReason::ProcessDied => "Your previous process crashed/died unexpectedly",
        RecoveryReason::Stalled => "Your previous process stalled (no activity for too long)",
        RecoveryReason::FatalError(e) => &format!("Your previous process hit a fatal error: {}", e),
    };

    let task_str = match &record.task_id {
        Some(task_id) => format!("You were working on task: {}", task_id.0),
        None => "No specific task was assigned".to_string(),
    };

    let status_str = match &record.last_status {
        Some(status) => format!("Last known status: {}", status),
        None => "No status was reported before crash".to_string(),
    };

    format!(
        r#"## RECOVERY NOTICE — You are resuming from a crashed session

{reason_str}.

### Context
- Queen ID: {queen_id}
- {task_str}
- {status_str}
- Recovery attempt: #{attempt}
- Session started: {started}

### Recovery Protocol
1. Re-read the PRD to understand overall progress
2. Check git status to see what files were modified
3. Determine what was completed vs what still needs doing
4. Report your findings via @hatchery:status
5. Continue working from where you left off

### Rules
- Do NOT redo work that's already complete (check git log)
- Report what you find immediately via @hatchery:knowledge
- If you can't determine the state, escalate via @hatchery:escalate

{discipline}
"#,
        queen_id = queen_id.0,
        attempt = record.recovery_count + 1,
        started = record.started_at.format("%Y-%m-%d %H:%M:%S UTC"),
        discipline = crate::v2::prompts::orchestration_discipline_block(),
    )
}

// ============================================================================
// Session File Discovery (for finding sessions without stored ID)
// ============================================================================

/// Resolve the Claude Code projects directory for a given working directory.
/// Returns the path like: ~/.claude/projects/C--Users-VA-PC-CODING-ML-TRADING-nemo/
pub fn claude_projects_dir(working_dir: &std::path::Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let claude_dir = home.join(".claude").join("projects");

    // Claude encodes project path: C:\Users\VA PC → C--Users-VA-PC
    let encoded = working_dir
        .to_string_lossy()
        .replace(['\\', '/'], "-")
        .replace(':', "-");
    // Remove leading dash if any
    let encoded = encoded.trim_start_matches('-').to_string();

    let project_dir = claude_dir.join(&encoded);
    if project_dir.exists() {
        Some(project_dir)
    } else {
        // Try without drive letter dash
        None
    }
}

/// Find the most recent session for a project directory.
/// Reads sessions-index.json and returns the most recently modified session ID.
pub fn find_latest_session(working_dir: &std::path::Path) -> Result<Option<String>> {
    let project_dir = claude_projects_dir(working_dir)
        .ok_or_else(|| anyhow!("Cannot resolve Claude projects directory"))?;

    let index_path = project_dir.join("sessions-index.json");
    if !index_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&index_path)
        .context("Failed to read sessions-index.json")?;

    // Parse as JSON value (the index format may vary)
    let index: serde_json::Value = serde_json::from_str(&content)
        .context("Failed to parse sessions-index.json")?;

    // Find the most recently modified session
    // Index is typically an object with session entries
    let mut latest_session: Option<(String, DateTime<Utc>)> = None;

    // Try array-of-objects format
    if let Some(entries) = index.as_array() {
        for entry in entries {
            if let (Some(session_id), Some(modified)) = (
                entry.get("sessionId").and_then(|v| v.as_str()),
                entry.get("modified").and_then(|v| v.as_str()),
            ) {
                if let Ok(modified_dt) = modified.parse::<DateTime<Utc>>() {
                    match &latest_session {
                        None => latest_session = Some((session_id.to_string(), modified_dt)),
                        Some((_, current_latest)) if modified_dt > *current_latest => {
                            latest_session = Some((session_id.to_string(), modified_dt));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(latest_session.map(|(id, _)| id))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_queen_id(name: &str) -> QueenId {
        QueenId(name.to_string())
    }

    fn make_task_id(name: &str) -> TaskId {
        TaskId(name.to_string())
    }

    // --- SessionTracker tests ---

    #[test]
    fn test_session_tracker_register_and_get() {
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-abc-123".to_string(), PathBuf::from("/tmp/work"));

        let record = tracker.get(&qid).unwrap();
        assert_eq!(record.session_id, "sess-abc-123");
        assert!(record.task_id.is_none());
        assert_eq!(record.recovery_count, 0);
    }

    #[test]
    fn test_session_tracker_set_task() {
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));
        tracker.set_task(&qid, make_task_id("task-1"));

        let record = tracker.get(&qid).unwrap();
        assert_eq!(record.task_id.as_ref().unwrap().0, "task-1");
    }

    #[test]
    fn test_session_tracker_update_status() {
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));
        tracker.update_status(&qid, "Working on sub-task 2/5");

        let record = tracker.get(&qid).unwrap();
        assert_eq!(record.last_status.as_deref(), Some("Working on sub-task 2/5"));
    }

    #[test]
    fn test_session_tracker_remove() {
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));
        assert_eq!(tracker.len(), 1);

        let removed = tracker.remove(&qid);
        assert!(removed.is_some());
        assert_eq!(tracker.len(), 0);
        assert!(tracker.get(&qid).is_none());
    }

    #[test]
    fn test_session_tracker_increment_recovery() {
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));
        assert_eq!(tracker.increment_recovery(&qid), Some(1));
        assert_eq!(tracker.increment_recovery(&qid), Some(2));
        assert_eq!(tracker.increment_recovery(&qid), Some(3));

        let record = tracker.get(&qid).unwrap();
        assert_eq!(record.recovery_count, 3);
    }

    #[test]
    fn test_session_tracker_tracked_queens() {
        let mut tracker = SessionTracker::new();
        tracker.register(make_queen_id("Q0"), "s0".to_string(), PathBuf::from("/a"));
        tracker.register(make_queen_id("Q1"), "s1".to_string(), PathBuf::from("/b"));

        let tracked = tracker.tracked_queens();
        assert_eq!(tracked.len(), 2);
    }

    // --- RecoveryManager tests ---

    #[test]
    fn test_recovery_detects_dead_queen() {
        let config = RecoveryConfig {
            max_recoveries: 3,
            recovery_cooldown: Duration::from_secs(0), // no cooldown for test
            ..Default::default()
        };
        let mut manager = RecoveryManager::new(config);
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-dead".to_string(), PathBuf::from("/tmp"));
        tracker.set_task(&qid, make_task_id("task-1"));

        // Queen is NOT alive → should produce recovery plan
        let plan = manager.check_health(&qid, false, &tracker);
        assert!(plan.is_some());

        let plan = plan.unwrap();
        assert_eq!(plan.queen_id.0, "Q0");
        assert_eq!(plan.session_id, "sess-dead");
        assert_eq!(plan.reason, RecoveryReason::ProcessDied);
        assert_eq!(plan.attempt, 1);
        assert!(plan.task_id.is_some());
    }

    #[test]
    fn test_recovery_alive_queen_no_plan() {
        let config = RecoveryConfig::default();
        let mut manager = RecoveryManager::new(config);
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-ok".to_string(), PathBuf::from("/tmp"));
        // Update status to prevent stall detection
        tracker.update_status(&qid, "working");

        // Queen IS alive → no recovery needed
        let plan = manager.check_health(&qid, true, &tracker);
        assert!(plan.is_none());
    }

    #[test]
    fn test_recovery_respects_max_recoveries() {
        let config = RecoveryConfig {
            max_recoveries: 2,
            recovery_cooldown: Duration::from_secs(0),
            ..Default::default()
        };
        let mut manager = RecoveryManager::new(config);
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));

        // Simulate 2 previous recoveries
        tracker.increment_recovery(&qid);
        tracker.increment_recovery(&qid);

        // 3rd attempt should be rejected (max_recoveries = 2)
        let plan = manager.check_health(&qid, false, &tracker);
        assert!(plan.is_none());
        assert!(manager.is_abandoned(&qid));
    }

    #[test]
    fn test_recovery_cooldown_respected() {
        let config = RecoveryConfig {
            max_recoveries: 5,
            recovery_cooldown: Duration::from_secs(9999),
            ..Default::default()
        };
        let mut manager = RecoveryManager::new(config);
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));

        // Mark recovery attempted
        manager.mark_recovery_attempted(&qid);

        // Immediately try again — should be blocked by cooldown
        let plan = manager.check_health(&qid, false, &tracker);
        assert!(plan.is_none());
    }

    #[test]
    fn test_recovery_prompt_contains_context() {
        let config = RecoveryConfig {
            recovery_cooldown: Duration::from_secs(0),
            ..Default::default()
        };
        let mut manager = RecoveryManager::new(config);
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp/project"));
        tracker.set_task(&qid, make_task_id("implement-auth"));
        tracker.update_status(&qid, "Completed 3/5 sub-tasks");

        let plan = manager.check_health(&qid, false, &tracker).unwrap();

        // Recovery prompt should contain useful context
        assert!(plan.recovery_prompt.contains("RECOVERY NOTICE"));
        assert!(plan.recovery_prompt.contains("Q0"));
        assert!(plan.recovery_prompt.contains("implement-auth"));
        assert!(plan.recovery_prompt.contains("Completed 3/5 sub-tasks"));
        assert!(plan.recovery_prompt.contains("Orchestration Discipline"));
        assert!(plan.recovery_prompt.contains("crashed"));
    }

    #[test]
    fn test_abandoned_queen_skipped() {
        let config = RecoveryConfig {
            max_recoveries: 1,
            recovery_cooldown: Duration::from_secs(0),
            ..Default::default()
        };
        let mut manager = RecoveryManager::new(config);
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));
        tracker.increment_recovery(&qid); // 1 recovery done, max is 1

        // This should abandon the queen
        let plan = manager.check_health(&qid, false, &tracker);
        assert!(plan.is_none());
        assert!(manager.is_abandoned(&qid));

        // Future checks should skip immediately
        let plan = manager.check_health(&qid, false, &tracker);
        assert!(plan.is_none());
    }

    #[test]
    fn test_reset_abandoned() {
        let config = RecoveryConfig {
            max_recoveries: 1,
            recovery_cooldown: Duration::from_secs(0),
            ..Default::default()
        };
        let mut manager = RecoveryManager::new(config);
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));
        tracker.increment_recovery(&qid);

        // Abandon
        manager.check_health(&qid, false, &tracker);
        assert!(manager.is_abandoned(&qid));

        // Reset abandonment (operator override)
        manager.reset_abandoned(&qid);
        assert!(!manager.is_abandoned(&qid));
    }

    #[test]
    fn test_untracked_queen_returns_none() {
        let config = RecoveryConfig::default();
        let mut manager = RecoveryManager::new(config);
        let tracker = SessionTracker::new(); // empty
        let qid = make_queen_id("Q-unknown");

        let plan = manager.check_health(&qid, false, &tracker);
        assert!(plan.is_none());
    }

    #[test]
    fn test_recovery_plan_fork_session_flag() {
        let config = RecoveryConfig {
            fork_on_resume: true,
            recovery_cooldown: Duration::from_secs(0),
            ..Default::default()
        };
        let mut manager = RecoveryManager::new(config);
        let mut tracker = SessionTracker::new();
        let qid = make_queen_id("Q0");

        tracker.register(qid.clone(), "sess-123".to_string(), PathBuf::from("/tmp"));

        let plan = manager.check_health(&qid, false, &tracker).unwrap();
        assert!(plan.fork_session); // Should use --fork-session
    }
}
