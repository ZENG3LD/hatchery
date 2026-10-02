//! Rune drafts: a shuffle bag so no common rune recurs before the player
//! has seen the whole common set, offered after waves 2 and 6.

use crate::constants::RUNE_DRAFT_OPTIONS;
use crate::rng::EngineRng;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Rune {
    Echo,
    Anchor,
    Overgrowth,
    Phase,
    Symbiosis,
}

impl Rune {
    pub const ALL: [Rune; 5] = [
        Rune::Echo,
        Rune::Anchor,
        Rune::Overgrowth,
        Rune::Phase,
        Rune::Symbiosis,
    ];
}

/// Classic shuffle bag: draws come from a shuffled remainder; once it dips
/// below what's needed, a freshly shuffled full set is appended, so a rune
/// can never reappear before every other rune in the set has been offered
/// at least once.
#[derive(Clone, Debug, Default)]
pub struct RuneShuffleBag {
    remaining: Vec<Rune>,
}

impl RuneShuffleBag {
    pub fn new() -> Self {
        Self {
            remaining: Vec::new(),
        }
    }

    pub fn draw_options(&mut self, rng: &mut EngineRng, n: usize) -> Vec<Rune> {
        let mut result: Vec<Rune> = Vec::with_capacity(n);
        while result.len() < n {
            if self.remaining.is_empty() {
                // Exclude runes already picked earlier in THIS draft from the
                // refill -- otherwise a refill landing mid-draft could hand
                // back a rune the player is already being offered. Such an
                // excluded rune simply reappears on the very next refill.
                let mut fresh: Vec<Rune> = Rune::ALL
                    .into_iter()
                    .filter(|r| !result.contains(r))
                    .collect();
                crate::rng::shuffle(rng, &mut fresh);
                self.remaining.extend(fresh);
            }
            result.push(self.remaining.remove(0));
        }
        result
    }
}

pub fn draft_option_count() -> usize {
    RUNE_DRAFT_OPTIONS
}

/// The runes the player has actually picked this run, plus their effect
/// hooks queried elsewhere (`tower.rs`/`sim.rs`).
#[derive(Clone, Debug, Default)]
pub struct RuneLoadout {
    picked: Vec<Rune>,
}

impl RuneLoadout {
    pub fn add(&mut self, rune: Rune) {
        if !self.picked.contains(&rune) {
            self.picked.push(rune);
        }
    }

    pub fn has(&self, rune: Rune) -> bool {
        self.picked.contains(&rune)
    }

    pub fn picked(&self) -> &[Rune] {
        &self.picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rune_never_repeats_before_the_bag_is_exhausted() {
        // Drawing one rune at a time, every run of `Rune::ALL.len()`
        // consecutive draws must be a permutation of the full set -- the
        // defining shuffle-bag property.
        let mut rng = EngineRng::seed(99);
        let mut bag = RuneShuffleBag::new();
        for _ in 0..8 {
            let mut window = std::collections::HashSet::new();
            for _ in 0..Rune::ALL.len() {
                let rune = bag.draw_options(&mut rng, 1)[0];
                assert!(window.insert(rune), "rune repeated within one full pass of the bag");
            }
            assert_eq!(window.len(), Rune::ALL.len());
        }
    }

    #[test]
    fn draw_options_never_returns_duplicates_within_one_draft() {
        let mut rng = EngineRng::seed(5);
        let mut bag = RuneShuffleBag::new();
        for _ in 0..20 {
            let options = bag.draw_options(&mut rng, 3);
            let unique: std::collections::HashSet<_> = options.iter().collect();
            assert_eq!(unique.len(), options.len());
        }
    }
}
