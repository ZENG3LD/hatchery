//! Time-aged combat visual effects, accumulated from `SimEvent`s and driven
//! purely by a caller-supplied SIMULATED render time (`interp::sim_time`,
//! never a real wall clock) -- the "slower layer that copies effects from
//! events and ages them by frame time" the task asks for, distinct from
//! [`crate::interp`]'s own job (interpolating a POSITION already present
//! in the current tick's state). Neither module ever mutates a
//! `SimulationSnapshot`, calls `Simulation::advance`, or otherwise
//! influences a single tick or a stable hash -- both are strictly
//! presentation, reading events/snapshots the sim already produced.
//!
//! # Attributing an `Impact` to the `Shot` that caused it
//!
//! `SimEvent` carries no per-attack identifier linking a `Shot` to its own
//! `Impact`s -- but `sim.rs`'s own `fire_tower` emits exactly one `Shot`
//! immediately before every `Impact` its own attack causes (primary hit,
//! then any chain/splash extra hits, all before moving on to the next
//! tower), and never interleaves two towers' own event sequences. This
//! module relies on that ACTUAL, verified emission order (not a documented
//! contract `SimEvent` itself makes) to group "every `Impact` between one
//! `Shot` and the next" as belonging to that one attack -- a presentation-
//! layer inference, never a sim fact, and never fed back into anything
//! that could make a wrong inference here affect a tick or a hash.

use std::time::Duration;

use hatchery_arcade_engine::{DynamicSprite, DynamicStroke, Rgb, TileId};
use hatchery_arcade_pet_bastion::board::HEARTSEED;
use hatchery_arcade_pet_bastion::constants::{FIXED_SCALE, TICK_MS};
use hatchery_arcade_pet_bastion::enemy::EnemyKind;
use hatchery_arcade_pet_bastion::event::SimEvent;
use hatchery_arcade_pet_bastion::ids::EntityId;
use hatchery_arcade_pet_bastion::snapshot::SimulationSnapshot;
use hatchery_arcade_pet_bastion::tower::TowerKind;

use crate::interp::{pet_position_tiles, resolve_position};
use crate::{enemy_color, tower_base_color, CIRCUIT_LINK_COLOR};

/// A flying shot's own trail+point lands exactly as the tick that resolved
/// it becomes the new `curr` -- see `interp`'s own doc comment for why
/// `born_at` is always the START of the tick that produced these events,
/// so a `PROJECTILE_FLIGHT_MS` of exactly one tick's length completes
/// flight precisely when the render sweep reaches `alpha = 1.0`.
const PROJECTILE_FLIGHT_MS: f64 = TICK_MS as f64;
const IMPACT_FLASH_MS: f64 = 150.0;
const DEATH_BURST_MS: f64 = 300.0;
const CHAIN_ARC_MS: f64 = 220.0;
const SPLASH_RING_MS: f64 = 260.0;
const LINK_PULSE_MS: f64 = 220.0;
const LINK_CHARGE_MS: f64 = 180.0;

/// Every tower-side firing tell below (`spawn_muzzle_tell`/
/// `spawn_relay_flicker`) is capped at or under this -- never
/// `PROJECTILE_FLIGHT_MS` plus any margin. A tower's own tell exists to
/// say "this body just discharged," and `born_at` is always the shot's OWN
/// `born_at` (never later, see [`EffectsLayer::spawn_shot_effects`]'s own
/// call sites below) -- so a tell whose OWN lifetime outlasted the flight
/// would still be on screen after its own projectile has already landed
/// somewhere else, reading as disconnected from the shot that caused it.
/// Capping every one of these at this one ceiling is what makes that
/// failure structurally unreachable, rather than a per-kind value each
/// tuned separately and each one individually at risk of drifting past it.
const MUZZLE_TELL_CEILING_MS: f64 = PROJECTILE_FLIGHT_MS;
/// Needle: a single precise shot -- the shortest tell here, matching
/// `tower.rs`'s own `interval_ticks: 17`, the fastest cadence of the six.
const NEEDLE_SPARK_MS: f64 = MUZZLE_TELL_CEILING_MS * 0.6;
/// Bell: a toll -- the one tell that plausibly lingers the whole flight.
const BELL_TOLL_MS: f64 = MUZZLE_TELL_CEILING_MS;
/// Prism: a refraction glint.
const PRISM_GLINT_MS: f64 = MUZZLE_TELL_CEILING_MS * 0.7;
/// Ember Nest: one ember's own puff -- see `EMBER_COUGH_OFFSETS`'s own doc
/// comment for the staggered three-Pip "cough" this duration times.
const EMBER_COUGH_MS: f64 = MUZZLE_TELL_CEILING_MS * 0.64;
const EMBER_COUGH_STAGGER_MS: u64 = 5;
/// Moonwell: a pulse.
const MOONWELL_PULSE_MS: f64 = MUZZLE_TELL_CEILING_MS;
/// Relay: a flicker -- the faintest, quickest tell here, matching a Relay's
/// own passive, never-attacks role (see `spawn_relay_flicker`'s own doc
/// comment).
const RELAY_FLICKER_MS: f64 = MUZZLE_TELL_CEILING_MS * 0.5;

