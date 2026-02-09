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
}
