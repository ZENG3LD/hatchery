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
    let mut tasks = Vec::new();
    let mut id = 1;

    for (line_number, line) in content.lines().enumerate() {
        if let Some(caps) = re.captures(line) {
            let done = caps.get(2).map(|m| m.as_str() != " ").unwrap_or(false);
            let description = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();

            tasks.push(Task {
                id,
                description,
                done,
                line_number,
            });
            id += 1;
        }
    }

    Ok(tasks)
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

    const SAMPLE_PRD: &str = r#"# Test PRD

## Tasks
- [ ] AC-1.1: First task
- [x] AC-1.2: Second task (done)
- [ ] AC-1.3: Third task
- [ ] AC-2.1: Fourth task
- [x] AC-2.2: Fifth task (done)
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
        use std::io::Write;

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
