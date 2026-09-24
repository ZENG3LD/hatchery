//! Heartbeat interval for Nydus maintenance.
//!
//! Scheduling is fully event-driven (wakeup notifications, EventBus).
//! The heartbeat is only a safety net for periodic maintenance:
//! deadlock detection, elastic pool enforcement, memory eviction.

use std::time::Duration;

/// Fixed heartbeat interval for periodic maintenance.
///
/// All real scheduling happens via events (TaskCompleted, wakeup, inject).
/// This heartbeat only triggers periodic_maintenance() as a fallback.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub struct HeuristicTick {
    interval: Duration,
}

impl HeuristicTick {
    pub fn new() -> Self {
        Self {
            interval: HEARTBEAT_INTERVAL,
        }
    }

    /// Create with custom interval (for tests).
    pub fn with_bounds(_min: Duration, max: Duration) -> Self {
        Self { interval: max }
    }

    /// No-op: scheduling is event-driven, not tick-driven.
    pub fn note_event(&mut self) {}

    /// No-op: scheduling is event-driven, not tick-driven.
    pub fn note_completion(&mut self) {}

    /// No-op: scheduling is event-driven, not tick-driven.
    pub fn note_failure(&mut self) {}

    /// Return the fixed heartbeat interval.
    pub fn next_interval(&mut self) -> Duration {
        self.interval
    }

    /// Current interval.
    pub fn current(&self) -> Duration {
        self.interval
    }

    /// Always zero — no idle tracking needed.
    pub fn idle_duration(&self) -> Duration {
        Duration::ZERO
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
    fn test_default_interval() {
        let tick = HeuristicTick::new();
        assert_eq!(tick.current(), Duration::from_secs(1));
    }

    #[test]
    fn test_next_interval_is_constant() {
        let mut tick = HeuristicTick::new();
        tick.note_completion();
        tick.note_event();
        tick.note_failure();
        assert_eq!(tick.next_interval(), Duration::from_secs(1));
        assert_eq!(tick.next_interval(), Duration::from_secs(1));
    }

    #[test]
    fn test_noop_methods() {
        let mut tick = HeuristicTick::new();
        tick.note_event();
        tick.note_completion();
        tick.note_failure();
        // No panics, no state changes
        assert_eq!(tick.current(), Duration::from_secs(1));
    }

    #[test]
    fn test_custom_interval() {
        let tick = HeuristicTick::with_bounds(
            Duration::from_millis(50),
            Duration::from_secs(5),
        );
        assert_eq!(tick.current(), Duration::from_secs(5));
    }
}
