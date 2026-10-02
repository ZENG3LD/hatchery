//! Boss state machines: Bellkeeper (wave 4) and Night Maw (wave 8).
//!
//! Boss speed is not given a numeric value anywhere in the design plan; the
//! values below are chosen and documented here rather than scattered as
//! magic numbers: Bellkeeper moves at Shellback's speed (a tanky single
//! body), Night Maw is slightly slower (an ominous, unhurried final boss).

use crate::board::{AnchorId, Board, RouteId};
use crate::constants::*;
use crate::ids::EntityId;
use crate::status::SlowState;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BossKind {
    Bellkeeper,
    NightMaw,
}

impl BossKind {
    pub fn base_hp(self) -> i32 {
        match self {
            BossKind::Bellkeeper => BELLKEEPER_BASE_HP,
            BossKind::NightMaw => NIGHT_MAW_BASE_HP,
        }
    }

    /// Chosen movement speed (tiles/second, fixed-point) -- see module docs.
    pub fn speed_fp(self) -> i64 {
        match self {
            BossKind::Bellkeeper => 6_000,
            BossKind::NightMaw => 5_000,
        }
    }

    /// HP-percent thresholds (of max HP, permille) at which this boss
    /// kind's own phase state changes: Bellkeeper's three escort triggers
    /// ([`BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE`]), Night Maw's split and
    /// final-phase thresholds ([`NIGHT_MAW_SPLIT_THRESHOLD_PERMILLE`]/
    /// [`NIGHT_MAW_FINAL_PHASE_THRESHOLD_PERMILLE`]). Static per kind, so
    /// this is looked up here rather than copied onto every [`Boss`]
    /// instance -- see `Boss`'s own `escort_triggered`/`split_triggered`/
    /// `final_phase` fields for which of these have actually been crossed
    /// by a given run.
    pub fn phase_thresholds_permille(self) -> &'static [i64] {
        match self {
            BossKind::Bellkeeper => &BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE,
            BossKind::NightMaw => &[NIGHT_MAW_SPLIT_THRESHOLD_PERMILLE, NIGHT_MAW_FINAL_PHASE_THRESHOLD_PERMILLE],
        }
    }
}

/// One physical boss body walking a route. Night Maw splits into two of
/// these sharing one HP pool (`Boss::shared_hp`); Bellkeeper only ever has
/// one.
///
/// Each body owns its own [`SlowState`] independently of any sibling body --
/// a split Night Maw's two halves can carry different slow stacks, exactly
/// as the plan requires ("each tело замедляется independently" -- the state
/// lives on the body, not the shared boss). Stun and knockback stay off this
/// struct entirely: the plan forbids both for bosses, so there is no field
/// to carry them.
#[derive(Clone, Debug)]
pub struct BossBody {
    pub id: EntityId,
    pub route: RouteId,
    pub segment_index: usize,
    pub offset_fp: i64,
    pub slows: SlowState,
}

impl BossBody {
    /// Combined multiplicative slow currently active on this body, capped
    /// at [`MAX_COMBINED_SLOW_PERMILLE`] -- the exact same rule
    /// [`crate::enemy::Enemy::combined_slow_permille`] applies, via the
    /// same shared [`SlowState`].
    pub fn combined_slow_permille(&self) -> i64 {
        self.slows.combined_permille()
    }

    /// Applies one slow application to this body only -- a sibling body
    /// from a Night Maw split is untouched.
    pub fn apply_slow(&mut self, magnitude_permille: i64, duration_ticks: u32) {
        self.slows.apply(magnitude_permille, duration_ticks);
    }

    /// Advances this body's slow durations by one tick, dropping expired
    /// ones. Movement itself is driven by [`BossBody::advance_movement`].
    pub fn tick_status(&mut self) {
        self.slows.tick();
    }