/// The three offsets Ember Nest's own `EffectKind::Pip` puff spawns at,
/// relative to the tower's own centre, in TILE units -- lifted straight
/// from `hatchery_arcade_engine::render::sprites`'s own `nest_path`
/// arm (its own `TowerEmberNest` match arm inside `paint_tile`), which
/// paints three embers at pixel-space offsets `(0.0, -0.2)`, `(-0.22,
/// 0.16)`, `(0.22, 0.16)` scaled by `half` (one HALF of one tile's own
/// width/height, i.e. `half == 0.5` tiles) -- so `dx_frac * half` in that
/// arm's own pixel space is `dx_frac * 0.5` tiles here. Reusing the exact
/// same three spots means the "cough" this module's own puff mimics
/// visibly comes from the same three places the tower's own body already
/// shows embers sitting, not an unrelated arrangement invented separately
/// on this side of the sim/presentation boundary.
const EMBER_COUGH_OFFSETS: [(f64, f64); 3] = [(0.0, -0.1), (-0.11, 0.08), (0.11, 0.08)];

#[derive(Clone, Copy, Debug)]
enum EffectKind {
    /// A shot in flight from `from` to `to`; renders both a moving point
    /// sprite and a trailing stroke behind it (see [`EffectsLayer::
    /// sprites`]).
    Projectile { from: (f64, f64), to: (f64, f64), color: Rgb },
    /// A static-endpoint stroke that simply fades out over its own
    /// lifetime -- a Prism chain's own visible arc between two targets, or
    /// the Living Circuit's own charge line from the pet to a bursting
    /// tower.
    Stroke { from: (f64, f64), to: (f64, f64), color: Rgb, bulge: f32 },
    /// A fixed-position sprite that grows from `1.0` to `grow_to_scale` and
    /// fades to transparent over its own lifetime -- an impact flash, a
    /// death burst, a splash wave, a Link Burst pulse.
    Pip { at: (f64, f64), tile: TileId, color: Rgb, grow_to_scale: f32 },
}

#[derive(Clone, Copy, Debug)]
struct Effect {
    kind: EffectKind,
    born_at: Duration,
    lifetime_ms: f64,
}

impl Effect {
    fn progress(&self, now: Duration) -> f64 {
        let age_ms = now.saturating_sub(self.born_at).as_secs_f64() * 1000.0;
        (age_ms / self.lifetime_ms.max(0.001)).clamp(0.0, 1.0)
    }

    fn is_expired(&self, now: Duration) -> bool {
        now.saturating_sub(self.born_at).as_secs_f64() * 1000.0 > self.lifetime_ms
    }
}

/// Accumulates and ages transient combat visuals. Owns no reference to a
/// `Simulation` -- only ever reads the `&[SimEvent]`/`&SimulationSnapshot`
/// a host hands it via [`EffectsLayer::ingest`], and its own `now` via
/// [`EffectsLayer::age`]/[`EffectsLayer::sprites`].
#[derive(Default)]
pub struct EffectsLayer {
    active: Vec<Effect>,
}

