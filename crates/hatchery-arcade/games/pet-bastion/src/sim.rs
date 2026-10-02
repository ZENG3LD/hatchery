//! `Simulation`: the pure, deterministic Pet Bastion run. Ties together the
//! board, towers, enemies, bosses, the Living Circuit and the wave/rune/
//! evolution schedule behind the local `MiniGame`-shaped contract in
//! `contract.rs`.
//!
//! Per-tick order of operations inside [`Simulation::advance`]:
//! 1. apply this call's commands (placement, upgrades, pet actions, draft
//!    picks, ...) -- `StartWave` and a completed Build countdown are the
//!    only things that consume this tick's `EngineRng` (to generate the
//!    upcoming wave's spawn plan), keeping the "one seeded PRNG owns all
//!    run randomness" rule intact even though wave generation happens well
//!    after `Simulation::new` (which never receives an `EngineRng` at all,
//!    per the engine contract's own split between `new` and `advance`);
//! 2. advance the current phase (Build countdown, or one Combat tick: spawn
//!    due enemies/boss, run boss abilities, recompute the Circuit, resolve
//!    tower attacks, advance the pet, advance enemy movement, check for
//!    leaks and wave completion).

use std::time::Duration;

use std::collections::HashSet;

use crate::board::{AnchorId, Board, BuildIneligibleReason, RouteId};
use crate::boss::{Boss, BossKind};
use crate::command::Command;
use crate::constants::*;
use crate::contract::{CellArea, GameEntry, MiniGame, RunOutcome};
use crate::enemy::{Enemy, EnemyKind};
use crate::event::SimEvent;
use crate::geometry::Tile;
use crate::hash::StableHasher;
use crate::ids::{EntityId, EntityIdAllocator};
use crate::pet::{self, Evolution, Pet, PetCharge};
use crate::rng::EngineRng;
use crate::rune::{Rune, RuneLoadout, RuneShuffleBag};
use crate::snapshot::{
    BossBodyView, BossView, BuildCellView, EnemyView, FieldZoneView, PetView, RunPhaseView, SimulationSnapshot,
    TowerView,
};
use crate::tower::{self, DamageFamily, Tower, TowerKind, UpgradeLevel};
use crate::wave::{self, BalanceOverrides, Difficulty, WavePlan};
use crate::zone::{self, WaveZones};

#[derive(Clone)]
pub struct PetBastionParams {
    pub difficulty: Difficulty,
    /// Search-time boss-HP/reward overrides -- see [`BalanceOverrides`]'s
    /// own doc. `BalanceOverrides::default()` (every field neutral) makes a
    /// run identical to one built before this field existed.
    pub balance_overrides: BalanceOverrides,
}

impl PetBastionParams {
    /// A run at `difficulty` with no calibration overrides -- the
    /// constructor every caller outside `hatchery-arcade-sweep`'s own
    /// grid search uses.
    pub fn new(difficulty: Difficulty) -> Self {
        PetBastionParams { difficulty, balance_overrides: BalanceOverrides::default() }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum RunPhase {
    Build { ticks_remaining: u32 },
    Combat,
    RuneDraft,
    EvolutionChoice,
    PetChargeDraft,
    Finished,
}

/// Disjoint mutable access to the pieces one tower attack needs, bundled so
/// `fire_tower`/`apply_link_burst` take one argument instead of six.
struct World<'a> {
    towers: &'a mut Vec<Tower>,
    enemies: &'a mut Vec<Enemy>,
    board: &'a Board,
    boss: &'a mut Option<Boss>,
    pet: &'a mut Pet,
    runes: &'a RuneLoadout,
    id_alloc: &'a mut EntityIdAllocator,
}

pub struct Simulation {
    seed: u64,
    difficulty: Difficulty,
    balance_overrides: BalanceOverrides,
    board: Board,
    tick_index: u64,
    id_alloc: EntityIdAllocator,

    towers: Vec<Tower>,
    enemies: Vec<Enemy>,
    boss: Option<Boss>,
    pet: Pet,

    sap: i32,
    integrity: i32,

    wave: u32,
    phase: RunPhase,
    wave_plan: Option<WavePlan>,
    combat_start_tick: u64,
    spawn_cursor: usize,
    boss_spawned_this_wave: bool,

    rune_bag: RuneShuffleBag,
    rune_loadout: RuneLoadout,
    rune_options: Vec<Rune>,

    pet_charge_bag: pet::PetChargeShuffleBag,
    pet_charge_options: Vec<PetCharge>,

    /// This wave's two field modifiers, drawn at the same `begin_combat`
    /// call that generates the wave's own spawn plan -- see `zone.rs`'s own
    /// module doc. `None` only before wave 1's own combat has ever begun.
    wave_zones: Option<WaveZones>,

    outcome: Option<RunOutcome>,
}

impl Simulation {
    fn linked_towers_now(&self) -> Vec<EntityId> {
        let night_maw_final = matches!(&self.boss, Some(b) if b.final_phase);
        if self.pet.full_circuit_ticks_remaining > 0 {
            return pet::all_attacking_towers(&self.towers);
        }
        let mut ids = match self.pet.current_anchor() {
            Some(anchor) => {
                let anchor_pos = Board::anchor_tile(anchor).to_fixed();
                let base_slots = self.pet.base_slots(night_maw_final);
                pet::compute_linked_towers(anchor_pos, &self.towers, base_slots)
            }
            None => Vec::new(),
        };
        if let Some((lingering_ids, ticks)) = &self.pet.lingering {
            if *ticks > 0 {
                for id in lingering_ids {
                    if !ids.contains(id) {
                        ids.push(*id);
                    }
                }
            }
        }
        ids
    }

    fn anchor_is_corrupted(&self, anchor: AnchorId) -> bool {
        matches!(
            &self.boss,
            Some(b) if b.corrupted_anchor.map(|(a, t)| a == anchor && t > 0).unwrap_or(false)
        )
    }

    fn apply_command(&mut self, command: &Command, rng: &mut EngineRng, events: &mut Vec<SimEvent>) {
        match command {
            Command::Place { tile, kind } => self.place_tower(*tile, *kind),
            Command::UpgradeToL2 { tower } => self.upgrade_tower(*tower, UpgradeLevel::L2),
            Command::UpgradeToL3 { tower, branch } => {
                self.upgrade_tower(*tower, UpgradeLevel::L3(*branch))
            }
            Command::Sell { tower } => self.sell_tower(*tower),
            Command::MovePet { anchor } => {
                let linked = self.linked_towers_now();
                let has_anchor_rune = self.rune_loadout.has(Rune::Anchor);
                self.pet.start_move(*anchor, &linked, has_anchor_rune);
            }
            Command::Blink { anchor } => {
                if self.pet.evolution != Some(Evolution::Wisp) && self.pet.spark < BLINK_COST {
                    return;
                }
                let cost = self.pet.blink(*anchor);
                self.pet.spark = self.pet.spark.saturating_sub(cost);
                self.on_pet_arrival(*anchor, events);
            }
            Command::PetPulse => self.use_pet_pulse(events),
            Command::FullCircuit => {
                if self.pet.spark >= FULL_CIRCUIT_COST {
                    self.pet.spark -= FULL_CIRCUIT_COST;
                    self.pet.start_full_circuit();
                }
            }
            Command::StartWave => {
                if matches!(self.phase, RunPhase::Build { .. }) {
                    self.begin_combat(rng, events);
                }
            }
            Command::DraftRune(rune) => self.pick_rune(*rune, events),
            Command::ChooseEvolution(evolution) => self.pick_evolution(*evolution, events),
            Command::DraftPetCharge(charge) => self.pick_pet_charge(*charge, events),
        }
    }

    /// Whether Place/Upgrade/Sell may act right now: the Build phase (as
    /// always) AND Combat -- the owner's own direct ask ("не могу добавлять
    /// башни в момент волны"), so the player is never left watching a wave
    /// helplessly just because the ten-second Build countdown already ran
    /// out. Deliberately excludes `RuneDraft`/`EvolutionChoice` (modal
    /// decision points the player must resolve before anything else
    /// proceeds -- the same reason `StartWave` only fires from `Build`) and
    /// `Finished` (nothing to build once the run is over; also already
    /// unreachable in practice, since `advance` returns before applying any
    /// command once `self.outcome` is set).
    fn build_actions_allowed(&self) -> bool {
        matches!(self.phase, RunPhase::Build { .. } | RunPhase::Combat)
    }

    /// Places a new tower at `tile`, if the current phase allows building,
    /// `tile` is inside the build zone, clears every static (route/anchor)
    /// rule, is not already occupied by another tower, and the player can
    /// afford `kind`'s base cost. Silent no-op otherwise -- the same
    /// "invalid command, ignore it" contract every other command in this
    /// function follows.
    ///
    /// A tower placed while `RunPhase::Combat` is already running starts
    /// with [`COMBAT_PLACEMENT_ARMING_TICKS`] of cooldown instead of firing
    /// on its very next eligible tick -- the cost side of allowing combat
    /// building at all: a tower dropped to snipe one specific leaking enemy
    /// still needs a moment to power up, the same way it visibly does when
    /// placed during Build (its first shot is never instant there either,
    /// since nothing is in range yet). Placed during Build, the countdown
    /// gives it far longer than that to arm, so no explicit delay is needed
    /// there.
    fn place_tower(&mut self, tile: Tile, kind: TowerKind) {
        if !self.build_actions_allowed() {
            return;
        }
        if self.board.static_build_reason(tile).is_some() {
            return;
        }
        if self.towers.iter().any(|t| t.position == tile) {
            return;
        }
        let cost = kind.base_stats().cost;
        if cost > self.sap {
            return;
        }
        self.sap -= cost;
        let id = self.id_alloc.next();
        let mut tower = Tower::new(id, tile, kind);
        if matches!(self.phase, RunPhase::Combat) {
            tower.cooldown_ticks = COMBAT_PLACEMENT_ARMING_TICKS;
        }
        self.towers.push(tower);
    }

    fn upgrade_tower(&mut self, tower_id: EntityId, level: UpgradeLevel) {
        if !self.build_actions_allowed() {
            return;
        }
        let Some(tower) = self.towers.iter_mut().find(|t| t.id == tower_id) else {
            return;
        };
        let valid_step = matches!(
            (tower.level, level),
            (UpgradeLevel::Base, UpgradeLevel::L2) | (UpgradeLevel::L2, UpgradeLevel::L3(_))
        );
        if !valid_step {
            return;
        }
        let base_cost = tower.kind.base_stats().cost;
        let step_cost = level.step_cost(base_cost);
        if step_cost > self.sap {
            return;
        }
        self.sap -= step_cost;
        tower.sap_invested += step_cost;
        tower.level = level;
    }

    fn sell_tower(&mut self, tower_id: EntityId) {
        if !self.build_actions_allowed() {
            return;
        }
        let Some(pos) = self.towers.iter().position(|t| t.id == tower_id) else {
            return;
        };
        let tower = self.towers.remove(pos);
        self.sap += tower::sell_price(tower.sap_invested);
    }

    fn use_pet_pulse(&mut self, events: &mut Vec<SimEvent>) {
        if self.pet.spark < PET_PULSE_COST {
            return;
        }
        let Some(anchor) = self.pet.current_anchor() else {
            return;
        };
        self.pet.spark -= PET_PULSE_COST;
        let origin = Board::anchor_tile(anchor).to_fixed();
        let damage = self.pet.pet_pulse_damage();
        let targets = tower::select_splash_targets(
            origin,
            PET_PULSE_RADIUS_FP,
            &self.enemies,
            &self.board,
            MAX_SPLASH_TARGETS,
        );
        for idx in targets {
            let enemy = &mut self.enemies[idx];
            let dealt = tower::apply_armour(damage, enemy.armour, 0, false);
            enemy.hp -= dealt;
            events.push(SimEvent::Impact {
                target: enemy.id,
                damage: dealt,
            });
            enemy.knockback(&self.board, PET_PULSE_KNOCKBACK_FP);
        }
        self.enemies.retain(|e| e.hp > 0);
        if let Some(boss) = &mut self.boss {
            for body in boss.bodies.clone() {
                let pos = self
                    .board
                    .route(body.route)
                    .position_at(body.segment_index, body.offset_fp);
                if origin.dist2(pos) <= PET_PULSE_RADIUS_FP * PET_PULSE_RADIUS_FP {
                    let dealt = tower::apply_armour(damage, 0, 0, false);
                    boss.apply_damage(dealt);
                    events.push(SimEvent::Impact {
                        target: body.id,
                        damage: dealt,
                    });
                }
            }
        }
    }