    /// Moves this body forward along its route by one tick's worth of
    /// distance at `base_speed_fp` (tiles/second, fixed-point -- the boss
    /// kind's own [`BossKind::speed_fp`]), honouring this body's own
    /// combined slow AND `zone_speed_permille` (1000 = unchanged) -- the
    /// enemy-speed field modifier, the same SEPARATE multiplier
    /// [`crate::enemy::Enemy::advance_movement`] applies; see that
    /// method's own doc for why it sits outside the tower-slow ceiling.
    /// Returns `true` if it reached the Heartseed this tick (a lap) -- it
    /// is not removed or stopped for that, it loops back to its own route
    /// start and keeps going, the same as `Enemy::advance_movement`;
    /// `sim.rs`'s own `advance_enemy_movement` charges Integrity for the
    /// arrival instead of ending the run outright. Bosses are never
    /// stunned, so unlike `Enemy::advance_movement` there is no stun guard
    /// here.
    pub fn advance_movement(&mut self, board: &Board, base_speed_fp: i64, zone_speed_permille: i64) -> bool {
        let per_tick = base_speed_fp / TICKS_PER_SECOND;
        let zoned = per_tick * zone_speed_permille / 1000;
        let slow = self.combined_slow_permille();
        let effective = zoned * (1000 - slow) / 1000;
        self.walk(board, effective.max(0))
    }

