//! Task relevance analyzer for the Hatchery Overlord.
//!
//! Compares task description with actual code changes to compute a relevance score.
//! Detects when Queens produce work that's unrelated to the assigned task.

use super::parsers::{DiffSummary, QualityScan};
use std::collections::HashSet;

// ============================================================================
// Public Types
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct TaskRelevanceReport {
    /// Overall relevance score (0.0 = no relation, 1.0 = perfect match)
    pub relevance_score: f64,
    /// Keywords extracted from task description
    pub task_keywords: Vec<String>,
    /// Keywords found in the diff (file paths, function names, identifiers)
    pub diff_keywords: Vec<String>,
    /// Overlapping keywords (present in both task and diff)
    pub matched_keywords: Vec<String>,
    /// Task keywords NOT found in diff (things the task asked for but code doesn't mention)
    pub missing_keywords: Vec<String>,
    /// Diff size relative to task complexity estimate
    pub size_ratio: SizeRatio,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SizeRatio {
    /// Diff seems proportional to task
    Proportional,
    /// Diff is suspiciously small for the task (e.g. 2 lines for a complex feature)
    TooSmall { expected_min: usize, actual: usize },
    /// Diff is suspiciously large for the task (e.g. 500 lines for "fix typo")
    TooLarge { expected_max: usize, actual: usize },
}

// ============================================================================
// Main Entry Point
// ============================================================================

/// Analyze relevance between task description and actual changes.
pub fn analyze_task_relevance(
    task_description: &str,
    diff: &DiffSummary,
    quality: &QualityScan,
) -> TaskRelevanceReport {
    let task_keywords = extract_task_keywords(task_description);
    let diff_keywords = extract_diff_keywords(diff, quality);

    let matched = find_matches(&task_keywords, &diff_keywords);
    let missing = find_missing(&task_keywords, &diff_keywords);

    let relevance_score = compute_relevance(&task_keywords, &matched);
    let size_ratio = estimate_size_ratio(task_description, diff);

    TaskRelevanceReport {
        relevance_score,
        task_keywords,
        diff_keywords,
        matched_keywords: matched,
        missing_keywords: missing,
        size_ratio,
    }
}

// ============================================================================
// Keyword Extraction
// ============================================================================

/// Stop words to filter out
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "in", "for", "to", "with", "and", "or", "of",
    "is", "be", "are", "was", "were", "been", "being", "have", "has",
    "had", "do", "does", "did", "will", "would", "could", "should",
    "may", "might", "shall", "can", "it", "its", "this", "that",
    "these", "those", "on", "at", "by", "from", "as", "into", "but",
    "not", "no", "so", "if", "then", "than", "when", "while", "all",
    "each", "every", "both", "few", "more", "most", "other", "some",
    "such", "only", "own", "same", "also", "very", "just", "about",
    "up", "out", "new", "use", "using",
];

/// Action words (kept separately for size estimation)
const ACTION_WORDS: &[&str] = &[
    "implement", "add", "create", "fix", "update", "change", "make",
    "write", "build", "refactor", "remove", "delete", "move", "rename",
    "modify", "improve", "optimize", "handle", "support", "enable",
    "ensure", "convert", "migrate", "integrate", "extract", "split",
];

/// Path noise words to filter from file paths
const PATH_NOISE: &[&str] = &[
    "src", "lib", "mod", "tests", "test", "crate", "crates", "rs",
    "main", "core", "utils", "helpers", "common", "types", "config",
];

/// Extract keywords from task description
fn extract_task_keywords(description: &str) -> Vec<String> {
    let mut keywords = HashSet::new();
    let lowercase = description.to_lowercase();

    // Split by whitespace and punctuation
    let words: Vec<String> = lowercase
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect();

    for word in words {
        // Skip stop words and action words
        if STOP_WORDS.contains(&word.as_str()) || ACTION_WORDS.contains(&word.as_str()) {
            continue;
        }

        // Skip very short words (< 3 chars)
        if word.len() < 3 {
            continue;
        }

        // Add the word itself
        keywords.insert(word.clone());

        // Split compound words (camelCase, snake_case)
        for subword in split_compound(&word) {
            if subword.len() >= 3 && !STOP_WORDS.contains(&subword.as_str()) {
                // Apply stemming
                let stemmed = stem_word(&subword);
                keywords.insert(stemmed);
            }
        }
    }

    let mut result: Vec<String> = keywords.into_iter().collect();
    result.sort();
    result
}