    fn pick_rune(&mut self, rune: Rune, events: &mut Vec<SimEvent>) {
        if self.phase != RunPhase::RuneDraft || !self.rune_options.contains(&rune) {
            return;
        }
        self.rune_loadout.add(rune);
        self.rune_options.clear();
        events.push(SimEvent::RuneChosen { rune });
        self.wave += 1;
        self.enter_build_phase();
    }

    fn pick_evolution(&mut self, evolution: Evolution, events: &mut Vec<SimEvent>) {
        if self.phase != RunPhase::EvolutionChoice {
            return;
        }
        self.pet.evolution = Some(evolution);
        if evolution == Evolution::Wisp {
            self.pet.wisp_free_blink_ticks_remaining = WISP_FREE_BLINK_INTERVAL_TICKS as u32;
        }
        events.push(SimEvent::EvolutionChosen { evolution });
        self.wave += 1;
        self.enter_build_phase();
    }

    fn pick_pet_charge(&mut self, charge: PetCharge, events: &mut Vec<SimEvent>) {
        if self.phase != RunPhase::PetChargeDraft || !self.pet_charge_options.contains(&charge) {
            return;
        }
        self.pet.charges.add(charge);
        self.pet_charge_options.clear();
        events.push(SimEvent::PetChargeChosen { charge });
        self.wave += 1;
        self.enter_build_phase();
    }

    fn enter_build_phase(&mut self) {
        self.phase = RunPhase::Build {
            ticks_remaining: BUILD_PHASE_TICKS as u32,
        };
    }

    fn on_pet_arrival(&mut self, anchor: AnchorId, events: &mut Vec<SimEvent>) {
        self.pet.on_arrival_shield();
        events.push(SimEvent::PetArrived { anchor });
        let linked = self.linked_towers_now();
        for tower in self.towers.iter_mut() {
            if linked.contains(&tower.id) && tower.can_link_burst(self.tick_index) {
                tower.last_link_burst_tick = Some(self.tick_index);
                events.push(SimEvent::LinkBurst { tower: tower.id });
            }
        }
        let bursting: Vec<EntityId> = self
            .towers
            .iter()
            .filter(|t| t.last_link_burst_tick == Some(self.tick_index))
            .map(|t| t.id)
            .collect();
        for id in bursting {
            let mut world = World {
                towers: &mut self.towers,
                enemies: &mut self.enemies,
                board: &self.board,
                boss: &mut self.boss,
                pet: &mut self.pet,
                runes: &self.rune_loadout,
                id_alloc: &mut self.id_alloc,
            };
            apply_link_burst(id, &mut world, events);
        }
    }

    fn begin_combat(&mut self, rng: &mut EngineRng, events: &mut Vec<SimEvent>) {
        self.wave_plan = Some(wave::generate_wave(self.wave, self.difficulty, self.balance_overrides, rng));
        // Field modifiers are drawn immediately after the wave plan, from
        // the SAME `rng` and the SAME `begin_combat` call -- the only other
        // draw this tick makes, keeping the "StartWave/a completed Build
        // countdown are the only things that consume this tick's own
        // EngineRng" rule (this module's own doc) intact for this draw too.
        let zones = zone::draw_wave_zones(self.wave, rng);
        self.wave_zones = Some(zones);
        self.phase = RunPhase::Combat;
        self.combat_start_tick = self.tick_index;
        self.spawn_cursor = 0;
        self.boss_spawned_this_wave = false;
        events.push(SimEvent::WaveStarted { wave: self.wave });
        events.push(SimEvent::FieldZonesRevealed { zones });
    }

    fn tick_build(&mut self, rng: &mut EngineRng, events: &mut Vec<SimEvent>) {
        // The Circuit is a build-phase concern too -- a player positions
        // the pet before launching a wave, not only during combat -- so the
        // pet (and its arrival/Link Burst handling) ticks here as well.
        self.tick_pet(events);
        let done = matches!(&self.phase, RunPhase::Build { ticks_remaining: 0 });
        if done {
            self.begin_combat(rng, events);
            return;
        }
        if let RunPhase::Build { ticks_remaining } = &mut self.phase {
            *ticks_remaining -= 1;
        }
    }

    fn tick_pet(&mut self, events: &mut Vec<SimEvent>) {
        if let Some(arrived) = self.pet.tick() {
            self.on_pet_arrival(arrived, events);
        }
    }

    fn spawn_enemy(&mut self, kind: EnemyKind, route: RouteId) {
        let hp_scalar = wave::effective_minion_hp_scalar_permille(self.wave, self.difficulty, self.balance_overrides);
        let id = self.id_alloc.next();
        self.enemies.push(Enemy::spawn(id, kind, route, hp_scalar));
    }

    fn tick_combat(&mut self, rng: &mut EngineRng, events: &mut Vec<SimEvent>) {
        self.spawn_due_enemies();
        self.run_boss_abilities(events);
        let linked = self.linked_towers_now();
        self.resolve_tower_attacks(&linked, events);
        self.tick_pet(events);
        self.advance_enemy_movement(events);
        self.check_wave_completion(rng, events);
    }

    fn spawn_due_enemies(&mut self) {
        let Some(plan) = self.wave_plan.clone() else {
            return;
        };
        if !self.boss_spawned_this_wave {
            if let Some(kind) = plan.boss {
                let id = self.id_alloc.next();
                self.boss = Some(Boss::new(kind, id, RouteId(0), plan.boss_max_hp));
            }
            self.boss_spawned_this_wave = true;
        }
        let elapsed = self.tick_index - self.combat_start_tick;
        while self.spawn_cursor < plan.spawns.len()
            && plan.spawns[self.spawn_cursor].tick_offset <= elapsed
        {
            let entry = plan.spawns[self.spawn_cursor];
            self.spawn_enemy(entry.kind, entry.route);
            self.spawn_cursor += 1;
        }
    }

    fn run_boss_abilities(&mut self, events: &mut Vec<SimEvent>) {
        let Some(boss) = &mut self.boss else {
            return;
        };
        match boss.kind {
            BossKind::Bellkeeper => {
                if boss.silence_ticks_remaining > 0 {
                    boss.silence_ticks_remaining -= 1;
                }
                if boss.bell_cooldown_ticks == 0 {
                    boss.silence_ticks_remaining = BELLKEEPER_SILENCE_TICKS as u32;
                    boss.bell_cooldown_ticks = BELLKEEPER_BELL_INTERVAL_TICKS as u32;
                } else {
                    boss.bell_cooldown_ticks -= 1;
                }
                if let Some(idx) = boss.newly_crossed_escort_threshold() {
                    events.push(SimEvent::BossEscortTriggered { threshold_index: idx });
                    let opposite = self.board.opposite_route(boss.bodies[0].route);
                    let hp_scalar =
                        wave::effective_minion_hp_scalar_permille(self.wave, self.difficulty, self.balance_overrides);
                    for _ in 0..BELLKEEPER_ESCORT_COUNT {
                        let id = self.id_alloc.next();
                        self.enemies
                            .push(Enemy::spawn(id, EnemyKind::Skitter, opposite, hp_scalar));
                    }
                }
            }
            BossKind::NightMaw => {
                if boss.corrupt_cooldown_ticks == 0 {
                    if let Some(anchor) = self.pet.current_anchor() {
                        boss.corrupted_anchor = Some((anchor, NIGHT_MAW_CORRUPT_DURATION_TICKS as u32));
                    }
                    boss.corrupt_cooldown_ticks = NIGHT_MAW_CORRUPT_INTERVAL_TICKS as u32;
                } else {
                    boss.corrupt_cooldown_ticks -= 1;
                }
                if let Some((_, ticks)) = &mut boss.corrupted_anchor {
                    *ticks = ticks.saturating_sub(1);
                    if *ticks == 0 {
                        boss.corrupted_anchor = None;
                    }
                }
                if boss.should_split() {
                    boss.split_triggered = true;
                    let first = boss.bodies[0].clone();
                    let other_route = self.board.opposite_route(first.route);
                    let progress = self
                        .board
                        .route(first.route)
                        .progress_fp(first.segment_index, first.offset_fp);
                    let (seg, off) = self.board.route(other_route).locate(progress);
                    let id = self.id_alloc.next();
                    boss.bodies.push(crate::boss::BossBody {
                        id,
                        route: other_route,
                        segment_index: seg,
                        offset_fp: off,
                        slows: crate::status::SlowState::default(),
                    });
                    events.push(SimEvent::BossSplit);
                }
                if boss.should_enter_final_phase() {
                    boss.final_phase = true;
                    events.push(SimEvent::BossFinalPhase);
                }
            }
        }
    }

    fn resolve_tower_attacks(&mut self, linked_in: &[EntityId], events: &mut Vec<SimEvent>) {
        let corrupted = self
            .pet
            .current_anchor()
            .map(|a| self.anchor_is_corrupted(a))
            .unwrap_or(false);
        let linked: Vec<EntityId> = if corrupted { Vec::new() } else { linked_in.to_vec() };
        let night_maw_final = matches!(&self.boss, Some(b) if b.final_phase);
        let bellkeeper_silence = matches!(
            &self.boss,
            Some(b) if b.kind == BossKind::Bellkeeper && b.silence_ticks_remaining > 0
        );

        for i in 0..self.towers.len() {
            if !self.towers[i].kind.attacks() {
                continue;
            }
            let is_linked = linked.contains(&self.towers[i].id);
            if bellkeeper_silence && !is_linked {
                continue;
            }
            let suppressed = self.enemies.iter().any(|e| {
                e.is_alive()
                    && e.kind == EnemyKind::Husher
                    && self.towers[i].position_fp().dist2(e.position(&self.board))
                        <= HUSHER_SUPPRESS_RADIUS_FP * HUSHER_SUPPRESS_RADIUS_FP
            });
            self.towers[i].suppressed = suppressed;

            if self.towers[i].cooldown_ticks > 0 {
                self.towers[i].cooldown_ticks -= 1;
                continue;
            }

            let fired = {
                let mut world = World {
                    towers: &mut self.towers,
                    enemies: &mut self.enemies,
                    board: &self.board,
                    boss: &mut self.boss,
                    pet: &mut self.pet,
                    runes: &self.rune_loadout,
                    id_alloc: &mut self.id_alloc,
                };
                fire_tower(i, is_linked, night_maw_final, false, self.wave_zones, &mut world, events)
            };
            if fired {
                let has_surge = is_linked && self.pet.charges.has(PetCharge::Surge);
                let interval = adjusted_interval(&self.towers[i], is_linked, suppressed, has_surge);
                self.towers[i].cooldown_ticks = interval;
            }
        }
    }

