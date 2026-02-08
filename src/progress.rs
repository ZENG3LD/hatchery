//! Progress display utilities.

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
        done, total, pct,
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
