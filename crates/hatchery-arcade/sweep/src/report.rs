//! Aggregation and stdout formatting for one (difficulty, policy) sweep
//! grid's worth of `SimulationReport`s.

use hatchery_arcade_engine::RunOutcome;
use hatchery_arcade_pet_bastion::wave::Difficulty;

use crate::policy::{ActionCounters, PolicyKind};

/// Which of the two ways a `Lost` run can end actually ended THIS one --
/// `sim.rs`'s own `advance_enemy_movement` checks a boss reaching the
/// Heartseed and Integrity hitting zero in the SAME tick, in that order,
/// so a run can only ever lose one way even when both conditions happen to
/// land together; this distinguishes the two the way `RunRecord`'s own
/// `boss_hp_permille_at_death` alone CANNOT. That field being `Some` only
/// ever meant "a boss was on the board when the run ended", not "this run
/// died to the boss" -- a real gap this crate's own balance work fell into
/// once (`CircuitPolicy`'s pre-fix, 100%-boss-focused build read as
/// "Bellkeeper at 1.0% HP, an almost-win" when 190/200 of those losses were
/// actually Integrity drained to 0/-1 by unguarded route-1 leaks, the
/// boss's own near-zero HP a correlation of a long fight, not the cause of
/// the loss -- see `CircuitPolicy`'s own doc in `policy.rs`). Derived from
/// `Integrity <= 0` on the final snapshot; the one case this can misread is
/// a boss reaching the Heartseed the SAME tick a leak ALSO happens to drop
/// Integrity to zero or below (rare, and even then the run's outcome was
/// still decided by the boss first -- `sim.rs`'s own `finish_run` no-ops
/// on the second call -- so at worst this mislabels which of two
/// simultaneous causes gets the credit, never fabricates a loss that
/// otherwise wouldn't have happened).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathCause {
    /// A boss body reached the Heartseed; Integrity was still above zero.
    HeartseedBreach,
    /// Integrity reached zero (or below) from leaks. A boss may still have
    /// been alive and on the board when this happened -- see this enum's
    /// own doc for why that boss's HP is then incidental, not evidence of
    /// "almost killed it".
    IntegrityDrained,
}

/// One completed (or ceiling-truncated) run's observable outcome.
pub struct RunRecord {
    pub seed: u64,
    pub ticks_run: u64,
    pub outcome: Option<RunOutcome>,
    /// The wave `snapshot.wave` reported on the FINAL tick -- for a `Lost`
    /// run, the wave the Heartseed fell on; for a `Won` run, always 8; for
    /// an unresolved (ceiling-truncated) run, whatever wave it had reached
    /// by `max_ticks`.
    pub death_wave: u32,
    pub final_hash: u64,
    /// Real action counts read back off the policy instance after the run
    /// finished -- see `policy.rs`'s own issued-equals-applied argument.
    pub actions: ActionCounters,
    /// The boss's `hp * 1000 / max_hp` on the FINAL tick, when a boss was
    /// present then -- `None` for a `Won` run, an unresolved run, or a loss
    /// with no boss on the board at all (a pure minion leak-out before any
    /// boss wave). Present for EITHER `DeathCause`, not just a genuine
    /// Heartseed breach -- see [`DeathCause`]'s own doc before reading this
    /// as "how close this policy got to a boss kill" on its own; pair it
    /// with `death_cause` first.
    pub boss_hp_permille_at_death: Option<i64>,
    /// `None` for a `Won` run or an unresolved (ceiling-truncated) run --
    /// [`DeathCause`] only classifies an actual `Lost` outcome.
    pub death_cause: Option<DeathCause>,
}

pub struct PolicyReport {
    pub kind: PolicyKind,
    pub records: Vec<RunRecord>,
    /// `(hash_a, hash_b)` from running the same seed through the same
    /// policy kind twice, from scratch.
    pub determinism: (u64, u64),
}

impl PolicyReport {
    pub fn new(kind: PolicyKind, records: Vec<RunRecord>, determinism: (u64, u64)) -> Self {
        Self { kind, records, determinism }
    }

    fn won(&self) -> usize {
        self.records.iter().filter(|r| r.outcome == Some(RunOutcome::Won)).count()
    }
    fn lost(&self) -> usize {
        self.records.iter().filter(|r| r.outcome == Some(RunOutcome::Lost)).count()
    }
    fn unresolved(&self) -> usize {
        self.records.iter().filter(|r| r.outcome.is_none()).count()
    }

    fn death_waves(&self) -> Vec<u32> {
        let mut waves: Vec<u32> =
            self.records.iter().filter(|r| r.outcome == Some(RunOutcome::Lost)).map(|r| r.death_wave).collect();
        waves.sort_unstable();
        waves
    }

    fn mean_ticks(&self) -> f64 {
        if self.records.is_empty() {
            return 0.0;
        }
        let sum: u64 = self.records.iter().map(|r| r.ticks_run).sum();
        sum as f64 / self.records.len() as f64
    }

