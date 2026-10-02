//! The eight-wave run: threat/reward budget formulas, difficulty presets,
//! and the wave generator that spends 95-105% of a wave's effective threat.

use crate::boss::BossKind;
use crate::board::RouteId;
use crate::constants::*;
use crate::enemy::EnemyKind;
use crate::rng::EngineRng;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Cozy,
    Standard,
    Wild,
}

#[derive(Clone, Copy, Debug)]
pub struct DifficultyPreset {
    pub threat_permille: i64,
    pub hp_permille: i64,
    pub reward_permille: i64,
    pub start_integrity: i32,
}

/// Search-time balance overrides layered on TOP of a `Difficulty` preset --
/// `gate4agent-arcade-sweep`'s own grid-search knobs
/// (`--bellkeeper-hp-mult`/`--night-maw-hp-mult`/`--reward-mult`), threaded
/// through `PetBastionParams` so the sweep can vary calibration numbers
/// without recompiling `constants.rs`. Neutral (every field `1000`
/// permille) reproduces the plain difficulty-preset arithmetic exactly, so
/// every non-sweep caller (normal play, every existing test) is unaffected
/// by this existing at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalanceOverrides {
    /// Extra multiplier on Bellkeeper's (wave 4) effective HP, permille.
    pub bellkeeper_hp_permille: i64,
    /// Extra multiplier on Night Maw's (wave 8) effective HP, permille.
    pub night_maw_hp_permille: i64,
    /// Extra multiplier on every wave's clear reward, permille.
    pub reward_permille: i64,
    /// Extra multiplier on minion HP for waves 5-8 only (waves 1-4 are
    /// unaffected) -- the sweep's own finding this exists to fix: with every
    /// other override neutral, 300 Cozy seeds that cleared a weakened
    /// Bellkeeper produced ZERO wave 5-7 deaths (`gate4agent-arcade-sweep
    /// --bellkeeper-hp-mult=300 --seeds=300`, every policy's own death-wave
    /// histogram read `w5=0 w6=0 w7=0`) -- the run is entirely wall-shaped
    /// around the two bosses, with nothing between them able to end a run.
    /// Scoped to wave 5+ specifically (not applied to waves 1-4) so it
    /// stays a lever for the currently-risk-free stretch between the two
    /// boss walls, not a second knob fighting over the SAME wave-4 pass
    /// rate `bellkeeper_hp_permille` already controls.
    pub late_minion_hp_permille: i64,
    /// Extra multiplier on starting Integrity, permille -- the sweep's own
    /// second finding: even a raised `late_minion_hp_permille` never turned
    /// into an actual wave 5-7 LOSS for a diversified build (`gate4agent-
    /// arcade-sweep`'s own greedy-policy runs kept converting the extra
    /// minion toughness into more wave-8 losses, never a single wave 5/6/7
    /// death, all the way up to a x1.4 minion-HP multiplier -- Integrity's
    /// own budget (14-25, difficulty-dependent) is simply generous enough
    /// that a stronger build never bleeds it out before wave 8). Tightening
    /// this budget directly, instead of only inflating minion HP further,
    /// is what actually lets wave 5-7 leaks end a run.
    pub integrity_permille: i64,
}

impl Default for BalanceOverrides {
    fn default() -> Self {
        BalanceOverrides {
            bellkeeper_hp_permille: 1000,
            night_maw_hp_permille: 1000,
            reward_permille: 1000,
            late_minion_hp_permille: 1000,
            integrity_permille: 1000,
        }
    }
}

impl BalanceOverrides {
    fn boss_hp_permille(self, kind: BossKind) -> i64 {
        match kind {
            BossKind::Bellkeeper => self.bellkeeper_hp_permille,
            BossKind::NightMaw => self.night_maw_hp_permille,
        }
    }
}

impl Difficulty {
    pub fn preset(self) -> DifficultyPreset {
        match self {
            Difficulty::Cozy => DifficultyPreset {
                threat_permille: COZY_THREAT_PERMILLE,
                hp_permille: COZY_HP_PERMILLE,
                reward_permille: COZY_REWARD_PERMILLE,
                start_integrity: COZY_INTEGRITY,
            },
            Difficulty::Standard => DifficultyPreset {
                threat_permille: STANDARD_THREAT_PERMILLE,
                hp_permille: STANDARD_HP_PERMILLE,
                reward_permille: STANDARD_REWARD_PERMILLE,
                start_integrity: STANDARD_INTEGRITY,
            },
            Difficulty::Wild => DifficultyPreset {
                threat_permille: WILD_THREAT_PERMILLE,
                hp_permille: WILD_HP_PERMILLE,
                reward_permille: WILD_REWARD_PERMILLE,
                start_integrity: WILD_INTEGRITY,
            },
        }
    }
}

