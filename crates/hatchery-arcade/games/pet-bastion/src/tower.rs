//! Towers: stats, upgrade tiers, targeting and damage-resolution math.
//!
//! This module is deliberately pure where it can be: stat lookup, armour
//! math, splash/chain target selection and jump falloff are all plain
//! functions over `&[Enemy]`/`&Board`. Per-tick orchestration (who attacks,
//! Link Burst triggering, rune application) lives in `sim.rs`, which is the
//! only place that needs simultaneous mutable access to towers, enemies, the
//! pet and the event log.

use crate::board::Board;
use crate::boss::Boss;
use crate::constants::*;
use crate::enemy::Enemy;
use crate::geometry::{tiles_to_fixed, FixedPos, Tile};
use crate::ids::EntityId;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TowerKind {
    Needle,
    Bell,
    Prism,
    EmberNest,
    Moonwell,
    Relay,
}

impl TowerKind {
    pub const ALL: [TowerKind; 6] = [
        TowerKind::Needle,
        TowerKind::Bell,
        TowerKind::Prism,
        TowerKind::EmberNest,
        TowerKind::Moonwell,
        TowerKind::Relay,
    ];

    pub fn attacks(self) -> bool {
        !matches!(self, TowerKind::Relay)
    }

    pub fn base_stats(self) -> TowerStats {
        match self {
            TowerKind::Needle => TowerStats {
                cost: 60,
                damage: 10,
                interval_ticks: 17, // 0.85s
                range_fp: tiles_to_fixed(3, 5),
                min_range_fp: None,
                pierce: 2,
                splash_radius_fp: None,
                chain_jumps: 0,
                slow_permille: None,
                family: Some(DamageFamily::Physical),
            },
            TowerKind::Bell => TowerStats {
                cost: 50,
                damage: 3,
                interval_ticks: 20, // 1.0s
                range_fp: tiles_to_fixed(2, 4),
                min_range_fp: None,
                pierce: 0,
                splash_radius_fp: None,
                chain_jumps: 0,
                slow_permille: Some(350),
                family: Some(DamageFamily::Impact),
            },
            TowerKind::Prism => TowerStats {
                cost: 85,
                damage: 18,
                interval_ticks: 24, // 1.2s
                // Range is not stated in the plan's table; chosen as a
                // mid-range value, documented in the implementation report.
                range_fp: tiles_to_fixed(3, 0),
                min_range_fp: None,
                pierce: 0,
                splash_radius_fp: None,
                chain_jumps: 2,
                slow_permille: None,
                family: Some(DamageFamily::Arcane),
            },
            TowerKind::EmberNest => TowerStats {
                cost: 70,
                damage: 8,
                interval_ticks: 13, // 0.65s
                // Range is not stated in the plan's table; chosen as a
                // short splash-tower value, documented in the report.
                range_fp: tiles_to_fixed(2, 0),
                min_range_fp: None,
                pierce: 0,
                splash_radius_fp: Some(tiles_to_fixed(2, 0)),
                chain_jumps: 0,
                slow_permille: None,
                family: Some(DamageFamily::Fire),
            },
            TowerKind::Moonwell => TowerStats {
                cost: 100,
                damage: 32,
                interval_ticks: 48, // 2.4s
                range_fp: tiles_to_fixed(6, 0),
                min_range_fp: Some(tiles_to_fixed(2, 0)),
                pierce: 0,
                splash_radius_fp: Some(tiles_to_fixed(1, 5)),
                chain_jumps: 0,
                slow_permille: None,
                family: Some(DamageFamily::Nature),
            },
            TowerKind::Relay => TowerStats {
                cost: 65,
                damage: 0,
                interval_ticks: 0,
                range_fp: 0,
                min_range_fp: None,
                pierce: 0,
                splash_radius_fp: None,
                chain_jumps: 0,
                slow_permille: None,
                family: None,
            },
        }
    }
}

/// A damage family, used by Mirror's resistance and Symbiosis' "different
/// types" adjacency check. Relay has none -- it never deals damage.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum DamageFamily {
    Physical,
    Impact,
    Arcane,
    Fire,
    Nature,
}