    /// Count of `Lost` runs whose [`DeathCause`] was a genuine boss
    /// reaching the Heartseed.
    fn heartseed_breaches(&self) -> usize {
        self.records.iter().filter(|r| r.death_cause == Some(DeathCause::HeartseedBreach)).count()
    }

    /// Count of `Lost` runs whose [`DeathCause`] was Integrity draining to
    /// zero from leaks -- see [`DeathCause`]'s own doc for why a boss can
    /// still have been alive and on the board when this happened.
    fn integrity_drains(&self) -> usize {
        self.records.iter().filter(|r| r.death_cause == Some(DeathCause::IntegrityDrained)).count()
    }

    /// Mean residual boss HP (as a percent of max) across every LOST run of
    /// the given [`DeathCause`] that actually had a boss on the board at
    /// death -- `None` when no such run exists in this report. Split by
    /// cause, not pooled across both, because pooling is exactly the
    /// misreading [`DeathCause`]'s own doc describes: an Integrity-drained
    /// loss's boss HP is a byproduct of how long the fight ran, not a
    /// measure of "how close this policy got to a kill".
    fn mean_boss_hp_percent_at_death_for(&self, cause: DeathCause) -> Option<f64> {
        let values: Vec<i64> = self
            .records
            .iter()
            .filter(|r| r.death_cause == Some(cause))
            .filter_map(|r| r.boss_hp_permille_at_death)
            .collect();
        if values.is_empty() {
            return None;
        }
        let sum: i64 = values.iter().sum();
        Some(sum as f64 / values.len() as f64 / 10.0)
    }

    /// Total real action counts across every swept run -- the proof that a
    /// policy actually exercised the Living Circuit (`PetPulse`/`Blink`/
    /// `FullCircuit`) rather than the sweep merely trusting its source
    /// code. A policy that never touches an ability shows `0` here, by
    /// construction, not by omission.
    fn total_actions(&self) -> ActionCounters {
        let mut total = ActionCounters::default();
        for r in &self.records {
            total.towers_placed += r.actions.towers_placed;
            total.bell_placed += r.actions.bell_placed;
            total.needle_placed += r.actions.needle_placed;
            total.prism_placed += r.actions.prism_placed;
            total.embernest_placed += r.actions.embernest_placed;
            total.moonwell_placed += r.actions.moonwell_placed;
            total.upgrades_l2 += r.actions.upgrades_l2;
            total.upgrades_l3 += r.actions.upgrades_l3;
            total.move_pet += r.actions.move_pet;
            total.blink += r.actions.blink;
            total.pet_pulse += r.actions.pet_pulse;
            total.full_circuit += r.actions.full_circuit;
        }
        total
    }

    /// `(min, max)` seed actually swept -- read back off the records
    /// themselves rather than trusted from the CLI flags, so this line
    /// stays honest even if a future caller assembles `records` from a
    /// non-contiguous seed set.
    fn seed_range(&self) -> Option<(u64, u64)> {
        let min = self.records.iter().map(|r| r.seed).min()?;
        let max = self.records.iter().map(|r| r.seed).max()?;
        Some((min, max))
    }

    /// Count of distinct `final_hash` values across the swept seeds -- a
    /// cheap sanity signal that the seed is actually reaching the sim (a
    /// policy/seed pairing that always converges on the same final state
    /// would report `1` here, a red flag independent of the dedicated
    /// same-seed-twice determinism check below).
    fn unique_final_hashes(&self) -> usize {
        let set: std::collections::BTreeSet<u64> = self.records.iter().map(|r| r.final_hash).collect();
        set.len()
    }

