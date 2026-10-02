//! Shared status-effect state used by both regular enemies (`enemy.rs`) and
//! boss bodies (`boss.rs`). Slow is the only status effect a boss body can
//! carry -- stun, knockback and family-resist stay exclusive to `Enemy` (see
//! `boss.rs`'s own module doc: bosses cannot be stunned or knocked back).
//!
//! Pulled out of `enemy.rs` so `BossBody` reuses the exact multiplicative
//! combine rule regular enemies already use (the plan's own `combined_slow =
//! 1 - product(1 - slow_i)`, capped at [`MAX_COMBINED_SLOW_PERMILLE`])
//! instead of a second parallel implementation. `Enemy`'s own behaviour is
//! unchanged by this move -- same fields, same math, same call sites, just
//! relocated behind `SlowState`.

use crate::constants::MAX_COMBINED_SLOW_PERMILLE;

/// One active slow application. Multiple simultaneous slows on the same
/// body combine multiplicatively, capped by [`MAX_COMBINED_SLOW_PERMILLE`].
#[derive(Clone, Copy, Debug)]
pub struct SlowEffect {
    pub magnitude_permille: i64,
    pub ticks_remaining: u32,
}

/// A body's set of currently active slow applications -- owned by `Enemy`
/// or `BossBody`, never shared between bodies (each body, including each
/// half of a split Night Maw, tracks its own independently).
#[derive(Clone, Debug, Default)]
pub struct SlowState {
    active: Vec<SlowEffect>,
}

impl SlowState {
    pub fn apply(&mut self, magnitude_permille: i64, duration_ticks: u32) {
        self.active.push(SlowEffect {
            magnitude_permille,
            ticks_remaining: duration_ticks,
        });
    }

    /// Combined multiplicative slow, capped at [`MAX_COMBINED_SLOW_PERMILLE`].
    pub fn combined_permille(&self) -> i64 {
        let mut remaining_permille = 1000i64;
        for slow in &self.active {
            remaining_permille = remaining_permille * (1000 - slow.magnitude_permille) / 1000;
        }
        let combined = 1000 - remaining_permille;
        combined.min(MAX_COMBINED_SLOW_PERMILLE).max(0)
    }

    /// Advances every active slow by one tick, dropping expired ones.
    pub fn tick(&mut self) {
        self.active.retain_mut(|s| {
            s.ticks_remaining = s.ticks_remaining.saturating_sub(1);
            s.ticks_remaining > 0
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_35_percent_slows_combine_past_the_ceiling_and_are_capped() {
        let mut state = SlowState::default();
        // 1 - 0.65^3 = 0.725, must be capped at 0.60.
        state.apply(350, 100);
        state.apply(350, 100);
        state.apply(350, 100);
        assert_eq!(state.combined_permille(), MAX_COMBINED_SLOW_PERMILLE);
    }

    #[test]
    fn a_single_slow_never_reaches_full_stop() {
        let mut state = SlowState::default();
        state.apply(999, 100);
        assert!(state.combined_permille() < 1000);
    }

    #[test]
    fn expired_slows_stop_contributing() {
        let mut state = SlowState::default();
        state.apply(500, 1);
        assert_eq!(state.combined_permille(), 500);
        state.tick();
        assert_eq!(state.combined_permille(), 0);
    }
}
