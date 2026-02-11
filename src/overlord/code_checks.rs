//! Deterministic code checks for the Overlord pipeline.
//!
//! Phase 2 of the Overlord pipeline: run deterministic checks on parsed data
//! and decide whether to auto-approve, auto-reject, or send to LLM review.

use super::parsers::{DiffSummary, TestResults, QualityScan, SessionSummary};

/// Deterministic verdict from code checks
#[derive(Debug, Clone, PartialEq)]
pub enum CodeCheckVerdict {
    /// All checks pass, auto-approve
    AllClear,
    /// Hard failure, auto-reject
    HardReject { reason: String },
    /// Ambiguous, needs LLM review
    NeedsReview { report: ReviewReport },
}

/// Structured report for LLM review (when deterministic checks are ambiguous)
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewReport {
    pub diff_summary: String,
    pub test_summary: String,
    pub quality_summary: String,
    pub session_summary: String,
    pub task_description: String,
}

/// Run deterministic code checks pipeline
///
/// Logic:
/// 1. No diff (total_added == 0 && total_removed == 0) → HardReject "empty work, no changes"
/// 2. Tests exist and failed > 0 → HardReject "tests failed: {N} failures"
/// 3. stub_ratio > 0.5 (more than 50% of added lines are stubs/TODOs) → HardReject "mostly stubs/TODOs"
/// 4. All clean (no quality hits, tests pass or no tests) → AllClear
/// 5. Otherwise → NeedsReview with formatted report
pub fn run_code_checks(
    diff: &DiffSummary,
    tests: Option<&TestResults>,
    quality: &QualityScan,
    session: &SessionSummary,
    task_description: &str,
) -> CodeCheckVerdict {
    // Check 1: No changes at all
    if diff.total_added == 0 && diff.total_removed == 0 {
        return CodeCheckVerdict::HardReject {
            reason: "empty work, no changes".to_string(),
        };
    }

    // Check 2: Tests failed
    if let Some(test_results) = tests {
        if test_results.failed > 0 {
            let failure_list = if test_results.failures.is_empty() {
                String::new()
            } else {
                format!(": {}", test_results.failures.join(", "))
            };
            return CodeCheckVerdict::HardReject {
                reason: format!(
                    "tests failed: {} failure{}{}",
                    test_results.failed,
                    if test_results.failed == 1 { "" } else { "s" },
                    failure_list
                ),
            };
        }
    }

    // Check 3: Too many stubs/TODOs
    if quality.stub_ratio > 0.5 {
        return CodeCheckVerdict::HardReject {
            reason: format!(
                "mostly stubs/TODOs: {:.1}% of added lines ({} hits / {} lines)",
                quality.stub_ratio * 100.0,
                quality.total_hits,
                diff.total_added
            ),
        };
    }

    // Check 4: All clean
    let tests_pass = tests.map(|t| t.failed == 0).unwrap_or(true);
    if quality.total_hits == 0 && tests_pass {
        return CodeCheckVerdict::AllClear;
    }

    // Check 5: Ambiguous, needs LLM review
    let diff_summary = format_diff_summary(diff);
    let test_summary = format_test_summary(tests);
    let quality_summary = format_quality_summary(quality);
    let session_summary_str = format_session_summary(session);

    CodeCheckVerdict::NeedsReview {
        report: ReviewReport {
            diff_summary,
            test_summary,
            quality_summary,
            session_summary: session_summary_str,
            task_description: task_description.to_string(),
        },
    }
}

fn format_diff_summary(diff: &DiffSummary) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Files changed: {}, +{} -{} lines",
        diff.total_files, diff.total_added, diff.total_removed
    ));

    if !diff.files.is_empty() {
        lines.push("Changed files:".to_string());
        for file in &diff.files {
            lines.push(format!("  {} (+{} -{})", file.path, file.added, file.removed));
        }
    }

    lines.join("\n")
}

fn format_test_summary(tests: Option<&TestResults>) -> String {
    match tests {
        Some(t) => {
            let mut lines = Vec::new();
            lines.push(format!(
                "Tests: {} total, {} passed, {} failed, {} ignored",
                t.total, t.passed, t.failed, t.ignored
            ));

            if !t.failures.is_empty() {
                lines.push("Failed tests:".to_string());
                for failure in &t.failures {
                    lines.push(format!("  - {}", failure));
                }
            }

            lines.join("\n")
        }
        None => "No test results available".to_string(),
    }
}

