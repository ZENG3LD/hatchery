//! Drives a real headless `slow_stack`-policy run to a genuine mid-run
//! snapshot worth previewing: wave 4's Bellkeeper fight, with towers of
//! several kinds, escort minions, the boss, the pet and an active Living
//! Circuit link all on the board at once. No synthetic/hand-built state
//! anywhere in this module -- every returned [`FoundFrame`] traces back
//! to a real `Simulation::advance` call from `Simulation::new(seed, ...)`.

use std::collections::HashSet;

use hatchery_arcade_engine::sweep_api::Policy;
use hatchery_arcade_engine::{EngineRng, MiniGame, RunOutcome};
use hatchery_arcade_pet_bastion::boss::BossKind;
use hatchery_arcade_pet_bastion::event::SimEvent;
use hatchery_arcade_pet_bastion::pet::PetState;
use hatchery_arcade_pet_bastion::sim::{PetBastionParams, Simulation};
use hatchery_arcade_pet_bastion::snapshot::SimulationSnapshot;
use hatchery_arcade_pet_bastion::wave::Difficulty;
use hatchery_arcade_sweep::policy::SlowStackPolicy;

/// One candidate mid-run frame and how it scored against [`frame_score`].
pub struct FoundFrame {
    pub seed: u64,
    pub tick_index: u64,
    pub score: i64,
    pub snapshot: SimulationSnapshot,
}

/// How "interesting" one wave-4-Bellkeeper snapshot is to look at: more
/// distinct tower kinds, more live enemies (escort minions included), an
/// active Living Circuit link, and the pet settled at an anchor (rather
/// than mid-`Moving`, which this preview's own adapter can only
/// interpolate) all score higher. `None` for any snapshot that is not
/// actually a wave 4 Bellkeeper fight at all -- a hard gate, not a soft
/// preference, per the task's own exact ask.
fn frame_score(snapshot: &SimulationSnapshot) -> Option<i64> {
    let boss = snapshot.boss.as_ref()?;
    if boss.kind != BossKind::Bellkeeper || snapshot.wave != 4 {
        return None;
    }
    let distinct_kinds: HashSet<_> = snapshot.towers.iter().map(|tower| tower.kind).collect();
    let circuit_active = !snapshot.pet.linked_towers.is_empty();
    let pet_settled = matches!(snapshot.pet.state, PetState::AtAnchor(_));
    Some(
        distinct_kinds.len() as i64 * 100
            + snapshot.enemies.len() as i64 * 10
            + if circuit_active { 50 } else { 0 }
            + if pet_settled { 5 } else { 0 },
    )
}

/// Runs one seed to completion under [`SlowStackPolicy`] at `difficulty`,
/// tracking the single best-scoring wave-4-Bellkeeper snapshot seen along
/// the way (see [`frame_score`]). Returns `None` if the run never won, or
/// never produced a single qualifying frame -- an uninteresting or
/// losing seed is skipped, never padded with a fabricated frame.
fn run_seed(seed: u64, difficulty: Difficulty, max_ticks: u64) -> Option<FoundFrame> {
    let mut game = Simulation::new(seed, PetBastionParams::new(difficulty));
    let mut rng = EngineRng::seed(seed);
    let mut policy = SlowStackPolicy::default();
    let mut best: Option<FoundFrame> = None;
    let mut tick_index = 0u64;

    while tick_index < max_ticks && game.is_finished().is_none() {
        let snapshot = game.snapshot();
        if let Some(score) = frame_score(&snapshot) {
            let better = best.as_ref().map(|found| score > found.score).unwrap_or(true);
            if better {
                best = Some(FoundFrame { seed, tick_index, score, snapshot: snapshot.clone() });
            }
        }
        let commands = policy.decide(&snapshot, tick_index);
        let _events = game.advance(&mut rng, &commands);
        tick_index += 1;
    }

    if game.is_finished() != Some(RunOutcome::Won) {
        return None;
    }
    best
}

