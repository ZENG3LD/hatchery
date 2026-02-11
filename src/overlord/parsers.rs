//! Parsers for git diffs, test results, and code quality scans.
//!
//! Phase 1 of the Overlord pipeline: parse raw tool outputs into structured data.

use regex::Regex;

/// Summary of git diff changes
#[derive(Debug, Clone, PartialEq)]
pub struct DiffSummary {
    pub files: Vec<ChangedFile>,
    pub total_added: usize,
    pub total_removed: usize,
    pub total_files: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChangedFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

/// Parse `git diff --numstat` output into DiffSummary
///
/// Format: `added\tremoved\tpath` (tab-separated)
/// Binary files show `-` for both counts (treated as 0)
pub fn parse_diff_summary(numstat_output: &str) -> DiffSummary {
    let mut files = Vec::new();
    let mut total_added = 0;
    let mut total_removed = 0;

    for line in numstat_output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let added = if parts[0] == "-" {
            0
        } else {
            parts[0].parse::<usize>().unwrap_or(0)
        };

        let removed = if parts[1] == "-" {
            0
        } else {
            parts[1].parse::<usize>().unwrap_or(0)
        };

        let path = parts[2].to_string();

        total_added += added;
        total_removed += removed;

        files.push(ChangedFile {
            path,
            added,
            removed,
        });
    }

    DiffSummary {
        total_files: files.len(),
        files,
        total_added,
        total_removed,
    }
}

/// Test results from cargo test
#[derive(Debug, Clone, PartialEq)]
pub struct TestResults {
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
    pub total: usize,
    pub raw_output: String,
    pub failures: Vec<String>,
}

/// Parse cargo test output
///
/// Looks for:
/// - `test result: ok. X passed; Y failed; Z ignored`
/// - `test result: FAILED. X passed; Y failed; Z ignored`
/// - Individual failures: `test some::test_name ... FAILED`
pub fn parse_test_results(output: &str) -> TestResults {
    let mut passed = 0;
    let mut failed = 0;
    let mut ignored = 0;
    let mut failures = Vec::new();

    // Parse individual test failures
    let test_failed_re = Regex::new(r"test\s+(\S+)\s+\.\.\.\s+FAILED").unwrap();
    for cap in test_failed_re.captures_iter(output) {
        if let Some(name) = cap.get(1) {
            failures.push(name.as_str().to_string());
        }
    }

    // Parse summary line
    let summary_re = Regex::new(
        r"test result: (?:ok|FAILED)\.\s*(\d+)\s+passed;\s*(\d+)\s+failed;\s*(\d+)\s+ignored"
    ).unwrap();

    if let Some(cap) = summary_re.captures(output) {
        passed = cap.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        failed = cap.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        ignored = cap.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
    }

    let total = passed + failed + ignored;

    TestResults {
        passed,
        failed,
        ignored,
        total,
        raw_output: output.to_string(),
        failures,
    }
}

