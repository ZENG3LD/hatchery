//! `hatchery-arcade-sweep` -- a headless CLI balance driver over
//! `hatchery_arcade_engine::sweep_api::simulate`. Runs a grid of seeds x
//! difficulties x policies for Pet Bastion: Night Garden and prints a
//! balance report: win rate per difficulty, death-wave distribution,
//! outcome distribution, ticks-per-run, and an explicit determinism check
//! (same seed + same policy, run twice, `final_hash` compared).
//!
//! Never depends on a terminal -- see `Cargo.toml`'s own `default-features
//! = false` structural guard on both its `engine` and `pet-bastion`
//! dependency edges.

mod cli;
mod policy;
mod report;

use hatchery_arcade_engine::sweep_api::simulate;
use hatchery_arcade_pet_bastion::sim::{PetBastionParams, Simulation};
use hatchery_arcade_pet_bastion::wave::{BalanceOverrides, Difficulty};

use cli::Cli;
use policy::{BaselinePolicy, CircuitPolicy, GreedyPolicy, PolicyKind, SlowStackPolicy};
use hatchery_arcade_engine::RunOutcome;
use report::{DeathCause, DifficultyReport, PolicyReport, RunRecord};

fn main() {
    let cli = match Cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("hatchery-arcade-sweep: {err}");
            eprintln!();
            eprintln!("{}", Cli::usage());
            std::process::exit(2);
        }
    };
    if cli.help {
        println!("{}", Cli::usage());
        return;
    }

    println!("hatchery-arcade-sweep");
    println!(
        "seeds={} start_seed={} max_ticks={} difficulty={:?}",
        cli.seeds, cli.start_seed, cli.max_ticks, cli.difficulty
    );
    println!(
        "balance overrides: bellkeeper_hp_mult={}pm night_maw_hp_mult={}pm reward_mult={}pm late_minion_hp_mult={}pm integrity_mult={}pm",
        cli.balance_overrides.bellkeeper_hp_permille,
        cli.balance_overrides.night_maw_hp_permille,
        cli.balance_overrides.reward_permille,
        cli.balance_overrides.late_minion_hp_permille,
        cli.balance_overrides.integrity_permille,
    );
    println!();

    for difficulty in cli.difficulty.selected() {
        let mut policy_reports = Vec::new();
        for kind in [PolicyKind::Baseline, PolicyKind::Greedy, PolicyKind::Circuit, PolicyKind::SlowStack] {
            let records = run_grid(difficulty, kind, cli.balance_overrides, cli.start_seed, cli.seeds, cli.max_ticks);
            let determinism = check_determinism(difficulty, kind, cli.balance_overrides, cli.start_seed, cli.max_ticks);
            policy_reports.push(PolicyReport::new(kind, records, determinism));
        }
        let report = DifficultyReport { difficulty, policies: policy_reports };
        report.print();
    }
}

/// Runs one (difficulty, policy) combination across `seeds` consecutive
/// seeds starting at `start_seed`, each through a fresh `Simulation` and a
/// fresh policy instance (a `Policy` is allowed to carry per-run mutable
/// state, e.g. a build-order cursor -- it must never leak across runs).
fn run_grid(
    difficulty: Difficulty,
    kind: PolicyKind,
    balance_overrides: BalanceOverrides,
    start_seed: u64,
    seeds: u64,
    max_ticks: u64,
) -> Vec<RunRecord> {
    (start_seed..start_seed + seeds)
        .map(|seed| {
            let params = PetBastionParams { difficulty, balance_overrides };
            let (report, actions) = match kind {
                PolicyKind::Baseline => {
                    let mut policy = BaselinePolicy::default();
                    let report = simulate::<Simulation>(seed, params, &mut policy, max_ticks);
                    (report, policy.counters)
                }
                PolicyKind::Greedy => {
                    let mut policy = GreedyPolicy::default();
                    let report = simulate::<Simulation>(seed, params, &mut policy, max_ticks);
                    (report, policy.counters)
                }
                PolicyKind::Circuit => {
                    let mut policy = CircuitPolicy::default();
                    let report = simulate::<Simulation>(seed, params, &mut policy, max_ticks);
                    (report, policy.counters)
                }
                PolicyKind::SlowStack => {
                    let mut policy = SlowStackPolicy::default();
                    let report = simulate::<Simulation>(seed, params, &mut policy, max_ticks);
                    (report, policy.counters)
                }
            };
            let boss_hp_permille_at_death =
                report.final_snapshot.boss.as_ref().map(|b| (b.hp as i64 * 1000) / (b.max_hp.max(1) as i64));
            // See `DeathCause`'s own doc for why this reads `integrity`
            // rather than trusting `boss_hp_permille_at_death.is_some()`
            // to mean "died to the boss".
            let death_cause = (report.outcome == Some(RunOutcome::Lost)).then(|| {
                if report.final_snapshot.integrity <= 0 {
                    DeathCause::IntegrityDrained
                } else {
                    DeathCause::HeartseedBreach
                }
            });
            RunRecord {
                seed,
                ticks_run: report.ticks_run,
                outcome: report.outcome,
                death_wave: report.final_snapshot.wave,
                final_hash: report.final_hash,
                actions,
                boss_hp_permille_at_death,
                death_cause,
            }
        })
        .collect()
}

/// Runs the SAME seed through the SAME policy kind twice, from scratch, and
/// compares `final_hash` -- the observable proof that "identical seed and
/// commands produce the same final stable hash" actually holds for this
/// game through the headless harness, not merely asserted.
fn check_determinism(
    difficulty: Difficulty,
    kind: PolicyKind,
    balance_overrides: BalanceOverrides,
    seed: u64,
    max_ticks: u64,
) -> (u64, u64) {
    let params_a = PetBastionParams { difficulty, balance_overrides };
    let params_b = PetBastionParams { difficulty, balance_overrides };
    let hash_a = match kind {
        PolicyKind::Baseline => {
            let mut policy = BaselinePolicy::default();
            simulate::<Simulation>(seed, params_a, &mut policy, max_ticks).final_hash
        }
        PolicyKind::Greedy => {
            let mut policy = GreedyPolicy::default();
            simulate::<Simulation>(seed, params_a, &mut policy, max_ticks).final_hash
        }
        PolicyKind::Circuit => {
            let mut policy = CircuitPolicy::default();
            simulate::<Simulation>(seed, params_a, &mut policy, max_ticks).final_hash
        }
        PolicyKind::SlowStack => {
            let mut policy = SlowStackPolicy::default();
            simulate::<Simulation>(seed, params_a, &mut policy, max_ticks).final_hash
        }
    };
    let hash_b = match kind {
        PolicyKind::Baseline => {
            let mut policy = BaselinePolicy::default();
            simulate::<Simulation>(seed, params_b, &mut policy, max_ticks).final_hash
        }
        PolicyKind::Greedy => {
            let mut policy = GreedyPolicy::default();
            simulate::<Simulation>(seed, params_b, &mut policy, max_ticks).final_hash
        }
        PolicyKind::Circuit => {
            let mut policy = CircuitPolicy::default();
            simulate::<Simulation>(seed, params_b, &mut policy, max_ticks).final_hash
        }
        PolicyKind::SlowStack => {
            let mut policy = SlowStackPolicy::default();
            simulate::<Simulation>(seed, params_b, &mut policy, max_ticks).final_hash
        }
    };
    (hash_a, hash_b)
}
