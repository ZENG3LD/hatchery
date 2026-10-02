//! The headless balance-harness surface: drives `MiniGame::advance` in a
//! tight, unthrottled loop, bypassing `Runner`'s wall-clock accumulator
//! entirely -- a sweep has no real-time pacing concern. This is the
//! crate's second consumer: `hatchery-arcade-sweep`'s own `main.rs`
//! calls [`simulate`] in a loop over thousands of `(seed, policy)` pairs.
//! Always compiled -- no `render` feature needed.

use crate::{
    game::{MiniGame, RunOutcome},
    rng::EngineRng,
};

pub trait Policy<G: MiniGame> {
    fn decide(&mut self, snapshot: &G::Snapshot, tick_index: u64) -> Vec<G::Command>;
}

pub struct SimulationReport<G: MiniGame> {
    pub ticks_run: u64,
    pub outcome: Option<RunOutcome>,
    pub final_snapshot: G::Snapshot,
    pub final_hash: u64,
}

/// Runs a game to completion or `max_ticks`.
pub fn simulate<G: MiniGame>(
    seed: u64,
    params: G::Params,
    policy: &mut dyn Policy<G>,
    max_ticks: u64,
) -> SimulationReport<G> {
    let mut game = G::new(seed, params);
    let mut rng = EngineRng::seed(seed);
    let mut tick_index = 0u64;
    while tick_index < max_ticks && game.is_finished().is_none() {
        let commands = policy.decide(&game.snapshot(), tick_index);
        game.advance(&mut rng, &commands);
        tick_index += 1;
    }
    SimulationReport {
        ticks_run: tick_index,
        outcome: game.is_finished(),
        final_hash: game.stable_hash(),
        final_snapshot: game.snapshot(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestCommand, TestGame, TestSnapshot};

    struct NoOpPolicy;
    impl Policy<TestGame> for NoOpPolicy {
        fn decide(&mut self, _snapshot: &TestSnapshot, _tick_index: u64) -> Vec<TestCommand> {
            Vec::new()
        }
    }

    #[test]
    fn simulate_runs_exactly_max_ticks_when_the_game_never_finishes() {
        let mut policy = NoOpPolicy;
        let report = simulate::<TestGame>(1, (), &mut policy, 7);
        assert_eq!(report.ticks_run, 7);
        assert_eq!(report.outcome, None);
    }

    #[test]
    fn stable_hash_is_identical_for_identical_seed_and_commands_replayed() {
        let mut a = NoOpPolicy;
        let mut b = NoOpPolicy;
        let report_a = simulate::<TestGame>(99, (), &mut a, 10);
        let report_b = simulate::<TestGame>(99, (), &mut b, 10);
        assert_eq!(report_a.final_hash, report_b.final_hash);
    }

    #[test]
    fn stable_hash_diverges_for_a_different_seed() {
        let mut a = NoOpPolicy;
        let mut b = NoOpPolicy;
        let report_a = simulate::<TestGame>(1, (), &mut a, 10);
        let report_b = simulate::<TestGame>(2, (), &mut b, 10);
        assert_ne!(report_a.final_hash, report_b.final_hash);
    }

    struct AddOnFirstTickPolicy(i64);
    impl Policy<TestGame> for AddOnFirstTickPolicy {
        fn decide(&mut self, _snapshot: &TestSnapshot, tick_index: u64) -> Vec<TestCommand> {
            if tick_index == 0 {
                vec![TestCommand::Add(self.0)]
            } else {
                Vec::new()
            }
        }
    }

    #[test]
    fn stable_hash_diverges_when_a_single_command_tick_differs() {
        let mut a = AddOnFirstTickPolicy(1);
        let mut b = AddOnFirstTickPolicy(2);
        let report_a = simulate::<TestGame>(7, (), &mut a, 5);
        let report_b = simulate::<TestGame>(7, (), &mut b, 5);
        assert_ne!(report_a.final_hash, report_b.final_hash);
    }
}