    /// Advances every enemy and every live boss body by one tick's worth of
    /// route movement, then charges Integrity for whoever reached the
    /// Heartseed this tick. Nobody is removed from the board for reaching
    /// it any more (the owner's own rule, "по прохождению волны они просто
    /// заново переносились в начало") -- `Enemy::walk`/`BossBody::walk`
    /// both loop the SAME unit back to its own route start instead, so
    /// `self.enemies.len()` and `self.boss.as_ref().map(|b| b.bodies.
    /// len())` are unaffected by anything in this function; only actual
    /// kills (elsewhere, via `retain(|e| e.hp > 0)`) ever shrink either.
    /// This is why `check_wave_completion`'s own `no_enemies` check now
    /// means "every spawned enemy has been killed", not "killed or leaked
    /// away" -- see that function's own doc.
    fn advance_enemy_movement(&mut self, events: &mut Vec<SimEvent>) {
        let mut looped_enemy_ids = Vec::new();
        for enemy in self.enemies.iter_mut() {
            enemy.tick_status();
            // The enemy-speed field modifier (`zone.rs`) is a positional
            // rate, recomputed from wherever the enemy currently stands
            // BEFORE this tick's own movement, exactly the way its slow is
            // already recomputed fresh every tick from live `SlowState`.
            let pos = enemy.position(&self.board);
            let zone_speed = self.wave_zones.map(|z| z.enemy_speed_permille(pos)).unwrap_or(1000);
            if enemy.advance_movement(&self.board, zone_speed) {
                looped_enemy_ids.push(enemy.id);
            }
        }
        // A dead boss body must not move at all, let alone register a
        // Heartseed arrival this tick -- guarding movement behind
        // `Boss::is_defeated` belongs here, in the movement step itself,
        // rather than as an earlier defeat check elsewhere in the tick:
        // this is the one place that decides whether a body's position
        // changes this tick, and skipping movement is what "a dead thing
        // doesn't walk" actually means. `resolve_tower_attacks` runs
        // earlier in this same `tick_combat` tick (see its call order), so
        // a killing blow landed this very tick is already reflected in
        // `boss.shared_hp` by the time this code runs -- "undefeated" is
        // the load-bearing word, not "was alive when the tick started".
        // (`check_wave_completion`, which runs after this function and is
        // the only other place `Boss::is_defeated` is read, already
        // handles the "boss is dead" half of wave completion on its own;
        // it does not need this function's help, only for this function to
        // stop stepping on it.)
        //
        // `Boss::is_defeated` reads the ONE `shared_hp` pool every one of a
        // boss's bodies draws from (Night Maw's two split halves included --
        // see `boss.rs`'s own module doc), so this check is inherently
        // whole-boss, not per-body.
        //
        // A boss reaching the Heartseed no longer ends the run outright --
        // the owner's own complaint ("сейчас босс ваншотит"): it now costs
        // [`BOSS_LAP_INTEGRITY_DAMAGE`] Integrity and loops back to its own
        // route start, the exact same treatment an ordinary enemy gets
        // (just a bigger, separately-tuned charge -- see that constant's
        // own doc for why). One charge per TICK, not per body: Night Maw's
        // two split bodies (`Boss::bodies`) walk independent routes of
        // slightly different total length (`board.rs`'s own `Route::
        // build`), so a same-tick double arrival is rare but not
        // impossible -- `boss_looped_this_tick` collapses however many
        // bodies arrive on the SAME tick into a single Integrity charge,
        // while each arriving body is still reset to its own route's start
        // individually by `BossBody::walk` regardless of how many others
        // arrived alongside it. A body that arrives on a LATER, separate
        // tick registers its own separate charge -- that is a genuinely
        // separate arrival, not the same one counted twice.
        let mut boss_looped_this_tick = false;
        if let Some(boss) = &mut self.boss {
            if !boss.is_defeated() {
                let base_speed_fp = boss.kind.speed_fp();
                for body in &mut boss.bodies {
                    body.tick_status();
                    let pos = self.board.route(body.route).position_at(body.segment_index, body.offset_fp);
                    let zone_speed = self.wave_zones.map(|z| z.enemy_speed_permille(pos)).unwrap_or(1000);
                    if body.advance_movement(&self.board, base_speed_fp, zone_speed) {
                        boss_looped_this_tick = true;
                    }
                }
            }
        }

        for enemy_id in looped_enemy_ids {
            self.lose_integrity(LEAK_INTEGRITY_DAMAGE);
            events.push(SimEvent::Leak {
                enemy: enemy_id,
                integrity_remaining: self.integrity,
            });
            if self.integrity <= 0 {
                self.finish_run(RunOutcome::Lost, events);
            }
        }

        if boss_looped_this_tick {
            // `self.boss` is still `Some` here: the only place that ever
            // takes it is `check_wave_completion`'s own defeat handling,
            // and that runs strictly after this function within the same
            // `tick_combat` tick (see its own call order).
            if let Some(kind) = self.boss.as_ref().map(|b| b.kind) {
                self.lose_integrity(BOSS_LAP_INTEGRITY_DAMAGE);
                events.push(SimEvent::BossLap {
                    kind,
                    integrity_remaining: self.integrity,
                });
                if self.integrity <= 0 {
                    self.finish_run(RunOutcome::Lost, events);
                }
            }
        }
    }

    fn lose_integrity(&mut self, amount: i32) {
        if self.pet.crab_shield > 0 {
            let absorbed = amount.min(self.pet.crab_shield);
            self.pet.crab_shield -= absorbed;
            let remaining = amount - absorbed;
            self.integrity -= remaining;
        } else {
            self.integrity -= amount;
        }
    }

    /// Ends the current wave once its spawn plan is exhausted AND every
    /// spawned enemy AND boss is gone. Before Heartseed looping (`advance_
    /// enemy_movement`'s own doc), an enemy left `self.enemies` two ways --
    /// killed, or leaked away -- so `no_enemies` meant "dead or reached the
    /// end". Now only a kill ever removes an enemy (a loop resets it in
    /// place instead), so `no_enemies` means "every spawned enemy has been
    /// KILLED" -- a wave a player cannot out-damage no longer times out on
    /// its own; it drains Integrity every lap (`LEAK_INTEGRITY_DAMAGE`/
    /// `BOSS_LAP_INTEGRITY_DAMAGE`) until `self.integrity <= 0` ends the run
    /// via `finish_run(RunOutcome::Lost, ..)` instead -- a real ending
    /// (Integrity strictly decreases every lap, so this always terminates),
    /// not a silent stall.
    fn check_wave_completion(&mut self, rng: &mut EngineRng, events: &mut Vec<SimEvent>) {
        if self.outcome.is_some() {
            return;
        }
        let Some(plan) = &self.wave_plan else {
            return;
        };
        let all_spawned = self.spawn_cursor >= plan.spawns.len();
        // Every spawned enemy has been KILLED (see this function's own doc
        // for why "or leaked away" is no longer part of this).
        let no_enemies = self.enemies.is_empty();
        let boss_done = match &self.boss {
            None => true,
            Some(b) => b.is_defeated(),
        };
        if !(all_spawned && no_enemies && boss_done) {
            return;
        }
        if let Some(boss) = self.boss.take() {
            events.push(SimEvent::BossDefeated { kind: boss.kind });
        }
        let reward = wave::effective_clear_reward(self.wave, self.difficulty, self.balance_overrides);
        self.sap += reward;
        events.push(SimEvent::WaveCompleted { wave: self.wave });

        if self.wave >= WAVE_COUNT {
            self.finish_run(RunOutcome::Won, events);
            return;
        }
        if RUNE_DRAFT_AFTER_WAVES.contains(&self.wave) {
            self.phase = RunPhase::RuneDraft;
            self.rune_options = self.rune_bag.draw_options(rng, crate::rune::draft_option_count());
            events.push(SimEvent::RuneOffered {
                options: self.rune_options.clone(),
            });
        } else if self.wave == EVOLUTION_AFTER_WAVE {
            self.phase = RunPhase::EvolutionChoice;
            events.push(SimEvent::EvolutionOffered);
        } else if PET_CHARGE_DRAFT_AFTER_WAVES.contains(&self.wave) {
            self.phase = RunPhase::PetChargeDraft;
            self.pet_charge_options = self.pet_charge_bag.draw_options(rng, pet::charge_draft_option_count());
            events.push(SimEvent::PetChargeOffered {
                options: self.pet_charge_options.clone(),
            });
        } else {
            self.wave += 1;
            self.enter_build_phase();
        }
    }