/// Extract keywords from diff (file paths, function names from AST)
fn extract_diff_keywords(diff: &DiffSummary, quality: &QualityScan) -> Vec<String> {
    let mut keywords = HashSet::new();

    // From file paths
    for file in &diff.files {
        // Split path by '/' and '.'
        let parts: Vec<&str> = file.path
            .split(|c: char| c == '/' || c == '\\' || c == '.')
            .filter(|p| !p.is_empty())
            .collect();

        for part in parts {
            let lower = part.to_lowercase();
            // Skip common path noise
            if PATH_NOISE.contains(&lower.as_str()) {
                continue;
            }

            if lower.len() >= 3 {
                keywords.insert(lower.clone());

                // Split compound words
                for subword in split_compound(&lower) {
                    if subword.len() >= 3 && !PATH_NOISE.contains(&subword.as_str()) {
                        let stemmed = stem_word(&subword);
                        keywords.insert(stemmed);
                    }
                }
            }
        }
    }

    // From AST function names
    if let Some(ast) = &quality.ast_report {
        for func in &ast.functions {
            let lower = func.name.to_lowercase();
            keywords.insert(lower.clone());

            // Split function name by '_' (snake_case)
            for subword in split_compound(&lower) {
                if subword.len() >= 3 && !STOP_WORDS.contains(&subword.as_str()) {
                    let stemmed = stem_word(&subword);
                    keywords.insert(stemmed);
                }
            }
        }
    }

    let mut result: Vec<String> = keywords.into_iter().collect();
    result.sort();
    result
}

/// Split compound words (camelCase, snake_case, hyphens)
fn split_compound(word: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut last_was_lower = false;

    for ch in word.chars() {
        if ch == '_' || ch == '-' {
            if !current.is_empty() {
                parts.push(current.to_lowercase());
                current.clear();
            }
            last_was_lower = false;
        } else if ch.is_uppercase() && last_was_lower {
            // camelCase boundary
            if !current.is_empty() {
                parts.push(current.to_lowercase());
                current.clear();
            }
            current.push(ch);
            last_was_lower = false;
        } else {
            current.push(ch);
            last_was_lower = ch.is_lowercase();
        }
    }

    if !current.is_empty() {
        parts.push(current.to_lowercase());
    }

    // If we only got one part (no compound structure), return just the original word
    if parts.is_empty() {
        vec![word.to_lowercase()]
    } else {
        parts
    }
}

/// Simple stemming: remove common suffixes
fn stem_word(word: &str) -> String {
    let suffixes = [
        "tion", "ment", "ness", "able", "ible", "ance", "ence",
        "ing", "ed", "er", "ly", "s",
    ];

    for suffix in &suffixes {
        if word.len() > suffix.len() + 2 && word.ends_with(suffix) {
            // Keep at least 3 chars of stem
            let stem = &word[..word.len() - suffix.len()];
            if stem.len() >= 3 {
                return stem.to_string();
            }
        }
    }

    word.to_string()
}

// ============================================================================
// Matching Logic
// ============================================================================

/// Find matches between task keywords and diff keywords
fn find_matches(task_kw: &[String], diff_kw: &[String]) -> Vec<String> {
    let mut matches = Vec::new();

    for tk in task_kw {
        for dk in diff_kw {
            // Exact match
            if tk == dk {
                matches.push(tk.clone());
                break;
            }

            // Prefix match (fuzzy): "reconnect" matches "reconnection"
            if tk.starts_with(dk) || dk.starts_with(tk) {
                matches.push(tk.clone());
                break;
            }

            // Substring match: "socket" matches "websocket"
            if (tk.len() >= 4 && dk.contains(tk)) || (dk.len() >= 4 && tk.contains(dk)) {
                matches.push(tk.clone());
                break;
            }
        }
    }

    matches.sort();
    matches.dedup();
    matches
}

/// Find task keywords that have NO match in diff keywords
fn find_missing(task_kw: &[String], diff_kw: &[String]) -> Vec<String> {
    let matched_set: HashSet<_> = find_matches(task_kw, diff_kw).into_iter().collect();

    let mut missing: Vec<String> = task_kw
        .iter()
        .filter(|tk| !matched_set.contains(*tk))
        .cloned()
        .collect();

    missing.sort();
    missing
}

// ============================================================================
// Relevance Score
// ============================================================================

/// Compute relevance score: ratio of matched keywords to total task keywords
fn compute_relevance(task_keywords: &[String], matched: &[String]) -> f64 {
    if task_keywords.is_empty() {
        return 1.0; // No keywords to match = benefit of doubt
    }
    matched.len() as f64 / task_keywords.len() as f64
}

