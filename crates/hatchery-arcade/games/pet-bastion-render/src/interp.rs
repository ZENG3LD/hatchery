//! Frame interpolation: the game-specific half of "fixed sim step plus
//! smooth render," paired with `hatchery_arcade_engine::render::interp::
//! tick_alpha`'s own generic timing-fraction half (see that module's own
//! doc comment for exactly why the split lands there and not here).
//!
//! [`FramePresenter`] holds the previous and current [`SimulationSnapshot`]
//! a host has seen (`push_tick`); [`interpolated_dynamic_sprites`] is the
//! actual domain knowledge -- which fields move (enemy positions, boss
//! body positions, the pet's own sub-tick position), matched by which key
//! (`EntityId` for enemies/boss bodies; the pet has exactly one instance,
//! so no key is needed), and how a fixed-point sim position becomes a
//! [`hatchery_arcade_engine::DynamicSprite`] a pixel-tier backend can
//! actually composite.
//!
//! Every position here is a LINEAR interpolation between two already-
//! computed sim snapshots -- never a re-derivation of route physics at a
//! fractional tick. That is a deliberate, documented approximation (the
//! owner's own "стандартная схема для игр: fixed step + interpolation"):
//! over one 50ms tick, a route-walking enemy's true position deviates from
//! its two sampled endpoints' own straight-line interpolation by an amount
//! bounded by that tick's own speed change (there is none within a single
//! tick -- `Enemy::advance_movement` applies one constant per-tick speed),
//! so the lerp is exact for every tick except the rare one where a route
//! segment boundary falls strictly inside it (a corner) -- a currently
//! accepted, cosmetic-only rounding at map corners, never a sim-affecting
//! one (this module never feeds a computed value back into `Simulation`).

use std::time::Duration;

use hatchery_arcade_engine::{DynamicSprite, TileId};
use hatchery_arcade_pet_bastion::board::Board;
use hatchery_arcade_pet_bastion::constants::{FIXED_SCALE, PET_MOVE_TICKS, PET_MOVE_TICKS_MOTH, TICK_MS};
use hatchery_arcade_pet_bastion::geometry::FixedPos;
use hatchery_arcade_pet_bastion::ids::EntityId;
use hatchery_arcade_pet_bastion::pet::{Evolution, PetState};
use hatchery_arcade_pet_bastion::snapshot::{PetView, SimulationSnapshot};

use crate::{boss_color, boss_hp_tenths, boss_tile_id, enemy_color, enemy_status_bg_variant, enemy_tile_id, evolution_bg, evolution_variant, PET_COLOR};

/// Deterministic simulated wall-clock instant, expressed purely from a tick
/// index and a fraction of the way toward the NEXT tick -- never a real
/// `Instant`. See this crate's own `effects` module for why aging combat
/// effects against SIMULATED time (not a real render-loop clock) keeps
/// this whole presentation layer reproducible from nothing but the sim's
/// own tick count, matching the "renderer must never itself become a
/// second source of non-determinism a replay would have to capture" spirit
/// of the engine's own determinism discipline, even though nothing here
/// actually feeds back into a tick or a hash.
pub fn sim_time(tick_index: u64, alpha: f64) -> Duration {
    let base = Duration::from_millis(tick_index.saturating_mul(TICK_MS as u64));
    base + Duration::from_secs_f64(alpha.clamp(0.0, 1.0) * TICK_MS as f64 / 1000.0)
}

