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

/// Find the first uncompleted task.
pub fn first_uncompleted(tasks: &[Task]) -> Option<&Task> {
    tasks.iter().find(|t| !t.done)
}

/// Count completed vs total tasks.
pub fn progress(tasks: &[Task]) -> (usize, usize) {
    let done = tasks.iter().filter(|t| t.done).count();
    (done, tasks.len())
}

/// Mark a task as completed in the PRD file on disk.
///
/// Replaces `- [ ]` with `- [x]` on the specific line.
pub fn mark_complete(path: &Path, task: &Task) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read PRD file: {}", path.display()))?;

    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    if task.line_number < lines.len() {
        let line = &mut lines[task.line_number];
        // Replace first occurrence of "[ ]" with "[x]"
        if let Some(pos) = line.find("[ ]") {
            line.replace_range(pos..pos + 3, "[x]");
        }
    }

    // Preserve trailing newline
    let mut output = lines.join("\n");
    if content.ends_with('\n') {
        output.push('\n');
    }

    std::fs::write(path, output)
        .with_context(|| format!("Failed to write PRD file: {}", path.display()))?;

    Ok(())
}

/// Divide uncompleted tasks among N workers (round-robin).
///
/// Returns a Vec of Vec<Task>, one per worker.
pub fn distribute_tasks(tasks: &[Task], worker_count: usize) -> Vec<Vec<Task>> {
    let mut buckets: Vec<Vec<Task>> = (0..worker_count).map(|_| Vec::new()).collect();
    let uncompleted: Vec<&Task> = tasks.iter().filter(|t| !t.done).collect();

    for (i, task) in uncompleted.into_iter().enumerate() {
        buckets[i % worker_count].push(task.clone());
    }

    buckets
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
    fn test_first_uncompleted() {
        let tasks = parse_prd_content(SAMPLE_PRD).unwrap();
        let first = first_uncompleted(&tasks).unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(first.description, "AC-1.1: First task");
    }

    #[test]
    fn test_progress() {
        let tasks = parse_prd_content(SAMPLE_PRD).unwrap();
        let (done, total) = progress(&tasks);
        assert_eq!(done, 2);
        assert_eq!(total, 5);
    }

    #[test]
    fn test_distribute_tasks() {
        let tasks = parse_prd_content(SAMPLE_PRD).unwrap();
        let buckets = distribute_tasks(&tasks, 2);
        assert_eq!(buckets.len(), 2);
        // 3 uncompleted tasks distributed among 2 workers: [task1, task3] and [task4]
        assert_eq!(buckets[0].len(), 2);
        assert_eq!(buckets[1].len(), 1);
    }
}
