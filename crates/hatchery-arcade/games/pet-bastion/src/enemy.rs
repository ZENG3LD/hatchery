//! Enemies: fixed stats, route-relative movement (no pathfinding), and the
//! status-effect rules that combine across towers (slow, stun, family
//! resist).

use crate::board::{Board, RouteId};
use crate::constants::*;
use crate::geometry::FixedPos;
use crate::ids::EntityId;
use crate::status::SlowState;
use crate::tower::DamageFamily;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum EnemyKind {
    Mite,
    Skitter,
    Shellback,
    Splitter,
    Husher,
    Mirror,
}

impl EnemyKind {
    pub const ALL: [EnemyKind; 6] = [
        EnemyKind::Mite,
        EnemyKind::Skitter,
        EnemyKind::Shellback,
        EnemyKind::Splitter,
        EnemyKind::Husher,
        EnemyKind::Mirror,
    ];

    pub fn base_stats(self) -> EnemyBaseStats {
        match self {
            EnemyKind::Mite => EnemyBaseStats {
                hp: 18,
                speed_fp: 14_000,
                armour: 0,
                threat: 8,
            },
            EnemyKind::Skitter => EnemyBaseStats {
                hp: 24,
                speed_fp: 18_000,
                armour: 0,
                threat: 16,
            },
            EnemyKind::Shellback => EnemyBaseStats {
                hp: 95,
                speed_fp: 6_000,
                armour: 3,
                threat: 30,
            },
            EnemyKind::Splitter => EnemyBaseStats {
                hp: 38,
                speed_fp: 10_000,
                armour: 1,
                threat: 32,
            },
            EnemyKind::Husher => EnemyBaseStats {
                hp: 70,
                speed_fp: 8_000,
                armour: 1,
                threat: 36,
            },
            EnemyKind::Mirror => EnemyBaseStats {
                hp: 60,
                speed_fp: 11_000,
                armour: 1,
                threat: 34,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EnemyBaseStats {
    pub hp: i32,
    /// Tiles/second, scaled by `FIXED_SCALE`.
    pub speed_fp: i64,
    pub armour: i32,
    pub threat: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ResistEffect {
    pub family: DamageFamily,
    pub ticks_remaining: u32,
}

/// A live enemy. Stores its route id, segment index and fixed-point offset
/// -- no pathfinding, movement is a pure integer walk along precomputed
/// segments.
#[derive(Clone, Debug)]
pub struct Enemy {
    pub id: EntityId,
    pub kind: EnemyKind,
    pub route: RouteId,
    pub segment_index: usize,
    pub offset_fp: i64,
    pub hp: i32,
    pub max_hp: i32,
    pub armour: i32,
    pub speed_fp: i64,
    pub slows: SlowState,
    pub stun_ticks: u32,
    pub resist: Option<ResistEffect>,
    pub last_hit_family: Option<DamageFamily>,
}

impl Enemy {
    pub fn spawn(id: EntityId, kind: EnemyKind, route: RouteId, hp_scalar_permille: i64) -> Self {
        let base = kind.base_stats();
        let hp = ((base.hp as i64 * hp_scalar_permille + 500) / 1000) as i32;
        Enemy {
            id,
            kind,
            route,
            segment_index: 0,
            offset_fp: 0,
            hp,
            max_hp: hp,
            armour: base.armour,
            speed_fp: base.speed_fp,
            slows: SlowState::default(),
            stun_ticks: 0,
            resist: None,
            last_hit_family: None,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }

    pub fn position(&self, board: &Board) -> FixedPos {
        board
            .route(self.route)
            .position_at(self.segment_index, self.offset_fp)
    }

    pub fn progress_fp(&self, board: &Board) -> i64 {
        board
            .route(self.route)
            .progress_fp(self.segment_index, self.offset_fp)
    }

    /// Combined multiplicative slow, capped at [`MAX_COMBINED_SLOW_PERMILLE`].
    pub fn combined_slow_permille(&self) -> i64 {
        self.slows.combined_permille()
    }

    pub fn apply_slow(&mut self, magnitude_permille: i64, duration_ticks: u32) {
        self.slows.apply(magnitude_permille, duration_ticks);
    }

    pub fn apply_stun(&mut self, ticks: u32) {
        self.stun_ticks = self.stun_ticks.max(ticks);
    }

    /// Records which damage family just hit this enemy and grants it a
    /// temporary resistance to that same family (Mirror's own mechanic;
    /// harmless no-op bookkeeping on any other kind since nothing reads it).
    pub fn note_hit(&mut self, family: DamageFamily) {
        self.last_hit_family = Some(family);
        if self.kind == EnemyKind::Mirror {
            self.resist = Some(ResistEffect {
                family,
                ticks_remaining: MIRROR_RESIST_TICKS as u32,
            });
        }
    }

    pub fn resist_permille_against(&self, family: DamageFamily) -> i64 {
        match self.resist {
            Some(r) if r.family == family && r.ticks_remaining > 0 => MIRROR_RESIST_PERMILLE,
            _ => 0,
        }
    }

    /// Advances status-effect durations by one tick. Movement itself is
    /// driven by the caller (it needs the board to walk segments).
    pub fn tick_status(&mut self) {
        self.slows.tick();
        self.stun_ticks = self.stun_ticks.saturating_sub(1);
        if let Some(r) = &mut self.resist {
            r.ticks_remaining = r.ticks_remaining.saturating_sub(1);
            if r.ticks_remaining == 0 {
                self.resist = None;
            }
        }
    }

    /// Moves the enemy forward along its route by one tick's worth of
    /// distance, honouring the combined slow AND `zone_speed_permille`
    /// (1000 = unchanged) -- the enemy-speed field modifier
    /// (`zone::WaveZones::enemy_speed_permille`), a SEPARATE multiplier
    /// applied outside the tower-slow stacking ceiling (`status.rs`'s own
    /// `MAX_COMBINED_SLOW_PERMILLE`), computed by the caller from this
    /// enemy's own current position -- see `constants.rs`'s own
    /// `ZONE_ENEMY_SPEED_PERMILLE` doc for why. Returns `true` if the enemy
    /// reached the Heartseed this tick -- it is not removed for that (see
    /// `walk`'s own doc): the SAME unit loops back to its own route start
    /// and keeps going, `sim.rs`'s own `advance_enemy_movement` charges
    /// Integrity for the arrival.
    pub fn advance_movement(&mut self, board: &Board, zone_speed_permille: i64) -> bool {
        if self.stun_ticks > 0 {
            return false;
        }
        let slow = self.combined_slow_permille();
        let per_tick = self.speed_fp / TICKS_PER_SECOND;
        let zoned = per_tick * zone_speed_permille / 1000;
        let effective = zoned * (1000 - slow) / 1000;
        self.walk(board, effective.max(0))
    }

    /// Moves the enemy backward along its route by a fixed-point distance
    /// (Pet Pulse knockback). Never moves it before the start of the route.
    pub fn knockback(&mut self, board: &Board, distance_fp: i64) {
        let route = board.route(self.route);
        let current = route.progress_fp(self.segment_index, self.offset_fp);
        let target = (current - distance_fp).max(0);
        self.seek(board, target);
    }

    /// Walks the enemy forward by `distance_fp` along its own route,
    /// wrapping back to the route's own start (segment 0, offset 0) the
    /// instant it would step past the final segment -- the owner's own
    /// rule ("по прохождению волны они просто заново переносились в
    /// начало"): a unit that reaches the Heartseed is not removed, it
    /// loops. Any leftover `distance_fp` this tick's overshoot carried past
    /// the exact end-of-route point is discarded rather than carried into
    /// the new lap, exactly the way the old "reached the end, stop" rule
    /// already discarded a leak's own overshoot -- per-tick movement is a
    /// small fraction of a whole lap for every enemy kind this crate
    /// defines, so this can only ever discard a fraction of one tick's
    /// step, never a meaningful slice of the new lap.
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

    /// Repositions the enemy to an absolute route progress value.
    fn seek(&mut self, board: &Board, target_progress_fp: i64) {
        let (idx, offset) = board.route(self.route).locate(target_progress_fp);
        self.segment_index = idx;
        self.offset_fp = offset;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::EntityIdAllocator;

    #[test]
    fn slow_combination_hits_the_ceiling() {
        let board = Board::new();
        let mut alloc = EntityIdAllocator::default();
        let mut enemy = Enemy::spawn(alloc.next(), EnemyKind::Mite, RouteId(0), 1000);
        // Three 35% slows multiplicatively combine to 1-0.65^3 = 0.725,
        // which must be capped at 0.60.
        enemy.apply_slow(350, 100);
        enemy.apply_slow(350, 100);
        enemy.apply_slow(350, 100);
        assert_eq!(enemy.combined_slow_permille(), MAX_COMBINED_SLOW_PERMILLE);

        let base_per_tick = enemy.speed_fp / TICKS_PER_SECOND;
        enemy.advance_movement(&board, 1000);
        let moved = enemy.offset_fp;
        let expected = base_per_tick * (1000 - MAX_COMBINED_SLOW_PERMILLE) / 1000;
        assert_eq!(moved, expected);
    }

    #[test]
    fn a_stunned_enemy_does_not_move() {
        let board = Board::new();
        let mut alloc = EntityIdAllocator::default();
        let mut enemy = Enemy::spawn(alloc.next(), EnemyKind::Mite, RouteId(0), 1000);
        enemy.apply_stun(5);
        enemy.advance_movement(&board, 1000);
        assert_eq!(enemy.offset_fp, 0);
        assert_eq!(enemy.segment_index, 0);
    }

    #[test]
    fn walking_past_the_last_segment_loops_back_to_the_route_start() {
        // The owner's own rule: a unit that reaches the Heartseed is not
        // removed, it loops back to its own route start and keeps going
        // (`walk`'s own doc). `hp`/`max_hp` are untouched by this at all --
        // it is the SAME unit continuing, not a fresh spawn.
        let board = Board::new();
        let mut alloc = EntityIdAllocator::default();
        let mut enemy = Enemy::spawn(alloc.next(), EnemyKind::Skitter, RouteId(0), 1000);
        let hp_before = enemy.hp;
        let total = board.route(RouteId(0)).total_length_fp();
        let reached_heartseed = enemy.walk(&board, total + 1);
        assert!(reached_heartseed);
        assert_eq!(enemy.segment_index, 0, "a looped enemy must sit back at its route's own first segment");
        assert_eq!(enemy.offset_fp, 0, "a looped enemy must sit back at its route's own start offset");
        assert_eq!(enemy.hp, hp_before, "looping must never change the enemy's own HP");
    }
}