/// The simulated instant a frame interpolating `prev -> curr` at `alpha`
/// must age [`crate::effects::EffectsLayer`] to -- ALWAYS `prev`'s own
/// tick index, never `curr`'s.
///
/// A host pushes a snapshot into [`FramePresenter`] AFTER the tick that
/// produced it (see [`FramePresenter::push_tick`]), so `curr.tick_index`
/// is always `prev.tick_index + 1`, and a frame drawn from that pair is
/// showing the interval the tick `prev.tick_index` covered -- the very
/// tick whose own events were ingested with `born_at = sim_time(prev.
/// tick_index, 0.0)` (see [`crate::effects::EffectsLayer::ingest`], and
/// `Runner::drain_recorded`'s own `tick_index_before` on the engine side,
/// which is that same number).
///
/// Aging against `curr.tick_index` instead puts every effect exactly one
/// `TICK_MS` into its own future, and that is not a cosmetic drift:
/// `effects::PROJECTILE_FLIGHT_MS` is deliberately exactly one tick long,
/// so a shot would already stand at `progress == 1.0` on the first frame
/// it could be drawn and be pruned by [`crate::effects::EffectsLayer::
/// age`] on the next -- a trail that is never actually seen, with every
/// impact flash and death burst additionally missing its own first tick.
/// This fn exists so that no call site has to re-derive any of that: it
/// is the ONE place the `prev`-not-`curr` rule lives, and both consumers
/// (the TUI's own `render_pet_arcade` and `hatchery-arcade-preview`'s
/// own sequence loop) go through it.
///
/// `prev: None` is the first frame of a fresh run -- `FramePresenter`'s
/// own eager first push establishes `curr` with no `prev` at all, and no
/// tick has run yet to produce a single effect, so `curr`'s own index is
/// both the only one available and harmless.
pub fn render_sim_time(prev: Option<&SimulationSnapshot>, curr: &SimulationSnapshot, alpha: f64) -> Duration {
    sim_time(prev.map_or(curr.tick_index, |p| p.tick_index), alpha)
}

/// Holds the previous and current [`SimulationSnapshot`] a host has seen --
/// the minimal state [`interpolated_dynamic_sprites`] needs to lerp
/// between two known ticks. A fresh presenter (no `curr` yet) has nothing
/// to interpolate; its first [`FramePresenter::push_tick`] establishes
/// `curr` with no `prev` (interpolation degrades to "just show `curr`,
/// `alpha` ignored" for exactly that first frame -- see
/// [`interpolated_dynamic_sprites`]'s own handling of `prev: None`).
#[derive(Default)]
pub struct FramePresenter {
    prev: Option<SimulationSnapshot>,
    curr: Option<SimulationSnapshot>,
}

impl FramePresenter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shifts `curr` into `prev` and stores `snapshot` as the new `curr` --
    /// call this exactly once per completed sim tick, with the snapshot
    /// taken AFTER that tick's own `advance` call.
    pub fn push_tick(&mut self, snapshot: SimulationSnapshot) {
        self.prev = self.curr.take();
        self.curr = Some(snapshot);
    }

    pub fn current(&self) -> Option<&SimulationSnapshot> {
        self.curr.as_ref()
    }

    pub fn previous(&self) -> Option<&SimulationSnapshot> {
        self.prev.as_ref()
    }
}

fn fixed_to_tile_f64(pos: FixedPos) -> (f64, f64) {
    (pos.x as f64 / FIXED_SCALE as f64, pos.y as f64 / FIXED_SCALE as f64)
}

fn lerp_fixed(a: FixedPos, b: FixedPos, alpha: f64) -> (f64, f64) {
    let x = a.x as f64 + (b.x as f64 - a.x as f64) * alpha;
    let y = a.y as f64 + (b.y as f64 - a.y as f64) * alpha;
    (x / FIXED_SCALE as f64, y / FIXED_SCALE as f64)
}

/// The pet's own continuous position, in fractional board tiles --
/// extends `snapshot_to_surface`'s own tile-snapped `pet_tile` with a
/// sub-TICK `alpha` (the same `0.0..=1.0` fraction
/// `hatchery_arcade_engine::render::interp::tick_alpha` produces): while
/// `Moving`, `ticks_remaining` is only ever an INTEGER count as of the
/// snapshot it came from, so subtracting `alpha` from it before computing
/// the travelled fraction is what actually lets the pet glide smoothly
/// between two 20Hz ticks, not just jump tick to tick.
pub fn pet_position_tiles(pet: &PetView, alpha: f64) -> (f64, f64) {
    let alpha = alpha.clamp(0.0, 1.0);
    match pet.state {
        PetState::AtAnchor(anchor) => {
            let tile = Board::anchor_tile(anchor);
            (tile.x as f64, tile.y as f64)
        }
        PetState::Moving { from, to, ticks_remaining } => {
            let total = match pet.evolution {
                Some(Evolution::Moth) => PET_MOVE_TICKS_MOTH as f64,
                _ => PET_MOVE_TICKS as f64,
            };
            let remaining = (ticks_remaining as f64 - alpha).max(0.0);
            let elapsed = (total - remaining).clamp(0.0, total.max(1.0));
            let frac = elapsed / total.max(1.0);
            let from_tile = Board::anchor_tile(from);
            let to_tile = Board::anchor_tile(to);
            (from_tile.x as f64 + (to_tile.x as f64 - from_tile.x as f64) * frac, from_tile.y as f64 + (to_tile.y as f64 - from_tile.y as f64) * frac)
        }
    }
}