/// Runs every seed in `0..max_seeds_to_try` under Standard difficulty and
/// keeps the single highest-scoring frame across every winning run --
/// not merely the first winning seed found, so the preview shows the
/// most varied board (most distinct tower kinds, most live enemies) a
/// modest seed search actually turns up. `None` if no seed in that range
/// both won AND produced a qualifying wave-4-Bellkeeper frame.
pub fn find_preview_frame(max_seeds_to_try: u64, max_ticks: u64) -> Option<FoundFrame> {
    (0..max_seeds_to_try).filter_map(|seed| run_seed(seed, Difficulty::Standard, max_ticks)).max_by_key(|found| found.score)
}

/// A short, contiguous run of REAL simulation ticks: `count_ticks + 1`
/// snapshots (`snapshots[0]` at `start_tick`, `snapshots[k]` at
/// `start_tick + k`) and the `count_ticks` real `SimEvent` lists each
/// `Simulation::advance` call between them actually returned -- exactly
/// what `hatchery-arcade-pet-bastion-render`'s own `interp`/`effects`
/// modules need to demonstrate genuine cross-tick interpolation, not a
/// single frame.
pub struct FrameSequence {
    pub start_tick: u64,
    pub snapshots: Vec<SimulationSnapshot>,
    pub events: Vec<Vec<SimEvent>>,
}

/// Re-runs `seed` under the SAME policy/difficulty `run_seed` already used
/// to find a candidate frame (deterministic replay -- see `sim.rs`'s own
/// "one seeded PRNG owns all run randomness" rule, which is exactly what
/// makes re-deriving identical state from scratch a real, not merely
/// plausible, reproduction), sliding a `count_ticks`-wide window of real
/// ticks forward from `min_start_tick` and returning the FIRST window
/// (within `min_start_tick ..= min_start_tick + search_ticks`) that
/// contains at least one real `SimEvent::Shot` -- so the demo sequence
/// actually shows combat, not just idle motion, without this preview tool
/// hand-picking or fabricating a single frame's own event list. Falls back
/// to the window starting exactly at `min_start_tick` if no combat fires
/// anywhere in the searched range. `None` only if the run ends (win or
/// loss) before even that first window fills, or `min_start_tick` is never
/// reached at all within `max_ticks`.
pub fn capture_best_combat_window(seed: u64, difficulty: Difficulty, min_start_tick: u64, search_ticks: u64, count_ticks: usize, max_ticks: u64) -> Option<FrameSequence> {
    let mut game = Simulation::new(seed, PetBastionParams::new(difficulty));
    let mut rng = EngineRng::seed(seed);
    let mut policy = SlowStackPolicy::default();
    let mut tick_index = 0u64;
    let mut snap_buf: Vec<SimulationSnapshot> = Vec::new();
    let mut event_buf: Vec<Vec<SimEvent>> = Vec::new();
    let mut fallback: Option<FrameSequence> = None;

    while tick_index < max_ticks && game.is_finished().is_none() {
        let snapshot_before = game.snapshot();
        if snap_buf.is_empty() {
            snap_buf.push(snapshot_before.clone());
        }
        let commands = policy.decide(&snapshot_before, tick_index);
        let tick_events = game.advance(&mut rng, &commands);
        event_buf.push(tick_events);
        snap_buf.push(game.snapshot());
        if event_buf.len() > count_ticks {
            event_buf.remove(0);
            snap_buf.remove(0);
        }
        tick_index += 1;

        if event_buf.len() != count_ticks {
            continue;
        }
        let window_start = tick_index - count_ticks as u64;
        if window_start < min_start_tick {
            continue;
        }
        if fallback.is_none() {
            fallback = Some(FrameSequence { start_tick: window_start, snapshots: snap_buf.clone(), events: event_buf.clone() });
        }
        let has_shot = event_buf.iter().flatten().any(|e| matches!(e, SimEvent::Shot { .. }));
        if has_shot {
            return Some(FrameSequence { start_tick: window_start, snapshots: snap_buf.clone(), events: event_buf.clone() });
        }
        if window_start >= min_start_tick + search_ticks {
            break;
        }
    }
    fallback
}
