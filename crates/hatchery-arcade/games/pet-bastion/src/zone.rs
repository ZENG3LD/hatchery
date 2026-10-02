//! Per-wave field modifiers: the owner's own ask ("еще какую-то рандомную
//! механику на раунд, где случайные области получают какой-то баф-дебаф").
//!
//! Each wave draws exactly two zones, deterministically from the run's own
//! `EngineRng`, at the same point `begin_combat` already draws that wave's
//! spawn plan (`wave::generate_wave`) -- see `sim.rs`'s own module doc for
//! why RNG draws are confined to that one moment. One zone always governs
//! ATTACKING-TOWER damage (a reason to build somewhere different this wave);
//! the other always governs ENEMY/boss movement speed (a reason to cluster
//! defense along a different stretch of the fixed route this wave, since a
//! slower stretch is exactly the kind of extra contact time Bell's own slow
//! already makes valuable -- see `constants.rs`'s own `ZONE_ENEMY_SPEED_PERMILLE`
//! doc). Both draw a Buff or a Debuff independently, so a wave might hand
//! the player a free damage lane AND a hostile fast lane in the same
//! sweep, or the reverse -- never a guaranteed net-positive or net-negative
//! wave.
//!
//! The two zones are always drawn on DISTINCT sectors of a fixed 4x2 grid
//! (`ZONE_SECTOR_COLS`/`ZONE_SECTOR_ROWS`) -- never the same area doing
//! double duty, so a player reading the board sees two separately legible
//! reasons to build differently, not one area with two stacked meanings.
//!
//! Deliberately excluded from Link Burst and from Pet Pulse, for the same
//! discipline `sim.rs`'s own Pet Charge wiring (`PetCharge`) follows: both
//! zone effects apply only in the ordinary tower-attack path
//! (`fire_tower`)/the ordinary movement path (`Enemy`/`BossBody::
//! advance_movement`), keeping the mechanic's surface small and
//! predictable rather than touching every damage/movement code path in the
//! crate.

use crate::constants::{ZONE_ENEMY_SPEED_PERMILLE, ZONE_SECTOR_COLS, ZONE_SECTOR_COUNT, ZONE_SECTOR_ROWS, ZONE_TOWER_DAMAGE_PERMILLE};
use crate::constants::{BOARD_HEIGHT, BOARD_WIDTH, FIXED_SCALE};
use crate::geometry::FixedPos;
use crate::rng::EngineRng;

/// One 7x7-tile cell of the board's `ZONE_SECTOR_COLS` x `ZONE_SECTOR_ROWS`
/// grid, row-major (`id = row * ZONE_SECTOR_COLS + col`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct SectorId(pub u8);

impl SectorId {
    /// This sector's tile rectangle: `x` in `[x0, x1)`, `y` in `[y0, y1)`.
    pub fn tile_bounds(self) -> (i32, i32, i32, i32) {
        let col = (self.0 as i32) % ZONE_SECTOR_COLS;
        let row = (self.0 as i32) / ZONE_SECTOR_COLS;
        let w = BOARD_WIDTH / ZONE_SECTOR_COLS;
        let h = BOARD_HEIGHT / ZONE_SECTOR_ROWS;
        (col * w, row * h, col * w + w, row * h + h)
    }