/// Every dynamic (continuously-positioned) entity for one render frame,
/// interpolated `alpha` (`0.0..=1.0`) of the way from `prev` toward `curr`
/// -- enemies and boss bodies matched by their own stable id (unmatched on
/// either side falls back to "just show the known side's own position,
/// unlerped": a freshly-spawned enemy has no `prev` sample to lerp FROM, a
/// just-removed one has no `curr` sample to show AT ALL, so it is simply
/// absent here -- see `hatchery-arcade-pet-bastion-render`'s own
/// `effects` module for how a kill's own visual continuity is carried by a
/// death effect instead, not by this function inventing a position for an
/// entity that no longer exists).
pub fn interpolated_dynamic_sprites(prev: Option<&SimulationSnapshot>, curr: &SimulationSnapshot, alpha: f64) -> Vec<DynamicSprite> {
    let alpha = alpha.clamp(0.0, 1.0);
    let mut out = Vec::with_capacity(curr.enemies.len() + 2);

    for enemy in &curr.enemies {
        let prev_pos = prev.and_then(|p| p.enemies.iter().find(|e| e.id == enemy.id)).map(|e| e.position);
        let (tile_x, tile_y) = match prev_pos {
            Some(prev_pos) => lerp_fixed(prev_pos, enemy.position, alpha),
            None => fixed_to_tile_f64(enemy.position),
        };
        let (bg, variant) = enemy_status_bg_variant(enemy);
        out.push(DynamicSprite { tile: enemy_tile_id(enemy.kind), variant, fg: enemy_color(enemy.kind), bg, tile_x, tile_y, scale: 1.0, alpha: 1.0 });
    }

    if let Some(boss) = &curr.boss {
        let prev_bodies = prev.and_then(|p| p.boss.as_ref()).map(|b| b.bodies.as_slice()).unwrap_or(&[]);
        let variant = boss_hp_tenths(boss);
        for body in &boss.bodies {
            let prev_pos = prev_bodies.iter().find(|pb| pb.id == body.id).map(|pb| pb.position);
            let (tile_x, tile_y) = match prev_pos {
                Some(prev_pos) => lerp_fixed(prev_pos, body.position, alpha),
                None => fixed_to_tile_f64(body.position),
            };
            out.push(DynamicSprite { tile: boss_tile_id(boss.kind), variant, fg: boss_color(boss.kind), bg: Some(crate::boss_bg(boss.kind)), tile_x, tile_y, scale: 1.0, alpha: 1.0 });
        }
    }

    let (pet_x, pet_y) = pet_position_tiles(&curr.pet, alpha);
    if !curr.pet.linked_towers.is_empty() {
        out.push(DynamicSprite { tile: TileId::CircuitLink, variant: 0, fg: crate::CIRCUIT_LINK_COLOR, bg: None, tile_x: pet_x, tile_y: pet_y, scale: 2.2, alpha: 0.55 });
    }
    out.push(DynamicSprite {
        tile: TileId::Pet,
        variant: evolution_variant(curr.pet.evolution),
        fg: PET_COLOR,
        bg: evolution_bg(curr.pet.evolution),
        tile_x: pet_x,
        tile_y: pet_y,
        scale: 1.0,
        alpha: 1.0,
    });

    out
}

