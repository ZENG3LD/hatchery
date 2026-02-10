//! Multi-signal completion detection for Claude Code subprocesses.
//!
//! Replaces the fragile `@hatchery:` text protocol with reliable
//! process-level signals: exit codes, NDJSON result events, timeouts.

use std::process::Command;
use std::time::{Duration, Instant};

/// Configuration for completion detection.
#[derive(Debug, Clone)]
pub struct CompletionConfig {
    /// Idle timeout — no stdout activity for this duration means task is stuck/done.
    /// Default: 120 seconds.
    pub idle_timeout: Duration,

    /// Maximum agent turns (maps to `--max-turns` CLI flag).
    /// None means unlimited.
    pub max_turns: Option<u32>,

    /// Maximum budget in USD (maps to `--max-budget-usd` CLI flag).
    /// None means unlimited.
    pub max_budget_usd: Option<f64>,

    /// Quality gate commands to run after completion (e.g. "cargo check").
    /// Empty means no quality gates.
    pub quality_gates: Vec<String>,

    /// Working directory for quality gate commands.
    pub working_dir: Option<std::path::PathBuf>,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(120),
            max_turns: None,
            max_budget_usd: None,
            quality_gates: Vec::new(),
            working_dir: None,
        }
    }
}

/// Signal that indicates a task may be complete.
#[derive(Debug)]
pub enum CompletionSignal {
    /// Process exited with an exit code.
    ProcessExit {
        code: Option<i32>,
    },
    /// Received a NDJSON `{"type":"result"}` event.
    ResultEvent {
        subtype: String,
        result_text: Option<String>,
        cost_usd: f64,
        duration_ms: u64,
        num_turns: u32,
        session_id: Option<String>,
    },
    /// No stdout activity for idle_timeout duration.
    IdleTimeout {
        idle_duration: Duration,
    },
    /// Max turns limit reached (from result event subtype).
    MaxTurns {
        turns: u32,
    },
}

/// Verdict from the completion detector.
#[derive(Debug)]
pub enum CompletionVerdict {
    /// Task completed successfully.
    Success {
        result_text: String,
        cost_usd: f64,
        duration_ms: u64,
        num_turns: u32,
        session_id: Option<String>,
        quality_passed: bool,
    },
    /// Task failed.
    Failed {
        error: String,
        cost_usd: f64,
        num_turns: u32,
    },
    /// Task timed out (idle or budget exceeded).
    TimedOut {
        reason: String,
    },
}

/// Multi-signal completion detector.
///
/// Evaluates completion signals against configuration to determine
/// whether a Claude Code subprocess task has finished.
pub struct CompletionDetector {
    config: CompletionConfig,
}

impl CompletionDetector {
    pub fn new(config: CompletionConfig) -> Self {
        Self { config }
    }

