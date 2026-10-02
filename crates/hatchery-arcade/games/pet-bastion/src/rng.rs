//! Thin re-export of the engine's seeded PRNG
//! ([`hatchery_arcade_engine::rng::EngineRng`]), plus this crate's own
//! Fisher-Yates [`shuffle`] helper. Shuffling a game-owned `Vec` (the rune
//! shuffle bag, a wave's spawn order) is game logic, not part of the
//! generic engine contract, so it lives here instead of on `EngineRng`
//! itself -- Rust's orphan rules forbid an inherent `impl` on a foreign
//! type anyway, so this is a free function taking `&mut EngineRng`, not a
//! method.

pub use hatchery_arcade_engine::EngineRng;

/// In-place Fisher-Yates shuffle driven by an [`EngineRng`].
pub fn shuffle<T>(rng: &mut EngineRng, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = rng.gen_range((i + 1) as u32) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_is_a_permutation_of_the_input() {
        let mut rng = EngineRng::seed(1);
        let mut items: Vec<u32> = (0..10).collect();
        shuffle(&mut rng, &mut items);
        let mut sorted = items.clone();
        sorted.sort();
        assert_eq!(sorted, (0..10).collect::<Vec<u32>>());
    }

    #[test]
    fn shuffle_of_an_empty_slice_never_panics() {
        let mut rng = EngineRng::seed(1);
        let mut items: Vec<u32> = Vec::new();
        shuffle(&mut rng, &mut items);
        assert!(items.is_empty());
    }
}