/// Looks up `id`'s own current fractional-tile position in `snapshot` --
/// searches towers (static, but still a valid resolve target for an
/// effect that originates at a tower), enemies, then boss bodies, in that
/// order. `None` if `id` names nothing in this snapshot at all (already
/// removed, or never existed on this snapshot's own side of a tick
/// boundary) -- the caller (`effects::EffectsLayer::ingest`) treats a miss
/// as "nothing to draw for this event," never a fabricated position.
///
/// A tower's own `position` is a `(i32, i32)` SURFACE-GRID cell index
/// (`snapshot.rs`'s own `TowerView::position`, the same value `lib.rs`'s
/// own `paint_tower` hands straight to `Surface::set`), not a fixed-point
/// sim coordinate -- so unlike the enemy/boss branches below (already
/// continuous, pixel-precise fixed-point positions via `fixed_to_tile_f64`),
/// resolving it needs the SAME `+0.5` a Surface cell index always needs to
/// reach its own visual CENTRE, exactly what `backend_pixel::
/// paint_overlay_layer` already adds (`tile_to_px(x as f64 + 0.5, y as
/// f64 + 0.5)`) before painting that tower's own body. Returning the raw
/// index here used to place every tower-anchored effect (this shot's own
/// flying start point, a Living Circuit charge line's tower endpoint) a
/// full half-tile up-and-left of the tower's own painted body -- subtle
/// for a moving trail streaking off toward a distant target, but glaring
/// for anything static AT the tower, which is exactly what this pass's own
/// muzzle tells are (caught by rendering a real captured combat frame and
/// measuring where the tell actually painted against the tower's own real
/// sprite, not by inspection alone -- see this pass's own verification).
pub fn resolve_position(snapshot: &SimulationSnapshot, id: EntityId) -> Option<(f64, f64)> {
    if let Some(tower) = snapshot.towers.iter().find(|t| t.id == id) {
        return Some((tower.position.0 as f64 + 0.5, tower.position.1 as f64 + 0.5));
    }
    if let Some(enemy) = snapshot.enemies.iter().find(|e| e.id == id) {
        return Some(fixed_to_tile_f64(enemy.position));
    }
    if let Some(boss) = &snapshot.boss {
        if let Some(body) = boss.bodies.iter().find(|b| b.id == id) {
            return Some(fixed_to_tile_f64(body.position));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatchery_arcade_pet_bastion::board::AnchorId;
    use hatchery_arcade_pet_bastion::enemy::EnemyKind;
    use hatchery_arcade_pet_bastion::ids::EntityIdAllocator;
    use hatchery_arcade_pet_bastion::snapshot::{EnemyView, PetView, RunPhaseView};
    use hatchery_arcade_pet_bastion::wave::Difficulty;

    fn base_snapshot() -> SimulationSnapshot {
        SimulationSnapshot {
            tick_index: 0,
            difficulty: Difficulty::Standard,
            wave: 1,
            phase: RunPhaseView::Combat,
            sap: 0,
            integrity: 20,
            crab_shield: 0,
            towers: Vec::new(),
            enemies: Vec::new(),
            boss: None,
            pet: PetView { state: PetState::AtAnchor(AnchorId(0)), spark: 0, evolution: None, linked_towers: Vec::new() },
            rune_options: Vec::new(),
            runes_picked: Vec::new(),
            pet_charge_options: Vec::new(),
            pet_charges_picked: Vec::new(),
            field_zones: Vec::new(),
            wave_plan: None,
            build_cells: Vec::new(),
        }
    }

    fn enemy_view(id: EntityId, x_tiles: i64, y_tiles: i64) -> EnemyView {
        EnemyView {
            id,
            kind: EnemyKind::Mite,
            position: FixedPos::new(x_tiles * FIXED_SCALE, y_tiles * FIXED_SCALE),
            route_progress_fp: 0,
            hp: 10,
            max_hp: 10,
            slow_permille: 0,
            stunned: false,
            last_hit_family: None,
            resist: None,
        }
    }

    #[test]
    fn an_enemy_present_in_both_snapshots_lerps_between_its_two_positions() {
        let mut alloc = EntityIdAllocator::default();
        let id = alloc.next();
        let mut prev = base_snapshot();
        prev.enemies.push(enemy_view(id, 0, 0));
        let mut curr = base_snapshot();
        curr.enemies.push(enemy_view(id, 10, 0));

        let sprites = interpolated_dynamic_sprites(Some(&prev), &curr, 0.5);
        let enemy_sprite = sprites.iter().find(|s| s.tile == TileId::EnemyMite).expect("enemy sprite must be present");
        assert!((enemy_sprite.tile_x - 5.0).abs() < 1e-9, "halfway between tile 0 and tile 10 must be tile 5, got {}", enemy_sprite.tile_x);
    }

    #[test]
    fn a_freshly_spawned_enemy_with_no_prev_sample_shows_its_own_current_position_unlerped() {
        let mut alloc = EntityIdAllocator::default();
        let id = alloc.next();
        let prev = base_snapshot();
        let mut curr = base_snapshot();
        curr.enemies.push(enemy_view(id, 7, 2));

        let sprites = interpolated_dynamic_sprites(Some(&prev), &curr, 0.9);
        let enemy_sprite = sprites.iter().find(|s| s.tile == TileId::EnemyMite).expect("enemy sprite must be present");
        assert!((enemy_sprite.tile_x - 7.0).abs() < 1e-9);
        assert!((enemy_sprite.tile_y - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_leaked_or_killed_enemy_absent_from_curr_produces_no_dynamic_sprite() {
        let mut alloc = EntityIdAllocator::default();
        let id = alloc.next();
        let mut prev = base_snapshot();
        prev.enemies.push(enemy_view(id, 3, 3));
        let curr = base_snapshot();

        let sprites = interpolated_dynamic_sprites(Some(&prev), &curr, 0.5);
        assert!(sprites.iter().all(|s| s.tile != TileId::EnemyMite));
    }

    #[test]
    fn pet_moving_between_two_anchors_advances_smoothly_with_alpha() {
        let mut pet = PetView { state: PetState::Moving { from: AnchorId(0), to: AnchorId(1), ticks_remaining: 10 }, spark: 0, evolution: None, linked_towers: Vec::new() };
        let (x0, y0) = pet_position_tiles(&pet, 0.0);
        let (x1, y1) = pet_position_tiles(&pet, 0.9);
        assert_ne!((x0, y0), (x1, y1), "advancing alpha within the same tick must move the pet's own continuous position");

        // ticks_remaining counting down (as a later snapshot would report)
        // combined with a fresh alpha=0.0 must land further along than the
        // earlier tick's own alpha=0.9 -- continuity across the tick seam.
        pet.state = PetState::Moving { from: AnchorId(0), to: AnchorId(1), ticks_remaining: 9 };
        let (x2, _y2) = pet_position_tiles(&pet, 0.0);
        let from = Board::anchor_tile(AnchorId(0));
        let to = Board::anchor_tile(AnchorId(1));
        let expected_dir = (to.x as f64 - from.x as f64).signum();
        if expected_dir != 0.0 {
            assert!((x2 - x1).signum() == expected_dir || (x2 - x1).abs() < 1e-9);
        }
    }

    #[test]
    fn resolve_position_finds_a_tower_an_enemy_and_a_boss_body_by_id() {
        let mut alloc = EntityIdAllocator::default();
        let tower_id = alloc.next();
        let enemy_id = alloc.next();
        let mut snapshot = base_snapshot();
        snapshot.towers.push(hatchery_arcade_pet_bastion::snapshot::TowerView {
            id: tower_id,
            kind: hatchery_arcade_pet_bastion::tower::TowerKind::Needle,
            level: hatchery_arcade_pet_bastion::tower::UpgradeLevel::Base,
            position: (4, 1),
            linked: false,
            cooldown_ticks: 0,
            stats: hatchery_arcade_pet_bastion::tower::effective_stats(hatchery_arcade_pet_bastion::tower::TowerKind::Needle, hatchery_arcade_pet_bastion::tower::UpgradeLevel::Base),
            next_upgrade_cost: None,
            sell_price: 0,
        });
        snapshot.enemies.push(enemy_view(enemy_id, 6, 6));

        // A tower resolves to its own CELL CENTRE (`position` plus
        // `0.5, 0.5`), matching where `paint_overlay_layer` actually
        // paints that tower's own body -- see `resolve_position`'s own
        // doc comment for why this differs from the enemy branch below
        // (already a continuous fixed-point position, needing no such
        // adjustment).
        assert_eq!(resolve_position(&snapshot, tower_id), Some((4.5, 1.5)));
        assert_eq!(resolve_position(&snapshot, enemy_id), Some((6.0, 6.0)));
        assert_eq!(resolve_position(&snapshot, alloc.next()), None);
    }
}