/// Code quality scan results
#[derive(Debug, Clone, PartialEq)]
pub struct QualityScan {
    pub hits: Vec<QualityHit>,
    pub total_hits: usize,
    pub stub_ratio: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QualityHit {
    pub file: String,
    pub line: usize,
    pub kind: QualityHitKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QualityHitKind {
    Todo,
    Stub,
    Mock,
    Unimplemented,
    Placeholder,
    EmptyFunction,
}

/// Scan diff content for quality issues
///
/// Takes the output of `git diff <base>` (unified diff format)
/// Only scans added lines (lines starting with `+`)
pub fn scan_code_quality(diff_output: &str) -> QualityScan {
    let mut hits = Vec::new();
    let mut current_file = String::new();
    let mut current_line = 0;
    let mut total_added_lines = 0;

    let file_header_re = Regex::new(r"^\+\+\+ b/(.+)$").unwrap();
    let hunk_header_re = Regex::new(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@").unwrap();

    for line in diff_output.lines() {
        // Track current file
        if let Some(cap) = file_header_re.captures(line) {
            current_file = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
            continue;
        }

        // Track current line number from hunk headers
        if let Some(cap) = hunk_header_re.captures(line) {
            current_line = cap.get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            continue;
        }

        // Only process added lines
        if line.starts_with('+') && !line.starts_with("+++") {
            total_added_lines += 1;
            let content = &line[1..]; // Remove '+' prefix

            // Check for quality issues
            let is_test_file = current_file.contains("test") || current_file.contains("tests");

            // TODO, FIXME
            if content.contains("TODO") || content.contains("FIXME") {
                hits.push(QualityHit {
                    file: current_file.clone(),
                    line: current_line,
                    kind: QualityHitKind::Todo,
                    text: content.trim().to_string(),
                });
            }

            // STUB
            if content.contains("STUB") || content.contains("stub") {
                hits.push(QualityHit {
                    file: current_file.clone(),
                    line: current_line,
                    kind: QualityHitKind::Stub,
                    text: content.trim().to_string(),
                });
            }

            // MOCK (but not in test files)
            if !is_test_file && (content.contains("MOCK") || content.contains("mock")) {
                hits.push(QualityHit {
                    file: current_file.clone(),
                    line: current_line,
                    kind: QualityHitKind::Mock,
                    text: content.trim().to_string(),
                });
            }

            // unimplemented!(), todo!()
            if content.contains("unimplemented!()") || content.contains("todo!()") {
                hits.push(QualityHit {
                    file: current_file.clone(),
                    line: current_line,
                    kind: QualityHitKind::Unimplemented,
                    text: content.trim().to_string(),
                });
            }

            // placeholder
            if content.to_lowercase().contains("placeholder") {
                hits.push(QualityHit {
                    file: current_file.clone(),
                    line: current_line,
                    kind: QualityHitKind::Placeholder,
                    text: content.trim().to_string(),
                });
            }

            // Empty function bodies: fn name() {}
            let empty_fn_re = Regex::new(r"fn\s+\w+[^{]*\{\s*\}").unwrap();
            if empty_fn_re.is_match(content) {
                hits.push(QualityHit {
                    file: current_file.clone(),
                    line: current_line,
                    kind: QualityHitKind::EmptyFunction,
                    text: content.trim().to_string(),
                });
            }

            current_line += 1;
        } else if !line.starts_with('-') && !line.starts_with('\\') && !line.starts_with("---") {
            // Context lines (no prefix or space prefix) also increment line counter
            if !line.starts_with("@@") && !line.starts_with("+++") && !line.starts_with("diff") {
                current_line += 1;
            }
        }
    }

    let total_hits = hits.len();
    let stub_ratio = if total_added_lines > 0 {
        total_hits as f64 / total_added_lines as f64
    } else {
        0.0
    };

    QualityScan {
        hits,
        total_hits,
        stub_ratio,
    }
}

/// Session summary from Queen event data
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub duration_secs: f64,
    pub total_cost_usd: f64,
    pub total_turns: usize,
    pub tools_used: Vec<String>,
    pub files_changed: usize,
}

/// Build a session summary from Queen event data
pub fn build_session_summary(
    duration_secs: f64,
    cost_usd: f64,
    turns: usize,
    tools: Vec<String>,
    files_changed: usize,
) -> SessionSummary {
    SessionSummary {
        duration_secs,
        total_cost_usd: cost_usd,
        total_turns: turns,
        tools_used: tools,
        files_changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_diff_numstat_basic() {
        let input = "10\t5\tsrc/lib.rs\n3\t0\tsrc/new_file.rs\n";
        let result = parse_diff_summary(input);

        assert_eq!(result.total_files, 2);
        assert_eq!(result.total_added, 13);
        assert_eq!(result.total_removed, 5);
        assert_eq!(result.files.len(), 2);
        assert_eq!(result.files[0].path, "src/lib.rs");
        assert_eq!(result.files[0].added, 10);
        assert_eq!(result.files[0].removed, 5);
        assert_eq!(result.files[1].path, "src/new_file.rs");
        assert_eq!(result.files[1].added, 3);
        assert_eq!(result.files[1].removed, 0);
    }

    #[test]
    fn test_parse_diff_numstat_empty() {
        let input = "";
        let result = parse_diff_summary(input);

        assert_eq!(result.total_files, 0);
        assert_eq!(result.total_added, 0);
        assert_eq!(result.total_removed, 0);
        assert_eq!(result.files.len(), 0);
    }

    #[test]
    fn test_parse_diff_numstat_binary() {
        let input = "10\t5\tsrc/lib.rs\n-\t-\tbinary_file.bin\n3\t2\tsrc/main.rs\n";
        let result = parse_diff_summary(input);

        assert_eq!(result.total_files, 3);
        assert_eq!(result.total_added, 13); // Binary file counts as 0
        assert_eq!(result.total_removed, 7);
        assert_eq!(result.files[1].path, "binary_file.bin");
        assert_eq!(result.files[1].added, 0);
        assert_eq!(result.files[1].removed, 0);
    }

    #[test]
    fn test_parse_test_results_ok() {
        let output = "test result: ok. 10 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out";
        let result = parse_test_results(output);

        assert_eq!(result.passed, 10);
        assert_eq!(result.failed, 0);
        assert_eq!(result.ignored, 1);
        assert_eq!(result.total, 11);
        assert_eq!(result.failures.len(), 0);
    }

    #[test]
    fn test_parse_test_results_failed() {
        let output = r#"
test some::test_foo ... FAILED
test some::test_bar ... ok
test other::test_baz ... FAILED

test result: FAILED. 8 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
"#;
        let result = parse_test_results(output);

        assert_eq!(result.passed, 8);
        assert_eq!(result.failed, 2);
        assert_eq!(result.ignored, 0);
        assert_eq!(result.total, 10);
        assert_eq!(result.failures.len(), 2);
        assert!(result.failures.contains(&"some::test_foo".to_string()));
        assert!(result.failures.contains(&"other::test_baz".to_string()));
    }

    #[test]
    fn test_parse_test_results_no_match() {
        let output = "Some random output without test results";
        let result = parse_test_results(output);

        assert_eq!(result.passed, 0);
        assert_eq!(result.failed, 0);
        assert_eq!(result.ignored, 0);
        assert_eq!(result.total, 0);
        assert!(!result.raw_output.is_empty());
    }

    #[test]
    fn test_scan_quality_todos() {
        let diff = r#"
+++ b/src/lib.rs
@@ -1,3 +1,5 @@
+// TODO: implement this
+fn placeholder() {}
"#;
        let result = scan_code_quality(diff);

        assert_eq!(result.total_hits, 3); // TODO + Placeholder + empty fn
        assert!(result.hits.iter().any(|h| matches!(h.kind, QualityHitKind::Todo)));
        assert!(result.hits.iter().any(|h| matches!(h.kind, QualityHitKind::Placeholder)));
        assert!(result.hits.iter().any(|h| matches!(h.kind, QualityHitKind::EmptyFunction)));
    }

    #[test]
    fn test_scan_quality_stubs() {
        let diff = r#"
+++ b/src/lib.rs
@@ -1,3 +1,5 @@
+// STUB: replace with real implementation
+fn stub_function() -> u32 { 0 }
"#;
        let result = scan_code_quality(diff);

        assert!(result.total_hits >= 1);
        assert!(result.hits.iter().any(|h| matches!(h.kind, QualityHitKind::Stub)));
    }

    #[test]
    fn test_scan_quality_unimplemented() {
        let diff = r#"
+++ b/src/lib.rs
@@ -1,3 +1,5 @@
+fn not_done() {
+    unimplemented!()
+}
"#;
        let result = scan_code_quality(diff);

        assert!(result.total_hits >= 1);
        assert!(result.hits.iter().any(|h| matches!(h.kind, QualityHitKind::Unimplemented)));
    }

    #[test]
    fn test_scan_quality_empty_fn() {
        let diff = r#"
+++ b/src/lib.rs
@@ -1,3 +1,5 @@
+fn empty() {}
+fn also_empty() { }
"#;
        let result = scan_code_quality(diff);

        assert_eq!(result.total_hits, 2);
        assert!(result.hits.iter().all(|h| matches!(h.kind, QualityHitKind::EmptyFunction)));
    }

    #[test]
    fn test_scan_quality_clean_diff() {
        let diff = r#"
+++ b/src/lib.rs
@@ -1,3 +1,5 @@
+pub fn add(a: u32, b: u32) -> u32 {
+    a + b
+}
"#;
        let result = scan_code_quality(diff);

        assert_eq!(result.total_hits, 0);
        assert_eq!(result.stub_ratio, 0.0);
    }

    #[test]
    fn test_session_summary() {
        let tools = vec!["Read".to_string(), "Write".to_string(), "Bash".to_string()];
        let summary = build_session_summary(120.5, 0.45, 15, tools.clone(), 3);

        assert_eq!(summary.duration_secs, 120.5);
        assert_eq!(summary.total_cost_usd, 0.45);
        assert_eq!(summary.total_turns, 15);
        assert_eq!(summary.tools_used, tools);
        assert_eq!(summary.files_changed, 3);
    }
}
