//! Deterministic code checks for the Overlord pipeline.
//!
//! Phase 2 of the Overlord pipeline: run deterministic checks on parsed data
//! and decide whether to auto-approve, auto-reject, or send to LLM review.

use super::parsers::{DiffSummary, TestResults, QualityScan, SessionSummary};
use super::task_relevance::analyze_task_relevance;

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

    // Check 3.5: AST quality analysis (for Rust files)
    if let Some(ast_report) = &quality.ast_report {
        // Hard reject if overall score is very low
        if ast_report.total_functions > 0 && ast_report.overall_score < 0.2 {
            return CodeCheckVerdict::HardReject {
                reason: format!(
                    "AST analysis: low quality code (score {:.2}), {}/{} functions are suspicious",
                    ast_report.overall_score,
                    ast_report.suspicious_count,
                    ast_report.total_functions
                ),
            };
        }

        // Hard reject if majority of functions are suspicious
        if ast_report.total_functions > 2 && ast_report.suspicious_count > ast_report.total_functions / 2 {
            return CodeCheckVerdict::HardReject {
                reason: format!(
                    "AST analysis: {}/{} functions are stubs/empty (suspicious)",
                    ast_report.suspicious_count,
                    ast_report.total_functions
                ),
            };
        }
    }

    // Check 3.7: Task relevance analysis
    // Only apply this check when combined with other very suspicious signals
    let relevance = analyze_task_relevance(task_description, diff, quality);

    // Hard reject only if ALL of:
    // 1. Very low relevance (< 0.2)
    // 2. Already above stub threshold (would be rejected by Check 3 anyway if > 0.5)
    // 3. Has suspicious AST functions (indicating stub implementations)
    let has_ast_stubs = quality
        .ast_report
        .as_ref()
        .map(|ast| ast.suspicious_count > 1)
        .unwrap_or(false);

    if has_ast_stubs
        && quality.stub_ratio > 0.35
        && relevance.task_keywords.len() >= 2
        && relevance.relevance_score < 0.2
    {
        return CodeCheckVerdict::HardReject {
            reason: format!(
                "low code quality + task relevance mismatch: score {:.2}, missing keywords: [{}]",
                relevance.relevance_score,
                relevance.missing_keywords.join(", ")
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

    // If there's an AST report, append a summary
    if let Some(ast) = &quality.ast_report {
        lines.push(format!(
            "AST Score: {:.2} ({} functions, {} suspicious)",
            ast.overall_score, ast.total_functions, ast.suspicious_count
        ));
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
        ChangedFile, QualityHit, QualityHitKind, parse_diff_summary, scan_code_quality,
    };

    // Helper to build realistic diff strings
    fn make_diff_str(filename: &str, added_lines: &[&str]) -> String {
        let n = added_lines.len();
        let mut diff = format!(
            "diff --git a/{f} b/{f}\n--- a/{f}\n+++ b/{f}\n@@ -1,0 +1,{n} @@\n",
            f = filename, n = n
        );
        for line in added_lines {
            diff.push_str(&format!("+{}\n", line));
        }
        diff
    }

    // Helper to build numstat output
    fn make_numstat(files: &[(&str, usize, usize)]) -> String {
        files.iter()
            .map(|(name, added, removed)| format!("{}\t{}\t{}", added, removed, name))
            .collect::<Vec<_>>()
            .join("\n")
    }

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
            ast_report: None,
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
            ast_report: None,
        };

        let summary = format_quality_summary(&quality);
        assert!(summary.contains("2 hits"));
        assert!(summary.contains("20.0%"));
        assert!(summary.contains("src/lib.rs"));
    }

    // ============================================================================
    // SCENARIO TESTS - Real-world situations a Queen might produce
    // ============================================================================

    // GROUP A: Clear rejections (HardReject expected)

    #[test]
    fn test_scenario_all_todo_stubs() {
        // Queen wrote 20 lines, all are TODOs and todo!()
        let diff_content = make_diff_str("src/feature.rs", &[
            "// TODO: implement this",
            "fn process() {",
            "    todo!()",
            "}",
            "// TODO: add validation",
            "fn validate() {",
            "    todo!()",
            "}",
            "// TODO: implement handler",
            "fn handle() {",
            "    todo!()",
            "}",
            "// TODO: add tests",
            "fn test_feature() {",
            "    todo!()",
            "}",
            "// TODO: document this",
            "// STUB: replace later",
            "fn stub_fn() {}",
            "// TODO: finish implementation",
        ]);

        let numstat = make_numstat(&[("src/feature.rs", 20, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Implement feature");

        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("mostly stubs") || reason.contains("TODO"));
            }
            _ => panic!("Expected HardReject for all TODOs, got {:?}", verdict),
        }
    }

    #[test]
    fn test_scenario_empty_commit() {
        // Queen committed nothing (0 added, 0 removed)
        let diff = make_diff(0, 0);
        let quality = make_quality(0, 0);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Fix bug");

        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("empty work"));
            }
            _ => panic!("Expected HardReject for empty commit, got {:?}", verdict),
        }
    }

    #[test]
    fn test_scenario_tests_failing() {
        // Queen wrote code but 3 tests fail
        let diff_content = make_diff_str("src/lib.rs", &[
            "pub fn calculate(x: u32) -> u32 {",
            "    x * 2",
            "}",
        ]);

        let numstat = make_numstat(&[("src/lib.rs", 3, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let tests = make_tests(5, 3, vec![
            "test_calculate_zero".to_string(),
            "test_calculate_negative".to_string(),
            "test_calculate_overflow".to_string(),
        ]);
        let session = make_session();

        let verdict = run_code_checks(&diff, Some(&tests), &quality, &session, "Implement calculate");

        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("tests failed"));
                assert!(reason.contains("3 failures"));
            }
            _ => panic!("Expected HardReject for failing tests, got {:?}", verdict),
        }
    }

    #[test]
    fn test_scenario_unimplemented_functions() {
        // 5 functions, all unimplemented!()
        let diff_content = make_diff_str("src/api.rs", &[
            "fn get_user() { unimplemented!() }",
            "fn create_user() { unimplemented!() }",
            "fn update_user() { unimplemented!() }",
            "fn delete_user() { unimplemented!() }",
            "fn list_users() { unimplemented!() }",
        ]);

        let numstat = make_numstat(&[("src/api.rs", 5, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Implement API");

        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("mostly stubs") || reason.contains("TODO"));
            }
            _ => panic!("Expected HardReject for all unimplemented, got {:?}", verdict),
        }
    }

    #[test]
    fn test_scenario_panic_everywhere() {
        // Multiple panic!() calls in production code
        let diff_content = make_diff_str("src/handler.rs", &[
            "fn handle_request() {",
            "    panic!(\"not implemented\")",
            "}",
            "fn process_data() {",
            "    panic!(\"TODO\")",
            "}",
            "fn validate_input() {",
            "    panic!(\"implement this\")",
            "}",
        ]);

        let numstat = make_numstat(&[("src/handler.rs", 9, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Add handler");

        // High stub_ratio due to multiple panic hits
        match verdict {
            CodeCheckVerdict::HardReject { .. } | CodeCheckVerdict::NeedsReview { .. } => {
                // Either is acceptable - depends on stub_ratio calculation
            }
            CodeCheckVerdict::AllClear => panic!("Expected rejection for panic everywhere"),
        }
    }

    // GROUP B: Ambiguous cases (NeedsReview expected)

    #[test]
    fn test_scenario_mixed_real_and_stubs() {
        // 80 lines real code + 5 TODOs (stub_ratio < 0.5 but > 0)
        let mut lines = vec![
            "// Real implementation",
            "pub struct Config {",
            "    pub api_key: String,",
            "    pub endpoint: String,",
            "}",
            "impl Config {",
            "    pub fn new(key: String, endpoint: String) -> Self {",
            "        Config { api_key: key, endpoint }",
            "    }",
            "    pub fn validate(&self) -> Result<(), String> {",
            "        if self.api_key.is_empty() {",
            "            return Err(\"API key required\".to_string());",
            "        }",
            "        Ok(())",
            "    }",
            "}",
            "pub fn process(config: &Config) -> Result<String, String> {",
            "    config.validate()?;",
            "    // TODO: implement actual API call",
            "    Ok(String::new())",
            "}",
        ];
        // Pad with real code to reach 85 lines total
        for _ in 0..64 {
            lines.push("    // More implementation");
        }
        lines.push("// TODO: add rate limiting");
        lines.push("// TODO: add retry logic");
        lines.push("// TODO: add metrics");
        lines.push("// TODO: add logging");

        let diff_content = make_diff_str("src/client.rs", &lines);
        let numstat = make_numstat(&[("src/client.rs", 85, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Implement client");

        match verdict {
            CodeCheckVerdict::NeedsReview { .. } => {
                // Expected: some TODOs but not overwhelming
            }
            _ => panic!("Expected NeedsReview for mixed content, got {:?}", verdict),
        }
    }

    #[test]
    fn test_scenario_single_todo_in_large_change() {
        // 200 lines added, 1 TODO comment
        let mut lines = vec![];
        for _ in 0..199 {
            lines.push("    let x = process_data();");
        }
        lines.push("    // TODO: optimize this loop");

        let diff_content = make_diff_str("src/processor.rs", &lines);
        let numstat = make_numstat(&[("src/processor.rs", 200, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Optimize processor");

        match verdict {
            CodeCheckVerdict::NeedsReview { .. } => {
                // Single TODO in 200 lines is minor but worth review
            }
            _ => panic!("Expected NeedsReview for single TODO, got {:?}", verdict),
        }
    }

    #[test]
    fn test_scenario_mock_in_production_code() {
        // Real implementation but references "mock" in non-test file
        let diff_content = make_diff_str("src/api_client.rs", &[
            "pub struct ApiClient {",
            "    endpoint: String,",
            "    // Using mock client for now",
            "    mock_mode: bool,",
            "}",
            "impl ApiClient {",
            "    pub fn new(endpoint: String) -> Self {",
            "        ApiClient {",
            "            endpoint,",
            "            mock_mode: true,",
            "        }",
            "    }",
            "}",
        ]);

        let numstat = make_numstat(&[("src/api_client.rs", 13, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Add API client");

        match verdict {
            CodeCheckVerdict::NeedsReview { .. } => {
                // Mock in production code is suspicious
            }
            CodeCheckVerdict::HardReject { reason } => {
                // AST analysis might catch this as low-quality stub
                assert!(reason.contains("AST") || reason.contains("quality"));
            }
            _ => panic!("Expected NeedsReview or HardReject for mock in prod, got {:?}", verdict),
        }
    }

    // GROUP C: Clean pass (AllClear expected)

    #[test]
    fn test_scenario_clean_implementation() {
        // 50 lines of real code, no markers, tests pass
        let diff_content = make_diff_str("src/calculator.rs", &[
            "pub fn add(a: u32, b: u32) -> u32 {",
            "    a.checked_add(b).unwrap_or(u32::MAX)",
            "}",
            "pub fn subtract(a: u32, b: u32) -> u32 {",
            "    a.saturating_sub(b)",
            "}",
            "pub fn multiply(a: u32, b: u32) -> u32 {",
            "    a.checked_mul(b).unwrap_or(u32::MAX)",
            "}",
            "pub fn divide(a: u32, b: u32) -> Result<u32, String> {",
            "    if b == 0 {",
            "        return Err(\"Division by zero\".to_string());",
            "    }",
            "    Ok(a / b)",
            "}",
        ]);

        let numstat = make_numstat(&[("src/calculator.rs", 15, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let tests = make_tests(8, 0, vec![]);
        let session = make_session();

        let verdict = run_code_checks(&diff, Some(&tests), &quality, &session, "Add calculator");

        assert_eq!(verdict, CodeCheckVerdict::AllClear);
    }

    #[test]
    fn test_scenario_small_bugfix() {
        // 2 lines changed, no markers, clean
        let diff_content = make_diff_str("src/validator.rs", &[
            "    if value < 0 {",
            "        return Err(\"Value must be positive\".to_string());",
        ]);

        let numstat = make_numstat(&[("src/validator.rs", 2, 2)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Fix validation");

        assert_eq!(verdict, CodeCheckVerdict::AllClear);
    }

    #[test]
    fn test_scenario_todo_in_test_file() {
        // TODO in test file should be ignored for stub detection
        let diff_content = make_diff_str("tests/integration_test.rs", &[
            "#[test]",
            "fn test_feature() {",
            "    // TODO: add more test cases",
            "    assert_eq!(1 + 1, 2);",
            "}",
        ]);

        let numstat = make_numstat(&[("tests/integration_test.rs", 5, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Add test");

        // TODOs in test files still get detected but shouldn't trigger harsh rejection
        match verdict {
            CodeCheckVerdict::AllClear | CodeCheckVerdict::NeedsReview { .. } => {
                // Both acceptable for test files with TODOs
            }
            CodeCheckVerdict::HardReject { .. } => {
                panic!("Should not hard reject TODOs in test files");
            }
        }
    }

    #[test]
    fn test_scenario_mock_in_test_file() {
        // "mock" in test file is fine
        let diff_content = make_diff_str("tests/api_test.rs", &[
            "fn setup_mock_server() {",
            "    let mock = MockServer::start();",
            "    mock.expect_get(\"/api/users\");",
            "}",
        ]);

        let numstat = make_numstat(&[("tests/api_test.rs", 4, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Add mock test");

        // Mock in test file should NOT be flagged
        assert_eq!(quality.total_hits, 0, "Mock in test file should not be flagged");
        assert_eq!(verdict, CodeCheckVerdict::AllClear);
    }

    // GROUP D: Edge cases (the hard ones)

    #[test]
    fn test_scenario_default_return_only() {
        // Function that just returns Default::default()
        let diff_content = make_diff_str("src/builder.rs", &[
            "pub fn build() -> Config {",
            "    return Default::default();",
            "}",
        ]);

        let numstat = make_numstat(&[("src/builder.rs", 3, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Add builder");

        // Should trigger DefaultReturn quality hit
        assert!(quality.hits.iter().any(|h| matches!(h.kind, QualityHitKind::DefaultReturn)));

        match verdict {
            CodeCheckVerdict::NeedsReview { .. } => {
                // Expected: suspicious but not hard reject
            }
            _ => panic!("Expected NeedsReview for default return, got {:?}", verdict),
        }
    }

    #[test]
    fn test_scenario_empty_function_body() {
        // fn process() {} - already detected by EmptyFunction
        let diff_content = make_diff_str("src/handler.rs", &[
            "fn process() {}",
            "fn validate() { }",
        ]);

        let numstat = make_numstat(&[("src/handler.rs", 2, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Add handlers");

        // Should detect EmptyFunction
        assert!(quality.hits.iter().any(|h| matches!(h.kind, QualityHitKind::EmptyFunction)));

        match verdict {
            CodeCheckVerdict::HardReject { .. } | CodeCheckVerdict::NeedsReview { .. } => {
                // Either is acceptable
            }
            CodeCheckVerdict::AllClear => panic!("Empty functions should not be AllClear"),
        }
    }

    #[test]
    fn test_scenario_formal_but_useless() {
        // Passes cargo check but function body is just `let _ = input; return 0;`
        let diff_content = make_diff_str("src/processor.rs", &[
            "pub fn process(input: &str) -> u32 {",
            "    let _ = input;",
            "    return 0;",
            "}",
        ]);

        let numstat = make_numstat(&[("src/processor.rs", 4, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Add processor");

        // Should trigger DefaultReturn for `return 0;`
        assert!(quality.hits.iter().any(|h| matches!(h.kind, QualityHitKind::DefaultReturn)));

        match verdict {
            CodeCheckVerdict::NeedsReview { .. } => {
                // Suspicious pattern caught
            }
            CodeCheckVerdict::HardReject { reason } => {
                // AST analysis might catch this as low-quality stub
                assert!(reason.contains("AST") || reason.contains("quality"));
            }
            _ => panic!("Expected NeedsReview or HardReject for useless function, got {:?}", verdict),
        }
    }

    #[test]
    fn test_scenario_one_line_real_fix() {
        // Legitimate 1-line fix. Must be AllClear, not confused with "small = suspicious"
        let diff_content = make_diff_str("src/utils.rs", &[
            "    if input.is_empty() { return Err(\"Empty input\".to_string()); }",
        ]);

        let numstat = make_numstat(&[("src/utils.rs", 1, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Fix validation");

        // No quality issues, should be AllClear
        assert_eq!(quality.total_hits, 0);
        assert_eq!(verdict, CodeCheckVerdict::AllClear);
    }

    #[test]
    fn test_scenario_ast_catches_multiple_stubs() {
        // Multiple stub functions - AST should catch them
        let diff_content = make_diff_str("src/api.rs", &[
            "pub fn get_user(id: u32) -> User {",
            "    Default::default()",
            "}",
            "",
            "pub fn create_user(name: &str) -> User {",
            "    let _ = name;",
            "    Default::default()",
            "}",
            "",
            "pub fn delete_user(id: u32) -> bool {",
            "    let _ = id;",
            "    false",
            "}",
        ]);

        let numstat = make_numstat(&[("src/api.rs", 13, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Implement API");

        // Should be caught by AST analysis (3 functions, all suspicious)
        match verdict {
            CodeCheckVerdict::HardReject { reason } => {
                assert!(reason.contains("AST") || reason.contains("stubs"));
            }
            _ => panic!("Expected HardReject for stub functions, got {:?}", verdict),
        }

        // Verify AST report exists and detected the issues
        if let Some(ast) = &quality.ast_report {
            assert_eq!(ast.total_functions, 3, "Should analyze 3 functions");
            assert!(ast.suspicious_count >= 2, "At least 2 functions should be suspicious");
            assert!(ast.overall_score < 0.3, "Overall score should be low");
        } else {
            panic!("AST report should exist for Rust file");
        }
    }

    #[test]
    fn test_scenario_ast_accepts_quality_code() {
        // Real implementation - AST should score high
        let diff_content = make_diff_str("src/validator.rs", &[
            "pub fn validate_email(email: &str) -> Result<(), String> {",
            "    if email.is_empty() {",
            "        return Err(\"Email cannot be empty\".to_string());",
            "    }",
            "    if !email.contains('@') {",
            "        return Err(\"Email must contain @\".to_string());",
            "    }",
            "    let parts: Vec<&str> = email.split('@').collect();",
            "    if parts.len() != 2 {",
            "        return Err(\"Invalid email format\".to_string());",
            "    }",
            "    if parts[0].is_empty() || parts[1].is_empty() {",
            "        return Err(\"Email parts cannot be empty\".to_string());",
            "    }",
            "    Ok(())",
            "}",
        ]);

        let numstat = make_numstat(&[("src/validator.rs", 16, 0)]);
        let diff = parse_diff_summary(&numstat);
        let quality = scan_code_quality(&diff_content);
        let session = make_session();

        let verdict = run_code_checks(&diff, None, &quality, &session, "Add email validator");

        // Should be AllClear - no quality hits, good AST score
        assert_eq!(verdict, CodeCheckVerdict::AllClear);

        // Verify AST report shows good quality
        if let Some(ast) = &quality.ast_report {
            assert_eq!(ast.total_functions, 1);
            assert!(ast.overall_score > 0.7, "Real code should score high, got {:.2}", ast.overall_score);
            assert_eq!(ast.suspicious_count, 0, "No functions should be suspicious");
        }
    }
}
