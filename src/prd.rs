//! PRD (Product Requirements Document) parser.
//!
//! Parses markdown files with checkbox-style acceptance criteria:
//! ```markdown
//! - [ ] AC-1.1: Uncompleted task
//! - [x] AC-1.2: Completed task
//! ```

use crate::cli::Task;
use anyhow::{Context, Result};
use regex::Regex;
use std::path::Path;

/// Parse a PRD markdown file into a list of tasks.
///
/// Tasks are any line matching `- [ ] ...` or `- [x] ...` pattern.
/// Returns tasks in order of appearance, with 1-based IDs.
pub fn parse_prd(path: &Path) -> Result<Vec<Task>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read PRD file: {}", path.display()))?;

    parse_prd_content(&content)
}

/// Parse PRD content string into tasks.
pub fn parse_prd_content(content: &str) -> Result<Vec<Task>> {
    let re = Regex::new(r"^(\s*[-*])\s+\[([ xX])\]\s+(.+)$")?;
    let track_header_re = Regex::new(r"^###\s+Track\s+[^:]+:\s+[^—]*—\s*depends on (.+)$")?;
    let mut tasks = Vec::new();
    let mut id = 1;
    let mut current_track_dependencies: Vec<String> = Vec::new();

    for (line_number, line) in content.lines().enumerate() {
        // Check if this is a track header with dependencies
        if let Some(caps) = track_header_re.captures(line) {
            let deps_str = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            current_track_dependencies = parse_dependencies_from_text(deps_str);
            continue;
        }

        // Check if this is a task header without dependencies (reset dependencies)
        if line.trim_start().starts_with("###") && !track_header_re.is_match(line) {
            current_track_dependencies.clear();
            continue;
        }

        // Check if this is a checkbox task
        if let Some(caps) = re.captures(line) {
            let done = caps.get(2).map(|m| m.as_str() != " ").unwrap_or(false);
            let description = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();

            // Parse skill hint from description
            let skill_hint = extract_skill_hint(&description);

            tasks.push(Task {
                id,
                description,
                done,
                line_number,
                dependencies: current_track_dependencies.clone(),
                skill_hint,
            });
            id += 1;
        }
    }

    Ok(tasks)
}

/// Extract dependency IDs from text like "prd-1 AND prd-2" or "prd-1, prd-2".
fn parse_dependencies_from_text(text: &str) -> Vec<String> {
    let prd_re = Regex::new(r"prd-(\d+)").expect("valid regex");
    prd_re
        .captures_iter(text)
        .filter_map(|cap| cap.get(0).map(|m| m.as_str().to_string()))
        .collect()
}

/// Extract skill hint from description like "**USE /carousel SKILL**" or "USE /carousel SKILL".
fn extract_skill_hint(description: &str) -> Option<String> {
    let skill_re = Regex::new(r"(?i)(?:\*\*)?USE\s+/(\w+)\s+SKILL(?:\*\*)?").expect("valid regex");
    skill_re
        .captures(description)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().to_lowercase()))
}

/// Count completed vs total tasks.
pub fn progress(tasks: &[Task]) -> (usize, usize) {
    let done = tasks.iter().filter(|t| t.done).count();
    (done, tasks.len())
}