/// Rounds `n/d` to the nearest integer, ties rounding up (positive values
/// only, which is all this crate ever divides).
pub fn round_div(n: i64, d: i64) -> i64 {
    (n + d / 2) / d
}

pub fn base_threat(wave: u32) -> u32 {
    BASE_THREAT[(wave - 1) as usize]
}

pub fn is_boss_wave(wave: u32) -> bool {
    BOSS_WAVES.contains(&wave)
}

pub fn effective_threat(wave: u32) -> u32 {
    let base = base_threat(wave) as i64;
    if is_boss_wave(wave) {
        round_div(base * BOSS_THREAT_MULT_NUM, BOSS_THREAT_MULT_DEN) as u32
    } else {
        base as u32
    }
}

pub fn hp_scalar_permille(wave: u32) -> i64 {
    HP_SCALAR_BASE_PERMILLE + HP_SCALAR_STEP_PERMILLE * (wave as i64 - 1)
}

/// HP scalar combined with the difficulty preset's own HP multiplier -- the
/// one function every HP-bearing spawn (minion or boss) scales through.
pub fn effective_hp_scalar_permille(wave: u32, difficulty: Difficulty) -> i64 {
    round_div(hp_scalar_permille(wave) * difficulty.preset().hp_permille, 1000)
}

/// [`effective_hp_scalar_permille`] plus [`BalanceOverrides::late_minion_hp_permille`]
/// for wave 5 onward -- the scalar every MINION spawn (never a boss) goes
/// through. Waves 1-4 ignore the override entirely (its own doc explains
/// why the scope is deliberately just 5-8). Unlike [`generate_wave`]'s own
/// `boss_max_hp` (which has a real constant, `BELLKEEPER_BASE_HP`/
/// `NIGHT_MAW_BASE_HP`, to fold an override into equivalently), there is
/// no standalone constant this override stands in for -- it is its own,
/// sweep-only concept layered on top of the wave/difficulty scalar. So
/// instead of chaining a second `round_div` on top of
/// [`effective_hp_scalar_permille`]'s own already-rounded result, this
/// combines every factor (the wave step, the difficulty's own HP permille,
/// and the override) into ONE multiplication and rounds ONCE -- the same
/// single-rounding discipline, applied the only way available when there
/// is no intermediate constant to target.
pub fn effective_minion_hp_scalar_permille(wave: u32, difficulty: Difficulty, overrides: BalanceOverrides) -> i64 {
    if wave < 5 {
        effective_hp_scalar_permille(wave, difficulty)
    } else {
        round_div(
            hp_scalar_permille(wave) * difficulty.preset().hp_permille * overrides.late_minion_hp_permille,
            1_000_000,
        )
    }
}

pub fn clear_reward(wave: u32) -> i32 {
    let et = effective_threat(wave) as i64;
    CLEAR_REWARD_BASE + round_div(et * CLEAR_REWARD_MULT_NUM, CLEAR_REWARD_MULT_DEN) as i32
}

/// Folds [`BalanceOverrides::reward_permille`] into the difficulty preset's
/// own `reward_permille` via ONE `round_div` (`scaled_reward_permille`)
/// before it ever multiplies `clear_reward`, instead of chaining a SECOND
/// `round_div` on top of the already-rounded preset-scaled reward. The two
/// orders are not the same integer arithmetic: chaining rounds twice
/// (`round(round(reward*preset/1000)*override/1000)`), which can drift a
/// full unit away from `round(reward*round(preset*override/1000)/1000)` --
/// the value this now computes, which is EXACTLY what running
/// `effective_clear_reward` again with the difficulty preset's own
/// `reward_permille` replaced by `round_div(preset*override, 1000)` would
/// produce. That equivalence is the whole point: it is what makes
/// `--reward-mult=<P>` an honest stand-in for "as if `*_REWARD_PERMILLE`
/// itself were `round_div(*_REWARD_PERMILLE * P, 1000)`", the comparison a
/// calibration search run through the override actually wants to make
/// against a direct constant edit. See [`generate_wave`]'s own
/// `boss_max_hp` computation for the identical fix applied to
/// `bellkeeper_hp_permille`/`night_maw_hp_permille`.
pub fn effective_clear_reward(wave: u32, difficulty: Difficulty, overrides: BalanceOverrides) -> i32 {
    let scaled_reward_permille = round_div(difficulty.preset().reward_permille * overrides.reward_permille, 1000);
    round_div(clear_reward(wave) as i64 * scaled_reward_permille, 1000) as i32
}

