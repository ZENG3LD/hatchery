//! Pure composition of independently-clocked wake deadlines.
//!
//! Generalizes the exact "fold every deadline through `Duration::min`"
//! shape `hatchery-tui`'s own `FrameScheduler::poll_timeout`/
//! `animation_wake_interval` already use, so a host with N
//! independently-clocked occupants (the pet's own 16ms animation cadence,
//! a hosted game's own fixed sim tick, an optional slower cosmetic
//! cadence, ...) never hand-rolls its own N-way `min()` every time a new
//! cadence is added.

use std::time::{Duration, Instant};

/// One independently-clocked "wake me by this instant, then again every
/// `interval`" contribution.
#[derive(Clone, Copy, Debug)]
pub struct Cadence {
    pub interval: Duration,
    pub next_due: Instant,
}

/// Pure, read-only composition -- takes `&[Cadence]`, not `&mut`, which
/// the type system alone already guarantees performs zero mutation. This
/// is the literal enforcement of the stated invariant: "no registered
/// cadence may speed up or slow down another." Composing N cadences to
/// decide how long `event::poll` may safely block never changes any ONE
/// cadence's own `interval`, and never advances any cadence's own
/// `next_due` -- that advancement is entirely the CALLER's job, done
/// independently per cadence, after its own deadline is the one that
/// actually fired.
///
/// `None` when `cadences` is empty -- "no registered cadence" means "no
/// opinion on how long to wait," never "wait zero."
pub fn tightest_wake(now: Instant, cadences: &[Cadence]) -> Option<Duration> {
    cadences.iter().map(|c| c.next_due.saturating_duration_since(now)).min()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cadence_at(now: Instant, ms: u64) -> Cadence {
        Cadence { interval: Duration::from_millis(ms), next_due: now + Duration::from_millis(ms) }
    }

    #[test]
    fn cadence_four_way_composition_never_perturbs_any_single_cadence() {
        let now = Instant::now();
        // The real named intervals this plan cites: 20Hz/50ms, 10Hz/100ms,
        // a 150ms cosmetic cadence, and the pet's own confirmed
        // `PET_ANIMATION_INTERVAL` of 16ms.
        let cadences = [cadence_at(now, 50), cadence_at(now, 100), cadence_at(now, 150), cadence_at(now, 16)];
        let before = cadences;

        for probe_ms in [0u64, 5, 20, 60] {
            let probe_now = now + Duration::from_millis(probe_ms);
            let expected = cadences.iter().map(|c| c.next_due.saturating_duration_since(probe_now)).min();
            assert_eq!(tightest_wake(probe_now, &cadences), expected);
        }

        // Composition itself must never mutate any cadence's own fields.
        for (a, b) in cadences.iter().zip(before.iter()) {
            assert_eq!(a.interval, b.interval);
            assert_eq!(a.next_due, b.next_due);
        }

        // The tightest of the four must be the 16ms one.
        assert_eq!(tightest_wake(now, &cadences), Some(Duration::from_millis(16)));
    }

    #[test]
    fn adding_a_cadence_never_changes_an_unrelated_ones_contribution() {
        let now = Instant::now();
        let fastest = cadence_at(now, 16);
        let slower = [cadence_at(now, 50), cadence_at(now, 100), cadence_at(now, 150)];

        let one = [fastest];
        assert_eq!(tightest_wake(now, &one), Some(Duration::from_millis(16)));

        for n in 1..=slower.len() {
            let mut set = vec![fastest];
            set.extend_from_slice(&slower[..n]);
            // Appending a SLOWER cadence must never change the result --
            // the faster one already present still wins.
            assert_eq!(tightest_wake(now, &set), Some(Duration::from_millis(16)));
        }

        // Removing a slower cadence from the full 4-element slice must not
        // change the result either.
        let full = [fastest, slower[0], slower[1], slower[2]];
        let without_slowest = [fastest, slower[0], slower[1]];
        assert_eq!(tightest_wake(now, &full), tightest_wake(now, &without_slowest));
    }

    #[test]
    fn empty_cadence_slice_has_no_opinion() {
        let now = Instant::now();
        assert_eq!(tightest_wake(now, &[]), None);
    }
}
