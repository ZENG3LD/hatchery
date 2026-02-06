//! Progress tracking and stall detection.
//!
//! Maintains a progress file that accumulates learnings across iterations.
//! When the file gets too large, it's compacted to keep only key information.

use anyhow::{Context, Result};
use chrono::Local;
use std::path::{Path, PathBuf};

/// Maximum progress file size before compaction (bytes).
const MAX_PROGRESS_SIZE: u64 = 20_000;

/// Progress tracker for a single worker.
pub struct ProgressTracker {
    path: PathBuf,
    /// Consecutive iterations without progress.
    stall_count: usize,
    /// Total iterations completed.
    iteration_count: usize,
}

impl ProgressTracker {
    /// Create a new progress tracker.
    ///
    /// If `path` is None, auto-generates from PRD path:
    /// `tasks/prd-name.md` → `tasks/progress-prd-name.txt`
    pub fn new(prd_path: &Path, explicit_path: Option<PathBuf>) -> Self {
        let path = explicit_path.unwrap_or_else(|| {
            let stem = prd_path.file_stem().unwrap_or_default().to_string_lossy();
            let parent = prd_path.parent().unwrap_or(Path::new("."));
            parent.join(format!("progress-{}.txt", stem))
        });

        Self {
            path,
            stall_count: 0,
            iteration_count: 0,
        }
    }

    /// Record a successful iteration.
    pub fn record_success(&mut self, task_desc: &str) -> Result<()> {
        self.stall_count = 0;
        self.iteration_count += 1;

        let entry = format!(
            "\n--- Iteration {} [{}] ---\n✓ Completed: {}\n",
            self.iteration_count,
            Local::now().format("%H:%M:%S"),
            task_desc,
        );

        self.append(&entry)?;
        self.maybe_compact()
    }

    /// Record a failed iteration.
    pub fn record_failure(&mut self, error: &str) -> Result<()> {
        self.stall_count += 1;
        self.iteration_count += 1;

        let entry = format!(
            "\n--- Iteration {} [{}] ---\n✗ Failed: {}\n",
            self.iteration_count,
            Local::now().format("%H:%M:%S"),
            error,
        );

        self.append(&entry)?;
        self.maybe_compact()
    }

    /// Record a no-progress iteration.
    pub fn record_no_progress(&mut self, reason: &str) -> Result<()> {
        self.stall_count += 1;
        self.iteration_count += 1;

        let entry = format!(
            "\n--- Iteration {} [{}] ---\n○ No progress: {}\n",
            self.iteration_count,
            Local::now().format("%H:%M:%S"),
            reason,
        );

        self.append(&entry)?;
        self.maybe_compact()
    }

    /// Check if the worker is stalled.
    pub fn is_stalled(&self, threshold: usize) -> bool {
        self.stall_count >= threshold
    }

    /// Get stall count.
    pub fn stall_count(&self) -> usize {
        self.stall_count
    }

    /// Get iteration count.
    pub fn iteration_count(&self) -> usize {
        self.iteration_count
    }

    /// Read the full progress file content.
    pub fn read(&self) -> Result<String> {
        if self.path.exists() {
            std::fs::read_to_string(&self.path)
                .with_context(|| format!("Failed to read progress: {}", self.path.display()))
        } else {
            Ok(String::new())
        }
    }

    /// Get progress file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn append(&self, content: &str) -> Result<()> {
        use std::io::Write;

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("Failed to open progress: {}", self.path.display()))?;

        file.write_all(content.as_bytes())?;
        Ok(())
    }

    fn maybe_compact(&self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let metadata = std::fs::metadata(&self.path)?;
        if metadata.len() <= MAX_PROGRESS_SIZE {
            return Ok(());
        }

        // Simple compaction: keep last 10KB
        let content = std::fs::read_to_string(&self.path)?;
        let bytes = content.as_bytes();
        if bytes.len() > 10_000 {
            // Find a clean line boundary near 10KB from end
            let start = bytes.len() - 10_000;
            let adjusted = content[start..]
                .find('\n')
                .map(|pos| start + pos + 1)
                .unwrap_or(start);

            let compacted = format!(
                "=== Progress compacted at {} ===\n\
                 (Earlier iterations removed. {} iterations total.)\n\n{}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                self.iteration_count,
                &content[adjusted..],
            );

            std::fs::write(&self.path, compacted)?;
        }

        Ok(())
    }
}

/// Simple progress bar for terminal output.
pub fn progress_bar(done: usize, total: usize, width: usize) -> String {
    if total == 0 {
        return format!("[{}] 0/0", " ".repeat(width));
    }

    let filled = (done * width) / total;
    let empty = width - filled;
    let pct = (done * 100) / total;

    format!(
        "[{}{}] {}/{} ({}%)",
        "█".repeat(filled),
        "░".repeat(empty),
        done,
        total,
        pct,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_bar() {
        assert_eq!(progress_bar(3, 10, 20), "[██████░░░░░░░░░░░░░░] 3/10 (30%)");
        assert_eq!(progress_bar(10, 10, 20), "[████████████████████] 10/10 (100%)");
        assert_eq!(progress_bar(0, 10, 20), "[░░░░░░░░░░░░░░░░░░░░] 0/10 (0%)");
    }
}
