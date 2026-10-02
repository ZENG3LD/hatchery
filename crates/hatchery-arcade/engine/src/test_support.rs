//! Shared, `#[cfg(test)]`-only `MiniGame` fixtures used by this crate's
//! own test suites (`runner`, `shell`, `cadence`, `replay`, `sweep_api`)
//! so each module's tests do not hand-roll a slightly different toy game.
//! Named `TestGame`/`TestGame2` rather than the plan's own
//! "`NullGame`/`NullGame2`" naming -- functionally identical fixtures,
//! renamed only to read clearly as test-only from any call site.

use std::time::Duration;

use crate::{
    admission::{AdmissionCredit, AdmissionError, AdmissionSource},
    game::{MiniGame, RunOutcome},
    hash::StableHasher,
    rng::EngineRng,
    shell::{CellArea, GameEntry},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TestCommand {
    Add(i64),
}

#[derive(Clone, Debug)]
pub(crate) struct TestEvent(pub u64);

#[derive(Clone, Copy, Debug)]
pub(crate) struct TestSnapshot {
    pub tick: u64,
    pub value: i64,
}

/// A tiny, real (not fake-passing) `MiniGame`: every tick sums the
/// commands' own deltas AND draws one `rng` value into `value`, so its
/// `stable_hash` is sensitive to BOTH the command log and the seed --
/// exactly the two properties this crate's `stable_hash_*` tests must
/// exercise.
pub(crate) struct TestGame {
    tick: u64,
    value: i64,
}

impl MiniGame for TestGame {
    type Params = ();
    type Command = TestCommand;
    type Event = TestEvent;
    type Snapshot = TestSnapshot;

    const TICK: Duration = Duration::from_millis(50);
    const RULES_VERSION: u32 = 1;

    fn new(_seed: u64, _params: Self::Params) -> Self {
        // The seed itself never leaks into `value` directly -- only via
        // `rng` draws inside `advance` -- so two runs with the SAME seed
        // always start identically, and only diverge if `advance` is fed
        // a different seed's own `rng` sequence.
        Self { tick: 0, value: 0 }
    }

    fn advance(&mut self, rng: &mut EngineRng, commands: &[Self::Command]) -> Vec<Self::Event> {
        self.tick += 1;
        for command in commands {
            let TestCommand::Add(delta) = command;
            self.value += *delta;
        }
        self.value += rng.gen_range(1000) as i64;
        vec![TestEvent(self.tick)]
    }

    fn snapshot(&self) -> Self::Snapshot {
        TestSnapshot { tick: self.tick, value: self.value }
    }

    fn stable_hash(&self) -> u64 {
        let mut hasher = StableHasher::new();
        hasher.write_u64(self.tick);
        hasher.write_i64(self.value);
        hasher.finish()
    }

    fn is_finished(&self) -> Option<RunOutcome> {
        None
    }
}

impl GameEntry for TestGame {
    const ID: &'static str = "test-game";
    const TITLE: &'static str = "Test Game";

    fn min_modal_size() -> CellArea {
        CellArea { width: 20, height: 10 }
    }

    fn preferred_modal_size() -> CellArea {
        CellArea { width: 40, height: 20 }
    }
}

/// A second, differently-sized fixture -- proves the game-select seam
/// genuinely generalizes over more than one concrete `MiniGame` type
/// (plan step 5).
pub(crate) struct TestGame2 {
    tick: u64,
}

impl MiniGame for TestGame2 {
    type Params = ();
    type Command = TestCommand;
    type Event = TestEvent;
    type Snapshot = TestSnapshot;

    const TICK: Duration = Duration::from_millis(100);
    const RULES_VERSION: u32 = 1;

    fn new(_seed: u64, _params: Self::Params) -> Self {
        Self { tick: 0 }
    }

    fn advance(&mut self, _rng: &mut EngineRng, _commands: &[Self::Command]) -> Vec<Self::Event> {
        self.tick += 1;
        Vec::new()
    }

    fn snapshot(&self) -> Self::Snapshot {
        TestSnapshot { tick: self.tick, value: 0 }
    }

    fn stable_hash(&self) -> u64 {
        let mut hasher = StableHasher::new();
        hasher.write_u64(self.tick);
        hasher.finish()
    }

    fn is_finished(&self) -> Option<RunOutcome> {
        None
    }
}

impl GameEntry for TestGame2 {
    const ID: &'static str = "test-game-2";
    const TITLE: &'static str = "Test Game 2";

    fn min_modal_size() -> CellArea {
        CellArea { width: 60, height: 30 }
    }

    fn preferred_modal_size() -> CellArea {
        CellArea { width: 80, height: 40 }
    }
}

/// A trivial in-memory `AdmissionSource` fixture shared by `admission.rs`
/// and `runner.rs`'s own test suites.
pub(crate) struct CounterSource(pub u32);

impl AdmissionSource for CounterSource {
    fn available(&self) -> AdmissionCredit {
        AdmissionCredit(self.0)
    }

    fn try_debit(&mut self, cost: AdmissionCredit) -> Result<(), AdmissionError> {
        if self.0 < cost.0 {
            return Err(AdmissionError::InsufficientCredit { required: cost, available: AdmissionCredit(self.0) });
        }
        self.0 -= cost.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises `TestSnapshot`'s and `TestEvent`'s own fields directly --
    /// otherwise nothing in this crate's test suites ever READS them
    /// (every other test only compares `stable_hash`/tick counts), which
    /// would make them dead code under `cfg(test)`.
    #[test]
    fn test_game_snapshot_and_events_reflect_advance() {
        let mut game = TestGame::new(1, ());
        let mut rng = EngineRng::seed(1);
        let events = game.advance(&mut rng, &[TestCommand::Add(5)]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, 1);
        let snap = game.snapshot();
        assert_eq!(snap.tick, 1);
        assert!(snap.value >= 5);
    }
}