/// Mark a task as done in the PRD file by updating its checkbox.
///
/// Task IDs are in the format "prd-N" where N is the 1-based task number.
/// This function finds the N-th checkbox line and replaces `[ ]` with `[x]`.
///
/// Uses atomic write (write to temp + rename) to avoid corruption.
pub fn mark_task_done(prd_path: &Path, task_id: &str) -> Result<()> {
    // Extract task number from "prd-N" format
    let task_num: usize = task_id
        .strip_prefix("prd-")
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid task ID format: {}", task_id))?;

    // Read current content
    let content = std::fs::read_to_string(prd_path)
        .with_context(|| format!("Failed to read PRD file: {}", prd_path.display()))?;

    // Parse to find the target checkbox line
    let re = Regex::new(r"^(\s*[-*])\s+\[([ xX])\]\s+(.+)$")?;
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut checkbox_count = 0;
    let mut updated = false;

    for line in &mut lines {
        if re.is_match(line) {
            checkbox_count += 1;
            if checkbox_count == task_num {
                // Replace [ ] with [x], but only if it's currently unchecked
                if line.contains("[ ]") {
                    *line = line.replace("[ ]", "[x]");
                    updated = true;
                }
                break;
            }
        }
    }

    if !updated {
        return Err(anyhow::anyhow!(
            "Task {} not found or already completed in PRD",
            task_id
        ));
    }

    // Atomic write: write to temp file then rename
    let temp_path = prd_path.with_extension("tmp");
    let new_content = lines.join("\n") + "\n";

    std::fs::write(&temp_path, new_content)
        .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;

    std::fs::rename(&temp_path, prd_path)
        .with_context(|| format!("Failed to rename temp file to: {}", prd_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SAMPLE_PRD: &str = r#"# Test PRD

## Tasks
- [ ] AC-1.1: First task
- [x] AC-1.2: Second task (done)
- [ ] AC-1.3: Third task
- [ ] AC-2.1: Fourth task
- [x] AC-2.2: Fifth task (done)
"#;

    const SAMPLE_PRD_WITH_DEPS: &str = r#"# Test PRD with Dependencies

### Track A: Core Engine — BOTTLENECK

- [ ] prd-1: Build core foundation
- [ ] prd-2: Add basic API

### Track B: CLI — depends on prd-1 AND prd-2

- [ ] prd-3: Create CLI interface **USE /carousel SKILL**
- [ ] prd-4: Add commands

### Track C: Advanced

- [ ] prd-5: Advanced features USE /research SKILL
"#;

    #[test]
    fn test_parse_prd() {
        let tasks = parse_prd_content(SAMPLE_PRD).unwrap();
        assert_eq!(tasks.len(), 5);
        assert_eq!(tasks[0].id, 1);
        assert_eq!(tasks[0].description, "AC-1.1: First task");
        assert!(!tasks[0].done);
        assert!(tasks[1].done);
        assert_eq!(tasks[1].description, "AC-1.2: Second task (done)");
        assert!(tasks[0].dependencies.is_empty());
        assert_eq!(tasks[0].skill_hint, None);
    }

    #[test]
    fn test_parse_prd_with_dependencies() {
        let tasks = parse_prd_content(SAMPLE_PRD_WITH_DEPS).unwrap();
        assert_eq!(tasks.len(), 5);

        // Track A tasks should have no dependencies
        assert_eq!(tasks[0].description, "prd-1: Build core foundation");
        assert!(tasks[0].dependencies.is_empty());
        assert_eq!(tasks[1].description, "prd-2: Add basic API");
        assert!(tasks[1].dependencies.is_empty());

        // Track B tasks should depend on prd-1 and prd-2
        assert_eq!(tasks[2].description, "prd-3: Create CLI interface **USE /carousel SKILL**");
        assert_eq!(tasks[2].dependencies, vec!["prd-1", "prd-2"]);
        assert_eq!(tasks[2].skill_hint, Some("carousel".to_string()));

        assert_eq!(tasks[3].description, "prd-4: Add commands");
        assert_eq!(tasks[3].dependencies, vec!["prd-1", "prd-2"]);
        assert_eq!(tasks[3].skill_hint, None);

        // Track C tasks should have no dependencies
        assert_eq!(tasks[4].description, "prd-5: Advanced features USE /research SKILL");
        assert!(tasks[4].dependencies.is_empty());
        assert_eq!(tasks[4].skill_hint, Some("research".to_string()));
    }

    #[test]
    fn test_progress() {
        let tasks = parse_prd_content(SAMPLE_PRD).unwrap();
        let (done, total) = progress(&tasks);
        assert_eq!(done, 2);
        assert_eq!(total, 5);
    }

    #[test]
    fn test_mark_task_done() {
        // Create a temp file with sample PRD content
        let temp_dir = std::env::temp_dir();
        let test_prd_path = temp_dir.join("test_prd_mark_done.md");

        {
            let mut file = std::fs::File::create(&test_prd_path).unwrap();
            file.write_all(SAMPLE_PRD.as_bytes()).unwrap();
        }

        // Mark task 1 as done (first unchecked task)
        mark_task_done(&test_prd_path, "prd-1").unwrap();

        // Read and verify
        let content = std::fs::read_to_string(&test_prd_path).unwrap();
        assert!(content.contains("- [x] AC-1.1: First task"));
        assert!(content.contains("- [ ] AC-1.3: Third task")); // Other tasks unchanged

        // Mark task 3 as done
        mark_task_done(&test_prd_path, "prd-3").unwrap();
        let content = std::fs::read_to_string(&test_prd_path).unwrap();
        assert!(content.contains("- [x] AC-1.3: Third task"));

        // Try to mark already-done task (should fail gracefully)
        let result = mark_task_done(&test_prd_path, "prd-2");
        assert!(result.is_err());

        // Cleanup
        let _ = std::fs::remove_file(&test_prd_path);
    }

    #[test]
    fn test_mark_task_done_invalid_id() {
        let temp_dir = std::env::temp_dir();
        let test_prd_path = temp_dir.join("test_prd_invalid.md");

        {
            let mut file = std::fs::File::create(&test_prd_path).unwrap();
            file.write_all(SAMPLE_PRD.as_bytes()).unwrap();
        }

        // Invalid task ID format
        let result = mark_task_done(&test_prd_path, "invalid-123");
        assert!(result.is_err());

        // Task ID out of range
        let result = mark_task_done(&test_prd_path, "prd-999");
        assert!(result.is_err());

        // Cleanup
        let _ = std::fs::remove_file(&test_prd_path);
    }
}