impl DamageFamily {
    /// Every family, in a fixed order -- the Attune pet charge
    /// (`pet.rs`/`sim.rs`'s own `resolved_family`) rotates a linked tower's
    /// outgoing family through this list by hit count, so Mirror's
    /// resistance (which only ever protects against the single family that
    /// hit it last) almost never lines up twice in a row.
    pub const ALL: [DamageFamily; 5] = [
        DamageFamily::Physical,
        DamageFamily::Impact,
        DamageFamily::Arcane,
        DamageFamily::Fire,
        DamageFamily::Nature,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum UpgradeBranch {
    Power,
    Utility,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum UpgradeLevel {
    Base,
    L2,
    L3(UpgradeBranch),
}

impl UpgradeLevel {
    /// Sap cost to move from the PREVIOUS level to this one (0 for `Base`,
    /// which is paid as the placement cost instead).
    pub fn step_cost(self, base_cost: i32) -> i32 {
        match self {
            UpgradeLevel::Base => 0,
            UpgradeLevel::L2 => permille_round(base_cost as i64, UPGRADE_L2_COST_PERMILLE) as i32,
            UpgradeLevel::L3(_) => permille_round(base_cost as i64, UPGRADE_L3_COST_PERMILLE) as i32,
        }
    }
}

fn permille_round(value: i64, permille: i64) -> i64 {
    (value * permille + 500) / 1000
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TowerStats {
    pub cost: i32,
    pub damage: i32,
    pub interval_ticks: u32,
    pub range_fp: i64,
    pub min_range_fp: Option<i64>,
    pub pierce: i32,
    pub splash_radius_fp: Option<i64>,
    /// Number of chain jumps beyond the primary target (Prism only).
    pub chain_jumps: u32,
    pub slow_permille: Option<i64>,
    pub family: Option<DamageFamily>,
}

/// Computes a tower's effective stats at a given upgrade level.
pub fn effective_stats(kind: TowerKind, level: UpgradeLevel) -> TowerStats {
    let mut stats = kind.base_stats();
    let damage_mult = match level {
        UpgradeLevel::Base => 1000,
        UpgradeLevel::L2 => UPGRADE_L2_DAMAGE_PERMILLE,
        UpgradeLevel::L3(UpgradeBranch::Power) => UPGRADE_L3_POWER_DAMAGE_PERMILLE,
        UpgradeLevel::L3(UpgradeBranch::Utility) => UPGRADE_L3_UTILITY_DAMAGE_PERMILLE,
    };
    if kind.attacks() {
        stats.damage = permille_round(stats.damage as i64, damage_mult) as i32;
    }
    if let UpgradeLevel::L3(UpgradeBranch::Utility) = level {
        match kind {
            TowerKind::Needle => stats.pierce += 1,
            TowerKind::Prism => stats.chain_jumps += 1,
            TowerKind::Bell => stats.slow_permille = stats.slow_permille.map(|s| s + 100),
            TowerKind::EmberNest | TowerKind::Moonwell => {
                stats.splash_radius_fp = stats.splash_radius_fp.map(|r| r + tiles_to_fixed(0, 5));
            }
            TowerKind::Relay => {}
        }
    }
    stats
}

/// Sap cost to move `current` to its next upgrade step, or `None` if
/// `current` is already at the maximum (`L3`). `L3`'s own [`UpgradeLevel::
/// step_cost`] is identical for either [`UpgradeBranch`] (the branch only
/// changes the resulting stats, never the cost), so this needs no branch
/// choice from the caller -- exactly the Sap [`crate::sim::Simulation`]'s
/// own `upgrade_tower` will charge once a branch is actually picked.
pub fn next_upgrade_cost(current: UpgradeLevel, base_cost: i32) -> Option<i32> {
    match current {
        UpgradeLevel::Base => Some(UpgradeLevel::L2.step_cost(base_cost)),
        UpgradeLevel::L2 => Some(UpgradeLevel::L3(UpgradeBranch::Power).step_cost(base_cost)),
        UpgradeLevel::L3(_) => None,
    }
}

/// Sap refund for selling a tower with `sap_invested` total Sap sunk into
/// it (placement + every upgrade step paid so far) -- the exact formula
/// [`crate::sim::Simulation`]'s own `sell_tower` applies.
pub fn sell_price(sap_invested: i32) -> i32 {
    ((sap_invested as i64 * SELL_REFUND_PERMILLE) / 1000) as i32
}

/// A placed tower's runtime state.
#[derive(Clone, Debug)]
pub struct Tower {
    pub id: EntityId,
    pub position: Tile,
    pub kind: TowerKind,
    pub level: UpgradeLevel,
    pub cooldown_ticks: u32,
    pub last_link_burst_tick: Option<u64>,
    /// Total attacks landed, used by the Echo rune (every Nth attack is
    /// repeated).
    pub attack_count: u32,
    /// Total hits landed, used by the Phase rune (every Nth hit ignores
    /// armour).
    pub hit_count: u32,
    /// Total Sap invested (placement + upgrades), for the sell refund.
    pub sap_invested: i32,
    /// Husky suppression: extra ticks added to the base interval this tick,
    /// recomputed every tick from nearby Hushers before the tower fires.
    pub suppressed: bool,
}

impl Tower {
    pub fn new(id: EntityId, position: Tile, kind: TowerKind) -> Self {
        let base = kind.base_stats();
        Tower {
            id,
            position,
            kind,
            level: UpgradeLevel::Base,
            cooldown_ticks: 0,
            last_link_burst_tick: None,
            attack_count: 0,
            hit_count: 0,
            sap_invested: base.cost,
            suppressed: false,
        }
    }

    pub fn stats(&self) -> TowerStats {
        effective_stats(self.kind, self.level)
    }

    pub fn position_fp(&self) -> FixedPos {
        self.position.to_fixed()
    }

    pub fn can_link_burst(&self, current_tick: u64) -> bool {
        match self.last_link_burst_tick {
            None => true,
            Some(last) => current_tick.saturating_sub(last) >= LINK_BURST_COOLDOWN_TICKS,
        }
    }
}

/// Applies the armour-after-pierce rule, floored at
/// [`ARMOUR_DAMAGE_FLOOR_PERMILLE`] of the original damage.
///
/// `ignore_armour` models the Phase rune (every Nth hit bypasses armour
/// entirely).
pub fn apply_armour(original_damage: i32, armour: i32, pierce: i32, ignore_armour: bool) -> i32 {
    if original_damage <= 0 {
        return 0;
    }
    if ignore_armour {
        return original_damage;
    }
    let effective_armour = (armour - pierce).max(0);
    let raw = original_damage - effective_armour;
    let floor = ((original_damage as i64 * ARMOUR_DAMAGE_FLOOR_PERMILLE + 999) / 1000) as i32;
    raw.max(floor).max(0)
}

/// Prism's per-jump damage, extending the plan's own two-jump falloff table
/// geometrically for any Link Burst jump beyond it.
pub fn prism_jump_damage(base_damage: i32, jump_index: usize) -> i32 {
    if jump_index == 0 {
        return base_damage;
    }
    let table_index = jump_index - 1;
    let permille = if table_index < PRISM_JUMP_FALLOFF_PERMILLE.len() {
        PRISM_JUMP_FALLOFF_PERMILLE[table_index]
    } else {
        let mut value = *PRISM_JUMP_FALLOFF_PERMILLE.last().unwrap_or(&1000);
        let extra = table_index - (PRISM_JUMP_FALLOFF_PERMILLE.len() - 1);
        for _ in 0..extra {
            value = value * PRISM_JUMP_EXTRA_RATIO_PERMILLE / 1000;
        }
        value
    };
    permille_round(base_damage as i64, permille) as i32
}

pub fn ember_splash_damage(base_damage: i32, is_primary: bool) -> i32 {
    if is_primary {
        base_damage
    } else {
        permille_round(base_damage as i64, EMBER_SPLASH_FALLOFF_PERMILLE) as i32
    }
}

/// Selects the primary target: nearest-eligible-by-range enemy with the
/// greatest route progress (closest to leaking), ties broken by the lowest
/// stable entity id.
pub fn select_primary_target(
    origin: FixedPos,
    range_fp: i64,
    min_range_fp: Option<i64>,
    enemies: &[Enemy],
    board: &Board,
) -> Option<usize> {
    let range2 = range_fp * range_fp;
    let min2 = min_range_fp.map(|m| m * m);
    let mut best: Option<(usize, i64, EntityId)> = None;
    for (i, enemy) in enemies.iter().enumerate() {
        if !enemy.is_alive() {
            continue;
        }
        let pos = enemy.position(board);
        let d2 = origin.dist2(pos);
        if d2 > range2 {
            continue;
        }
        if let Some(min2) = min2 {
            if d2 < min2 {
                continue;
            }
        }
        let progress = enemy.progress_fp(board);
        let better = match best {
            None => true,
            Some((_, best_progress, best_id)) => {
                progress > best_progress || (progress == best_progress && enemy.id < best_id)
            }
        };
        if better {
            best = Some((i, progress, enemy.id));
        }
    }
    best.map(|(i, _, _)| i)
}

/// A resolved target: either a regular enemy (by index into the caller's
/// slice) or a boss body (by index into `Boss::bodies`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetRef {
    Enemy(usize),
    Boss(usize),
}

/// Same rule as [`select_primary_target`], but considers boss bodies too --
/// a boss competes for "furthest along its route" exactly like any other
/// enemy, ties broken by the lowest stable entity id.
pub fn select_primary_target_unified(
    origin: FixedPos,
    range_fp: i64,
    min_range_fp: Option<i64>,
    enemies: &[Enemy],
    boss: Option<&Boss>,
    board: &Board,
) -> Option<TargetRef> {
    let range2 = range_fp * range_fp;
    let min2 = min_range_fp.map(|m| m * m);
    let mut best: Option<(i64, EntityId, TargetRef)> = None;
    let consider = |progress: i64, id: EntityId, target: TargetRef, best: &mut Option<(i64, EntityId, TargetRef)>| {
        let better = match best {
            None => true,
            Some((bp, bid, _)) => progress > *bp || (progress == *bp && id < *bid),
        };
        if better {
            *best = Some((progress, id, target));
        }
    };
    for (i, enemy) in enemies.iter().enumerate() {
        if !enemy.is_alive() {
            continue;
        }
        let d2 = origin.dist2(enemy.position(board));
        if d2 > range2 {
            continue;
        }
        if let Some(min2) = min2 {
            if d2 < min2 {
                continue;
            }
        }
        consider(enemy.progress_fp(board), enemy.id, TargetRef::Enemy(i), &mut best);
    }
    if let Some(boss) = boss {
        for (i, body) in boss.bodies.iter().enumerate() {
            let route = board.route(body.route);
            let pos = route.position_at(body.segment_index, body.offset_fp);
            let d2 = origin.dist2(pos);
            if d2 > range2 {
                continue;
            }
            if let Some(min2) = min2 {
                if d2 < min2 {
                    continue;
                }
            }
            let progress = route.progress_fp(body.segment_index, body.offset_fp);
            consider(progress, body.id, TargetRef::Boss(i), &mut best);
        }
    }
    best.map(|(_, _, t)| t)
}

/// Shared nearest-unvisited chaining walk behind both [`select_chain_targets`]
/// (chain starting from an already-chosen enemy primary) and
/// [`select_chain_targets_from`] (chain starting from an arbitrary origin,
/// e.g. a boss body position, that is not itself one of `enemies`). Appends
/// indices to `chosen` (already-present ones are ineligible) until
/// `chosen.len() == max_total` or no eligible enemy remains in range of the
/// last-chosen position.
fn extend_chain(
    chosen: &mut Vec<usize>,
    mut last_pos: FixedPos,
    max_total: usize,
    range_fp: i64,
    enemies: &[Enemy],
    board: &Board,
) {
    let range2 = range_fp * range_fp;
    while chosen.len() < max_total {
        let mut best: Option<(usize, i64, EntityId)> = None;
        for (i, enemy) in enemies.iter().enumerate() {
            if chosen.contains(&i) || !enemy.is_alive() {
                continue;
            }
            let d2 = last_pos.dist2(enemy.position(board));
            if d2 > range2 {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, best_d2, best_id)) => d2 < best_d2 || (d2 == best_d2 && enemy.id < best_id),
            };
            if better {
                best = Some((i, d2, enemy.id));
            }
        }
        match best {
            Some((i, _, _)) => {
                last_pos = enemies[i].position(board);
                chosen.push(i);
            }
            None => break,
        }
    }
}

/// Selects chain-jump targets for Prism: from the last-hit position, the
/// nearest not-yet-hit alive enemy within `range_fp`, ties broken by lowest
/// entity id. Returns indices including the primary, capped at
/// `max_total`.
pub fn select_chain_targets(
    primary_idx: usize,
    max_total: usize,
    range_fp: i64,
    enemies: &[Enemy],
    board: &Board,
) -> Vec<usize> {
    let mut chosen = vec![primary_idx];
    let last_pos = enemies[primary_idx].position(board);
    extend_chain(&mut chosen, last_pos, max_total, range_fp, enemies, board);
    chosen
}

/// Same chaining rule as [`select_chain_targets`], but starting from an
/// arbitrary `origin` that is not itself an entry in `enemies` -- used when
/// the chain's primary hit landed on a boss body instead of an enemy
/// (`fire_tower`'s and `apply_link_burst`'s `TargetRef::Boss` handling, in
/// `sim.rs`). Returns up to `max_extra` chained enemy indices; the caller is
/// responsible for accounting for the boss's own hit in whatever total-jump
/// budget it is enforcing (see [`crate::constants::MAX_CHAIN_TARGETS`]).
pub fn select_chain_targets_from(
    origin: FixedPos,
    max_extra: usize,
    range_fp: i64,
    enemies: &[Enemy],
    board: &Board,
) -> Vec<usize> {
    let mut chosen = Vec::new();
    extend_chain(&mut chosen, origin, max_extra, range_fp, enemies, board);
    chosen
}

/// Selects splash targets around `origin` within `radius_fp`, preferring the
/// furthest-progressed enemies first (ties by lowest id), capped at
/// `max_targets`.
pub fn select_splash_targets(
    origin: FixedPos,
    radius_fp: i64,
    enemies: &[Enemy],
    board: &Board,
    max_targets: usize,
) -> Vec<usize> {
    let radius2 = radius_fp * radius_fp;
    let mut candidates: Vec<(usize, i64, EntityId)> = enemies
        .iter()
        .enumerate()
        .filter(|(_, e)| e.is_alive())
        .filter_map(|(i, e)| {
            let d2 = origin.dist2(e.position(board));
            if d2 <= radius2 {
                Some((i, e.progress_fp(board), e.id))
            } else {
                None
            }
        })
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    candidates.truncate(max_targets);
    candidates.into_iter().map(|(i, _, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armour_never_reduces_damage_below_the_floor() {
        // 3 damage vs 3 armour, no pierce: raw would be 0, floored to
        // ceil(3 * 0.20) = 1.
        assert_eq!(apply_armour(3, 3, 0, false), 1);
    }

    #[test]
    fn pierce_reduces_effective_armour_before_the_floor_applies() {
        // 10 damage, 3 armour, 2 pierce -> effective armour 1 -> raw 9.
        assert_eq!(apply_armour(10, 3, 2, false), 9);
    }

    #[test]
    fn phase_rune_ignores_armour_entirely() {
        assert_eq!(apply_armour(10, 999, 0, true), 10);
    }

    #[test]
    fn next_upgrade_cost_walks_base_to_l2_to_l3_then_stops() {
        let base_cost = TowerKind::Needle.base_stats().cost;
        assert_eq!(next_upgrade_cost(UpgradeLevel::Base, base_cost), Some(UpgradeLevel::L2.step_cost(base_cost)));
        assert_eq!(
            next_upgrade_cost(UpgradeLevel::L2, base_cost),
            Some(UpgradeLevel::L3(UpgradeBranch::Power).step_cost(base_cost))
        );
        // Cost is branch-independent -- L3(Utility) must charge the exact
        // same Sap as L3(Power) for the same step.
        assert_eq!(
            next_upgrade_cost(UpgradeLevel::L2, base_cost),
            Some(UpgradeLevel::L3(UpgradeBranch::Utility).step_cost(base_cost))
        );
        assert_eq!(next_upgrade_cost(UpgradeLevel::L3(UpgradeBranch::Power), base_cost), None);
        assert_eq!(next_upgrade_cost(UpgradeLevel::L3(UpgradeBranch::Utility), base_cost), None);
    }

    #[test]
    fn sell_price_is_half_of_sap_invested() {
        assert_eq!(sell_price(100), 50);
        assert_eq!(sell_price(0), 0);
    }

    #[test]
    fn prism_chain_falls_off_and_extends_geometrically() {
        let base = 18;
        assert_eq!(prism_jump_damage(base, 0), 18);
        assert_eq!(prism_jump_damage(base, 1), 13); // round(18*0.70)=12.6->13
        assert_eq!(prism_jump_damage(base, 2), 8); // round(18*0.45)=8.1->8
        // Extended (Link Burst) jump uses the geometric ratio.
        let extended = prism_jump_damage(base, 3);
        assert!(extended > 0 && extended <= prism_jump_damage(base, 2));
    }

    #[test]
    fn chain_target_selection_never_exceeds_the_global_cap() {
        let board = Board::new();
        let mut alloc = crate::ids::EntityIdAllocator::default();
        let mut enemies = Vec::new();
        for _ in 0..8 {
            enemies.push(Enemy::spawn(
                alloc.next(),
                crate::enemy::EnemyKind::Mite,
                crate::board::RouteId(0),
                1000,
            ));
        }
        let chosen = select_chain_targets(0, MAX_CHAIN_TARGETS, tiles_to_fixed(50, 0), &enemies, &board);
        assert!(chosen.len() <= MAX_CHAIN_TARGETS);
    }

    #[test]
    fn splash_target_selection_never_exceeds_the_global_cap() {
        let board = Board::new();
        let mut alloc = crate::ids::EntityIdAllocator::default();
        let mut enemies = Vec::new();
        for _ in 0..12 {
            enemies.push(Enemy::spawn(
                alloc.next(),
                crate::enemy::EnemyKind::Mite,
                crate::board::RouteId(0),
                1000,
            ));
        }
        let origin = enemies[0].position(&board);
        let chosen = select_splash_targets(origin, tiles_to_fixed(50, 0), &enemies, &board, MAX_SPLASH_TARGETS);
        assert!(chosen.len() <= MAX_SPLASH_TARGETS);
    }
}