    fn finish_run(&mut self, outcome: RunOutcome, events: &mut Vec<SimEvent>) {
        if self.outcome.is_some() {
            return;
        }
        self.outcome = Some(outcome);
        self.phase = RunPhase::Finished;
        events.push(match outcome {
            RunOutcome::Won => SimEvent::RunWon,
            RunOutcome::Lost => SimEvent::RunLost,
        });
    }
}

/// Applies the multiplicative link-speed bonus (plus the Surge pet charge,
/// on top, only while linked) and Husher suppression to a tower's base
/// attack interval, in that order, then floors at 1 tick.
fn adjusted_interval(tower: &Tower, linked: bool, suppressed: bool, has_surge: bool) -> u32 {
    let base = tower.stats().interval_ticks as i64;
    let mut permille = 1000i64;
    if linked {
        permille = (permille * 1000) / LINKED_ATTACK_SPEED_PERMILLE;
        if has_surge {
            permille = (permille * 1000) / PET_CHARGE_SURGE_EXTRA_SPEED_PERMILLE;
        }
    }
    if suppressed {
        permille = (permille * HUSHER_SUPPRESS_INTERVAL_PERMILLE) / 1000;
    }
    (((base * permille) + 500) / 1000).max(1) as u32
}

fn symbiosis_bonus_permille(idx: usize, towers: &[Tower], runes: &RuneLoadout) -> i64 {
    if !runes.has(Rune::Symbiosis) {
        return 0;
    }
    let me = &towers[idx];
    let mut bonus = 0i64;
    for (j, other) in towers.iter().enumerate() {
        if j == idx || other.kind == me.kind || !other.kind.attacks() {
            continue;
        }
        let d2 = me.position_fp().dist2(other.position_fp());
        if d2 <= SYMBIOSIS_ADJACENCY_FP * SYMBIOSIS_ADJACENCY_FP {
            bonus += SYMBIOSIS_DAMAGE_BONUS_PERMILLE;
        }
    }
    bonus
}

/// Extra splash/chain target-cap slot from the Bloom pet charge, while
/// `active` (linked and held) -- see [`PetCharge::Bloom`]'s own doc.
fn bloom_cap(base_cap: usize, active: bool) -> usize {
    if active {
        base_cap + 1
    } else {
        base_cap
    }
}

/// Extra splash/chain search radius from the Bloom pet charge, while
/// `active` -- see [`PetCharge::Bloom`]'s own doc.
fn bloom_radius(base_fp: i64, active: bool) -> i64 {
    if active {
        (base_fp * (1000 + PET_CHARGE_BLOOM_RADIUS_PERMILLE)) / 1000
    } else {
        base_fp
    }
}

/// The damage family one hit is resolved against, for Mirror's resistance
/// and its own `note_hit` bookkeeping: `stats.family` normally, or -- while
/// `linked` and the Attune pet charge is held -- every family in rotation
/// by `hit_count`, so the SAME family essentially never lands on the same
/// enemy twice in a row (Mirror's resist only ever protects against the one
/// family that hit it last). See [`PetCharge::Attune`]'s own doc.
fn resolved_family(stats_family: Option<DamageFamily>, hit_count: u32, linked: bool, charges: &pet::PetChargeLoadout) -> DamageFamily {
    if linked && charges.has(PetCharge::Attune) {
        DamageFamily::ALL[(hit_count as usize) % DamageFamily::ALL.len()]
    } else {
        stats_family.unwrap_or(DamageFamily::Physical)
    }
}

/// Fires one tower's attack, resolving its targeting, damage, splash/chain
/// caps, applicable runes/pet charges and any resulting kills. Returns
/// whether an attack actually happened (a target existed).
fn fire_tower(
    idx: usize,
    linked: bool,
    night_maw_final_phase: bool,
    is_echo_bonus: bool,
    wave_zones: Option<WaveZones>,
    world: &mut World<'_>,
    events: &mut Vec<SimEvent>,
) -> bool {
    let stats = world.towers[idx].stats();
    let origin = world.towers[idx].position_fp();
    let kind = world.towers[idx].kind;
    let tower_id = world.towers[idx].id;

    let target = tower::select_primary_target_unified(
        origin,
        stats.range_fp,
        stats.min_range_fp,
        world.enemies,
        world.boss.as_ref(),
        world.board,
    );
    let Some(target) = target else {
        return false;
    };

    world.towers[idx].attack_count += 1;
    let symbiosis_bonus = symbiosis_bonus_permille(idx, world.towers, world.runes);
    let fang_bonus = if linked && world.pet.charges.has(PetCharge::Fang) { PET_CHARGE_FANG_DAMAGE_PERMILLE } else { 0 };
    let zone_bonus = wave_zones.map(|z| z.tower_damage_bonus_permille(origin)).unwrap_or(0);
    let damage_bonus_permille = symbiosis_bonus + fang_bonus + zone_bonus;
    let effective_damage = if damage_bonus_permille != 0 {
        ((stats.damage as i64 * (1000 + damage_bonus_permille)) / 1000).max(0) as i32
    } else {
        stats.damage
    };
    // Splash/chain reach, shared by both the `TargetRef::Boss` and
    // `TargetRef::Enemy` arms below -- the Bloom pet charge widens both a
    // tower's target-cap slot and its own search radius, only while linked.
    let bloom = linked && world.pet.charges.has(PetCharge::Bloom);

    match target {
        tower::TargetRef::Boss(body_idx) => {
            // `body_id`/`boss_origin` are captured up front from this same
            // tick's own `select_primary_target_unified` result -- the boss
            // is guaranteed to still have this body immediately afterward
            // (nothing mutates `world.boss` between the selection above and
            // here), so a missing body only defensively short-circuits an
            // otherwise-impossible state instead of indexing blindly.
            let Some((body_id, boss_origin)) = world.boss.as_ref().and_then(|b| b.bodies.get(body_idx)).map(|body| {
                (
                    body.id,
                    world.board.route(body.route).position_at(body.segment_index, body.offset_fp),
                )
            }) else {
                return true;
            };
            events.push(SimEvent::Shot {
                tower: tower_id,
                target: body_id,
            });

            world.towers[idx].hit_count += 1;
            let ignore_armour =
                world.runes.has(Rune::Phase) && world.towers[idx].hit_count % PHASE_EVERY_NTH_HIT == 0;
            let dealt = tower::apply_armour(effective_damage, 0, stats.pierce, ignore_armour);

            // Overgrowth's carry-forward: the amount by which this hit
            // exceeded the boss's remaining HP (`Boss::apply_damage`'s own
            // return value already computes exactly that -- the applied
            // amount, capped at remaining HP -- so overkill is simply
            // `dealt` minus it, zero unless this hit was the boss's killing
            // blow). This is the only way Overgrowth's overkill can ever
            // involve a boss body: a boss is always this attack's own jump
            // 0 (never a later chain/splash jump -- see the Prism/Ember
            // Nest/Moonwell block below for why), so a boss can only ever
            // *feed* overkill forward into the next jump target, never
            // *receive* carried-in overkill on its own finishing blow.
            let mut overkill: i32 = 0;
            if let Some(b) = world.boss.as_mut() {
                let applied = b.apply_damage(dealt);
                if let Some(slow) = stats.slow_permille {
                    b.bodies[body_idx].apply_slow(slow, (TICKS_PER_SECOND * 2) as u32);
                }
                if world.runes.has(Rune::Overgrowth) {
                    overkill = dealt - applied;
                }
            }
            events.push(SimEvent::Impact {
                target: body_id,
                damage: dealt,
            });
            if linked {
                world.pet.credit_linked_kill(false);
            }

            // Splash/chain, symmetric with the `TargetRef::Enemy` arm below:
            // Prism's extra jumps and Ember Nest's/Moonwell's splash reach
            // nearby *minions* around the boss's own position (the plan's
            // own reason splash/chain matters against a boss wave at all --
            // its escort). They never reach a second body of the very same
            // boss: Night Maw's two split bodies already share one HP pool,
            // so a same-boss jump would just double-hit that one pool
            // instead of reaching an actual escort target, and
            // `select_chain_targets_from`/`select_splash_targets` are only
            // ever given `world.enemies` here, never `world.boss`, so this
            // is structural rather than an extra check. Needle and Bell
            // have neither chain nor splash to begin with (mirroring the
            // `_ => vec![primary_idx]` fallthrough below), so they fall to
            // `Vec::new()`.
            let extra_indices: Vec<usize> = match kind {
                TowerKind::Prism => {
                    let max_extra = (stats.chain_jumps as usize).min(bloom_cap(MAX_CHAIN_TARGETS, bloom).saturating_sub(1));
                    tower::select_chain_targets_from(
                        boss_origin,
                        max_extra,
                        bloom_radius(stats.range_fp, bloom),
                        world.enemies,
                        world.board,
                    )
                }
                TowerKind::EmberNest | TowerKind::Moonwell => match stats.splash_radius_fp {
                    Some(radius) => tower::select_splash_targets(
                        boss_origin,
                        bloom_radius(radius, bloom),
                        world.enemies,
                        world.board,
                        bloom_cap(MAX_SPLASH_TARGETS, bloom).saturating_sub(1),
                    ),
                    None => Vec::new(),
                },
                _ => Vec::new(),
            };

            for (extra_index, &enemy_idx) in extra_indices.iter().enumerate() {
                let jump_index = extra_index + 1;
                world.towers[idx].hit_count += 1;
                let ignore_armour = world.runes.has(Rune::Phase)
                    && world.towers[idx].hit_count % PHASE_EVERY_NTH_HIT == 0;
                let base_for_hit = match kind {
                    TowerKind::Prism => tower::prism_jump_damage(effective_damage, jump_index),
                    TowerKind::EmberNest => tower::ember_splash_damage(effective_damage, false),
                    _ => effective_damage,
                };
                let armour = world.enemies[enemy_idx].armour;
                let family = resolved_family(stats.family, world.towers[idx].hit_count, linked, &world.pet.charges);
                let resist = world.enemies[enemy_idx].resist_permille_against(family);
                let mut dealt = tower::apply_armour(base_for_hit, armour, stats.pierce, ignore_armour);
                if resist > 0 {
                    dealt = ((dealt as i64 * (1000 - resist)) / 1000) as i32;
                }
                if world.runes.has(Rune::Overgrowth) && overkill > 0 {
                    dealt += overkill;
                    overkill = 0;
                }

                let hp_before = world.enemies[enemy_idx].hp;
                world.enemies[enemy_idx].hp -= dealt;
                events.push(SimEvent::Impact {
                    target: world.enemies[enemy_idx].id,
                    damage: dealt,
                });
                if let Some(slow) = stats.slow_permille {
                    world.enemies[enemy_idx].apply_slow(slow, (TICKS_PER_SECOND * 2) as u32);
                }
                world.enemies[enemy_idx].note_hit(family);
                if world.runes.has(Rune::Overgrowth) && world.enemies[enemy_idx].hp < 0 {
                    overkill += -world.enemies[enemy_idx].hp;
                }
                let killed = hp_before > 0 && world.enemies[enemy_idx].hp <= 0;
                if killed {
                    let dead = world.enemies[enemy_idx].clone();
                    events.push(SimEvent::Kill {
                        target: dead.id,
                        kind: dead.kind,
                    });
                    if dead.kind == EnemyKind::Splitter {
                        for _ in 0..SPLITTER_SPAWN_COUNT {
                            let id = world.id_alloc.next();
                            let mut mite = Enemy::spawn(id, EnemyKind::Mite, dead.route, 1000);
                            mite.segment_index = dead.segment_index;
                            mite.offset_fp = dead.offset_fp;
                            world.enemies.push(mite);
                        }
                    }
                    if linked {
                        world.pet.credit_linked_kill(night_maw_final_phase);
                    }
                }
            }
            world.enemies.retain(|e| e.hp > 0);
        }
        tower::TargetRef::Enemy(primary_idx) => {
            let target_id = world.enemies[primary_idx].id;
            events.push(SimEvent::Shot {
                tower: tower_id,
                target: target_id,
            });

            let hit_indices: Vec<usize> = match kind {
                TowerKind::Prism => tower::select_chain_targets(
                    primary_idx,
                    (stats.chain_jumps as usize + 1).min(bloom_cap(MAX_CHAIN_TARGETS, bloom)),
                    bloom_radius(stats.range_fp, bloom),
                    world.enemies,
                    world.board,
                ),
                TowerKind::EmberNest | TowerKind::Moonwell => {
                    if let Some(radius) = stats.splash_radius_fp {
                        let origin = world.enemies[primary_idx].position(world.board);
                        tower::select_splash_targets(
                            origin,
                            bloom_radius(radius, bloom),
                            world.enemies,
                            world.board,
                            bloom_cap(MAX_SPLASH_TARGETS, bloom),
                        )
                    } else {
                        vec![primary_idx]
                    }
                }
                _ => vec![primary_idx],
            };

            let mut overkill: i32 = 0;
            for (jump_index, &enemy_idx) in hit_indices.iter().enumerate() {
                world.towers[idx].hit_count += 1;
                let ignore_armour = world.runes.has(Rune::Phase)
                    && world.towers[idx].hit_count % PHASE_EVERY_NTH_HIT == 0;
                let base_for_hit = match kind {
                    TowerKind::Prism => tower::prism_jump_damage(effective_damage, jump_index),
                    TowerKind::EmberNest => tower::ember_splash_damage(effective_damage, jump_index == 0),
                    _ => effective_damage,
                };
                let armour = world.enemies[enemy_idx].armour;
                let family = resolved_family(stats.family, world.towers[idx].hit_count, linked, &world.pet.charges);
                let resist = world.enemies[enemy_idx].resist_permille_against(family);
                let mut dealt = tower::apply_armour(base_for_hit, armour, stats.pierce, ignore_armour);
                if resist > 0 {
                    dealt = ((dealt as i64 * (1000 - resist)) / 1000) as i32;
                }
                if jump_index == 0 && world.runes.has(Rune::Overgrowth) && overkill > 0 {
                    dealt += overkill;
                    overkill = 0;
                }

                let hp_before = world.enemies[enemy_idx].hp;
                world.enemies[enemy_idx].hp -= dealt;
                events.push(SimEvent::Impact {
                    target: world.enemies[enemy_idx].id,
                    damage: dealt,
                });
                if let Some(slow) = stats.slow_permille {
                    world.enemies[enemy_idx].apply_slow(slow, (TICKS_PER_SECOND * 2) as u32);
                }
                world.enemies[enemy_idx].note_hit(family);
                if world.runes.has(Rune::Overgrowth) && world.enemies[enemy_idx].hp < 0 {
                    overkill += -world.enemies[enemy_idx].hp;
                }
                let killed = hp_before > 0 && world.enemies[enemy_idx].hp <= 0;
                if killed {
                    let dead = world.enemies[enemy_idx].clone();
                    events.push(SimEvent::Kill {
                        target: dead.id,
                        kind: dead.kind,
                    });
                    if dead.kind == EnemyKind::Splitter {
                        for _ in 0..SPLITTER_SPAWN_COUNT {
                            let id = world.id_alloc.next();
                            let mut mite = Enemy::spawn(id, EnemyKind::Mite, dead.route, 1000);
                            mite.segment_index = dead.segment_index;
                            mite.offset_fp = dead.offset_fp;
                            world.enemies.push(mite);
                        }
                    }
                    if linked {
                        world.pet.credit_linked_kill(night_maw_final_phase);
                    }
                }
            }
            world.enemies.retain(|e| e.hp > 0);
        }
    }

    if !is_echo_bonus
        && world.runes.has(Rune::Echo)
        && world.towers[idx].attack_count % ECHO_EVERY_NTH_ATTACK == 0
    {
        if let Some(echo_idx) = world.towers.iter().position(|t| t.kind == kind && t.id != tower_id) {
            fire_tower(echo_idx, linked, night_maw_final_phase, true, wave_zones, world, events);
        }
    }

    true
}

/// Applies one already-fully-computed (armour/pierce/amp already folded in
/// by the caller) Link Burst damage instance to a boss body, and records its
/// `Impact` event. Takes `&mut Boss` rather than `&mut World` on purpose --
/// every call site below reaches this from inside a block that already
/// holds `world.boss.as_mut()`, so a helper over the whole `World` would
/// re-borrow it and fail to compile. Link Burst never touches hit_count-
/// based runes (Phase, Overgrowth) for any tower kind or target, boss
/// included -- see the enemy-hit code just above each call site, which
/// doesn't touch them either; this keeps boss parity with that existing
/// scope rather than opening a new rune surface as part of this fix.
fn apply_link_burst_boss_hit(boss: &mut Boss, body_idx: usize, dealt: i32, events: &mut Vec<SimEvent>) {
    let Some(body) = boss.bodies.get(body_idx) else {
        return;
    };
    let body_id = body.id;
    boss.apply_damage(dealt);
    events.push(SimEvent::Impact {
        target: body_id,
        damage: dealt,
    });
}

/// Applies a tower's Link Burst effect. See `constants.rs` for the numeric
/// choices behind each tower's flavour text.
///
/// Boss parity: Needle, Prism, Ember Nest and Moonwell's bursts all reach a
/// boss body exactly the way their normal attacks do (`fire_tower`'s
/// `TargetRef::Boss` arm above). Without this, Link Burst -- the Living
/// Circuit's whole payoff on arrival, the mechanic the plan says exists "to
/// test Circuit movement, not only total damage" -- was losing all of its
/// damage against a boss for four of the five attacking towers. Bell is the
/// deliberate exception: its Link Burst is a group stun, and bosses can
/// never be stunned (a plan rule, also enforced elsewhere by never calling
/// `apply_stun` on a boss body at all), so it stays enemy-only on purpose,
/// not by omission like the other four were.
fn apply_link_burst(tower_id: EntityId, world: &mut World<'_>, events: &mut Vec<SimEvent>) {
    let Some(idx) = world.towers.iter().position(|t| t.id == tower_id) else {
        return;
    };
    let kind = world.towers[idx].kind;
    let stats = world.towers[idx].stats();
    let origin = world.towers[idx].position_fp();
    let amplified = world.towers.iter().any(|t| {
        t.kind == TowerKind::Relay
            && t.position_fp().dist2(origin) <= RELAY_AMPLIFY_RADIUS_FP * RELAY_AMPLIFY_RADIUS_FP
    });
    let amp = |damage: i32| -> i32 {
        if amplified {
            ((damage as i64 * (1000 + RELAY_AMPLIFY_BONUS_PERMILLE)) / 1000) as i32
        } else {
            damage
        }
    };

    match kind {
        TowerKind::Needle => {
            for _ in 0..3 {
                let Some(target) = tower::select_primary_target_unified(
                    origin,
                    stats.range_fp,
                    stats.min_range_fp,
                    world.enemies,
                    world.boss.as_ref(),
                    world.board,
                ) else {
                    break;
                };
                match target {
                    tower::TargetRef::Enemy(target_idx) => {
                        let armour = world.enemies[target_idx].armour;
                        let dealt = amp(tower::apply_armour(stats.damage, armour, stats.pierce, false));
                        world.enemies[target_idx].hp -= dealt;
                        events.push(SimEvent::Impact {
                            target: world.enemies[target_idx].id,
                            damage: dealt,
                        });
                        world.enemies.retain(|e| e.hp > 0);
                    }
                    tower::TargetRef::Boss(body_idx) => {
                        let dealt = amp(tower::apply_armour(stats.damage, 0, stats.pierce, false));
                        if let Some(boss) = world.boss.as_mut() {
                            apply_link_burst_boss_hit(boss, body_idx, dealt, events);
                        }
                    }
                }
            }
        }
        TowerKind::Bell => {
            // Deliberately enemy-only -- see this function's own doc above.
            let targets =
                tower::select_splash_targets(origin, stats.range_fp, world.enemies, world.board, MAX_SPLASH_TARGETS);
            for idx in targets {
                world.enemies[idx].apply_stun(BELL_GROUP_STUN_TICKS as u32);
            }
        }
        TowerKind::Prism => {
            let primary = tower::select_primary_target_unified(
                origin,
                stats.range_fp,
                stats.min_range_fp,
                world.enemies,
                world.boss.as_ref(),
                world.board,
            );
            match primary {
                Some(tower::TargetRef::Enemy(primary_idx)) => {
                    let extended = (stats.chain_jumps as usize + 1 + 2).min(MAX_CHAIN_TARGETS);
                    let chain =
                        tower::select_chain_targets(primary_idx, extended, stats.range_fp, world.enemies, world.board);
                    for (jump_index, idx) in chain.into_iter().enumerate() {
                        let base = amp(tower::prism_jump_damage(stats.damage, jump_index));
                        let armour = world.enemies[idx].armour;
                        let dealt = tower::apply_armour(base, armour, stats.pierce, false);
                        world.enemies[idx].hp -= dealt;
                        events.push(SimEvent::Impact {
                            target: world.enemies[idx].id,
                            damage: dealt,
                        });
                    }
                    world.enemies.retain(|e| e.hp > 0);
                }
                Some(tower::TargetRef::Boss(body_idx)) => {
                    // Same boss-body budget/never-jump-to-a-second-body
                    // reasoning as `fire_tower`'s own `TargetRef::Boss` arm:
                    // the boss fills jump 0, remaining jumps (up to the
                    // Link Burst's extended count) chain into nearby
                    // minions only, never another body of the same boss.
                    let Some(boss_origin) = world.boss.as_ref().and_then(|b| b.bodies.get(body_idx)).map(|body| {
                        world.board.route(body.route).position_at(body.segment_index, body.offset_fp)
                    }) else {
                        return;
                    };
                    let dealt = amp(tower::apply_armour(stats.damage, 0, stats.pierce, false));
                    if let Some(boss) = world.boss.as_mut() {
                        apply_link_burst_boss_hit(boss, body_idx, dealt, events);
                    }

                    let extended = (stats.chain_jumps as usize + 1 + 2).min(MAX_CHAIN_TARGETS);
                    let max_extra = extended.saturating_sub(1);
                    let chain = tower::select_chain_targets_from(
                        boss_origin,
                        max_extra,
                        stats.range_fp,
                        world.enemies,
                        world.board,
                    );
                    for (extra_index, idx) in chain.into_iter().enumerate() {
                        let jump_index = extra_index + 1;
                        let base = amp(tower::prism_jump_damage(stats.damage, jump_index));
                        let armour = world.enemies[idx].armour;
                        let dealt = tower::apply_armour(base, armour, stats.pierce, false);
                        world.enemies[idx].hp -= dealt;
                        events.push(SimEvent::Impact {
                            target: world.enemies[idx].id,
                            damage: dealt,
                        });
                    }
                    world.enemies.retain(|e| e.hp > 0);
                }
                None => {}
            }
        }
        TowerKind::EmberNest => {
            let radius = stats.splash_radius_fp.unwrap_or(stats.range_fp);
            let targets = tower::select_splash_targets(origin, radius, world.enemies, world.board, MAX_SPLASH_TARGETS);
            for (i, idx) in targets.into_iter().enumerate() {
                let base = amp(tower::ember_splash_damage(stats.damage, i == 0));
                let armour = world.enemies[idx].armour;
                let dealt = tower::apply_armour(base, armour, stats.pierce, false);
                world.enemies[idx].hp -= dealt;
                events.push(SimEvent::Impact {
                    target: world.enemies[idx].id,
                    damage: dealt,
                });
            }
            world.enemies.retain(|e| e.hp > 0);

            // The burning field is centred on the tower itself (unlike the
            // primary attack's own splash, which centres on whichever enemy
            // it hit) -- a boss body standing in that same radius takes the
            // field too, at full (non-falloff) strength and independent of
            // the MAX_SPLASH_TARGETS enemy cap above, mirroring
            // `Simulation::use_pet_pulse`'s own boss-in-radius handling for
            // the same reason: a boss isn't one of the "up to five enemies"
            // that cap is about, it's simply physically inside the blast.
            if let Some(boss) = world.boss.as_mut() {
                for (body_idx, body) in boss.bodies.clone().into_iter().enumerate() {
                    let pos = world.board.route(body.route).position_at(body.segment_index, body.offset_fp);
                    if origin.dist2(pos) <= radius * radius {
                        let dealt = amp(tower::apply_armour(stats.damage, 0, stats.pierce, false));
                        apply_link_burst_boss_hit(boss, body_idx, dealt, events);
                    }
                }
            }
        }
        TowerKind::Moonwell => {
            // A full multi-tick lingering-field entity is out of scope for
            // this pass (documented simplification); the burst instead
            // deals one immediate hit worth one tick's share of its
            // per-second lingering damage.
            let radius = stats.splash_radius_fp.unwrap_or(0);
            let targets = tower::select_splash_targets(origin, radius, world.enemies, world.board, MAX_SPLASH_TARGETS);
            for idx in targets {
                let base = amp(((stats.damage as i64 * MOONWELL_LINGER_TICK_DAMAGE_PERMILLE) / 1000) as i32);
                let armour = world.enemies[idx].armour;
                let dealt = tower::apply_armour(base, armour, stats.pierce, false);
                world.enemies[idx].hp -= dealt;
                events.push(SimEvent::Impact {
                    target: world.enemies[idx].id,
                    damage: dealt,
                });
            }
            world.enemies.retain(|e| e.hp > 0);

            // Same tower-centred-field reasoning as Ember Nest above: a
            // boss body in radius takes the field's per-tick share too,
            // independent of the enemy cap.
            if let Some(boss) = world.boss.as_mut() {
                for (body_idx, body) in boss.bodies.clone().into_iter().enumerate() {
                    let pos = world.board.route(body.route).position_at(body.segment_index, body.offset_fp);
                    if origin.dist2(pos) <= radius * radius {
                        let dealt = amp(((stats.damage as i64 * MOONWELL_LINGER_TICK_DAMAGE_PERMILLE) / 1000) as i32);
                        apply_link_burst_boss_hit(boss, body_idx, dealt, events);
                    }
                }
            }
        }
        TowerKind::Relay => {
            // Relay itself never bursts -- it never attacks.
        }
    }
}

impl MiniGame for Simulation {
    type Params = PetBastionParams;
    type Command = Command;
    type Event = SimEvent;
    type Snapshot = SimulationSnapshot;