    fn contains_fixed(self, pos: FixedPos) -> bool {
        let (x0, y0, x1, y1) = self.tile_bounds();
        let min_x = x0 as i64 * FIXED_SCALE;
        let min_y = y0 as i64 * FIXED_SCALE;
        let max_x = x1 as i64 * FIXED_SCALE;
        let max_y = y1 as i64 * FIXED_SCALE;
        pos.x >= min_x && pos.x < max_x && pos.y >= min_y && pos.y < max_y
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ZoneKind {
    TowerDamage,
    EnemySpeed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ZonePolarity {
    Buff,
    Debuff,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct WaveZone {
    pub kind: ZoneKind,
    pub sector: SectorId,
    pub polarity: ZonePolarity,
}

/// Both of one wave's field modifiers, plus the wave number they were drawn
/// for -- the same "tag with the wave, filter by `self.wave` at snapshot
/// time" pattern `wave::WavePlan`'s own `wave` field already uses, rather
/// than clearing this out explicitly at some other point in the tick
/// (`sim.rs`'s own `snapshot`/`wave_plan` doc explains why).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct WaveZones {
    pub wave: u32,
    pub tower_damage: WaveZone,
    pub enemy_speed: WaveZone,
}

/// Draws `wave`'s two field modifiers from `rng`. Called once per wave, from
/// the same `begin_combat` call that generates that wave's own spawn plan,
/// immediately after it -- see this module's own doc.
pub fn draw_wave_zones(wave: u32, rng: &mut EngineRng) -> WaveZones {
    let tower_sector = SectorId(rng.gen_range(ZONE_SECTOR_COUNT as u32) as u8);
    // Distinct sector for the enemy-speed zone: draw among the remaining
    // `ZONE_SECTOR_COUNT - 1` sectors, then remap past the excluded one --
    // keeps the two zones always separate, never the same area doing
    // double duty (this module's own doc).
    let mut enemy_sector = rng.gen_range((ZONE_SECTOR_COUNT - 1) as u32) as u8;
    if enemy_sector >= tower_sector.0 {
        enemy_sector += 1;
    }
    let tower_polarity = if rng.gen_range(2) == 0 { ZonePolarity::Buff } else { ZonePolarity::Debuff };
    let enemy_polarity = if rng.gen_range(2) == 0 { ZonePolarity::Buff } else { ZonePolarity::Debuff };
    WaveZones {
        wave,
        tower_damage: WaveZone { kind: ZoneKind::TowerDamage, sector: tower_sector, polarity: tower_polarity },
        enemy_speed: WaveZone { kind: ZoneKind::EnemySpeed, sector: SectorId(enemy_sector), polarity: enemy_polarity },
    }
}

impl WaveZones {
    /// Extra damage permille bonus (positive) or penalty (negative) for an
    /// attacking tower at `pos`, zero outside the tower-damage sector.
    pub fn tower_damage_bonus_permille(&self, pos: FixedPos) -> i64 {
        if !self.tower_damage.sector.contains_fixed(pos) {
            return 0;
        }
        match self.tower_damage.polarity {
            ZonePolarity::Buff => ZONE_TOWER_DAMAGE_PERMILLE,
            ZonePolarity::Debuff => -ZONE_TOWER_DAMAGE_PERMILLE,
        }
    }

    /// Movement-speed permille multiplier (1000 = unchanged) for an enemy
    /// or boss body at `pos` -- outside the enemy-speed sector this is
    /// always 1000, a pure identity multiplier.
    pub fn enemy_speed_permille(&self, pos: FixedPos) -> i64 {
        if !self.enemy_speed.sector.contains_fixed(pos) {
            return 1000;
        }
        match self.enemy_speed.polarity {
            ZonePolarity::Buff => 1000 - ZONE_ENEMY_SPEED_PERMILLE,
            ZonePolarity::Debuff => 1000 + ZONE_ENEMY_SPEED_PERMILLE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_zones_are_always_on_distinct_sectors() {
        for seed in 0..500u64 {
            let mut rng = EngineRng::seed(seed);
            let zones = draw_wave_zones(1, &mut rng);
            assert_ne!(
                zones.tower_damage.sector, zones.enemy_speed.sector,
                "seed {seed}: the tower-damage and enemy-speed zones must never land on the same sector"
            );
        }
    }

    #[test]
    fn every_sector_id_produces_valid_in_bounds_tile_bounds() {
        for id in 0..ZONE_SECTOR_COUNT as u8 {
            let (x0, y0, x1, y1) = SectorId(id).tile_bounds();
            assert!(x0 >= 0 && x1 <= BOARD_WIDTH && x0 < x1, "sector {id} x bounds out of range: [{x0},{x1})");
            assert!(y0 >= 0 && y1 <= BOARD_HEIGHT && y0 < y1, "sector {id} y bounds out of range: [{y0},{y1})");
        }
    }

    #[test]
    fn tower_damage_bonus_is_zero_outside_the_sector_and_signed_inside_it() {
        let zones = WaveZones {
            wave: 1,
            tower_damage: WaveZone { kind: ZoneKind::TowerDamage, sector: SectorId(0), polarity: ZonePolarity::Buff },
            enemy_speed: WaveZone { kind: ZoneKind::EnemySpeed, sector: SectorId(1), polarity: ZonePolarity::Debuff },
        };
        let (x0, y0, _, _) = SectorId(0).tile_bounds();
        let inside = crate::geometry::Tile::new(x0, y0).to_fixed();
        let outside = crate::geometry::Tile::new(BOARD_WIDTH - 1, BOARD_HEIGHT - 1).to_fixed();
        assert_eq!(zones.tower_damage_bonus_permille(inside), ZONE_TOWER_DAMAGE_PERMILLE);
        assert_eq!(zones.tower_damage_bonus_permille(outside), 0);
    }

    #[test]
    fn enemy_speed_permille_is_1000_outside_the_sector_and_shifted_inside_it() {
        let zones = WaveZones {
            wave: 1,
            tower_damage: WaveZone { kind: ZoneKind::TowerDamage, sector: SectorId(0), polarity: ZonePolarity::Buff },
            enemy_speed: WaveZone { kind: ZoneKind::EnemySpeed, sector: SectorId(1), polarity: ZonePolarity::Debuff },
        };
        let (x0, y0, _, _) = SectorId(1).tile_bounds();
        let inside = crate::geometry::Tile::new(x0, y0).to_fixed();
        let outside = crate::geometry::Tile::new(0, BOARD_HEIGHT - 1).to_fixed();
        assert_eq!(zones.enemy_speed_permille(inside), 1000 + ZONE_ENEMY_SPEED_PERMILLE);
        assert_eq!(zones.enemy_speed_permille(outside), 1000);
    }
}