/// Starting Integrity for a run: the difficulty preset's own value, plus
/// [`BalanceOverrides::integrity_permille`].
pub fn effective_start_integrity(difficulty: Difficulty, overrides: BalanceOverrides) -> i32 {
    round_div(difficulty.preset().start_integrity as i64 * overrides.integrity_permille, 1000) as i32
}

fn roster_for_wave(wave: u32) -> &'static [EnemyKind] {
    match wave {
        1 => &[EnemyKind::Mite],
        2 => &[EnemyKind::Mite, EnemyKind::Skitter],
        3 | 4 => &[EnemyKind::Mite, EnemyKind::Skitter, EnemyKind::Splitter],
        5 => &[
            EnemyKind::Mite,
            EnemyKind::Skitter,
            EnemyKind::Splitter,
            EnemyKind::Shellback,
        ],
        6 => &[
            EnemyKind::Mite,
            EnemyKind::Skitter,
            EnemyKind::Splitter,
            EnemyKind::Shellback,
            EnemyKind::Husher,
        ],
        // Mirror is not named against a specific wave in the plan's own
        // per-wave text; it is introduced here alongside wave 7's "mixed
        // elite pressure from both entrances" and included in wave 8's
        // escort, a chosen placement documented in the implementation
        // report.
        _ => &[
            EnemyKind::Mite,
            EnemyKind::Skitter,
            EnemyKind::Splitter,
            EnemyKind::Shellback,
            EnemyKind::Husher,
            EnemyKind::Mirror,
        ],
    }
}

