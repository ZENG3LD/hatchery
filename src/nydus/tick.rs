//! Heuristic tick interval for event-driven Nydus.
//!
//! Instead of a fixed 2-second sleep, adapts the tick interval based on activity:
//! - Shrinks to minimum when completions are happening (fast scheduling)
//! - Backs off exponentially when idle (save CPU)
//! - Falls back to a maximum interval as a safety net

use std::time::{Duration, Instant};

/// Adaptive tick interval controller.
///
/// Tracks activity and adjusts the sleep duration between Nydus ticks.
/// When tasks are completing rapidly, ticks happen frequently.
/// When nothing is happening, ticks slow down to save resources.
#[derive(Debug)]
pub struct HeuristicTick {
    /// Minimum interval (floor). Default: 10ms
    min_interval: Duration,
    /// Maximum interval (ceiling / fallback). Default: 500ms
    max_interval: Duration,
    /// Current computed interval
    current_interval: Duration,
    /// Last time a meaningful event occurred (completion, failure, etc.)
    last_meaningful_event: Instant,
    /// Events received since last interval computation
    events_since_tick: usize,
    /// Completions received since last interval computation
    completions_since_tick: usize,
    /// Failures received since last interval computation
    failures_since_tick: usize,
}

impl HeuristicTick {
    /// Create with default parameters.
    pub fn new() -> Self {
        Self::with_bounds(Duration::from_millis(10), Duration::from_millis(500))
    }

    /// Create with custom min/max bounds.
    pub fn with_bounds(min_interval: Duration, max_interval: Duration) -> Self {
        Self {
            min_interval,
            max_interval,
            current_interval: min_interval, // start responsive
            last_meaningful_event: Instant::now(),
            events_since_tick: 0,
            completions_since_tick: 0,
            failures_since_tick: 0,
        }
    }

    /// Record that a generic event occurred (status update, progress, etc.)
    pub fn note_event(&mut self) {
        self.events_since_tick += 1;
    }

    /// Record that a task completed — this shrinks the interval.
    pub fn note_completion(&mut self) {
        self.completions_since_tick += 1;
        self.last_meaningful_event = Instant::now();
    }

    /// Record that a task failed — also meaningful.
    pub fn note_failure(&mut self) {
        self.failures_since_tick += 1;
        self.last_meaningful_event = Instant::now();
    }

    /// Compute and return the next sleep duration.
    /// Resets internal counters.
    pub fn next_interval(&mut self) -> Duration {
        let meaningful = self.completions_since_tick + self.failures_since_tick;
        let idle_time = self.last_meaningful_event.elapsed();

        if meaningful > 0 {
            // Tasks completing/failing — stay responsive
            self.current_interval = self.min_interval;
        } else if idle_time > Duration::from_secs(5) {
            // Long idle — back off to max
            self.current_interval = self.max_interval;
        } else if idle_time > Duration::from_secs(2) {
            // Medium idle — linear backoff (+50ms increments, capped)
            self.current_interval = (self.current_interval + Duration::from_millis(50)).min(self.max_interval);
        } else if self.events_since_tick > 0 {
            // Some activity but no completions — moderate interval
            self.current_interval = Duration::from_millis(50);
        }
        // else: keep current interval

        // Reset counters
        self.events_since_tick = 0;
        self.completions_since_tick = 0;
        self.failures_since_tick = 0;

        self.current_interval
    }

    /// Get the current interval without resetting counters.
    pub fn current(&self) -> Duration {
        self.current_interval
    }

    /// Time since last meaningful event.
    pub fn idle_duration(&self) -> Duration {
        self.last_meaningful_event.elapsed()
    }
}

impl Default for HeuristicTick {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_start_interval() {
        let tick = HeuristicTick::new();
        assert_eq!(tick.current(), Duration::from_millis(10));
    }

    #[test]
    fn test_shrinks_on_completion() {
        let mut tick = HeuristicTick::new();
        tick.note_completion();
        let interval = tick.next_interval();
        assert_eq!(interval, Duration::from_millis(10));
    }

    #[test]
    fn test_shrinks_on_failure() {
        let mut tick = HeuristicTick::new();
        tick.note_failure();
        let interval = tick.next_interval();
        assert_eq!(interval, Duration::from_millis(10));
    }

    #[test]
    fn test_moderate_on_events_only() {
        let mut tick = HeuristicTick::new();
        tick.note_event();
        tick.note_event();
        let interval = tick.next_interval();
        assert_eq!(interval, Duration::from_millis(50));
    }

    #[test]
    fn test_no_change_when_quiet() {
        let mut tick = HeuristicTick::new();
        // No events at all, short idle time
        let interval = tick.next_interval();
        // Should keep current (10ms) since idle < 2s
        assert_eq!(interval, Duration::from_millis(10));
    }

    #[test]
    fn test_resets_counters() {
        let mut tick = HeuristicTick::new();
        tick.note_event();
        tick.note_completion();
        let _ = tick.next_interval();
        // After next_interval, counters should be reset
        // No events -> should keep current
        let interval = tick.next_interval();
        // Since last_meaningful_event is recent, should stay at min (10ms)
        // because idle_time < 2s and events_since_tick == 0
        assert_eq!(interval, Duration::from_millis(10));
    }

    #[test]
    fn test_custom_bounds() {
        let mut tick = HeuristicTick::with_bounds(
            Duration::from_millis(50),
            Duration::from_secs(10),
        );
        tick.note_completion();
        assert_eq!(tick.next_interval(), Duration::from_millis(50));
    }
}