    /// Walks this body forward by `distance_fp` along its own route,
    /// wrapping back to the route's own start (segment 0, offset 0) the
    /// instant it would step past the final segment -- see
    /// `crate::enemy::Enemy::walk`'s own doc for the full "why loop, why
    /// discard the overshoot" reasoning; this is the exact same rule
    /// applied to a boss body instead of an ordinary enemy.
    fn walk(&mut self, board: &Board, mut distance_fp: i64) -> bool {
        let route = board.route(self.route);
        loop {
            if distance_fp <= 0 {
                return false;
            }
            let Some(seg) = route.segments.get(self.segment_index) else {
                self.segment_index = 0;
                self.offset_fp = 0;
                return true;
            };
            let remaining_in_segment = seg.length_fp - self.offset_fp;
            if distance_fp < remaining_in_segment {
                self.offset_fp += distance_fp;
                return false;
            }
            distance_fp -= remaining_in_segment;
            self.segment_index += 1;
            self.offset_fp = 0;
            if self.segment_index >= route.segment_count() {
                self.segment_index = 0;
                return true;
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Boss {
    pub kind: BossKind,
    pub bodies: Vec<BossBody>,
    pub shared_hp: i32,
    pub shared_max_hp: i32,

    // Bellkeeper
    pub bell_cooldown_ticks: u32,
    pub silence_ticks_remaining: u32,
    pub escort_triggered: [bool; 3],

    // Night Maw
    pub split_triggered: bool,
    pub final_phase: bool,
    pub corrupt_cooldown_ticks: u32,
    pub corrupted_anchor: Option<(AnchorId, u32)>,
}

impl Boss {
    pub fn new(kind: BossKind, id: EntityId, route: RouteId, max_hp: i32) -> Self {
        Boss {
            kind,
            bodies: vec![BossBody {
                id,
                route,
                segment_index: 0,
                offset_fp: 0,
                slows: SlowState::default(),
            }],
            shared_hp: max_hp,
            shared_max_hp: max_hp,
            bell_cooldown_ticks: BELLKEEPER_BELL_INTERVAL_TICKS as u32,
            silence_ticks_remaining: 0,
            escort_triggered: [false; 3],
            split_triggered: false,
            final_phase: false,
            corrupt_cooldown_ticks: NIGHT_MAW_CORRUPT_INTERVAL_TICKS as u32,
            corrupted_anchor: None,
        }
    }

    pub fn is_defeated(&self) -> bool {
        self.shared_hp <= 0
    }

    pub fn hp_permille(&self) -> i64 {
        if self.shared_max_hp <= 0 {
            0
        } else {
            (self.shared_hp as i64 * 1000) / self.shared_max_hp as i64
        }
    }

    /// Applies shared damage, returning the amount actually removed (never
    /// more than remaining HP).
    pub fn apply_damage(&mut self, amount: i32) -> i32 {
        let applied = amount.min(self.shared_hp.max(0));
        self.shared_hp = (self.shared_hp - amount).max(0);
        applied
    }

    /// Checks the three Bellkeeper escort thresholds (75/50/25%), most
    /// severe first, marking each as triggered at most once. Returns the
    /// index of a newly crossed threshold, if any.
    pub fn newly_crossed_escort_threshold(&mut self) -> Option<usize> {
        let pm = self.hp_permille();
        for (i, &threshold) in BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE.iter().enumerate() {
            if !self.escort_triggered[i] && pm <= threshold {
                self.escort_triggered[i] = true;
                return Some(i);
            }
        }
        None
    }

    pub fn should_split(&self) -> bool {
        self.kind == BossKind::NightMaw
            && !self.split_triggered
            && self.hp_permille() <= NIGHT_MAW_SPLIT_THRESHOLD_PERMILLE
    }

    pub fn should_enter_final_phase(&self) -> bool {
        self.kind == BossKind::NightMaw
            && !self.final_phase
            && self.hp_permille() <= NIGHT_MAW_FINAL_PHASE_THRESHOLD_PERMILLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::EntityIdAllocator;

    #[test]
    fn phase_thresholds_permille_matches_the_constants_each_kind_actually_checks_against() {
        assert_eq!(BossKind::Bellkeeper.phase_thresholds_permille(), BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE);
        assert_eq!(
            BossKind::NightMaw.phase_thresholds_permille(),
            [NIGHT_MAW_SPLIT_THRESHOLD_PERMILLE, NIGHT_MAW_FINAL_PHASE_THRESHOLD_PERMILLE]
        );
    }

    #[test]
    fn escort_thresholds_fire_once_each_on_downward_crossing() {
        let mut alloc = EntityIdAllocator::default();
        let mut boss = Boss::new(BossKind::Bellkeeper, alloc.next(), RouteId(0), 1000);
        assert_eq!(boss.newly_crossed_escort_threshold(), None);
        boss.apply_damage(260); // 74% remaining, crosses 75%
        assert_eq!(boss.newly_crossed_escort_threshold(), Some(0));
        assert_eq!(boss.newly_crossed_escort_threshold(), None);
        boss.apply_damage(250); // 49% remaining, crosses 50%
        assert_eq!(boss.newly_crossed_escort_threshold(), Some(1));
    }

    #[test]
    fn night_maw_splits_at_half_hp_and_enters_final_phase_at_a_quarter() {
        let mut alloc = EntityIdAllocator::default();
        let mut boss = Boss::new(BossKind::NightMaw, alloc.next(), RouteId(0), 4200);
        assert!(!boss.should_split());
        boss.apply_damage(2101); // just under half remaining
        assert!(boss.should_split());
        boss.split_triggered = true;
        assert!(!boss.should_enter_final_phase());
        boss.apply_damage(1050); // drops to <=25%
        assert!(boss.should_enter_final_phase());
    }

    /// Ticks `body` forward (via [`BossBody::advance_movement`]) until its
    /// route progress reaches `target_progress_fp`, returning the tick
    /// count. Mirrors how `sim.rs`'s own `advance_enemy_movement` drives a
    /// boss body every tick (`tick_status` then `advance_movement`), just
    /// looped directly instead of through a full `Simulation`.
    fn ticks_to_reach(body: &mut BossBody, board: &Board, base_speed_fp: i64, target_progress_fp: i64) -> u64 {
        let mut ticks = 0u64;
        loop {
            let progress = board.route(body.route).progress_fp(body.segment_index, body.offset_fp);
            if progress >= target_progress_fp {
                return ticks;
            }
            body.tick_status();
            body.advance_movement(board, base_speed_fp, 1000);
            ticks += 1;
            assert!(ticks < 1_000_000, "boss body never reached the target progress -- movement is stuck");
        }
    }

    #[test]
    fn one_slow_source_makes_a_boss_body_cross_its_first_leg_measurably_slower_by_the_expected_ratio() {
        // The defect this guards: `advance_enemy_movement` used to compute
        // boss displacement purely from `boss.kind.speed_fp()`, with no
        // reference to any slow state at all -- a Bell tower's 35% slow
        // (`TowerKind::Bell`'s own `stats.slow_permille`, applied via the
        // exact same `apply_slow(slow, TICKS_PER_SECOND * 2)` call
        // `fire_tower`'s `TargetRef::Enemy` branch already used) had zero
        // effect on a boss body's route-crossing time. Route 0's own first
        // leg (`board::Route::build`'s `(0,3)-(20,3)` first waypoint pair)
        // is a real 20-tile stretch, not a synthetic distance.
        let board = Board::new();
        let mut alloc = EntityIdAllocator::default();
        let first_leg_fp = board.route(RouteId(0)).segments[0].length_fp;

        let mut control = Boss::new(BossKind::Bellkeeper, alloc.next(), RouteId(0), 1000);
        let base_speed_fp = control.kind.speed_fp();
        let unslowed_ticks = ticks_to_reach(&mut control.bodies[0], &board, base_speed_fp, first_leg_fp);

        let mut slowed = Boss::new(BossKind::Bellkeeper, alloc.next(), RouteId(0), 1000);
        // A duration comfortably longer than the whole crossing so this
        // measures one continuous 35% slow, not a decaying one.
        slowed.bodies[0].apply_slow(350, 10_000);
        let slowed_ticks = ticks_to_reach(&mut slowed.bodies[0], &board, base_speed_fp, first_leg_fp);

        assert!(
            slowed_ticks > unslowed_ticks,
            "a slowed boss body must take strictly more ticks to cross the same leg (unslowed={unslowed_ticks}, slowed={slowed_ticks})"
        );
        // Concrete ratio, not a bare inequality: at 35% slow each tick
        // covers only 65% of the unslowed distance, so the crossing must
        // take at least 1/0.65 as many ticks (floor, integer arithmetic).
        let expected_minimum_slowed_ticks = unslowed_ticks * 1000 / 650;
        assert!(
            slowed_ticks >= expected_minimum_slowed_ticks,
            "slowed_ticks={slowed_ticks} must be at least {expected_minimum_slowed_ticks} (unslowed={unslowed_ticks} inflated by 1/(1-0.35))"
        );
    }

    #[test]
    fn combined_slow_on_a_boss_body_follows_the_spec_formula_and_caps_at_60_percent() {
        // Same multiplicative rule `Enemy::combined_slow_permille` already
        // proves in `enemy.rs`'s own `slow_combination_hits_the_ceiling`,
        // now proven on `BossBody` via the same shared `SlowState`.
        let board = Board::new();
        let mut alloc = EntityIdAllocator::default();
        let mut boss = Boss::new(BossKind::Bellkeeper, alloc.next(), RouteId(0), 1000);

        boss.bodies[0].apply_slow(350, 100);
        assert_eq!(boss.bodies[0].combined_slow_permille(), 350);

        // Two independent 35% slows: 1 - 0.65^2 = 0.5775 -> 578 permille
        // (rounded via the same integer math `combined_permille` uses),
        // still under the 60% ceiling -- the formula applies exactly, it
        // is not simply clamped from the first source onward.
        boss.bodies[0].apply_slow(350, 100);
        assert_eq!(boss.bodies[0].combined_slow_permille(), 578);

        // A third 35% source pushes the raw product (1 - 0.65^3 = 0.725)
        // past the ceiling -- it must saturate at exactly the cap.
        boss.bodies[0].apply_slow(350, 100);
        assert_eq!(boss.bodies[0].combined_slow_permille(), MAX_COMBINED_SLOW_PERMILLE);

        // A fourth source must not push it any further -- the cap holds,
        // it does not keep climbing with more stacked sources.
        boss.bodies[0].apply_slow(350, 100);
        assert_eq!(boss.bodies[0].combined_slow_permille(), MAX_COMBINED_SLOW_PERMILLE);

        // The capped value is reflected in actual movement too: exactly 40%
        // of the unslowed per-tick distance, never less.
        let per_tick = boss.kind.speed_fp() / TICKS_PER_SECOND;
        boss.bodies[0].advance_movement(&board, boss.kind.speed_fp(), 1000);
        assert_eq!(boss.bodies[0].offset_fp, per_tick * (1000 - MAX_COMBINED_SLOW_PERMILLE) / 1000);
    }

    #[test]
    fn a_boss_body_can_never_be_slowed_to_a_full_stop() {
        let board = Board::new();
        let mut alloc = EntityIdAllocator::default();
        let mut boss = Boss::new(BossKind::Bellkeeper, alloc.next(), RouteId(0), 1000);

        // Twenty near-total (99.9%) slow sources stacked at once -- the
        // most hostile input the multiplicative formula can ever receive.
        for _ in 0..20 {
            boss.bodies[0].apply_slow(999, 1_000);
        }
        assert_eq!(
            boss.bodies[0].combined_slow_permille(),
            MAX_COMBINED_SLOW_PERMILLE,
            "even near-total slows must saturate at the 60% ceiling, not approach 100%"
        );

        let base_speed_fp = boss.kind.speed_fp();
        let per_tick = base_speed_fp / TICKS_PER_SECOND;
        boss.bodies[0].advance_movement(&board, base_speed_fp, 1000);
        assert!(boss.bodies[0].offset_fp > 0, "a boss body must always keep moving, never fully stop");
        assert_eq!(boss.bodies[0].offset_fp, per_tick * (1000 - MAX_COMBINED_SLOW_PERMILLE) / 1000);
    }
}