    /// Evaluate a completion signal and return a verdict.
    pub fn evaluate(&self, signal: &CompletionSignal) -> CompletionVerdict {
        match signal {
            CompletionSignal::ResultEvent {
                subtype,
                result_text,
                cost_usd,
                duration_ms,
                num_turns,
                session_id,
            } => {
                if subtype == "success" {
                    // Check for rate limit in output — these are NOT real completions
                    let result_lower = result_text.as_deref().unwrap_or("").to_lowercase();
                    if result_lower.contains("hit your limit")
                        || result_lower.contains("rate limit")
                        || (result_lower.contains("resets ") && result_lower.len() < 200)
                    {
                        return CompletionVerdict::Failed {
                            error: "rate_limit_hit".to_string(),
                            cost_usd: *cost_usd,
                            num_turns: *num_turns,
                        };
                    }

                    let quality_passed = self.run_quality_gates();
                    CompletionVerdict::Success {
                        result_text: result_text.clone().unwrap_or_default(),
                        cost_usd: *cost_usd,
                        duration_ms: *duration_ms,
                        num_turns: *num_turns,
                        session_id: session_id.clone(),
                        quality_passed,
                    }
                } else {
                    // error_max_turns, error_max_budget_usd, error_during_execution
                    CompletionVerdict::Failed {
                        error: format!("Result subtype: {}", subtype),
                        cost_usd: *cost_usd,
                        num_turns: *num_turns,
                    }
                }
            }

            CompletionSignal::ProcessExit { code } => {
                match code {
                    Some(0) => {
                        let quality_passed = self.run_quality_gates();
                        CompletionVerdict::Success {
                            result_text: String::new(),
                            cost_usd: 0.0,
                            duration_ms: 0,
                            num_turns: 0,
                            session_id: None,
                            quality_passed,
                        }
                    }
                    Some(code) => CompletionVerdict::Failed {
                        error: format!("Process exited with code {}", code),
                        cost_usd: 0.0,
                        num_turns: 0,
                    },
                    None => CompletionVerdict::Failed {
                        error: "Process killed by signal".to_string(),
                        cost_usd: 0.0,
                        num_turns: 0,
                    },
                }
            }

            CompletionSignal::IdleTimeout { idle_duration } => {
                CompletionVerdict::TimedOut {
                    reason: format!(
                        "No output for {:.0}s (timeout: {:.0}s)",
                        idle_duration.as_secs_f64(),
                        self.config.idle_timeout.as_secs_f64()
                    ),
                }
            }

            CompletionSignal::MaxTurns { turns } => {
                CompletionVerdict::TimedOut {
                    reason: format!("Max turns reached: {}", turns),
                }
            }
        }
    }

    /// Check if enough time has passed since last activity to trigger idle timeout.
    pub fn is_idle_timeout(&self, last_activity: Instant) -> bool {
        last_activity.elapsed() >= self.config.idle_timeout
    }