pub fn boss_for_wave(wave: u32) -> Option<BossKind> {
    match wave {
        4 => Some(BossKind::Bellkeeper),
        8 => Some(BossKind::NightMaw),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SpawnEntry {
    pub tick_offset: u64,
    pub kind: EnemyKind,
    pub route: RouteId,
}

#[derive(Clone, Debug)]
pub struct WavePlan {
    pub wave: u32,
    pub spawns: Vec<SpawnEntry>,
    pub boss: Option<BossKind>,
    pub boss_max_hp: i32,
    pub total_threat_spent: u32,
    pub budget_lo: u32,
    pub budget_hi: u32,
}

/// Finds a combination of `roster` kinds (repeats allowed) whose total
/// threat is closest to `[lo, hi]` (preferring any sum actually inside that
/// window over the nearest sum outside it), via bounded unbounded-knapsack
/// reachability. Which sum is targeted is fully deterministic given
/// `lo`/`hi`/`target`; which MULTISET of kinds reconstructs that sum is
/// resolved via `rng`, picking uniformly among every kind that validly
/// reaches it at each backtracking step -- never a fixed roster-order
/// preference. Without that randomised backtrack, reconstruction would
/// always prefer the cheapest kind whenever a sum happens to be reachable
/// through it alone (Mite's threat of 8 divides most achievable sums),
/// which would silently starve every wave of the pricier kinds its own
/// roster was supposed to introduce (Skitter, Splitter, ...) -- a real bug
/// found while testing, not a hypothetical one.
fn closest_achievable_combo(
    roster: &[EnemyKind],
    lo: i64,
    hi: i64,
    target: i64,
    rng: &mut EngineRng,
) -> Vec<EnemyKind> {
    let lo = lo.max(0);
    let hi = hi.max(0);
    let target = target.max(0);
    let max_threat = roster.iter().map(|k| k.base_stats().threat as i64).max().unwrap_or(0);
    if max_threat == 0 {
        return Vec::new();
    }
    let search_cap = (hi + max_threat) as usize;

    let mut is_reachable = vec![false; search_cap + 1];
    is_reachable[0] = true;
    for s in 1..=search_cap {
        is_reachable[s] = roster.iter().any(|k| {
            let t = k.base_stats().threat as usize;
            t <= s && is_reachable[s - t]
        });
    }

    let mut best_s: i64 = 0;
    let mut best_dist = i64::MAX;
    let mut best_in_window = false;
    for s in 0..=search_cap as i64 {
        if !is_reachable[s as usize] {
            continue;
        }
        let in_window = s >= lo && s <= hi;
        let dist = (s - target).abs();
        let improves = match (in_window, best_in_window) {
            (true, false) => true,
            (false, true) => false,
            _ => dist < best_dist,
        };
        if improves {
            best_s = s;
            best_dist = dist;
            best_in_window = in_window;
        }
    }

    let mut kinds = Vec::new();
    let mut remaining = best_s;
    while remaining > 0 {
        let choices: Vec<EnemyKind> = roster
            .iter()
            .copied()
            .filter(|k| {
                let t = k.base_stats().threat as i64;
                t <= remaining && is_reachable[(remaining - t) as usize]
            })
            .collect();
        let pick = weighted_pick(&choices, rng);
        kinds.push(pick);
        remaining -= pick.base_stats().threat as i64;
    }
    kinds
}

/// Reconstruction-time sampling weight for `kind` -- see
/// [`ENEMY_SWARM_WEIGHT_MITE`]'s own doc for why this exists and what it
/// deliberately does NOT change (the total threat spent).
fn swarm_bias_weight(kind: EnemyKind) -> u32 {
    match kind {
        EnemyKind::Mite => ENEMY_SWARM_WEIGHT_MITE,
        EnemyKind::Skitter => ENEMY_SWARM_WEIGHT_SKITTER,
        EnemyKind::Splitter => ENEMY_SWARM_WEIGHT_SPLITTER,
        EnemyKind::Shellback => ENEMY_SWARM_WEIGHT_SHELLBACK,
        EnemyKind::Husher => ENEMY_SWARM_WEIGHT_HUSHER,
        EnemyKind::Mirror => ENEMY_SWARM_WEIGHT_MIRROR,
    }
}

/// Picks one kind from `choices` (already verified non-empty and each
/// individually valid by the caller) with probability proportional to its
/// own [`swarm_bias_weight`], via a single `rng.gen_range` roll over the
/// summed weight followed by a linear walk of the cumulative weights --
/// deterministic given `rng`'s own state, same as every other draw this
/// crate's PRNG drives.
fn weighted_pick(choices: &[EnemyKind], rng: &mut EngineRng) -> EnemyKind {
    let total_weight: u32 = choices.iter().map(|k| swarm_bias_weight(*k)).sum();
    let mut roll = rng.gen_range(total_weight);
    for &kind in choices {
        let weight = swarm_bias_weight(kind);
        if roll < weight {
            return kind;
        }
        roll -= weight;
    }
    // Unreachable given `total_weight` is the exact sum just rolled
    // against, kept only so this stays a total function rather than
    // panicking on a future weight-table typo. Falls back to Mite (every
    // roster from wave 1 on always includes it) rather than indexing
    // `choices` again, so an empty `choices` slice -- which the caller's
    // own invariant says never happens -- still cannot panic here either.
    choices.last().copied().unwrap_or(EnemyKind::Mite)
}

/// Generates one wave's spawn plan, spending as close to
/// [`WAVE_BUDGET_MIN_PERMILLE`]-[`WAVE_BUDGET_MAX_PERMILLE`] of that wave's
/// effective threat (scaled by the difficulty preset) as the wave's own
/// roster of integer-threat enemy kinds can achieve. Boss waves additionally
/// spawn exactly one boss, separately from the escort threat budget -- the
/// boss's own HP is its own balance point, not bought out of the minion
/// threat pool.
///
/// Early waves have a deliberately narrow roster (wave 1 is Mites only, by
/// design -- "Teach Circuit movement with Mites"); an 8-threat-per-unit
/// granularity cannot always land inside a +-5% band around every target
/// (wave 1's own window is only 6 threat wide). The search below always
/// finds the closest achievable total via bounded unbounded-knapsack
/// reachability, preferring a point inside the window whenever the roster
/// can reach one.
pub fn generate_wave(wave: u32, difficulty: Difficulty, overrides: BalanceOverrides, rng: &mut EngineRng) -> WavePlan {
    let preset = difficulty.preset();
    let base_et = effective_threat(wave) as i64;
    let et = round_div(base_et * preset.threat_permille, 1000).max(1);
    let lo = round_div(et * WAVE_BUDGET_MIN_PERMILLE, 1000);
    let hi = round_div(et * WAVE_BUDGET_MAX_PERMILLE, 1000);
    let roster = roster_for_wave(wave);

    let mut forced: Vec<EnemyKind> = Vec::new();
    let mut forced_spent: i64 = 0;
    if wave == 8 {
        for _ in 0..NIGHT_MAW_ESCORT_HUSHER_COUNT {
            forced.push(EnemyKind::Husher);
            forced_spent += EnemyKind::Husher.base_stats().threat as i64;
        }
    }

    let mut kinds = closest_achievable_combo(roster, lo - forced_spent, hi - forced_spent, et - forced_spent, rng);
    crate::rng::shuffle(rng, &mut kinds);
    let spent = forced_spent + kinds.iter().map(|k| k.base_stats().threat as i64).sum::<i64>();
    kinds.splice(0..0, forced);

    // Spawns land in [`WAVE_SPAWN_CLUSTER_SIZE`]-enemy packs, each member
    // [`MIN_SPAWN_INTERVAL_TICKS`] after the previous one (tight -- read as
    // "arriving together"), with a larger gap between one pack and the
    // next -- "плотные волны из слабых противников", not the old evenly
    // trickled single-file spacing every count of enemies used to get
    // squeezed into. `cluster_interval` is still sized off the wave's own
    // `target_ticks` (via `cluster_count`, not `count`), so a wave with
    // more, cheaper enemies (this pass's own composition change --
    // [`ENEMY_SWARM_WEIGHT_MITE`]'s own doc) does not compress its whole
    // spawn plan into a shorter total span than before; it just spends
    // that span delivering packs instead of single units.
    let count = kinds.len().max(1);
    let target_ticks = WAVE_COMBAT_TARGET_TICKS[(wave - 1) as usize];
    let cluster_size = WAVE_SPAWN_CLUSTER_SIZE.max(1);
    let cluster_count = count.div_ceil(cluster_size).max(1);
    let cluster_interval = (target_ticks / cluster_count as u64).max(MIN_SPAWN_INTERVAL_TICKS);
    let spawns = kinds
        .into_iter()
        .enumerate()
        .map(|(i, kind)| {
            let cluster_index = (i / cluster_size) as u64;
            let within_cluster = (i % cluster_size) as u64;
            SpawnEntry {
                tick_offset: cluster_index * cluster_interval + within_cluster * MIN_SPAWN_INTERVAL_TICKS,
                kind,
                route: RouteId((i % 2) as u8),
            }
        })
        .collect();

    let boss = boss_for_wave(wave);
    // `overrides.boss_hp_permille(k)` folds into `k.base_hp()` FIRST, via
    // its own `round_div`, before the unchanged `effective_hp_scalar_permille`
    // pipeline runs -- not applied as a THIRD `round_div` on top of the
    // already wave/difficulty-scaled HP. That ordering makes an override
    // permille arithmetically equivalent to editing `BELLKEEPER_BASE_HP`/
    // `NIGHT_MAW_BASE_HP` itself to `scaled_base_hp` and re-running the
    // SAME (otherwise unmodified) formula -- the comparison a calibration
    // search run through `--bellkeeper-hp-mult`/`--night-maw-hp-mult`
    // actually wants against a direct constant edit. The previous order
    // (override applied AFTER the wave/difficulty scalar) chained a
    // SECOND independent rounding on top of a value the scalar's own
    // rounding had already committed to, which could drift a full HP unit
    // away from what editing the base constant directly and running the
    // unmodified pipeline once would give -- the concrete case a prior
    // calibration pass hit and worked around by searching directly against
    // the constant instead of through this override (see
    // `BELLKEEPER_BASE_HP`'s own doc); this fix removes the need for that
    // workaround.
    let boss_max_hp = boss
        .map(|k| {
            let scaled_base_hp = round_div(k.base_hp() as i64 * overrides.boss_hp_permille(k), 1000);
            round_div(scaled_base_hp * effective_hp_scalar_permille(wave, difficulty), 1000) as i32
        })
        .unwrap_or(0);

    WavePlan {
        wave,
        spawns,
        boss,
        boss_max_hp,
        total_threat_spent: spent as u32,
        budget_lo: lo as u32,
        budget_hi: hi as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worked_table_matches_the_plans_own_numbers() {
        let expected_base = [60, 76, 97, 123, 156, 198, 252, 320];
        let expected_effective = [60, 76, 97, 154, 156, 198, 252, 400];
        let expected_reward = [59, 70, 84, 123, 124, 153, 189, 290];
        for w in 1..=8u32 {
            assert_eq!(base_threat(w), expected_base[(w - 1) as usize], "wave {w} base");
            assert_eq!(
                effective_threat(w),
                expected_effective[(w - 1) as usize],
                "wave {w} effective"
            );
            assert_eq!(clear_reward(w), expected_reward[(w - 1) as usize], "wave {w} reward");
        }
    }

    #[test]
    fn every_wave_spends_the_closest_the_roster_can_reach_to_its_budget_window() {
        // Enemy threat costs are integers (8, 16, 30, 32, 34, 36); a window
        // as narrow as wave 1's (roster: Mite only, threat 8, window width
        // 6) cannot always be hit exactly. The generator must still land
        // within one roster-minimum-threat unit of the window -- i.e. it
        // never leaves an achievable closer point on the table.
        for seed in 0..200u64 {
            let mut rng = EngineRng::seed(seed);
            for wave in 1..=8u32 {
                let plan = generate_wave(wave, Difficulty::Standard, BalanceOverrides::default(), &mut rng);
                let min_threat = roster_for_wave(wave)
                    .iter()
                    .map(|k| k.base_stats().threat)
                    .min()
                    .unwrap_or(1) as i64;
                let spent = plan.total_threat_spent as i64;
                assert!(
                    spent >= plan.budget_lo as i64 - min_threat && spent <= plan.budget_hi as i64 + min_threat,
                    "seed {seed} wave {wave} spent {} too far from [{}, {}]",
                    spent,
                    plan.budget_lo,
                    plan.budget_hi
                );
            }
        }
    }

    #[test]
    fn a_roster_with_fine_enough_granularity_lands_inside_the_window() {
        // From wave 5 onward the roster is varied enough (8/16/30/32
        // threat, gcd small relative to window width) that the closest
        // achievable point should actually fall inside the window itself,
        // not just near it.
        for seed in 0..50u64 {
            let mut rng = EngineRng::seed(seed + 1000);
            for wave in 5..=8u32 {
                let plan = generate_wave(wave, Difficulty::Standard, BalanceOverrides::default(), &mut rng);
                assert!(
                    plan.total_threat_spent >= plan.budget_lo && plan.total_threat_spent <= plan.budget_hi,
                    "seed {seed} wave {wave} spent {} outside [{}, {}]",
                    plan.total_threat_spent,
                    plan.budget_lo,
                    plan.budget_hi
                );
            }
        }
    }

    #[test]
    fn wave_eight_always_includes_the_guaranteed_husher_escort() {
        let mut rng = EngineRng::seed(1);
        let plan = generate_wave(8, Difficulty::Standard, BalanceOverrides::default(), &mut rng);
        let husher_count = plan
            .spawns
            .iter()
            .filter(|s| s.kind == EnemyKind::Husher)
            .count();
        assert!(husher_count >= NIGHT_MAW_ESCORT_HUSHER_COUNT as usize);
        assert_eq!(plan.boss, Some(BossKind::NightMaw));
    }

    /// The swarm-bias reconstruction weighting (see [`ENEMY_SWARM_WEIGHT_MITE`]'s
    /// own doc) must actually change composition, not just be present in
    /// source: a late wave's own roster includes cheap AND pricier kinds
    /// (Mite/Skitter vs Splitter/Shellback/Husher/Mirror), and the SAME
    /// total threat window as before this pass must now be reached with
    /// meaningfully more, individually weaker, enemies -- Mite the clear
    /// plurality kind, not an even split across the whole roster.
    #[test]
    fn late_waves_spend_their_budget_on_many_more_weaker_enemies_than_an_unbiased_reconstruction_would() {
        let mut total_count = 0usize;
        let mut mite_count = 0usize;
        for seed in 0..100u64 {
            let mut rng = EngineRng::seed(seed + 2000);
            let plan = generate_wave(8, Difficulty::Standard, BalanceOverrides::default(), &mut rng);
            total_count += plan.spawns.len();
            mite_count += plan.spawns.iter().filter(|s| s.kind == EnemyKind::Mite).count();
        }
        let avg_count = total_count as f64 / 100.0;
        // Before this pass's swarm-bias weighting, wave 8's own average
        // spawn count (uniform-among-choices reconstruction, same total
        // threat window) measured 15.5 over 30 seeds -- comfortably below
        // this floor; see this test's own PR doc for the exact before/after
        // table.
        assert!(avg_count >= 20.0, "wave 8 average spawn count {avg_count:.1} is not meaningfully denser");
        let mite_share = mite_count as f64 / total_count as f64;
        assert!(mite_share >= 0.5, "Mite must be the clear plurality kind under the swarm bias, got {mite_share:.2}");
    }

    /// Spawns within one pack land [`MIN_SPAWN_INTERVAL_TICKS`] apart (tight
    /// -- "arriving together"); the gap between the LAST member of one pack
    /// and the FIRST member of the next must be strictly larger, so a real
    /// pack boundary exists rather than every spawn being uniformly spaced.
    #[test]
    fn spawns_land_in_tight_packs_separated_by_a_real_gap() {
        let mut rng = EngineRng::seed(42);
        let plan = generate_wave(7, Difficulty::Standard, BalanceOverrides::default(), &mut rng);
        assert!(
            plan.spawns.len() > WAVE_SPAWN_CLUSTER_SIZE,
            "need more than one full pack to exercise a pack boundary"
        );
        for (i, spawn) in plan.spawns.iter().enumerate() {
            if i % WAVE_SPAWN_CLUSTER_SIZE == 0 {
                continue;
            }
            let prev = plan.spawns[i - 1].tick_offset;
            assert_eq!(
                spawn.tick_offset - prev,
                MIN_SPAWN_INTERVAL_TICKS,
                "spawn {i} is not tightly packed with its predecessor"
            );
        }
        let pack_boundary_gap = plan.spawns[WAVE_SPAWN_CLUSTER_SIZE].tick_offset - plan.spawns[WAVE_SPAWN_CLUSTER_SIZE - 1].tick_offset;
        assert!(
            pack_boundary_gap > MIN_SPAWN_INTERVAL_TICKS,
            "the first pack boundary must be a real gap, got {pack_boundary_gap} ticks"
        );
    }

    #[test]
    fn late_minion_hp_override_is_neutral_before_wave_five_and_applies_from_wave_five_on() {
        let overrides = BalanceOverrides { late_minion_hp_permille: 2000, ..BalanceOverrides::default() };
        for wave in 1..=4u32 {
            assert_eq!(
                effective_minion_hp_scalar_permille(wave, Difficulty::Standard, overrides),
                effective_hp_scalar_permille(wave, Difficulty::Standard),
                "wave {wave} must ignore late_minion_hp_permille"
            );
        }
        for wave in 5..=8u32 {
            let expected = effective_hp_scalar_permille(wave, Difficulty::Standard) * 2;
            assert_eq!(
                effective_minion_hp_scalar_permille(wave, Difficulty::Standard, overrides),
                expected,
                "wave {wave} must apply the x2.0 late_minion_hp_permille override"
            );
        }
    }

    #[test]
    fn integrity_override_scales_the_difficultys_own_starting_value() {
        let neutral = BalanceOverrides::default();
        assert_eq!(effective_start_integrity(Difficulty::Standard, neutral), STANDARD_INTEGRITY);
        let halved = BalanceOverrides { integrity_permille: 500, ..BalanceOverrides::default() };
        assert_eq!(effective_start_integrity(Difficulty::Standard, halved), STANDARD_INTEGRITY / 2);
    }

    #[test]
    fn balance_overrides_default_is_neutral_everywhere() {
        let neutral = BalanceOverrides::default();
        let mut rng_a = EngineRng::seed(7);
        let mut rng_b = EngineRng::seed(7);
        for wave in 1..=8u32 {
            let with_default = generate_wave(wave, Difficulty::Cozy, neutral, &mut rng_a);
            let explicit_neutral = generate_wave(
                wave,
                Difficulty::Cozy,
                BalanceOverrides {
                    bellkeeper_hp_permille: 1000,
                    night_maw_hp_permille: 1000,
                    reward_permille: 1000,
                    late_minion_hp_permille: 1000,
                    integrity_permille: 1000,
                },
                &mut rng_b,
            );
            assert_eq!(with_default.boss_max_hp, explicit_neutral.boss_max_hp, "wave {wave}");
        }
    }
}
