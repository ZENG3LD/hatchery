//! Integration tests for overlord parsers and code checks

use hatchery::overlord::parsers::*;
use hatchery::overlord::code_checks::*;

#[test]
fn test_full_pipeline_auto_approve() {
    let diff = parse_diff_summary("10\t5\tsrc/lib.rs\n");
    let tests = parse_test_results("test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out");
    let quality = scan_code_quality("+++ b/src/lib.rs\n@@ -1,3 +1,5 @@\n+pub fn add(a: u32) -> u32 { a }");
    let session = build_session_summary(100.0, 0.3, 10, vec!["Read".to_string()], 1);

    let verdict = run_code_checks(&diff, Some(&tests), &quality, &session, "Test task");

    assert_eq!(verdict, CodeCheckVerdict::AllClear);
}

#[test]
fn test_full_pipeline_auto_reject_failed_tests() {
    let diff = parse_diff_summary("10\t5\tsrc/lib.rs\n");
    let tests = parse_test_results("test result: FAILED. 3 passed; 2 failed; 0 ignored");
    let quality = scan_code_quality("+++ b/src/lib.rs\n@@ -1,3 +1,5 @@\n+pub fn add(a: u32) -> u32 { a }");
    let session = build_session_summary(100.0, 0.3, 10, vec!["Read".to_string()], 1);

    let verdict = run_code_checks(&diff, Some(&tests), &quality, &session, "Test task");

    match verdict {
        CodeCheckVerdict::HardReject { reason } => {
            assert!(reason.contains("tests failed"));
        }
        _ => panic!("Expected HardReject"),
    }
}

#[test]
fn test_full_pipeline_needs_review() {
    let diff = parse_diff_summary("20\t5\tsrc/lib.rs\n");
    let tests = parse_test_results("test result: ok. 5 passed; 0 failed; 0 ignored");
    let quality_diff = r#"
+++ b/src/lib.rs
@@ -1,3 +1,10 @@
+pub fn add(a: u32) -> u32 { a }
+pub fn sub(a: u32) -> u32 { a }
+pub fn mul(a: u32) -> u32 { a }
+pub fn div(a: u32) -> u32 { a }
+// TODO: implement properly
"#;
    let quality = scan_code_quality(quality_diff);
    let session = build_session_summary(100.0, 0.3, 10, vec!["Read".to_string()], 1);

    let verdict = run_code_checks(&diff, Some(&tests), &quality, &session, "Test task");

    match verdict {
        CodeCheckVerdict::NeedsReview { report } => {
            assert!(report.diff_summary.contains("20"));
            assert!(report.quality_summary.contains("TODO"));
        }
        _ => panic!("Expected NeedsReview"),
    }
}