    const TICK: Duration = Duration::from_millis(TICK_MS as u64);
    const RULES_VERSION: u32 = 1;

    fn new(seed: u64, params: Self::Params) -> Self {
        Simulation {
            seed,
            difficulty: params.difficulty,
            balance_overrides: params.balance_overrides,
            board: Board::new(),
            tick_index: 0,
            id_alloc: EntityIdAllocator::default(),
            towers: Vec::new(),
            enemies: Vec::new(),
            boss: None,
            pet: Pet::new(AnchorId(0)),
            sap: START_SAP,
            integrity: wave::effective_start_integrity(params.difficulty, params.balance_overrides),
            wave: 1,
            phase: RunPhase::Build {
                ticks_remaining: BUILD_PHASE_TICKS as u32,
            },
            wave_plan: None,
            combat_start_tick: 0,
            spawn_cursor: 0,
            boss_spawned_this_wave: false,
            rune_bag: RuneShuffleBag::new(),
            rune_loadout: RuneLoadout::default(),
            rune_options: Vec::new(),
            pet_charge_bag: pet::PetChargeShuffleBag::new(),
            pet_charge_options: Vec::new(),
            wave_zones: None,
            outcome: None,
        }
    }

    fn advance(&mut self, rng: &mut EngineRng, commands: &[Self::Command]) -> Vec<Self::Event> {
        let mut events = Vec::new();
        if self.outcome.is_some() {
            return events;
        }
        for command in commands {
            self.apply_command(command, rng, &mut events);
        }
        match self.phase {
            RunPhase::Build { .. } => self.tick_build(rng, &mut events),
            RunPhase::Combat => self.tick_combat(rng, &mut events),
            RunPhase::RuneDraft | RunPhase::EvolutionChoice | RunPhase::PetChargeDraft | RunPhase::Finished => {}
        }
        self.tick_index += 1;
        events
    }

    fn snapshot(&self) -> Self::Snapshot {
        let linked = self.linked_towers_now();
        let towers = self
            .towers
            .iter()
            .map(|t| {
                let stats = t.stats();
                let base_cost = t.kind.base_stats().cost;
                TowerView {
                    id: t.id,
                    kind: t.kind,
                    level: t.level,
                    position: (t.position.x, t.position.y),
                    linked: linked.contains(&t.id),
                    cooldown_ticks: t.cooldown_ticks,
                    stats,
                    next_upgrade_cost: tower::next_upgrade_cost(t.level, base_cost),
                    sell_price: tower::sell_price(t.sap_invested),
                }
            })
            .collect();
        let enemies = self
            .enemies
            .iter()
            .map(|e| EnemyView {
                id: e.id,
                kind: e.kind,
                position: e.position(&self.board),
                route_progress_fp: e.progress_fp(&self.board),
                hp: e.hp,
                max_hp: e.max_hp,
                slow_permille: e.combined_slow_permille(),
                stunned: e.stun_ticks > 0,
                last_hit_family: e.last_hit_family,
                resist: e.resist,
            })
            .collect();
        let boss = self.boss.as_ref().map(|b| BossView {
            kind: b.kind,
            hp: b.shared_hp,
            max_hp: b.shared_max_hp,
            hp_permille: b.hp_permille(),
            bodies: b
                .bodies
                .iter()
                .map(|body| BossBodyView {
                    id: body.id,
                    position: self.board.route(body.route).position_at(body.segment_index, body.offset_fp),
                    slow_permille: body.combined_slow_permille(),
                })
                .collect(),
            final_phase: b.final_phase,
            split_triggered: b.split_triggered,
            escort_triggered: b.escort_triggered,
        });
        // The current wave's own spawn plan only reflects `self.wave` while
        // combat for that wave is actually running -- `self.wave_plan` is
        // otherwise stale (still the JUST-FINISHED wave's plan, not yet
        // overwritten until the next `begin_combat`; see `snapshot.rs`'s
        // own `wave_plan` doc). Filtering on `plan.wave == self.wave`
        // keeps a Build-phase (or draft/evolution-phase) snapshot from
        // ever showing a mismatched, already-cleared wave's composition.
        let wave_plan = self.wave_plan.as_ref().filter(|plan| plan.wave == self.wave).cloned();
        // Occupied tower tiles as an O(1)-lookup set, built once here
        // rather than re-scanning `self.towers` for every one of
        // `build_zone_cells`' own tiles below -- free placement (`board.
        // rs`'s own module doc) means that is now the WHOLE 28x14 board
        // (392 tiles), not a route-proximity subset, but 392 cheap tuple
        // maps plus an O(1) HashSet lookup each is still microseconds at
        // 20 ticks/second, not a real per-tick cost (measured: `board::
        // tests::free_placement_makes_nearly_the_whole_board_buildable`).
        let occupied: HashSet<Tile> = self.towers.iter().map(|t| t.position).collect();
        let build_cells = self
            .board
            .build_zone_cells()
            .iter()
            .map(|&(tile, static_reason)| {
                let reason = static_reason
                    .or_else(|| occupied.contains(&tile).then_some(BuildIneligibleReason::Occupied));
                BuildCellView { tile: (tile.x, tile.y), reason }
            })
            .collect();
        let phase = match &self.phase {
            RunPhase::Build { ticks_remaining } => RunPhaseView::Build {
                ticks_remaining: *ticks_remaining,
            },
            RunPhase::Combat => RunPhaseView::Combat,
            RunPhase::RuneDraft => RunPhaseView::RuneDraft,
            RunPhase::EvolutionChoice => RunPhaseView::EvolutionChoice,
            RunPhase::PetChargeDraft => RunPhaseView::PetChargeDraft,
            RunPhase::Finished => match self.outcome {
                Some(RunOutcome::Won) => RunPhaseView::Victory,
                _ => RunPhaseView::Defeat,
            },
        };
        SimulationSnapshot {
            tick_index: self.tick_index,
            difficulty: self.difficulty,
            wave: self.wave,
            phase,
            sap: self.sap,
            integrity: self.integrity,
            crab_shield: self.pet.crab_shield,
            towers,
            enemies,
            boss,
            pet: PetView {
                state: self.pet.state,
                spark: self.pet.spark,
                evolution: self.pet.evolution,
                linked_towers: linked,
            },
            rune_options: self.rune_options.clone(),
            runes_picked: self.rune_loadout.picked().to_vec(),
            pet_charge_options: self.pet_charge_options.clone(),
            pet_charges_picked: self.pet.charges.picked().to_vec(),
            field_zones: self
                .wave_zones
                .filter(|z| z.wave == self.wave)
                .map(|z| {
                    vec![
                        FieldZoneView {
                            kind: z.tower_damage.kind,
                            polarity: z.tower_damage.polarity,
                            tile_bounds: z.tower_damage.sector.tile_bounds(),
                        },
                        FieldZoneView {
                            kind: z.enemy_speed.kind,
                            polarity: z.enemy_speed.polarity,
                            tile_bounds: z.enemy_speed.sector.tile_bounds(),
                        },
                    ]
                })
                .unwrap_or_default(),
            wave_plan,
            build_cells,
        }
    }