impl EffectsLayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawns every effect implied by one tick's worth of `events`,
    /// resolving positions against `snapshot_before` (the snapshot taken
    /// immediately BEFORE the `Simulation::advance` call that produced
    /// `events` -- towers never move, and every enemy/boss body this tick's
    /// attacks/kills touched is still at its pre-tick position there,
    /// exactly where it was when this tick's combat actually resolved).
    /// `born_at` should be `interp::sim_time(tick_index_before_advance,
    /// 0.0)` -- see [`PROJECTILE_FLIGHT_MS`]'s own doc for why.
    pub fn ingest(&mut self, events: &[SimEvent], snapshot_before: &SimulationSnapshot, born_at: Duration) {
        let mut current_shot: Option<(EntityId, Vec<EntityId>)> = None;
        for event in events {
            match event {
                SimEvent::Shot { tower, .. } => {
                    if let Some(shot) = current_shot.take() {
                        self.spawn_shot_effects(shot, snapshot_before, born_at);
                    }
                    current_shot = Some((*tower, Vec::new()));
                }
                SimEvent::Impact { target, .. } => {
                    if let Some((_, targets)) = &mut current_shot {
                        targets.push(*target);
                    }
                    self.spawn_impact_flash(*target, snapshot_before, born_at);
                }
                SimEvent::Kill { target, kind } => {
                    self.spawn_death_burst(*target, *kind, snapshot_before, born_at);
                }
                SimEvent::Leak { .. } => {
                    self.spawn_leak_flash(born_at);
                }
                SimEvent::LinkBurst { tower } => {
                    self.spawn_link_burst(*tower, snapshot_before, born_at);
                }
                _ => {}
            }
        }
        if let Some(shot) = current_shot.take() {
            self.spawn_shot_effects(shot, snapshot_before, born_at);
        }
    }

    fn spawn_shot_effects(&mut self, (tower, targets): (EntityId, Vec<EntityId>), snapshot_before: &SimulationSnapshot, born_at: Duration) {
        let Some(tower_pos) = resolve_position(snapshot_before, tower) else { return };
        let Some(tower_view) = snapshot_before.towers.iter().find(|t| t.id == tower) else { return };
        let Some(&primary_target) = targets.first() else { return };
        let Some(primary_pos) = resolve_position(snapshot_before, primary_target) else { return };
        let color = tower_base_color(tower_view.kind);

        self.active.push(Effect {
            kind: EffectKind::Projectile { from: tower_pos, to: primary_pos, color },
            born_at,
            lifetime_ms: PROJECTILE_FLIGHT_MS,
        });

        // The tower's own tell that it just fired -- see this module's own
        // top-level doc comment: purely presentation, born at the SAME
        // `born_at` the flying shot above carries (never later) and always
        // capped at `MUZZLE_TELL_CEILING_MS`, so it can never still be on
        // screen once its own shot has already landed (see that constant's
        // own doc comment). One handful of `EffectKind::Pip`s per kind
        // below -- no new `EffectKind` variant: `Pip`'s own grow/shrink
        // axis (`grow_to_scale` above OR below `1.0`) and `TileId`'s own
        // shape/colour choice already give every distinct read a tower's
        // own firing character needs.
        self.spawn_muzzle_tell(tower_view.kind, tower_pos, color, born_at);
        // A placed Relay's own tell -- see `spawn_relay_flicker`'s own doc
        // comment for why this is unconditional here (Relay never fires a
        // `Shot` of its own) rather than living inside `spawn_muzzle_tell`.
        self.spawn_relay_flicker(tower_view.linked, snapshot_before, born_at);

        if tower_view.kind == TowerKind::Prism && targets.len() >= 2 {
            for (i, pair) in targets.windows(2).enumerate() {
                let (Some(from), Some(to)) = (resolve_position(snapshot_before, pair[0]), resolve_position(snapshot_before, pair[1])) else { continue };
                // A small per-jump stagger so a multi-jump chain visibly
                // propagates target to target, rather than every arc
                // flashing on simultaneously.
                let stagger = Duration::from_millis((i as u64) * 40);
                self.active.push(Effect { kind: EffectKind::Stroke { from, to, color, bulge: 0.35 }, born_at: born_at + stagger, lifetime_ms: CHAIN_ARC_MS });
            }
        }

        if matches!(tower_view.kind, TowerKind::EmberNest | TowerKind::Moonwell) {
            if let Some(radius_fp) = tower_view.stats.splash_radius_fp {
                if radius_fp > 0 {
                    self.active.push(Effect { kind: EffectKind::Pip { at: primary_pos, tile: TileId::SplashRing, color, grow_to_scale: 1.0 + (radius_fp as f32 / FIXED_SCALE as f32) }, born_at, lifetime_ms: SPLASH_RING_MS });
                }
            }
        }
    }

    /// A tower's own muzzle tell -- what the BODY that just fired shows,
    /// anchored on `tower_pos` (never the target). Every arm below shares
    /// one contract: `born_at` passed straight through unchanged (the
    /// shot's own instant, never delayed) and a `lifetime_ms` at or under
    /// `MUZZLE_TELL_CEILING_MS` (see that constant's own doc comment) --
    /// only shape/colour/growth differ, matched to what each kind actually
    /// does when it fires.
    fn spawn_muzzle_tell(&mut self, kind: TowerKind, tower_pos: (f64, f64), color: Rgb, born_at: Duration) {
        match kind {
            TowerKind::Needle => {
                // A single precise shot -- one small bright point that
                // SHRINKS (`grow_to_scale < 1.0`, the one case in this
                // catalog where a Pip collapses rather than expands) and
                // vanishes over this catalog's shortest tell
                // (`NEEDLE_SPARK_MS`), the "snap" a precise single-target
                // bolt's own muzzle should read as. Reuses
                // `TileId::Projectile`'s own bright-point-plus-glow shape --
                // the same visual vocabulary this module already uses for
                // "a shot discharging," just static instead of travelling.
                self.active.push(Effect { kind: EffectKind::Pip { at: tower_pos, tile: TileId::Projectile, color, grow_to_scale: 0.55 }, born_at, lifetime_ms: NEEDLE_SPARK_MS });
            }
            TowerKind::Bell => {
                // A toll -- an expanding ring, the same "sound wave"
                // reading `TileId::SplashRing` already carries for an
                // area-damage wave, reused here centred on the TOWER
                // rather than a target: position and colour (Bell's own
                // violet, never Ember Nest's orange or Moonwell's blue)
                // are what keep this from ever being confused with an
                // actual splash. The longest tell in this catalog
                // (`BELL_TOLL_MS` == `MUZZLE_TELL_CEILING_MS`) -- a toll is
                // the one tell here that plausibly rings out the whole
                // flight, never past it.
                self.active.push(Effect { kind: EffectKind::Pip { at: tower_pos, tile: TileId::SplashRing, color, grow_to_scale: 2.1 }, born_at, lifetime_ms: BELL_TOLL_MS });
            }
            TowerKind::Prism => {
                // A refraction glint -- `TileId::ImpactFlash`'s own
                // ring-plus-dot-plus-rays starburst, tinted Prism's own
                // magenta rather than the plain white every genuine hit
                // always uses (`spawn_impact_flash`), so the two never
                // read as the same event even though they share a shape:
                // one is white and sits on the TARGET, this one is
                // magenta and sits on the TOWER.
                self.active.push(Effect { kind: EffectKind::Pip { at: tower_pos, tile: TileId::ImpactFlash, color, grow_to_scale: 1.5 }, born_at, lifetime_ms: PRISM_GLINT_MS });
            }
            TowerKind::EmberNest => {
                // A cough of embers -- three small sparks, staggered a few
                // ms apart (the same per-jump stagger trick the Prism-chain
                // arm above already uses for a chain that visibly
                // propagates rather than flashing on all at once), at the
                // SAME three offsets `EMBER_COUGH_OFFSETS`'s own doc
                // comment ties back to the tower's own body art, so the
                // puff visibly comes from the same three spots the tower's
                // own sprite already shows embers sitting.
                for (i, (dx, dy)) in EMBER_COUGH_OFFSETS.iter().enumerate() {
                    let stagger = Duration::from_millis(i as u64 * EMBER_COUGH_STAGGER_MS);
                    let at = (tower_pos.0 + dx, tower_pos.1 + dy);
                    self.active.push(Effect { kind: EffectKind::Pip { at, tile: TileId::Projectile, color, grow_to_scale: 1.25 }, born_at: born_at + stagger, lifetime_ms: EMBER_COUGH_MS });
                }
            }
            TowerKind::Moonwell => {
                // A pulse -- literally `TileId::LinkPulse`'s own name,
                // tinted Moonwell's own blue rather than the Living
                // Circuit's lavender (`CIRCUIT_LINK_COLOR`) an actual Link
                // Burst uses at a bursting tower's own position
                // (`spawn_link_burst`), so the two never share a colour
                // even on the rare tick a Moonwell IS the tower bursting.
                // A Moonwell's own splash wave (this fn's own caller, the
                // `TowerKind::EmberNest | TowerKind::Moonwell` branch just
                // below) still fires separately at the TARGET -- both are
                // readable in the same frame since they never share a
                // position.
                self.active.push(Effect { kind: EffectKind::Pip { at: tower_pos, tile: TileId::LinkPulse, color, grow_to_scale: 1.7 }, born_at, lifetime_ms: MOONWELL_PULSE_MS });
            }
            TowerKind::Relay => {
                // Unreachable in practice: `TowerKind::attacks` is `false`
                // for Relay (`tower.rs`'s own doc line: "Relay itself never
                // bursts -- it never attacks"), so `sim.rs`'s own
                // `fire_tower` never emits a `Shot` whose own `tower` is a
                // Relay, and `spawn_shot_effects` never calls this fn with
                // `kind == TowerKind::Relay` at all. A Relay's OWN tell is
                // not a response to a shot it never fires -- see
                // `spawn_relay_flicker`'s own doc comment for the tell it
                // actually gets instead.
            }
        }
    }

    /// A Relay's own tell for a shot it did NOT fire: Relay never attacks
    /// (`tower.rs`'s own `TowerKind::attacks` returns `false` for it), so
    /// unlike every kind `spawn_muzzle_tell` handles above, its own
    /// "I just fired" moment does not exist. What does exist is the moment
    /// a tower Relay's own presence may be feeding actually fires:
    /// `pet::compute_linked_towers` grants one extra Circuit slot per
    /// placed Relay ("Relay itself never bursts... extends Circuit by one
    /// tower" -- `sim.rs`/`tower.rs`'s own doc lines), so every currently
    /// LINKED tower's shot (`tower_view.linked`, the same per-tower Circuit
    /// membership flag `paint_tower` in this crate's own `lib.rs` already
    /// reads for the link background tint) is, in part, a signal Relay's
    /// own machinery helped carry.
    ///
    /// A presentation-layer inference (see this module's own top-level doc
    /// comment for the standard this crate already holds inferences to,
    /// same as the `Shot`-then-`Impact` grouping `ingest` relies on):
    /// `compute_linked_towers` does not expose which specific slot came
    /// from which specific Relay, so this cannot prove THIS Relay's own
    /// extra slot is what let THIS tower link -- it flickers every placed
    /// Relay on every linked tower's shot, which is the honest amount a
    /// tower with zero attack stats of its own can ever show. Uses
    /// `TileId::DeathBurst`'s own thin, soft ring at `RELAY_FLICKER_MS`
    /// (this catalog's faintest, quickest tell) -- a Relay's own passive,
    /// background role next to five towers that actually attack.
    fn spawn_relay_flicker(&mut self, firing_tower_linked: bool, snapshot_before: &SimulationSnapshot, born_at: Duration) {
        if !firing_tower_linked {
            return;
        }
        for relay in snapshot_before.towers.iter().filter(|t| t.kind == TowerKind::Relay) {
            let Some(pos) = resolve_position(snapshot_before, relay.id) else { continue };
            self.active.push(Effect { kind: EffectKind::Pip { at: pos, tile: TileId::DeathBurst, color: tower_base_color(TowerKind::Relay), grow_to_scale: 1.3 }, born_at, lifetime_ms: RELAY_FLICKER_MS });
        }
    }

    fn spawn_impact_flash(&mut self, target: EntityId, snapshot_before: &SimulationSnapshot, born_at: Duration) {
        let Some(at) = resolve_position(snapshot_before, target) else { return };
        self.active.push(Effect { kind: EffectKind::Pip { at, tile: TileId::ImpactFlash, color: Rgb(255, 255, 255), grow_to_scale: 1.6 }, born_at, lifetime_ms: IMPACT_FLASH_MS });
    }

    fn spawn_death_burst(&mut self, target: EntityId, kind: EnemyKind, snapshot_before: &SimulationSnapshot, born_at: Duration) {
        let Some(at) = resolve_position(snapshot_before, target) else { return };
        self.active.push(Effect { kind: EffectKind::Pip { at, tile: TileId::DeathBurst, color: enemy_color(kind), grow_to_scale: 2.2 }, born_at, lifetime_ms: DEATH_BURST_MS });
    }

    fn spawn_leak_flash(&mut self, born_at: Duration) {
        let at = (HEARTSEED.x as f64 + 0.5, HEARTSEED.y as f64 + 0.5);
        self.active.push(Effect { kind: EffectKind::Pip { at, tile: TileId::ImpactFlash, color: Rgb(220, 60, 60), grow_to_scale: 1.8 }, born_at, lifetime_ms: IMPACT_FLASH_MS });
    }

    fn spawn_link_burst(&mut self, tower: EntityId, snapshot_before: &SimulationSnapshot, born_at: Duration) {
        let Some(tower_pos) = resolve_position(snapshot_before, tower) else { return };
        self.active.push(Effect { kind: EffectKind::Pip { at: tower_pos, tile: TileId::LinkPulse, color: CIRCUIT_LINK_COLOR, grow_to_scale: 1.8 }, born_at, lifetime_ms: LINK_PULSE_MS });
        let pet_pos = pet_position_tiles(&snapshot_before.pet, 0.0);
        self.active.push(Effect { kind: EffectKind::Stroke { from: pet_pos, to: tower_pos, color: CIRCUIT_LINK_COLOR, bulge: 0.18 }, born_at, lifetime_ms: LINK_CHARGE_MS });
    }

    /// Drops every effect whose own lifetime has elapsed as of `now`.
    pub fn age(&mut self, now: Duration) {
        self.active.retain(|effect| !effect.is_expired(now));
    }

    /// How many effects are currently alive -- test/diagnostic use only.
    pub fn len(&self) -> usize {
        self.active.len()
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// Renders every currently-alive effect as of `now` -- does NOT prune
    /// expired effects itself (call [`EffectsLayer::age`] first); a caller
    /// that ages then immediately renders the same `now` never sees a
    /// stale, fully-expired effect either way.
    pub fn sprites(&self, now: Duration) -> (Vec<DynamicSprite>, Vec<DynamicStroke>) {
        let mut sprites = Vec::new();
        let mut strokes = Vec::new();
        for effect in &self.active {
            let p = effect.progress(now);
            match effect.kind {
                EffectKind::Projectile { from, to, color } => {
                    let pos = (from.0 + (to.0 - from.0) * p, from.1 + (to.1 - from.1) * p);
                    strokes.push(DynamicStroke { from_tile: from, to_tile: pos, color, width_px: 1.6, bulge: 0.0, alpha: (1.0 - p * 0.3) as f32 });
                    sprites.push(DynamicSprite { tile: TileId::Projectile, variant: 0, fg: color, bg: None, tile_x: pos.0, tile_y: pos.1, scale: 1.0, alpha: (1.0 - p * 0.2) as f32 });
                }
                EffectKind::Stroke { from, to, color, bulge } => {
                    strokes.push(DynamicStroke { from_tile: from, to_tile: to, color, width_px: 1.4, bulge, alpha: (1.0 - p) as f32 });
                }
                EffectKind::Pip { at, tile, color, grow_to_scale } => {
                    let scale = 1.0 + (grow_to_scale - 1.0) * p as f32;
                    sprites.push(DynamicSprite { tile, variant: 0, fg: color, bg: None, tile_x: at.0, tile_y: at.1, scale, alpha: (1.0 - p) as f32 });
                }
            }
        }
        (sprites, strokes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatchery_arcade_pet_bastion::board::AnchorId;
    use hatchery_arcade_pet_bastion::geometry::FixedPos;
    use hatchery_arcade_pet_bastion::ids::EntityIdAllocator;
    use hatchery_arcade_pet_bastion::pet::PetState;
    use hatchery_arcade_pet_bastion::snapshot::{EnemyView, PetView, RunPhaseView, TowerView};
    use hatchery_arcade_pet_bastion::tower::{effective_stats, TowerKind, UpgradeLevel};
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

    fn tower_view(id: EntityId, kind: TowerKind, x: i32, y: i32) -> TowerView {
        TowerView { id, kind, level: UpgradeLevel::Base, position: (x, y), linked: false, cooldown_ticks: 0, stats: effective_stats(kind, UpgradeLevel::Base), next_upgrade_cost: None, sell_price: 0 }
    }

    fn enemy_view(id: EntityId, x_tiles: i64, y_tiles: i64) -> EnemyView {
        EnemyView { id, kind: EnemyKind::Mite, position: FixedPos::new(x_tiles * FIXED_SCALE, y_tiles * FIXED_SCALE), route_progress_fp: 0, hp: 10, max_hp: 10, slow_permille: 0, stunned: false, last_hit_family: None, resist: None }
    }

    #[test]
    fn a_shot_and_impact_spawns_a_projectile_that_lands_exactly_at_alpha_one() {
        let mut alloc = EntityIdAllocator::default();
        let tower_id = alloc.next();
        let enemy_id = alloc.next();
        let mut snapshot = base_snapshot();
        snapshot.towers.push(tower_view(tower_id, TowerKind::Needle, 2, 2));
        snapshot.enemies.push(enemy_view(enemy_id, 8, 2));

        let events = vec![SimEvent::Shot { tower: tower_id, target: enemy_id }, SimEvent::Impact { target: enemy_id, damage: 5 }];
        let mut layer = EffectsLayer::new();
        layer.ingest(&events, &snapshot, Duration::ZERO);
        assert!(!layer.is_empty());

        let (sprites_start, _) = layer.sprites(Duration::ZERO);
        let projectile_start = sprites_start.iter().find(|s| s.tile == TileId::Projectile).expect("a projectile sprite must exist right at birth");
        // `2.5`, not the tower's own raw `(2, 2)` -- `resolve_position`'s
        // own doc comment: a tower resolves to its own CELL CENTRE.
        assert!((projectile_start.tile_x - 2.5).abs() < 1e-6, "at age 0 the projectile must sit at the tower's own position");

        let (sprites_end, _) = layer.sprites(Duration::from_millis(TICK_MS as u64));
        let projectile_end = sprites_end.iter().find(|s| s.tile == TileId::Projectile).expect("a projectile sprite must still exist at exactly one tick of age");
        assert!((projectile_end.tile_x - 8.0).abs() < 1e-6, "at age == one full tick the projectile must have arrived at the target");
    }

    #[test]
    fn age_prunes_an_effect_past_its_own_lifetime() {
        let mut alloc = EntityIdAllocator::default();
        let enemy_id = alloc.next();
        let mut snapshot = base_snapshot();
        snapshot.enemies.push(enemy_view(enemy_id, 3, 3));
        let events = vec![SimEvent::Kill { target: enemy_id, kind: EnemyKind::Mite }];

        let mut layer = EffectsLayer::new();
        layer.ingest(&events, &snapshot, Duration::ZERO);
        assert_eq!(layer.len(), 1);

        layer.age(Duration::from_millis(1));
        assert_eq!(layer.len(), 1, "an effect must survive well within its own lifetime");

        layer.age(Duration::from_secs(10));
        assert!(layer.is_empty(), "an effect must be pruned once its own lifetime has elapsed");
    }

    #[test]
    fn a_prism_chain_of_three_targets_spawns_two_arc_strokes() {
        let mut alloc = EntityIdAllocator::default();
        let tower_id = alloc.next();
        let e1 = alloc.next();
        let e2 = alloc.next();
        let e3 = alloc.next();
        let mut snapshot = base_snapshot();
        snapshot.towers.push(tower_view(tower_id, TowerKind::Prism, 1, 1));
        snapshot.enemies.push(enemy_view(e1, 2, 1));
        snapshot.enemies.push(enemy_view(e2, 3, 1));
        snapshot.enemies.push(enemy_view(e3, 4, 1));

        let events = vec![
            SimEvent::Shot { tower: tower_id, target: e1 },
            SimEvent::Impact { target: e1, damage: 5 },
            SimEvent::Impact { target: e2, damage: 3 },
            SimEvent::Impact { target: e3, damage: 2 },
        ];
        let mut layer = EffectsLayer::new();
        layer.ingest(&events, &snapshot, Duration::ZERO);
        let (_, strokes) = layer.sprites(Duration::from_millis(1));
        // One projectile trail (tower -> e1) plus two chain arcs (e1->e2,
        // e2->e3) -- at least 3 strokes must exist; the exact count also
        // includes the projectile's own trail.
        assert!(strokes.len() >= 3, "expected a projectile trail plus 2 chain arcs, got {}", strokes.len());
    }

    #[test]
    fn a_link_burst_spawns_a_pulse_and_a_charge_stroke_from_the_pet() {
        let mut alloc = EntityIdAllocator::default();
        let tower_id = alloc.next();
        let mut snapshot = base_snapshot();
        snapshot.towers.push(tower_view(tower_id, TowerKind::Needle, 5, 5));
        let events = vec![SimEvent::LinkBurst { tower: tower_id }];

        let mut layer = EffectsLayer::new();
        layer.ingest(&events, &snapshot, Duration::ZERO);
        let (sprites, strokes) = layer.sprites(Duration::from_millis(1));
        assert!(sprites.iter().any(|s| s.tile == TileId::LinkPulse));
        assert!(!strokes.is_empty(), "Link Burst must also draw a charge stroke from the pet to the bursting tower");
    }

    #[test]
    fn a_projectile_toward_an_unresolvable_target_is_silently_skipped_not_a_panic() {
        let mut alloc = EntityIdAllocator::default();
        let tower_id = alloc.next();
        let ghost_target = alloc.next();
        let mut snapshot = base_snapshot();
        snapshot.towers.push(tower_view(tower_id, TowerKind::Needle, 1, 1));
        let events = vec![SimEvent::Shot { tower: tower_id, target: ghost_target }, SimEvent::Impact { target: ghost_target, damage: 1 }];

        let mut layer = EffectsLayer::new();
        layer.ingest(&events, &snapshot, Duration::ZERO);
        let (sprites, _) = layer.sprites(Duration::ZERO);
        assert!(sprites.iter().all(|s| s.tile != TileId::Projectile), "an unresolvable target must never produce a fabricated projectile");
    }

    /// A host renders a frame from the `prev -> curr` snapshot pair the
    /// tick it just ran produced, so the ONLY frame-time index under
    /// which that tick's own effects are alive is `prev`'s -- see
    /// [`crate::interp::render_sim_time`]'s own doc comment. This test
    /// pins both halves of that: through the helper a shot is visible
    /// across the whole frame window, and through `curr.tick_index` --
    /// the wrong index, off by exactly one tick -- it is already spent on
    /// the first frame and pruned after it, which is what a live board
    /// showing no trails at all actually looks like.
    #[test]
    fn a_frame_ages_effects_by_the_previous_snapshots_tick_not_the_current_one() {
        let mut alloc = EntityIdAllocator::default();
        let tower_id = alloc.next();
        let enemy_id = alloc.next();

        // The pair a host holds after the tick with index `K` ran: `prev`
        // is the state that tick started from, `curr` the state it
        // produced -- exactly one apart, the invariant `FramePresenter::
        // push_tick` maintains.
        const K: u64 = 7;
        let mut prev = base_snapshot();
        prev.tick_index = K;
        prev.towers.push(tower_view(tower_id, TowerKind::Needle, 2, 2));
        prev.enemies.push(enemy_view(enemy_id, 8, 2));
        let mut curr = prev.clone();
        curr.tick_index = K + 1;

        let events = vec![SimEvent::Shot { tower: tower_id, target: enemy_id }, SimEvent::Impact { target: enemy_id, damage: 5 }];
        let born_at = crate::interp::sim_time(K, 0.0);

        let mut right = EffectsLayer::new();
        right.ingest(&events, &prev, born_at);
        let mut wrong = EffectsLayer::new();
        wrong.ingest(&events, &prev, born_at);

        // Four frames across the one tick this shot flies for.
        let mut right_frames_with_a_projectile = 0usize;
        let mut wrong_frames_with_a_projectile = 0usize;
        for step in 0..4 {
            let alpha = step as f64 / 4.0;

            let now = crate::interp::render_sim_time(Some(&prev), &curr, alpha);
            right.age(now);
            if right.sprites(now).0.iter().any(|s| s.tile == TileId::Projectile) {
                right_frames_with_a_projectile += 1;
            }

            let wrong_now = crate::interp::sim_time(curr.tick_index, alpha);
            wrong.age(wrong_now);
            if wrong.sprites(wrong_now).0.iter().any(|s| s.tile == TileId::Projectile) {
                wrong_frames_with_a_projectile += 1;
            }
        }

        assert_eq!(right_frames_with_a_projectile, 4, "aged by `prev`'s own tick the shot must be drawn on every frame of the window it flies across");
        assert!(
            wrong_frames_with_a_projectile <= 1,
            "aging by `curr`'s tick index is one whole tick late: the shot is already spent on the first frame and pruned after it (saw it on {wrong_frames_with_a_projectile} frames)"
        );
    }

    /// Every attacking kind must show SOME tell at the TOWER itself, not
    /// only the flying shot's own start point -- the whole point of this
    /// pass's own `spawn_muzzle_tell`. Distance-from-tower (not exact
    /// equality) is what tolerates Ember Nest's own three staggered,
    /// slightly-offset embers while still ruling out anything that only
    /// paints at the target.
    #[test]
    fn each_attacking_tower_kind_paints_a_muzzle_tell_at_the_tower_itself() {
        for kind in [TowerKind::Needle, TowerKind::Bell, TowerKind::Prism, TowerKind::EmberNest, TowerKind::Moonwell] {
            let mut alloc = EntityIdAllocator::default();
            let tower_id = alloc.next();
            let enemy_id = alloc.next();
            let mut snapshot = base_snapshot();
            snapshot.towers.push(tower_view(tower_id, kind, 2, 2));
            snapshot.enemies.push(enemy_view(enemy_id, 8, 2));
            let events = vec![SimEvent::Shot { tower: tower_id, target: enemy_id }, SimEvent::Impact { target: enemy_id, damage: 5 }];

            let mut layer = EffectsLayer::new();
            layer.ingest(&events, &snapshot, Duration::ZERO);
            let (sprites, _) = layer.sprites(Duration::ZERO);
            // `2.5, 2.5`, not the tower's own raw `(2, 2)` -- a tower
            // resolves to its own cell CENTRE (`resolve_position`'s own
            // doc comment).
            let near_tower = sprites.iter().filter(|s| ((s.tile_x - 2.5).powi(2) + (s.tile_y - 2.5).powi(2)).sqrt() < 0.3).count();
            assert!(near_tower >= 2, "{kind:?} must paint at least the flying shot's own start point PLUS a distinct muzzle tell at the tower, saw {near_tower} sprite(s) there");
        }
    }

    /// The exact failure mode this pass's own task named directly: "a
    /// muzzle flash that outlives its shot ... is the failure mode here."
    /// Aged to just past `PROJECTILE_FLIGHT_MS` (the shot's own flight
    /// time), NOTHING this module paints may still sit at the tower's own
    /// position -- the flying projectile itself is gone by then (its own
    /// lifetime is exactly `PROJECTILE_FLIGHT_MS`), and every muzzle tell
    /// is capped at `MUZZLE_TELL_CEILING_MS == PROJECTILE_FLIGHT_MS` (see
    /// that constant's own doc comment).
    #[test]
    fn no_tower_side_firing_tell_outlives_the_shot_it_belongs_to() {
        for kind in [TowerKind::Needle, TowerKind::Bell, TowerKind::Prism, TowerKind::EmberNest, TowerKind::Moonwell] {
            let mut alloc = EntityIdAllocator::default();
            let tower_id = alloc.next();
            let enemy_id = alloc.next();
            let mut snapshot = base_snapshot();
            snapshot.towers.push(tower_view(tower_id, kind, 2, 2));
            snapshot.enemies.push(enemy_view(enemy_id, 8, 2));
            let events = vec![SimEvent::Shot { tower: tower_id, target: enemy_id }, SimEvent::Impact { target: enemy_id, damage: 5 }];

            let mut layer = EffectsLayer::new();
            layer.ingest(&events, &snapshot, Duration::ZERO);
            let past_flight = Duration::from_millis(PROJECTILE_FLIGHT_MS as u64 + 1);
            layer.age(past_flight);
            let (sprites, _) = layer.sprites(past_flight);
            let still_at_tower = sprites.iter().any(|s| ((s.tile_x - 2.5).powi(2) + (s.tile_y - 2.5).powi(2)).sqrt() < 0.3);
            assert!(!still_at_tower, "{kind:?}'s own tower-side firing tell must be gone once its own shot has already landed, saw one still present at {past_flight:?}");
        }
    }

    /// `spawn_relay_flicker`'s own whole contract: a placed Relay flickers
    /// when a LINKED tower it may be feeding fires, and stays dark for an
    /// UNLINKED tower's own shot.
    #[test]
    fn a_relay_tower_flickers_when_a_linked_tower_it_feeds_fires_never_when_unlinked() {
        let mut alloc = EntityIdAllocator::default();
        let needle_id = alloc.next();
        let relay_id = alloc.next();
        let enemy_id = alloc.next();
        let mut snapshot = base_snapshot();
        snapshot.towers.push(tower_view(needle_id, TowerKind::Needle, 2, 2));
        snapshot.towers.push(tower_view(relay_id, TowerKind::Relay, 5, 5));
        snapshot.towers[0].linked = true;
        snapshot.enemies.push(enemy_view(enemy_id, 8, 2));
        let events = vec![SimEvent::Shot { tower: needle_id, target: enemy_id }, SimEvent::Impact { target: enemy_id, damage: 5 }];

        let mut linked_layer = EffectsLayer::new();
        linked_layer.ingest(&events, &snapshot, Duration::ZERO);
        let (linked_sprites, _) = linked_layer.sprites(Duration::ZERO);
        // `5.5, 5.5`, not the Relay's own raw `(5, 5)` -- a tower resolves
        // to its own cell CENTRE (`resolve_position`'s own doc comment).
        assert!(
            linked_sprites.iter().any(|s| (s.tile_x - 5.5).abs() < 1e-6 && (s.tile_y - 5.5).abs() < 1e-6),
            "a placed Relay must flicker when a LINKED tower it feeds fires"
        );

        snapshot.towers[0].linked = false;
        let mut unlinked_layer = EffectsLayer::new();
        unlinked_layer.ingest(&events, &snapshot, Duration::ZERO);
        let (unlinked_sprites, _) = unlinked_layer.sprites(Duration::ZERO);
        assert!(
            !unlinked_sprites.iter().any(|s| (s.tile_x - 5.5).abs() < 1e-6 && (s.tile_y - 5.5).abs() < 1e-6),
            "an UNLINKED tower's own shot must never flicker a Relay"
        );
    }
}