    pub fn print(&self) {
        let total = self.records.len();
        let won = self.won();
        let lost = self.lost();
        let unresolved = self.unresolved();
        let win_rate = pct(won, total);

        println!("  policy: {}", self.kind);
        println!(
            "    outcomes: {won} won ({win_pct:.1}%), {lost} lost ({lost_pct:.1}%), {unresolved} unresolved ({unres_pct:.1}%) -- n={total}",
            win_pct = win_rate,
            lost_pct = pct(lost, total),
            unres_pct = pct(unresolved, total),
        );
        println!("    avg ticks/run: {:.1} ({:.1}s sim-time at 50ms/tick)", self.mean_ticks(), self.mean_ticks() * 0.05);
        if let Some((lo, hi)) = self.seed_range() {
            println!("    seeds swept: {lo}..={hi} ({total} runs, {} distinct final_hash values)", self.unique_final_hashes());
        }

        let actions = self.total_actions();
        let per_run = |n: u32| if total == 0 { 0.0 } else { n as f64 / total as f64 };
        println!(
            "    actions (total over {total} runs, avg/run): placed={p} ({p_avg:.2}) upgrades_l2={u2} ({u2_avg:.2}) upgrades_l3={u3} ({u3_avg:.2}) move_pet={mv} ({mv_avg:.2})",
            p = actions.towers_placed,
            p_avg = per_run(actions.towers_placed),
            u2 = actions.upgrades_l2,
            u2_avg = per_run(actions.upgrades_l2),
            u3 = actions.upgrades_l3,
            u3_avg = per_run(actions.upgrades_l3),
            mv = actions.move_pet,
            mv_avg = per_run(actions.move_pet),
        );
        println!(
            "    build kinds (total, avg/run): needle={n} ({n_avg:.2}) bell={b} ({b_avg:.2}) prism={pr} ({pr_avg:.2}) embernest={e} ({e_avg:.2}) moonwell={m} ({m_avg:.2})",
            n = actions.needle_placed,
            n_avg = per_run(actions.needle_placed),
            b = actions.bell_placed,
            b_avg = per_run(actions.bell_placed),
            pr = actions.prism_placed,
            pr_avg = per_run(actions.prism_placed),
            e = actions.embernest_placed,
            e_avg = per_run(actions.embernest_placed),
            m = actions.moonwell_placed,
            m_avg = per_run(actions.moonwell_placed),
        );
        println!(
            "    Living Circuit actions (total, avg/run): blink={bl} ({bl_avg:.2}) pet_pulse={pp} ({pp_avg:.2}) full_circuit={fc} ({fc_avg:.2})",
            bl = actions.blink,
            bl_avg = per_run(actions.blink),
            pp = actions.pet_pulse,
            pp_avg = per_run(actions.pet_pulse),
            fc = actions.full_circuit,
            fc_avg = per_run(actions.full_circuit),
        );

        let waves = self.death_waves();
        if waves.is_empty() {
            println!("    death wave (lost runs only): n/a -- no losses recorded");
        } else {
            let sum: u64 = waves.iter().map(|&w| w as u64).sum();
            let mean = sum as f64 / waves.len() as f64;
            let median = median_u32(&waves);
            println!("    death wave (lost runs only): mean={mean:.2} median={median:.1} n={}", waves.len());
            print!("    death wave histogram: ");
            for wave in 1..=8u32 {
                let count = waves.iter().filter(|&&w| w == wave).count();
                print!("w{wave}={count} ");
            }
            println!();
        }
        println!(
            "    death cause (lost runs only): heartseed_breach={} integrity_drained={}",
            self.heartseed_breaches(),
            self.integrity_drains()
        );
        match self.mean_boss_hp_percent_at_death_for(DeathCause::HeartseedBreach) {
            Some(pct) => println!("    mean boss HP% remaining at a genuine Heartseed breach: {pct:.1}%"),
            None => println!("    mean boss HP% remaining at a genuine Heartseed breach: n/a -- no such losses recorded"),
        }
        if let Some(pct) = self.mean_boss_hp_percent_at_death_for(DeathCause::IntegrityDrained) {
            println!(
                "    mean boss HP% remaining at an Integrity-drained death: {pct:.1}% (incidental -- see DeathCause's own doc)"
            );
        }

        let (a, b) = self.determinism;
        let status = if a == b { "MATCH" } else { "MISMATCH" };
        println!("    determinism check (seed replayed twice): {a:#018x} == {b:#018x} -> {status}");
    }
}

fn pct(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64 * 100.0
    }
}

/// Median of an already-sorted slice; even-length slices average the two
/// middle elements (as `f64`, so a median can legitimately land on a
/// half-wave).
fn median_u32(sorted: &[u32]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        sorted[n / 2] as f64
    } else {
        (sorted[n / 2 - 1] as f64 + sorted[n / 2] as f64) / 2.0
    }
}

pub struct DifficultyReport {
    pub difficulty: Difficulty,
    pub policies: Vec<PolicyReport>,
}

impl DifficultyReport {
    pub fn print(&self) {
        println!("=== difficulty: {:?} ===", self.difficulty);
        for policy in &self.policies {
            policy.print();
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(seed: u64, outcome: Option<RunOutcome>, death_wave: u32, ticks: u64) -> RunRecord {
        let death_cause = (outcome == Some(RunOutcome::Lost)).then_some(DeathCause::HeartseedBreach);
        RunRecord {
            seed,
            ticks_run: ticks,
            outcome,
            death_wave,
            final_hash: seed,
            actions: ActionCounters::default(),
            boss_hp_permille_at_death: None,
            death_cause,
        }
    }

    #[test]
    fn median_of_odd_length_is_the_middle_element() {
        assert_eq!(median_u32(&[1, 3, 5]), 3.0);
    }

    #[test]
    fn median_of_even_length_averages_the_two_middles() {
        assert_eq!(median_u32(&[1, 2, 3, 4]), 2.5);
    }

    #[test]
    fn win_rate_and_counts_reflect_the_recorded_outcomes() {
        let records = vec![
            record(0, Some(RunOutcome::Won), 8, 100),
            record(1, Some(RunOutcome::Lost), 3, 50),
            record(2, None, 5, 30_000),
        ];
        let report = PolicyReport::new(PolicyKind::Baseline, records, (7, 7));
        assert_eq!(report.won(), 1);
        assert_eq!(report.lost(), 1);
        assert_eq!(report.unresolved(), 1);
        assert_eq!(report.death_waves(), vec![3]);
    }
}