fn format_quality_summary(quality: &QualityScan) -> String {
    if quality.total_hits == 0 {
        return "No quality issues detected".to_string();
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "Quality issues: {} hits (stub ratio: {:.1}%)",
        quality.total_hits,
        quality.stub_ratio * 100.0
    ));

    // Group by kind
    let mut by_kind: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for hit in &quality.hits {
        let kind_str = format!("{:?}", hit.kind);
        *by_kind.entry(kind_str).or_insert(0) += 1;
    }

    for (kind, count) in by_kind.iter() {
        lines.push(format!("  {}: {}", kind, count));
    }

    // Show first 5 hits as examples
    if quality.total_hits > 0 {
        lines.push("Examples:".to_string());
        for (i, hit) in quality.hits.iter().take(5).enumerate() {
            lines.push(format!(
                "  {}. {}:{} [{:?}] {}",
                i + 1,
                hit.file,
                hit.line,
                hit.kind,
                hit.text.chars().take(60).collect::<String>()
            ));
        }
    }

    lines.join("\n")
}

fn format_session_summary(session: &SessionSummary) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Duration: {:.1}s, Cost: ${:.4}, Turns: {}, Files: {}",
        session.duration_secs, session.total_cost_usd, session.total_turns, session.files_changed
    ));

    if !session.tools_used.is_empty() {
        lines.push(format!("Tools: {}", session.tools_used.join(", ")));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlord::parsers::{
        ChangedFile, QualityHit, QualityHitKind,
    };

    fn make_diff(added: usize, removed: usize) -> DiffSummary {
        DiffSummary {
            files: vec![ChangedFile {
                path: "src/lib.rs".to_string(),
                added,
                removed,
            }],
            total_added: added,
            total_removed: removed,
            total_files: 1,
        }
    }

    fn make_tests(passed: usize, failed: usize, failures: Vec<String>) -> TestResults {
        TestResults {
            passed,
            failed,
            ignored: 0,
            total: passed + failed,
            raw_output: String::new(),
            failures,
        }
    }

    fn make_quality(hits: usize, added_lines: usize) -> QualityScan {
        let hit_vec: Vec<QualityHit> = (0..hits)
            .map(|i| QualityHit {
                file: "src/lib.rs".to_string(),
                line: i,
                kind: QualityHitKind::Todo,
                text: "// TODO".to_string(),
            })
            .collect();

        let stub_ratio = if added_lines > 0 {
            hits as f64 / added_lines as f64
        } else {
            0.0
        };

        QualityScan {
            hits: hit_vec,
            total_hits: hits,
            stub_ratio,
        }
    }

    fn make_session() -> SessionSummary {
        SessionSummary {
            duration_secs: 100.0,
            total_cost_usd: 0.5,
            total_turns: 10,
            tools_used: vec!["Read".to_string()],
            files_changed: 1,
        }
    }

    #[test]
    fn test_empty_diff_hard_reject() {
        let diff = make_diff(0, 0);
        let quality = make_quality(0, 0);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Test task");

        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("empty work"));
            }
            _ => panic!("Expected HardReject for empty diff"),
        }
    }

    #[test]
    fn test_failed_tests_hard_reject() {
        let diff = make_diff(10, 5);
        let tests = make_tests(8, 2, vec!["test_foo".to_string(), "test_bar".to_string()]);
        let quality = make_quality(0, 10);
        let session = make_session();

        let verdict = run_code_checks(&diff, Some(&tests), &quality, &session, "Test task");

        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("tests failed"));
                assert!(reason.contains("2 failures"));
            }
            _ => panic!("Expected HardReject for failed tests"),
        }
    }

    #[test]
    fn test_high_stub_ratio_hard_reject() {
        let diff = make_diff(10, 0);
        let quality = make_quality(8, 10); // 80% stub ratio
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Test task");

        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("mostly stubs"));
            }
            _ => panic!("Expected HardReject for high stub ratio"),
        }
    }

    #[test]
    fn test_all_clear() {
        let diff = make_diff(10, 5);
        let tests = make_tests(10, 0, vec![]);
        let quality = make_quality(0, 10);
        let session = make_session();

        let verdict = run_code_checks(&diff, Some(&tests), &quality, &session, "Test task");

        assert_eq!(verdict, CodeCheckVerdict::AllClear);
    }

    #[test]
    fn test_all_clear_no_tests() {
        let diff = make_diff(10, 5);
        let quality = make_quality(0, 10);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Test task");

        assert_eq!(verdict, CodeCheckVerdict::AllClear);
    }

    #[test]
    fn test_needs_review_with_quality_hits() {
        let diff = make_diff(20, 5);
        let tests = make_tests(10, 0, vec![]);
        let quality = make_quality(3, 20); // 15% stub ratio (below 50% threshold)
        let session = make_session();

        let verdict = run_code_checks(&diff, Some(&tests), &quality, &session, "Test task");

        match verdict {
            CodeCheckVerdict::NeedsReview { report } => {
                assert!(report.diff_summary.contains("20"));
                assert!(report.test_summary.contains("10 passed"));
                assert!(report.quality_summary.contains("3 hits"));
                assert_eq!(report.task_description, "Test task");
            }
            _ => panic!("Expected NeedsReview for ambiguous case"),
        }
    }

    #[test]
    fn test_needs_review_threshold_boundary() {
        let diff = make_diff(10, 0);
        let quality = make_quality(5, 10); // Exactly 50% - should be HardReject
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Test task");

        // At exactly 50%, we're still below the >0.5 threshold, so NeedsReview
        // But actually 5/10 = 0.5, and 0.5 is NOT > 0.5, so this goes to NeedsReview
        match verdict {
            CodeCheckVerdict::NeedsReview { .. } => {
                // This is correct - 0.5 is not > 0.5
            }
            _ => panic!("Expected NeedsReview at exactly 50%"),
        }
    }

    #[test]
    fn test_needs_review_just_above_threshold() {
        let diff = make_diff(10, 0);
        let quality = make_quality(6, 10); // 60% stub ratio
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Test task");

        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("mostly stubs"));
            }
            _ => panic!("Expected HardReject for >50% stubs"),
        }
    }

    #[test]
    fn test_format_diff_summary() {
        let diff = DiffSummary {
            files: vec![
                ChangedFile {
                    path: "src/lib.rs".to_string(),
                    added: 10,
                    removed: 5,
                },
                ChangedFile {
                    path: "src/main.rs".to_string(),
                    added: 3,
                    removed: 0,
                },
            ],
            total_added: 13,
            total_removed: 5,
            total_files: 2,
        };

        let summary = format_diff_summary(&diff);
        assert!(summary.contains("Files changed: 2"));
        assert!(summary.contains("+13 -5"));
        assert!(summary.contains("src/lib.rs"));
        assert!(summary.contains("src/main.rs"));
    }

    #[test]
    fn test_format_test_summary_with_failures() {
        let tests = make_tests(8, 2, vec!["test_a".to_string(), "test_b".to_string()]);
        let summary = format_test_summary(Some(&tests));

        assert!(summary.contains("10 total"));
        assert!(summary.contains("8 passed"));
        assert!(summary.contains("2 failed"));
        assert!(summary.contains("test_a"));
        assert!(summary.contains("test_b"));
    }

    #[test]
    fn test_format_test_summary_none() {
        let summary = format_test_summary(None);
        assert_eq!(summary, "No test results available");
    }

    #[test]
    fn test_format_quality_summary_clean() {
        let quality = make_quality(0, 10);
        let summary = format_quality_summary(&quality);
        assert_eq!(summary, "No quality issues detected");
    }

    #[test]
    fn test_format_quality_summary_with_hits() {
        let quality = QualityScan {
            hits: vec![
                QualityHit {
                    file: "src/lib.rs".to_string(),
                    line: 42,
                    kind: QualityHitKind::Todo,
                    text: "// TODO: fix this".to_string(),
                },
                QualityHit {
                    file: "src/main.rs".to_string(),
                    line: 10,
                    kind: QualityHitKind::Stub,
                    text: "fn stub() {}".to_string(),
                },
            ],
            total_hits: 2,
            stub_ratio: 0.2,
        };

        let summary = format_quality_summary(&quality);
        assert!(summary.contains("2 hits"));
        assert!(summary.contains("20.0%"));
        assert!(summary.contains("src/lib.rs"));
    }
}