// ============================================================================
// Size Ratio Estimation
// ============================================================================

/// Estimate if diff size is proportional to task complexity
fn estimate_size_ratio(task_description: &str, diff: &DiffSummary) -> SizeRatio {
    // Heuristic: count words in task to estimate complexity
    let task_words = task_description.split_whitespace().count();
    let total_changes = diff.total_added + diff.total_removed;

    // Very rough heuristics:
    // - Simple task (< 10 words, like "fix typo in README"): expect 1-50 lines
    // - Medium task (10-30 words): expect 10-200 lines
    // - Complex task (30+ words): expect 30-500 lines

    let (min_expected, max_expected) = if task_words < 10 {
        (1, 50)
    } else if task_words < 30 {
        (10, 200)
    } else {
        (30, 500)
    };

    if total_changes < min_expected {
        SizeRatio::TooSmall {
            expected_min: min_expected,
            actual: total_changes,
        }
    } else if total_changes > max_expected {
        SizeRatio::TooLarge {
            expected_max: max_expected,
            actual: total_changes,
        }
    } else {
        SizeRatio::Proportional
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlord::ast_analyzer::{AstQualityReport, FunctionQuality, FunctionMetrics};
    use crate::overlord::parsers::ChangedFile;

    fn make_diff(files: Vec<(&str, usize, usize)>) -> DiffSummary {
        let mut total_added = 0;
        let mut total_removed = 0;
        let changed_files: Vec<ChangedFile> = files
            .into_iter()
            .map(|(path, added, removed)| {
                total_added += added;
                total_removed += removed;
                ChangedFile {
                    path: path.to_string(),
                    added,
                    removed,
                }
            })
            .collect();

        DiffSummary {
            total_files: changed_files.len(),
            files: changed_files,
            total_added,
            total_removed,
        }
    }

    fn make_quality_with_functions(names: &[&str]) -> QualityScan {
        let functions = names
            .iter()
            .map(|n| FunctionQuality {
                name: n.to_string(),
                line: 0,
                is_method: false,
                score: 0.8,
                metrics: FunctionMetrics {
                    param_count: 0,
                    params_used: 0,
                    uses_self: false,
                    statement_count: 5,
                    expression_count: 3,
                    has_control_flow: true,
                    function_call_count: 1,
                    has_computation: true,
                    local_binding_count: 1,
                    returns_literal: false,
                    returns_default: false,
                },
                issues: vec![],
            })
            .collect();

        QualityScan {
            hits: vec![],
            total_hits: 0,
            stub_ratio: 0.0,
            ast_report: Some(AstQualityReport {
                functions,
                overall_score: 0.8,
                total_functions: names.len(),
                suspicious_count: 0,
            }),
        }
    }

    fn make_quality_empty() -> QualityScan {
        QualityScan {
            hits: vec![],
            total_hits: 0,
            stub_ratio: 0.0,
            ast_report: None,
        }
    }

    #[test]
    fn test_perfect_relevance() {
        let task = "implement websocket reconnection";
        let diff = make_diff(vec![("src/websocket/reconnect.rs", 50, 10)]);
        let quality = make_quality_with_functions(&["reconnect", "handle_websocket"]);

        let report = analyze_task_relevance(task, &diff, &quality);

        // Should have high relevance (websocket and reconnect both present)
        assert!(
            report.relevance_score > 0.8,
            "Expected score > 0.8, got {:.2}",
            report.relevance_score
        );
        assert!(report.matched_keywords.len() >= 2);
    }

    #[test]
    fn test_zero_relevance() {
        let task = "implement payment processing";
        let diff = make_diff(vec![("src/logging.rs", 20, 5)]);
        let quality = make_quality_with_functions(&["log_message", "write_log"]);

        let report = analyze_task_relevance(task, &diff, &quality);

        // Should have low relevance (payment/processing not found anywhere)
        assert!(
            report.relevance_score < 0.2,
            "Expected score < 0.2, got {:.2}",
            report.relevance_score
        );
        assert!(report.missing_keywords.len() >= 1);
    }

    #[test]
    fn test_partial_relevance() {
        let task = "add retry logic with backoff for WebSocket";
        let diff = make_diff(vec![("src/retry.rs", 30, 0)]);
        let quality = make_quality_with_functions(&["retry_connection", "calculate_delay"]);

        let report = analyze_task_relevance(task, &diff, &quality);

        // Should have partial relevance (retry present, websocket/backoff missing)
        assert!(
            report.relevance_score > 0.2 && report.relevance_score < 0.8,
            "Expected score 0.2-0.8, got {:.2}",
            report.relevance_score
        );
        assert!(report.matched_keywords.len() >= 1);
        assert!(report.missing_keywords.len() >= 1);
    }

    #[test]
    fn test_keyword_extraction_camelcase() {
        let keywords = extract_task_keywords("WebSocket handler");
        // "WebSocket" should split to ["websocket", "web", "socket"]
        // "handler" stays as is
        assert!(keywords.contains(&"websocket".to_string()) || keywords.contains(&"socket".to_string()));
        assert!(keywords.contains(&"handler".to_string()));
    }

    #[test]
    fn test_keyword_extraction_snake_case() {
        let keywords = extract_task_keywords("exponential_backoff retry");
        // "exponential_backoff" should split
        assert!(keywords.contains(&"exponential".to_string()) || keywords.contains(&"backoff".to_string()));
        assert!(keywords.contains(&"retry".to_string()));
    }

    #[test]
    fn test_stop_words_filtered() {
        let keywords = extract_task_keywords("implement the new WebSocket handler");
        // Should NOT contain "implement", "the", "new"
        assert!(!keywords.contains(&"implement".to_string()));
        assert!(!keywords.contains(&"the".to_string()));
        assert!(!keywords.contains(&"new".to_string()));
        // Should contain "websocket", "handler"
        assert!(keywords.len() >= 1); // At least websocket or handler
    }

    #[test]
    fn test_size_ratio_too_small() {
        let task = "Implement comprehensive WebSocket reconnection system with exponential backoff and connection pooling";
        let diff = make_diff(vec![("src/lib.rs", 3, 0)]);
        let quality = make_quality_empty();

        let report = analyze_task_relevance(task, &diff, &quality);

        match report.size_ratio {
            SizeRatio::TooSmall { .. } => {}
            _ => panic!("Expected TooSmall, got {:?}", report.size_ratio),
        }
    }

    #[test]
    fn test_size_ratio_too_large() {
        let task = "fix typo";
        let diff = make_diff(vec![("src/lib.rs", 300, 0)]);
        let quality = make_quality_empty();

        let report = analyze_task_relevance(task, &diff, &quality);

        match report.size_ratio {
            SizeRatio::TooLarge { .. } => {}
            _ => panic!("Expected TooLarge, got {:?}", report.size_ratio),
        }
    }

    #[test]
    fn test_size_ratio_proportional() {
        let task = "Add error handling for network failures";
        let diff = make_diff(vec![("src/network.rs", 30, 10)]);
        let quality = make_quality_empty();

        let report = analyze_task_relevance(task, &diff, &quality);

        // Task is 6 words, so 10-200 lines expected, 40 total is proportional
        assert_eq!(report.size_ratio, SizeRatio::Proportional);
    }

    #[test]
    fn test_file_path_keywords() {
        let diff = make_diff(vec![("src/websocket/handler.rs", 20, 5)]);
        let quality = make_quality_empty();

        let keywords = extract_diff_keywords(&diff, &quality);

        // Should extract "websocket" and "handler"
        assert!(keywords.contains(&"websocket".to_string()));
        assert!(keywords.contains(&"handler".to_string()));
    }

    #[test]
    fn test_function_name_keywords() {
        let diff = make_diff(vec![("src/lib.rs", 10, 0)]);
        let quality = make_quality_with_functions(&["handle_reconnection", "process_event"]);

        let keywords = extract_diff_keywords(&diff, &quality);

        // Should extract "handle", "reconnection", "process", "event"
        assert!(keywords.iter().any(|k| k.contains("handle") || k.contains("reconnect")));
        assert!(keywords.iter().any(|k| k.contains("process") || k.contains("event")));
    }

    #[test]
    fn test_stemming() {
        // "reconnection" should stem to "reconnec" (remove "tion")
        let stemmed = stem_word("reconnection");
        assert_eq!(stemmed, "reconnec");

        // "implementation" should stem to "implementa" (remove "tion")
        let stemmed = stem_word("implementation");
        assert_eq!(stemmed, "implementa");

        // "running" should stem to "runn" (remove "ing")
        let stemmed = stem_word("running");
        assert_eq!(stemmed, "runn");

        // "handler" should stem to "handl" (removing "er")
        let stemmed = stem_word("handler");
        assert_eq!(stemmed, "handl");
    }

    #[test]
    fn test_empty_task() {
        let task = "";
        let diff = make_diff(vec![("src/lib.rs", 10, 0)]);
        let quality = make_quality_empty();

        let report = analyze_task_relevance(task, &diff, &quality);

        // Empty task = benefit of doubt
        assert_eq!(report.relevance_score, 1.0);
        assert!(report.task_keywords.is_empty());
    }

    #[test]
    fn test_empty_diff() {
        let task = "implement websocket reconnection";
        let diff = make_diff(vec![]);
        let quality = make_quality_empty();

        let report = analyze_task_relevance(task, &diff, &quality);

        // Task has keywords but diff is empty = score 0.0
        assert_eq!(report.relevance_score, 0.0);
        assert!(report.diff_keywords.is_empty());
        assert_eq!(report.missing_keywords.len(), report.task_keywords.len());
    }

    #[test]
    fn test_split_compound_camelcase() {
        let parts = split_compound("WebSocket");
        assert_eq!(parts, vec!["web", "socket"]);
    }

    #[test]
    fn test_split_compound_snake_case() {
        let parts = split_compound("exponential_backoff");
        assert_eq!(parts, vec!["exponential", "backoff"]);
    }

    #[test]
    fn test_split_compound_single_word() {
        let parts = split_compound("handler");
        assert_eq!(parts, vec!["handler"]);
    }

    #[test]
    fn test_fuzzy_matching() {
        let task_kw = vec!["reconnect".to_string()];
        let diff_kw = vec!["reconnection".to_string()];

        let matches = find_matches(&task_kw, &diff_kw);
        // "reconnect" should match "reconnection" via prefix match
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_substring_matching() {
        let task_kw = vec!["socket".to_string()];
        let diff_kw = vec!["websocket".to_string()];

        let matches = find_matches(&task_kw, &diff_kw);
        // "socket" should match "websocket" via substring match
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_no_action_words_in_keywords() {
        let keywords = extract_task_keywords("implement add create fix update websocket");
        // Should NOT contain action words
        assert!(!keywords.contains(&"implement".to_string()));
        assert!(!keywords.contains(&"add".to_string()));
        assert!(!keywords.contains(&"create".to_string()));
        assert!(!keywords.contains(&"fix".to_string()));
        assert!(!keywords.contains(&"update".to_string()));
        // Should contain "websocket"
        assert!(keywords.contains(&"websocket".to_string()));
    }

    #[test]
    fn test_no_path_noise_in_diff_keywords() {
        let diff = make_diff(vec![("src/lib/mod/test.rs", 10, 0)]);
        let quality = make_quality_empty();

        let keywords = extract_diff_keywords(&diff, &quality);

        // Should NOT contain path noise
        assert!(!keywords.contains(&"src".to_string()));
        assert!(!keywords.contains(&"lib".to_string()));
        assert!(!keywords.contains(&"mod".to_string()));
        assert!(!keywords.contains(&"test".to_string()));
    }

    #[test]
    fn test_realistic_scenario_websocket() {
        let task = "Implement WebSocket reconnection with exponential backoff";
        let diff = make_diff(vec![
            ("src/websocket/reconnect.rs", 25, 5),
            ("src/websocket/backoff.rs", 20, 0),
        ]);
        let quality = make_quality_with_functions(&[
            "reconnect",
            "calculate_backoff",
            "calculate_exponential_delay",
            "handle_disconnect",
        ]);

        let report = analyze_task_relevance(task, &diff, &quality);

        // Should have high relevance
        // Task has "websocket", "reconnection", "exponential", "backoff"
        // All 4 keywords should match
        assert!(
            report.relevance_score > 0.7,
            "Score: {:.2}, matched: {:?}, missing: {:?}",
            report.relevance_score,
            report.matched_keywords,
            report.missing_keywords
        );
        // Task has 6 words, so expected range is 1-50 lines, 50 is proportional
        assert_eq!(report.size_ratio, SizeRatio::Proportional);
    }

    #[test]
    fn test_realistic_scenario_wrong_work() {
        let task = "Fix authentication bug in login endpoint";
        let diff = make_diff(vec![("src/database/migrations.rs", 200, 50)]);
        let quality = make_quality_with_functions(&["migrate_schema", "rollback_migration"]);

        let report = analyze_task_relevance(task, &diff, &quality);

        // Should have low relevance (doing migrations instead of fixing auth)
        assert!(
            report.relevance_score < 0.3,
            "Score: {:.2}, expected < 0.3",
            report.relevance_score
        );

        // Also suspiciously large diff for a "bug fix"
        match report.size_ratio {
            SizeRatio::TooLarge { .. } => {}
            _ => panic!("Expected TooLarge for 250-line bug fix"),
        }
    }
}