    fn stable_hash(&self) -> u64 {
        let mut h = StableHasher::new();
        h.write_u64(self.tick_index);
        h.write_u64(self.seed);
        h.write_i64(self.sap as i64);
        h.write_i64(self.integrity as i64);
        h.write_u64(self.wave as u64);
        h.write_u64(self.pet.spark as u64);
        for tower in &self.towers {
            h.write_u64(tower.id.0 as u64);
            h.write(&[tower_kind_byte(tower.kind)]);
            h.write_u64(tower.cooldown_ticks as u64);
        }
        for enemy in &self.enemies {
            h.write_u64(enemy.id.0 as u64);
            h.write_i64(enemy.hp as i64);
            h.write_u64(enemy.segment_index as u64);
            h.write_i64(enemy.offset_fp);
        }
        if let Some(boss) = &self.boss {
            h.write_i64(boss.shared_hp as i64);
        }
        // Pet Charges: order is draft order (deterministic given the seed,
        // same as every other insertion-order hash in this function), one
        // byte per picked charge.
        for charge in self.pet.charges.picked() {
            h.write(&[pet_charge_byte(*charge)]);
        }
        // Field modifiers: which wave they were drawn for, then both
        // zones' sector + polarity -- both change play (tower damage,
        // enemy speed), so both must be hashed.
        if let Some(zones) = &self.wave_zones {
            h.write_u64(zones.wave as u64);
            h.write(&[zones.tower_damage.sector.0, zone_polarity_byte(zones.tower_damage.polarity)]);
            h.write(&[zones.enemy_speed.sector.0, zone_polarity_byte(zones.enemy_speed.polarity)]);
        }
        h.finish()
    }

    fn is_finished(&self) -> Option<RunOutcome> {
        self.outcome
    }
}

/// Stable machine identifier for the arcade catalog (`ArcadeShell::select`,
/// save-file/replay `map_id` slot). Never changes once shipped -- a save or
/// replay decoded against a later build must still resolve to this game.
const GAME_ID: &str = "pet-bastion-night-garden";
const GAME_TITLE: &str = "Pet Bastion: Night Garden";

/// Terminal-cell chrome this game's own body needs beyond the raw board,
/// at the glyph tier's 1-cell-per-tile floor footprint -- the readability
/// contract's own floor case ("the one tier every game must be fully
/// legible on alone"). Counted honestly against what a player actually
/// needs to read to play, not eyeballed:
///
/// - a resource/status side panel to the right of the board, showing
///   Sap/Spark/Integrity/Wave/Phase as short labelled lines (`"Spark 3/8"`,
///   `"HP    18/20"`, `"Wave  4/8"`, ...) -- each such line comfortably fits
///   14 columns; the panel needs no more width than that at the floor tier
///   (a "список волн"/rune-ladder view with more detail is a PREFERRED-only
///   affordance, see below);
/// - one row of chrome above the board, for the wave/phase line and the
///   controls a host puts on it. This used to be a bottom hint row
///   printing a keybind reminder; the owner had that removed ("подписи
///   ... просто пространство экрана забирает"), so the row that remains
///   is the header, not a legend. A host that wants to name a binding
///   puts it on the control that performs it.
const MIN_HUD_PANEL_WIDTH: u16 = 14;
const MIN_HEADER_ROWS: u16 = 1;

/// Terminal-cell chrome budget at the PREFERRED size, where the board is
/// drawn at 2 terminal columns per tile (a common terminal-game convention
/// for square-looking tiles, since a terminal cell is roughly twice as tall
/// as it is wide -- only width is doubled, height stays 1 row per tile).
/// This buys room for:
///
/// - a roomier HUD panel (full labels, a per-wave progress ladder -- the
///   "список волн" this task asks for -- and the currently-held rune list,
///   not just abbreviated numbers);
/// - one header row above the board for the wave/phase/timer line.
///
/// It used to buy two more rows below the board for a keybind legend. That
/// legend is gone at the owner's own direction, and the rows went with it
/// rather than being left reserved and idle -- an overlay that asks a host
/// for space it no longer paints is just taking the screen away from
/// whatever else the operator has open.
const PREFERRED_BOARD_COLS_PER_TILE: u16 = 2;
const PREFERRED_HUD_PANEL_WIDTH: u16 = 30;
const PREFERRED_HEADER_ROWS: u16 = 1;

impl GameEntry for Simulation {
    const ID: &'static str = GAME_ID;
    const TITLE: &'static str = GAME_TITLE;

    /// `BOARD_WIDTH`/`BOARD_HEIGHT` (`constants.rs`) are the 28x14 logical
    /// tile grid; at the glyph tier's 1-cell-per-tile floor footprint that
    /// is 28x14 terminal cells for the board itself. Add the HUD side panel
    /// (`MIN_HUD_PANEL_WIDTH`) to the width and one header row
    /// (`MIN_HEADER_ROWS`) to the height -- the smallest size this game is
    /// still fully playable at: every board tile still gets its own
    /// distinct glyph cell (never shared/scaled down), and the resource
    /// panel still shows Sap/Spark/Integrity/Wave without truncation.
    fn min_modal_size() -> CellArea {
        CellArea {
            width: BOARD_WIDTH as u16 + MIN_HUD_PANEL_WIDTH,
            height: BOARD_HEIGHT as u16 + MIN_HEADER_ROWS,
        }
    }

    /// The board at `PREFERRED_BOARD_COLS_PER_TILE` (2) terminal columns
    /// per tile -- `BOARD_WIDTH * 2` wide, `BOARD_HEIGHT` tall (only width
    /// scales; see `PREFERRED_BOARD_COLS_PER_TILE`'s own doc) -- plus the
    /// roomier `PREFERRED_HUD_PANEL_WIDTH` side panel and one header row.
    /// This is the size the game is actually
    /// designed to be played at; `ArcadeShell::negotiate_size` clamps down
    /// to whatever is smaller than `available`, never scales this UP.
    fn preferred_modal_size() -> CellArea {
        CellArea {
            width: BOARD_WIDTH as u16 * PREFERRED_BOARD_COLS_PER_TILE + PREFERRED_HUD_PANEL_WIDTH,
            height: BOARD_HEIGHT as u16 + PREFERRED_HEADER_ROWS,
        }
    }
}

/// Small helper so `stable_hash` can encode a `TowerKind` as a byte.
fn tower_kind_byte(kind: TowerKind) -> u8 {
    match kind {
        TowerKind::Needle => 0,
        TowerKind::Bell => 1,
        TowerKind::Prism => 2,
        TowerKind::EmberNest => 3,
        TowerKind::Moonwell => 4,
        TowerKind::Relay => 5,
    }
}

/// Small helper so `stable_hash` can encode a `PetCharge` as a byte.
fn pet_charge_byte(charge: PetCharge) -> u8 {
    match charge {
        PetCharge::Surge => 0,
        PetCharge::Fang => 1,
        PetCharge::Bloom => 2,
        PetCharge::Attune => 3,
    }
}

/// Small helper so `stable_hash` can encode a `zone::ZonePolarity` as a byte.
fn zone_polarity_byte(polarity: zone::ZonePolarity) -> u8 {
    match polarity {
        zone::ZonePolarity::Buff => 0,
        zone::ZonePolarity::Debuff => 1,
    }
}

/// Direct tests of `fire_tower`/`apply_link_burst`'s boss handling -- this
/// module's own orchestration layer, which needs simultaneous mutable
/// access to towers/enemies/boss/pet/events (see the module doc), exactly
/// what a hand-built [`World`] gives here without needing a full
/// `Simulation`/`Command` round trip through the real 28x14 board.
///
/// That board-level route is deliberately NOT used for the boss-reach
/// cases below: with free placement, a real build could in principle land
/// a tower at any exact reach the test wants, but hand-placing both the
/// tower and the boss body directly is still simpler and more exact than
/// steering a real `Simulation`'s RNG-driven boss spawn onto a specific
/// fixed-point x. A hand-placed boss body sidesteps that entirely while
/// still exercising the exact same production code every real fight calls.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::enemy::EnemyKind;
    use crate::geometry::{tiles_to_fixed, Tile};
    use crate::ids::EntityIdAllocator;

    /// Owned state backing one hand-built [`World`]: a single tower at
    /// tile `(10, 2)` and a single-body boss on route 0's first (y=3) leg,
    /// `dy` tiles fixed-point below the tower and `dx_fp` fixed-point units
    /// off its own x (0 places it directly under the tower). No armour
    /// scaling, no other towers (so Relay's amplify check never triggers)
    /// unless a test adds one.
    struct LinkBurstRig {
        towers: Vec<Tower>,
        enemies: Vec<Enemy>,
        board: Board,
        boss: Option<Boss>,
        pet: Pet,
        runes: RuneLoadout,
        id_alloc: EntityIdAllocator,
    }

    impl LinkBurstRig {
        fn new(kind: TowerKind, dx_fp: i64, boss_max_hp: i32) -> (Self, EntityId, EntityId) {
            let mut id_alloc = EntityIdAllocator::default();
            let board = Board::new();
            let tower_id = id_alloc.next();
            let tower_pos = Tile::new(10, 2);
            let tower = Tower::new(tower_id, tower_pos, kind);
            let boss_id = id_alloc.next();
            let mut boss = Boss::new(BossKind::Bellkeeper, boss_id, RouteId(0), boss_max_hp);
            // Route 0's first leg is the horizontal `(0,3)-(20,3)` waypoint
            // pair: offset_fp along segment 0 IS the absolute route-0 x
            // coordinate, and its y is fixed at 3.0 tiles (30_000 fp) for
            // the whole leg.
            boss.bodies[0].offset_fp = tower_pos.to_fixed().x + dx_fp;
            let rig = LinkBurstRig {
                towers: vec![tower],
                enemies: Vec::new(),
                board,
                boss: Some(boss),
                pet: Pet::new(AnchorId(0)),
                runes: RuneLoadout::default(),
                id_alloc,
            };
            (rig, tower_id, boss_id)
        }

        fn world(&mut self) -> World<'_> {
            World {
                towers: &mut self.towers,
                enemies: &mut self.enemies,
                board: &self.board,
                boss: &mut self.boss,
                pet: &mut self.pet,
                runes: &self.runes,
                id_alloc: &mut self.id_alloc,
            }
        }

        fn boss_hp(&self) -> i32 {
            self.boss.as_ref().map(|b| b.shared_hp).unwrap_or(0)
        }