    /// Run quality gate commands. Returns true if all pass (or if no gates configured).
    fn run_quality_gates(&self) -> bool {
        if self.config.quality_gates.is_empty() {
            return true;
        }

        let working_dir = self.config.working_dir.as_deref();

        for gate_cmd in &self.config.quality_gates {
            let parts: Vec<&str> = gate_cmd.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let mut cmd = Command::new(parts[0]);
            if parts.len() > 1 {
                cmd.args(&parts[1..]);
            }
            if let Some(dir) = working_dir {
                cmd.current_dir(dir);
            }

            match cmd.output() {
                Ok(output) => {
                    if !output.status.success() {
                        eprintln!(
                            "[COMPLETION] Quality gate failed: {} (exit {})",
                            gate_cmd,
                            output.status.code().unwrap_or(-1)
                        );
                        return false;
                    }
                }
                Err(e) => {
                    eprintln!("[COMPLETION] Quality gate error: {} — {}", gate_cmd, e);
                    return false;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_result_event() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ResultEvent {
            subtype: "success".to_string(),
            result_text: Some("All done".to_string()),
            cost_usd: 0.15,
            duration_ms: 5000,
            num_turns: 3,
            session_id: Some("sess-123".to_string()),
        };
        match detector.evaluate(&signal) {
            CompletionVerdict::Success { result_text, cost_usd, num_turns, quality_passed, .. } => {
                assert_eq!(result_text, "All done");
                assert_eq!(cost_usd, 0.15);
                assert_eq!(num_turns, 3);
                assert!(quality_passed);
            }
            other => panic!("Expected Success, got {:?}", other),
        }
    }

    #[test]
    fn test_error_result_event() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ResultEvent {
            subtype: "error_max_turns".to_string(),
            result_text: None,
            cost_usd: 5.0,
            duration_ms: 60000,
            num_turns: 20,
            session_id: None,
        };
        match detector.evaluate(&signal) {
            CompletionVerdict::Failed { error, cost_usd, num_turns } => {
                assert!(error.contains("error_max_turns"));
                assert_eq!(cost_usd, 5.0);
                assert_eq!(num_turns, 20);
            }
            other => panic!("Expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn test_process_exit_success() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ProcessExit { code: Some(0) };
        match detector.evaluate(&signal) {
            CompletionVerdict::Success { quality_passed, .. } => {
                assert!(quality_passed);
            }
            other => panic!("Expected Success, got {:?}", other),
        }
    }

    #[test]
    fn test_process_exit_error() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ProcessExit { code: Some(1) };
        match detector.evaluate(&signal) {
            CompletionVerdict::Failed { error, .. } => {
                assert!(error.contains("code 1"));
            }
            other => panic!("Expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn test_process_killed() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ProcessExit { code: None };
        match detector.evaluate(&signal) {
            CompletionVerdict::Failed { error, .. } => {
                assert!(error.contains("signal"));
            }
            other => panic!("Expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn test_idle_timeout() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::IdleTimeout {
            idle_duration: Duration::from_secs(130),
        };
        match detector.evaluate(&signal) {
            CompletionVerdict::TimedOut { reason } => {
                assert!(reason.contains("130"));
            }
            other => panic!("Expected TimedOut, got {:?}", other),
        }
    }

    #[test]
    fn test_max_turns() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::MaxTurns { turns: 20 };
        match detector.evaluate(&signal) {
            CompletionVerdict::TimedOut { reason } => {
                assert!(reason.contains("20"));
            }
            other => panic!("Expected TimedOut, got {:?}", other),
        }
    }

    #[test]
    fn test_is_idle_timeout_false() {
        let config = CompletionConfig {
            idle_timeout: Duration::from_secs(120),
            ..Default::default()
        };
        let detector = CompletionDetector::new(config);
        let last_activity = Instant::now();
        assert!(!detector.is_idle_timeout(last_activity));
    }

    #[test]
    fn test_quality_gate_no_gates() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        assert!(detector.run_quality_gates());
    }

    #[test]
    fn test_rate_limit_detection_hit_your_limit() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ResultEvent {
            subtype: "success".to_string(),
            result_text: Some("You've hit your limit · resets 1pm".to_string()),
            cost_usd: 0.05,
            duration_ms: 1000,
            num_turns: 1,
            session_id: None,
        };
        match detector.evaluate(&signal) {
            CompletionVerdict::Failed { error, cost_usd, num_turns } => {
                assert_eq!(error, "rate_limit_hit");
                assert_eq!(cost_usd, 0.05);
                assert_eq!(num_turns, 1);
            }
            other => panic!("Expected Failed due to rate limit, got {:?}", other),
        }
    }

    #[test]
    fn test_rate_limit_detection_rate_limit_keyword() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ResultEvent {
            subtype: "success".to_string(),
            result_text: Some("Error: API rate limit exceeded".to_string()),
            cost_usd: 0.02,
            duration_ms: 500,
            num_turns: 1,
            session_id: None,
        };
        match detector.evaluate(&signal) {
            CompletionVerdict::Failed { error, .. } => {
                assert_eq!(error, "rate_limit_hit");
            }
            other => panic!("Expected Failed due to rate limit, got {:?}", other),
        }
    }

    #[test]
    fn test_rate_limit_detection_resets_keyword() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ResultEvent {
            subtype: "success".to_string(),
            result_text: Some("Limit resets at 3pm".to_string()),
            cost_usd: 0.01,
            duration_ms: 200,
            num_turns: 1,
            session_id: None,
        };
        match detector.evaluate(&signal) {
            CompletionVerdict::Failed { error, .. } => {
                assert_eq!(error, "rate_limit_hit");
            }
            other => panic!("Expected Failed due to rate limit, got {:?}", other),
        }
    }

    #[test]
    fn test_normal_success_not_flagged_as_rate_limit() {
        let detector = CompletionDetector::new(CompletionConfig::default());
        let signal = CompletionSignal::ResultEvent {
            subtype: "success".to_string(),
            result_text: Some("Task completed successfully. All tests pass.".to_string()),
            cost_usd: 0.15,
            duration_ms: 5000,
            num_turns: 3,
            session_id: Some("sess-123".to_string()),
        };
        match detector.evaluate(&signal) {
            CompletionVerdict::Success { result_text, .. } => {
                assert!(result_text.contains("successfully"));
            }
            other => panic!("Expected Success for normal completion, got {:?}", other),
        }
    }
}
