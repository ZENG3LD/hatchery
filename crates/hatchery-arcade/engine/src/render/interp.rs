//! The one piece of the fixed-step-plus-interpolation pattern that is
//! genuinely GENERIC across every hosted game: turning "how much wall-clock
//! time has passed since the sim's last completed tick" into a `0.0..=1.0`
//! render-time fraction. Nothing else about interpolation belongs here --
//! see this module's own "Why this lives in `engine`, and nothing else
//! does" doc section.
//!
//! # Why this lives in `engine`, and nothing else does
//!
//! The engine's own sim core runs at a fixed tick (20Hz for Pet Bastion,
//! `crate::game::MiniGame::TICK`) and never at the render's own faster
//! cadence (60Hz) -- interpolation is what lets a 60Hz render still look
//! smooth on top of that unchanged 20Hz sim, per the owner's own explicit
//! mandate: "симуляция остаётся на 20 тиках в секунду и не меняется ни на
//! бит." Splitting WHAT that interpolation looks like from HOW its own
//! timing fraction is computed matters because only the timing half is
//! actually game-agnostic: [`tick_alpha`] does not know an `EnemyView` from
//! a `BossBodyView`, only that a host is asking "how far between two known
//! sim states am I right now." WHICH fields to interpolate, by what key
//! (an enemy's own `EntityId`, a boss body's own `id`, the pet's own
//! `PetState`), and how to turn that into paintable data is Pet Bastion's
//! own domain knowledge -- see `hatchery-arcade-pet-bastion-render`'s own
//! `interp` module, which is where that half actually lives, exactly as
//! this whole pass's own task framing asked for.

use std::time::Duration;

/// How far the render clock has progressed from the sim's last completed
/// tick towards its next one, clamped to `0.0..=1.0`. A host already
/// tracks "elapsed time since I last ran a tick" for its own fixed-step
/// accumulator (`crate::runner::Runner`'s own `accumulator` field is the
/// canonical example); this is the pure arithmetic step from that Duration
/// to the fraction a renderer actually wants -- `0.0` right when a tick
/// just fired, approaching `1.0` just before the next one does.
///
/// `tick.is_zero()` returns `0.0` (never divides by zero, never panics) --
/// a degenerate "no real tick length" input has no meaningful progress to
/// report.
pub fn tick_alpha(elapsed_since_tick: Duration, tick: Duration) -> f64 {
    if tick.is_zero() {
        return 0.0;
    }
    (elapsed_since_tick.as_secs_f64() / tick.as_secs_f64()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_elapsed_is_zero_alpha() {
        assert_eq!(tick_alpha(Duration::ZERO, Duration::from_millis(50)), 0.0);
    }

    #[test]
    fn half_the_tick_elapsed_is_half_alpha() {
        assert_eq!(tick_alpha(Duration::from_millis(25), Duration::from_millis(50)), 0.5);
    }

    #[test]
    fn elapsed_past_a_full_tick_clamps_at_one() {
        assert_eq!(tick_alpha(Duration::from_millis(500), Duration::from_millis(50)), 1.0);
    }

    #[test]
    fn a_zero_length_tick_never_divides_by_zero() {
        assert_eq!(tick_alpha(Duration::from_millis(10), Duration::ZERO), 0.0);
    }
}