        /// Adds a Mite at route-0 offset `x_tiles`, for the chain/splash
        /// and Overgrowth tests below.
        fn add_mite_at(&mut self, x_tiles: i64) -> EntityId {
            let id = self.id_alloc.next();
            let mut mite = Enemy::spawn(id, EnemyKind::Mite, RouteId(0), 1000);
            mite.segment_index = 0;
            mite.offset_fp = tiles_to_fixed(x_tiles, 0);
            self.enemies.push(mite);
            id
        }
    }

    fn impact_damage(events: &[SimEvent], target: EntityId) -> Option<i32> {
        events.iter().find_map(|e| match e {
            SimEvent::Impact { target: t, damage } if *t == target => Some(*damage),
            _ => None,
        })
    }

    /// Runs `apply_link_burst` on a tower positioned 1.0 tile above a
    /// boss body (dead centre under it, `dx_fp = 0`) and returns the
    /// boss's HP loss -- the "control" (no Circuit, no burst) is the
    /// starting HP itself, since nothing in this function's scope can
    /// otherwise touch the boss.
    fn link_burst_boss_damage(kind: TowerKind) -> i32 {
        let (mut rig, tower_id, _boss_id) = LinkBurstRig::new(kind, 0, 1000);
        let hp_before_control = rig.boss_hp();
        let mut events = Vec::new();
        {
            let mut world = rig.world();
            apply_link_burst(tower_id, &mut world, &mut events);
        }
        let hp_after_circuit = rig.boss_hp();
        hp_before_control - hp_after_circuit
    }

    #[test]
    fn link_burst_needle_deals_extra_damage_to_a_boss_versus_a_never_linked_control() {
        // Three immediate full-damage shots (boss armour is frozen at 0 --
        // see `fire_tower`'s own `TargetRef::Boss` handling), no falloff:
        // exactly 3x base damage, every time.
        let expected = 3 * TowerKind::Needle.base_stats().damage;
        let extra = link_burst_boss_damage(TowerKind::Needle);
        assert_eq!(
            extra, expected,
            "control (no Circuit) leaves the boss untouched; Needle's Link Burst must deal exactly {expected} extra damage"
        );
    }

    #[test]
    fn link_burst_prism_deals_extra_damage_to_a_boss_versus_a_never_linked_control() {
        // No other enemy on the board in this rig, so the extended chain
        // has nothing to jump to -- the guaranteed floor is exactly the
        // primary hit's own base damage (jump 0 has no falloff).
        let expected = tower::prism_jump_damage(TowerKind::Prism.base_stats().damage, 0);
        let extra = link_burst_boss_damage(TowerKind::Prism);
        assert_eq!(
            extra, expected,
            "control (no Circuit) leaves the boss untouched; Prism's Link Burst primary hit must deal exactly {expected} extra damage"
        );
    }

    #[test]
    fn link_burst_ember_nest_deals_extra_damage_to_a_boss_versus_a_never_linked_control() {
        // Ember Nest's burst field is centred on the tower itself; a boss
        // 1.0 tile away is comfortably inside its base 2.0-tile radius, no
        // falloff at the "primary" (is_primary == true) slot.
        let expected = tower::ember_splash_damage(TowerKind::EmberNest.base_stats().damage, true);
        let extra = link_burst_boss_damage(TowerKind::EmberNest);
        assert_eq!(
            extra, expected,
            "control (no Circuit) leaves the boss untouched; Ember Nest's Link Burst field must deal exactly {expected} extra damage"
        );
    }

    #[test]
    fn link_burst_moonwell_deals_extra_damage_to_a_boss_versus_a_never_linked_control() {
        // One tick's worth of Moonwell's lingering-field share, same
        // formula the enemy-facing code already used before this fix.
        let expected = ((TowerKind::Moonwell.base_stats().damage as i64 * MOONWELL_LINGER_TICK_DAMAGE_PERMILLE) / 1000) as i32;
        let extra = link_burst_boss_damage(TowerKind::Moonwell);
        assert_eq!(
            extra, expected,
            "control (no Circuit) leaves the boss untouched; Moonwell's Link Burst field must deal exactly {expected} extra damage"
        );
    }

    #[test]
    fn bell_link_burst_never_stuns_a_boss() {
        // Deliberately different from the four towers above: Bell's burst
        // must find and stun a real target (proving the burst code path
        // actually runs, not that nothing was in range) while leaving the
        // boss completely untouched -- a boss has no stun state at all
        // (`BossBody` carries none, by design), so "untouched" here means
        // its HP never moves.
        let (mut rig, tower_id, _boss_id) = LinkBurstRig::new(TowerKind::Bell, 0, 1000);
        let enemy_id = rig.add_mite_at(10);
        let hp_before = rig.boss_hp();
        let mut events = Vec::new();
        {
            let mut world = rig.world();
            apply_link_burst(tower_id, &mut world, &mut events);
        }
        assert_eq!(rig.boss_hp(), hp_before, "Bell's Link Burst must never deal boss damage");
        assert!(events.is_empty(), "Bell's Link Burst must never emit a boss Impact event");
        let enemy = rig.enemies.iter().find(|e| e.id == enemy_id).expect("enemy still present");
        assert!(
            enemy.stun_ticks > 0,
            "Bell's Link Burst must still stun a real nearby enemy -- confirming the burst executed"
        );
    }

    #[test]
    fn prism_chain_reaches_a_nearby_minion_when_its_primary_target_is_the_boss() {
        // The Mite sits behind the boss (lower route-0 progress), so the
        // boss -- ahead of it and therefore closer to leaking -- wins
        // primary target selection; the chain then reaches back to it from
        // the boss's own position.
        let (mut rig, _tower_id, boss_id) = LinkBurstRig::new(TowerKind::Prism, 0, 1000);
        let mite_id = rig.add_mite_at(8);
        let mut events = Vec::new();
        let fired = {
            let mut world = rig.world();
            fire_tower(0, false, false, false, None, &mut world, &mut events)
        };
        assert!(fired, "the tower must find and fire on the boss as its primary target");

        let boss_damage = impact_damage(&events, boss_id).expect("boss must take an Impact this attack");
        let mite_damage = impact_damage(&events, mite_id).expect("chain must reach the nearby Mite too");
        assert_eq!(
            boss_damage,
            tower::prism_jump_damage(TowerKind::Prism.base_stats().damage, 0),
            "the primary hit on the boss must be full, unfalloff'd damage"
        );
        assert_eq!(
            mite_damage,
            tower::prism_jump_damage(TowerKind::Prism.base_stats().damage, 1),
            "the chain's first extra jump must land on the nearby Mite at the normal jump-1 falloff, at 0 Mite armour"
        );
    }

    #[test]
    fn ember_nest_splash_reaches_a_nearby_minion_when_its_primary_target_is_the_boss() {
        let (mut rig, _tower_id, boss_id) = LinkBurstRig::new(TowerKind::EmberNest, 0, 1000);
        let mite_id = rig.add_mite_at(9);
        let mut events = Vec::new();
        let fired = {
            let mut world = rig.world();
            fire_tower(0, false, false, false, None, &mut world, &mut events)
        };
        assert!(fired, "the tower must find and fire on the boss as its primary target");

        let boss_damage = impact_damage(&events, boss_id).expect("boss must take an Impact this attack");
        let mite_damage = impact_damage(&events, mite_id).expect("splash must reach the nearby Mite too");
        assert_eq!(
            boss_damage,
            tower::ember_splash_damage(TowerKind::EmberNest.base_stats().damage, true),
            "the primary hit on the boss must be full, unfalloff'd damage"
        );
        assert_eq!(
            mite_damage,
            tower::ember_splash_damage(TowerKind::EmberNest.base_stats().damage, false),
            "the splash's extra hit must land on the nearby Mite at the normal splash falloff, at 0 Mite armour"
        );
    }

    #[test]
    fn overgrowth_transfers_a_killed_bosss_overkill_to_the_next_chain_target() {
        // The boss's own remaining HP (5) is well under Prism's base
        // damage (18): the primary hit both kills it and produces a known,
        // exact overkill (18 - 5 = 13), which Overgrowth must carry into
        // the very next jump -- the nearby Mite reached by the chain.
        let (mut rig, _tower_id, boss_id) = LinkBurstRig::new(TowerKind::Prism, 0, 5);
        rig.runes.add(Rune::Overgrowth);
        let mite_id = rig.add_mite_at(8);
        let mut events = Vec::new();
        let fired = {
            let mut world = rig.world();
            fire_tower(0, false, false, false, None, &mut world, &mut events)
        };
        assert!(fired);

        assert_eq!(rig.boss_hp(), 0, "the boss's remaining 5 HP must be overkilled by the 18-damage primary hit");
        let boss_damage = impact_damage(&events, boss_id).expect("boss must take an Impact this attack");
        assert_eq!(boss_damage, 18, "the primary hit's raw damage (pre-clamp) must be the tower's full base damage");
        let overkill = boss_damage - 5;
        assert_eq!(overkill, 13, "overkill must be exactly the raw hit minus the boss's pre-hit HP");

        let mite_damage = impact_damage(&events, mite_id).expect("the chain must still reach the nearby Mite");
        let baseline = tower::prism_jump_damage(TowerKind::Prism.base_stats().damage, 1);
        assert_eq!(
            mite_damage,
            baseline + overkill,
            "Overgrowth must add the boss's exact overkill on top of the chain's normal jump-1 damage"
        );
    }

    /// Builds a `Simulation` sitting mid-wave-8 combat with a hand-placed,
    /// single-body boss one fixed-point unit from the very end of route 0's
    /// final leg -- "one route step away from a Heartseed breach", the
    /// exact situation `advance_enemy_movement` resolves every combat tick.
    /// The wave plan is otherwise already spent (no more spawns, no other
    /// enemies), so the very next real `advance` call both resolves this
    /// boss's fate AND checks wave completion in that same tick, exercising
    /// the real per-tick order (`resolve_tower_attacks`, then
    /// `advance_enemy_movement`, then `check_wave_completion` -- see
    /// `tick_combat`) rather than contriving several separate calls to fake
    /// it. Wave 8 specifically so a wave completion this tick resolves all
    /// the way to `RunOutcome::Won`, the strongest, most measurable signal
    /// that a same-tick kill was not also counted as a loss.
    fn wave_eight_boss_one_step_from_the_heartseed(boss_kind: BossKind, boss_max_hp: i32) -> (Simulation, EngineRng) {
        let mut sim = Simulation::new(21, PetBastionParams::new(Difficulty::Standard));
        let rng = EngineRng::seed(21);
        sim.wave = WAVE_COUNT;
        sim.phase = RunPhase::Combat;
        sim.wave_plan = Some(WavePlan {
            wave: WAVE_COUNT,
            spawns: Vec::new(),
            boss: None,
            boss_max_hp: 0,
            total_threat_spent: 0,
            budget_lo: 0,
            budget_hi: 0,
        });
        sim.boss_spawned_this_wave = true;

        let final_leg = sim.board.route(RouteId(0)).segments.len() - 1;
        let leg_length_fp = sim.board.route(RouteId(0)).segments[final_leg].length_fp;
        let boss_id = sim.id_alloc.next();
        let mut boss = Boss::new(boss_kind, boss_id, RouteId(0), boss_max_hp);
        boss.bodies[0].segment_index = final_leg;
        boss.bodies[0].offset_fp = leg_length_fp - 1;
        sim.boss = Some(boss);
        (sim, rng)
    }

    #[test]
    fn a_boss_killed_the_same_tick_it_takes_its_last_route_step_wins_the_run_not_loses_it() {
        // The defect this guards: `advance_enemy_movement` used to move
        // every boss body unconditionally, with no check for whether
        // `resolve_tower_attacks` -- which runs earlier in this very same
        // `tick_combat` tick -- had already brought its shared HP to zero.
        // A dead body one fixed-point unit from route 0's own end (see
        // `wave_eight_boss_one_step_from_the_heartseed`) would still
        // "arrive" and register `RunOutcome::Lost` before
        // `check_wave_completion` ever got a chance to see the boss as
        // defeated -- reproducing the reported bug's own signature (`mean
        // boss HP% remaining at death` reading 0.0-0.1% for
        // `circuit`/`slow_stack`: the boss is driven to zero HP and the run
        // is STILL lost, same tick).
        let (mut sim, mut rng) = wave_eight_boss_one_step_from_the_heartseed(BossKind::Bellkeeper, 10);
        // One Needle placed directly on the boss's own position -- in
        // range, unlinked, firing this exact tick (a freshly placed
        // tower's cooldown always starts at 0) -- deals exactly its 10
        // base damage (a boss's armour is always treated as 0, per
        // `fire_tower`'s own `TargetRef::Boss` arm), bringing the boss's HP
        // to exactly zero the same tick its last route step would
        // otherwise land.
        let tower_id = sim.id_alloc.next();
        let tower = Tower::new(tower_id, crate::board::HEARTSEED, TowerKind::Needle);
        sim.towers.push(tower);

        let events = sim.advance(&mut rng, &[]);

        assert_eq!(
            sim.is_finished(),
            Some(RunOutcome::Won),
            "a boss killed the same tick it reaches the Heartseed must win the run, not lose it"
        );
        assert!(
            events.iter().any(|e| matches!(e, SimEvent::RunWon)),
            "the tick that lands the killing blow and the last route step together must emit RunWon"
        );
        assert!(
            !events.iter().any(|e| matches!(e, SimEvent::RunLost)),
            "a boss already dead going into this tick's movement step must never emit RunLost"
        );
    }

    #[test]
    fn a_still_alive_boss_that_reaches_the_heartseed_loops_back_and_costs_integrity_instead_of_losing_the_run() {
        // The owner's own rule change: a boss reaching the Heartseed no
        // longer ends the run outright ("сейчас босс ваншотит"). Guarding
        // movement behind `Boss::is_defeated` must still never suppress a
        // genuine Heartseed arrival by a boss that is very much still
        // alive -- No tower is placed at all here, so
        // `resolve_tower_attacks` deals zero damage this tick, the same as
        // any ordinary undefended lane -- it must now cost
        // `BOSS_LAP_INTEGRITY_DAMAGE` and loop the body back to route 0's
        // own start instead of ending the run.
        let (mut sim, mut rng) = wave_eight_boss_one_step_from_the_heartseed(BossKind::NightMaw, 1000);
        let integrity_before = sim.integrity;

        let events = sim.advance(&mut rng, &[]);

        assert_eq!(
            sim.is_finished(),
            None,
            "a boss lap must not end the run by itself -- Standard's own Integrity budget survives one lap"
        );
        assert_eq!(
            sim.integrity,
            integrity_before - BOSS_LAP_INTEGRITY_DAMAGE,
            "a boss lap must cost exactly BOSS_LAP_INTEGRITY_DAMAGE"
        );
        assert!(
            matches!(
                &events.iter().find(|e| matches!(e, SimEvent::BossLap { .. })),
                Some(SimEvent::BossLap { kind: BossKind::NightMaw, integrity_remaining }) if *integrity_remaining == sim.integrity
            ),
            "a genuine Heartseed arrival by a live boss must emit BossLap with the post-charge Integrity"
        );
        assert!(
            !events.iter().any(|e| matches!(e, SimEvent::RunLost | SimEvent::RunWon)),
            "a boss lap alone must never end the run either way"
        );
        let boss = sim.boss.as_ref().expect("the boss must still be on the board after looping");
        assert_eq!(boss.bodies[0].segment_index, 0, "a looped boss body must sit back at its route's own first segment");
        assert_eq!(boss.bodies[0].offset_fp, 0, "a looped boss body must sit back at its route's own start offset");
    }

    /// A bare Combat-phase `Simulation` with an already-spent, empty wave
    /// plan and no towers/enemies/boss yet -- the shared starting point
    /// for the snapshot-fidelity tests below, each of which hand-places
    /// exactly the entities it needs via direct field access (the same
    /// discipline `wave_eight_boss_one_step_from_the_heartseed` already
    /// uses).
    fn bare_combat_sim(seed: u64) -> (Simulation, EngineRng) {
        let mut sim = Simulation::new(seed, PetBastionParams::new(Difficulty::Standard));
        let rng = EngineRng::seed(seed);
        sim.phase = RunPhase::Combat;
        sim.wave_plan = Some(WavePlan {
            wave: sim.wave,
            spawns: Vec::new(),
            boss: None,
            boss_max_hp: 0,
            total_threat_spent: 0,
            budget_lo: 0,
            budget_hi: 0,
        });
        sim.boss_spawned_this_wave = true;
        (sim, rng)
    }

    #[test]
    fn snapshot_tower_stats_reflect_the_upgrade_level_and_govern_the_real_hit() {
        // L3(Utility) is the one branch that actually changes Needle's
        // pierce (+1 over base) -- range/damage alone would not
        // distinguish "the snapshot used effective_stats" from "the
        // snapshot used base_stats", since a bug reverting to base stats
        // would still show SOME damage number, just the wrong one; pierce
        // against an armoured target makes the two cases land visibly
        // different dealt-damage totals.
        let (mut sim, mut rng) = bare_combat_sim(50);
        let tower_id = sim.id_alloc.next();
        let mut tower = Tower::new(tower_id, Tile::new(0, 3), TowerKind::Needle);
        tower.level = UpgradeLevel::L3(tower::UpgradeBranch::Utility);
        sim.towers.push(tower);

        // Shellback (3 armour) directly under the tower, well inside
        // Needle's base range.
        let enemy_id = sim.id_alloc.next();
        let mut enemy = Enemy::spawn(enemy_id, EnemyKind::Shellback, RouteId(0), 1000);
        enemy.offset_fp = tiles_to_fixed(0, 0);
        sim.enemies.push(enemy);

        let snap = sim.snapshot();
        let view = &snap.towers[0];
        let expected_stats = tower::effective_stats(TowerKind::Needle, UpgradeLevel::L3(tower::UpgradeBranch::Utility));
        assert_eq!(view.stats, expected_stats, "TowerView::stats must be exactly effective_stats(kind, level), not base_stats");
        assert_eq!(
            view.stats.pierce,
            TowerKind::Needle.base_stats().pierce + 1,
            "L3 Utility must report the real +1 pierce over base"
        );

        let events = sim.advance(&mut rng, &[]);
        let dealt = events
            .iter()
            .find_map(|e| match e {
                SimEvent::Impact { target, damage } if *target == enemy_id => Some(*damage),
                _ => None,
            })
            .expect("the tower must land a hit this tick");
        let expected_dealt = tower::apply_armour(view.stats.damage, EnemyKind::Shellback.base_stats().armour, view.stats.pierce, false);
        assert_eq!(
            dealt, expected_dealt,
            "the snapshot's own reported damage/pierce must be exactly what the real hit used"
        );
    }

    #[test]
    fn snapshot_tower_range_is_the_exact_cutoff_real_targeting_uses() {
        let (mut sim, mut rng) = bare_combat_sim(51);
        let tower_id = sim.id_alloc.next();
        let tower = Tower::new(tower_id, Tile::new(0, 3), TowerKind::Needle);
        sim.towers.push(tower);
        let range_fp = sim.snapshot().towers[0].stats.range_fp;

        // Both enemies sit on route 0's own first (y=3) leg, so their
        // `offset_fp` IS their absolute x -- and the tower sits at x=0 on
        // that same leg, so distance is exactly `offset_fp`, no
        // perpendicular component to account for.
        let in_range_id = sim.id_alloc.next();
        let mut in_range = Enemy::spawn(in_range_id, EnemyKind::Mite, RouteId(0), 1000);
        in_range.offset_fp = range_fp; // exactly at the boundary -- still eligible (d2 > range2 excludes, not >=).
        sim.enemies.push(in_range);

        let out_of_range_id = sim.id_alloc.next();
        let mut out_of_range = Enemy::spawn(out_of_range_id, EnemyKind::Mite, RouteId(0), 1000);
        out_of_range.offset_fp = range_fp + 1; // one fixed-point unit past the boundary.
        sim.enemies.push(out_of_range);

        let events = sim.advance(&mut rng, &[]);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SimEvent::Impact { target, .. } if *target == in_range_id)),
            "an enemy exactly at the snapshot's own reported range must be hit"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SimEvent::Impact { target, .. } if *target == out_of_range_id)),
            "an enemy one fixed-point unit past the snapshot's own reported range must not be hit"
        );
    }

    #[test]
    fn snapshot_shows_mirrors_resist_exactly_while_it_is_active_and_matching_the_hitting_family() {
        let (mut sim, mut rng) = bare_combat_sim(52);
        let tower_id = sim.id_alloc.next();
        let tower = Tower::new(tower_id, Tile::new(0, 3), TowerKind::Needle); // Physical family.
        sim.towers.push(tower);

        let mirror_id = sim.id_alloc.next();
        let mut mirror = Enemy::spawn(mirror_id, EnemyKind::Mirror, RouteId(0), 1000);
        mirror.offset_fp = tiles_to_fixed(1, 0);
        sim.enemies.push(mirror);

        let before = sim.snapshot();
        assert!(before.enemies[0].resist.is_none(), "must not show a resist before this enemy has ever been hit");
        assert!(before.enemies[0].last_hit_family.is_none());

        sim.advance(&mut rng, &[]);
        let after_hit = sim.snapshot();
        let mirror_view = after_hit.enemies.iter().find(|e| e.id == mirror_id).expect("Mirror survives one Needle hit");
        assert_eq!(mirror_view.last_hit_family, Some(DamageFamily::Physical));
        let resist = mirror_view.resist.expect("Mirror must show an active resist the instant it is hit");
        assert_eq!(resist.family, DamageFamily::Physical);
        // `note_hit` (inside `resolve_tower_attacks`) sets `ticks_remaining`
        // to the full `MIRROR_RESIST_TICKS`, but `advance_enemy_movement`'s
        // own `tick_status()` call -- later in this SAME `tick_combat` --
        // already decrements it once before this snapshot is taken.
        assert_eq!(resist.ticks_remaining, MIRROR_RESIST_TICKS as u32 - 1);

        // Remove the tower so nothing refreshes the resist, then run past
        // its whole window -- it must disappear from the snapshot exactly
        // when it actually stops applying, not linger or vanish early.
        sim.towers.clear();
        for _ in 0..=MIRROR_RESIST_TICKS {
            sim.advance(&mut rng, &[]);
        }
        let after_expiry = sim.snapshot();
        let mirror_view = after_expiry.enemies.iter().find(|e| e.id == mirror_id).expect("Mirror is still alive, untouched");
        assert!(mirror_view.resist.is_none(), "the resist must be gone once its own ticks_remaining has run out");
        // The hit record itself is not a timed effect -- it must persist.
        assert_eq!(mirror_view.last_hit_family, Some(DamageFamily::Physical));
    }

    #[test]
    fn snapshot_shows_a_boss_bodys_slow_exactly_while_a_bell_hit_is_keeping_it_active() {
        let (mut sim, mut rng) = bare_combat_sim(53);
        let boss_id = sim.id_alloc.next();
        sim.boss = Some(Boss::new(BossKind::Bellkeeper, boss_id, RouteId(0), 1000));

        let before = sim.snapshot().boss.expect("boss present");
        assert_eq!(before.bodies[0].slow_permille, 0, "an untouched boss body must show no slow");

        let tower_id = sim.id_alloc.next();
        // Dead on the boss body's own starting position.
        let tower = Tower::new(tower_id, Tile::new(0, 3), TowerKind::Bell);
        sim.towers.push(tower);

        sim.advance(&mut rng, &[]);
        let after = sim.snapshot().boss.expect("boss still present");
        let expected_slow = TowerKind::Bell.base_stats().slow_permille.expect("Bell always carries a slow");
        assert_eq!(
            after.bodies[0].slow_permille, expected_slow,
            "the snapshot must show the exact slow a real Bell hit just applied to this boss body"
        );
    }

    #[test]
    fn snapshot_current_wave_plan_matches_the_real_generated_plan_while_combat_runs() {
        let (mut sim, _rng) = bare_combat_sim(54);
        // `bare_combat_sim` seeds a wave plan whose own `wave` already
        // equals `sim.wave` -- the exact condition `Simulation::snapshot`
        // gates on -- so it must be visible immediately, unlike the
        // Build-phase case `tests.rs`'s own integration test covers.
        let snap = sim.snapshot();
        let plan = snap.wave_plan.expect("a plan generated for the CURRENT wave must be shown during Combat");
        assert_eq!(plan.wave, sim.wave);

        // Simulate reaching the next wave's Build phase without yet
        // regenerating a plan (`begin_combat` is the only place that
        // does) -- `wave_plan` still holds the old plan in memory, but it
        // must no longer be shown once `self.wave` has moved on.
        sim.wave += 1;
        sim.phase = RunPhase::Build { ticks_remaining: 1 };
        let snap = sim.snapshot();
        assert!(
            snap.wave_plan.is_none(),
            "a stale plan for an already-finished wave must never be shown as the current one"
        );
    }
}
