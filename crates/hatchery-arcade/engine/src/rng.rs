//! Explicit, struct-owned seeded PRNG for the sim core.
//!
//! [`EngineRng`] is xorshift64* -- fast, well-distributed enough for
//! gameplay randomness, NOT cryptographically secure and not intended to
//! be. It is deliberately the ONLY randomness source a
//! [`crate::game::MiniGame`] may consult: never a global/thread-local
//! generator. Contrast `uzor_text::ascii::GlitchLetter`'s legitimate use
//! of the `fastrand` crate's global thread-local state for decorative
//! glitch/spark jitter -- correct for a non-deterministic visual effect,
//! wrong for anything a replay or the headless balance harness must
//! reproduce bit-for-bit, since a thread-local generator's state is not
//! seed-reproducible across processes/thread scheduling.
//! [`crate::runner::Runner`] owns exactly one `EngineRng` per run, seeded
//! once at construction and threaded by `&mut` through every `advance()`
//! call.

/// Seeded xorshift64* generator. `Clone`/`Copy` are derived purely for
/// test/debug ergonomics (snapshotting a generator's state to compare two
/// independently-advanced copies) -- production code should still only
/// ever hold ONE live generator per run, per the module doc above.
#[derive(Debug, Clone, Copy)]
pub struct EngineRng(u64);

impl EngineRng {
    /// Seeds the generator. `0` is a fixed point for xorshift (it would
    /// never leave the all-zero state), so the seed is forced odd.
    pub fn seed(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Documented, accepted small modulo bias at gameplay scale (this is
    /// not a cryptographic generator) -- reported honestly rather than
    /// hidden. `upper_exclusive == 0` is treated as `1` (a degenerate
    /// single-outcome range) rather than dividing by zero.
    pub fn gen_range(&mut self, upper_exclusive: u32) -> u32 {
        self.next_u32() % upper_exclusive.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_produces_the_same_sequence() {
        let mut a = EngineRng::seed(42);
        let mut b = EngineRng::seed(42);
        for _ in 0..16 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = EngineRng::seed(1);
        let mut b = EngineRng::seed(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn gen_range_never_reaches_the_exclusive_upper_bound() {
        let mut rng = EngineRng::seed(7);
        for _ in 0..1000 {
            assert!(rng.gen_range(5) < 5);
        }
    }

    #[test]
    fn gen_range_of_zero_never_panics() {
        let mut rng = EngineRng::seed(7);
        assert_eq!(rng.gen_range(0), 0);
    }
}
