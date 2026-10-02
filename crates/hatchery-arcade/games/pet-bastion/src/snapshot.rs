//! Immutable, renderer-facing view of the current run. Owned data, never a
//! borrow of `Simulation` -- a caller can hold a `SimulationSnapshot` across
//! the next `advance` call.
//!
//! # What is a per-instance field here versus a per-kind lookup
//!
//! `Simulation::snapshot()` runs every tick and already clones a `Vec` per
//! entity category, so this module is deliberately selective about what it
//! copies onto each view: only state that is genuinely per-instance and
//! can change tick to tick (HP, position, active status effects, an
//! upgrade level chosen at runtime, ...) lives on `TowerView`/`EnemyView`/
//! `BossView`. State that is constant for every entity of a given kind for
//! the entire run (an enemy's base armour/speed/threat, say) is NOT
//! duplicated onto every one of that kind's instances here -- it is looked
//! up once, on demand, via the existing per-kind reference functions
//! (`EnemyKind::base_stats`, `TowerKind::base_stats`, `BossKind::base_hp`/
//! `BossKind::speed_fp`/`BossKind::phase_thresholds_permille`), all
//! already public. See each view struct's own doc comment for exactly
//! which fields this applies to.

use crate::board::BuildIneligibleReason;
use crate::boss::BossKind;
use crate::enemy::{EnemyKind, ResistEffect};
use crate::geometry::FixedPos;
use crate::ids::EntityId;
use crate::pet::{Evolution, PetCharge, PetState};
use crate::rune::Rune;
use crate::tower::{DamageFamily, TowerKind, TowerStats, UpgradeLevel};
use crate::wave::{Difficulty, WavePlan};
use crate::zone::{ZoneKind, ZonePolarity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunPhaseView {
    Build { ticks_remaining: u32 },
    Combat,
    RuneDraft,
    EvolutionChoice,
    PetChargeDraft,
    Victory,
    Defeat,
}

#[derive(Clone, Debug)]
pub struct TowerView {
    pub id: EntityId,
    pub kind: TowerKind,
    pub level: UpgradeLevel,
    pub position: (i32, i32),
    pub linked: bool,
    pub cooldown_ticks: u32,
    /// Damage family, damage, attack interval, range/dead-zone, pierce,
    /// splash radius, chain jumps and slow -- all at this tower's CURRENT
    /// level. Exactly `tower::effective_stats(kind, level)`, the same
    /// value `fire_tower` computes to resolve this tower's own attacks;
    /// not a second, independently-maintained copy.
    pub stats: TowerStats,
    /// Sap cost of this tower's next upgrade step (`tower::
    /// next_upgrade_cost`), or `None` if it is already at the max level
    /// (`UpgradeLevel::L3`).
    pub next_upgrade_cost: Option<i32>,
    /// Sap refund if sold right now (`tower::sell_price`, the same
    /// formula `Simulation::sell_tower` applies).
    pub sell_price: i32,
}

#[derive(Clone, Debug)]
pub struct EnemyView {
    pub id: EntityId,
    pub kind: EnemyKind,
    pub position: FixedPos,
    /// Absolute distance travelled along this enemy's own route so far --
    /// the same quantity targeting itself ranks by.
    pub route_progress_fp: i64,
    pub hp: i32,
    pub max_hp: i32,
    pub slow_permille: i64,
    pub stunned: bool,
    /// Damage family of the most recent hit landed on this enemy, if it
    /// has been hit at all yet this run (`Enemy::note_hit`).
    pub last_hit_family: Option<DamageFamily>,
    /// Mirror's own mechanic: a temporary resistance to whichever family
    /// just hit it. `Some` for exactly as long as it is actually applying
    /// (`Enemy::resist_permille_against`/`Enemy::tick_status` retire it to
    /// `None` the instant `ticks_remaining` reaches zero) -- never shown
    /// as active a tick longer than it really is. Always `None` for every
    /// kind other than Mirror, since nothing else ever sets it.
    ///
    /// This enemy's armour and base (unslowed) speed are NOT included
    /// here: both are fixed for the enemy's whole lifetime, identical
    /// across every instance of the same `kind` -- look them up once via
    /// `EnemyKind::base_stats(enemy.kind)` rather than paying to copy them
    /// onto every enemy, every tick.
    pub resist: Option<ResistEffect>,
}

/// One boss body's position and its own current slow -- bosses are
/// slowed exactly like any other enemy (e.g. a Bell hit calls the same
/// `BossBody::apply_slow` an `Enemy` uses), and Night Maw's two split
/// bodies can carry different slow stacks, so this is genuinely
/// per-body state, not a single boss-wide value.
///
/// `id` is a pure display-layer addition (`BossBody::id` already exists
/// internally; this only exposes it): a renderer that interpolates a
/// body's position between two snapshots (see
/// `hatchery-arcade-pet-bastion-render`'s own frame-interpolation
/// module) needs a stable key to match "the same body" across two ticks --
/// index-in-`bodies` alone is not stable across a Night Maw split, which
/// pushes a brand-new second body onto the end of the list.
#[derive(Clone, Copy, Debug)]
pub struct BossBodyView {
    pub id: EntityId,
    pub position: FixedPos,
    pub slow_permille: i64,
}

/// Bosses carry no armour and no per-family resistance in these rules:
/// every boss-damage arm in `sim.rs`'s own `fire_tower`/
/// `apply_link_burst_boss_hit` hardcodes armour at `0`, and nothing in
/// `boss.rs` ever gives a `Boss`/`BossBody` a Mirror-style resist. There is
/// nothing real to expose for either, so neither field exists here.
#[derive(Clone, Debug)]
pub struct BossView {
    pub kind: BossKind,
    pub hp: i32,
    pub max_hp: i32,
    /// `hp * 1000 / max_hp` -- `Boss::hp_permille`, the exact value every
    /// phase-threshold check in `boss.rs` reads against. See
    /// `BossKind::phase_thresholds_permille` for the threshold values
    /// themselves (static per kind, so looked up there rather than
    /// copied here).
    pub hp_permille: i64,
    pub bodies: Vec<BossBodyView>,
    pub final_phase: bool,
    /// Night Maw only -- always `false` for a Bellkeeper (`Boss` never
    /// sets it for that kind).
    pub split_triggered: bool,
    /// Bellkeeper only -- always `[false; 3]` for a Night Maw (`Boss`
    /// never sets any of these for that kind). Index order matches
    /// `BossKind::phase_thresholds_permille`'s own Bellkeeper array
    /// (75%/50%/25%).
    pub escort_triggered: [bool; 3],
}

/// One build-zone tile's current buildability, for a UI to highlight
/// where the player may build (or why a cell is off-limits). Free
/// placement (`board.rs`'s own module doc) means EVERY board tile appears
/// here -- there is no narrower proximity zone left to enumerate around
/// (see `Board::build_zone_cells`'s own doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildCellView {
    pub tile: (i32, i32),
    /// `None` means this tile is buildable right now.
    pub reason: Option<BuildIneligibleReason>,
}

#[derive(Clone, Debug)]
pub struct PetView {
    pub state: PetState,
    pub spark: u32,
    pub evolution: Option<Evolution>,
    pub linked_towers: Vec<EntityId>,
}

/// One of the current wave's two field modifiers (`zone.rs`'s own module
/// doc), pre-resolved to a tile rectangle so a renderer never needs to
/// import `zone.rs`'s own sector-grid geometry to draw the highlighted
/// area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldZoneView {
    pub kind: ZoneKind,
    pub polarity: ZonePolarity,
    /// `(x0, y0, x1, y1)`, `x` in `[x0, x1)`, `y` in `[y0, y1)`.
    pub tile_bounds: (i32, i32, i32, i32),
}

#[derive(Clone, Debug)]
pub struct SimulationSnapshot {
    pub tick_index: u64,
    pub difficulty: Difficulty,
    pub wave: u32,
    pub phase: RunPhaseView,
    pub sap: i32,
    pub integrity: i32,
    pub crab_shield: i32,
    pub towers: Vec<TowerView>,
    pub enemies: Vec<EnemyView>,
    pub boss: Option<BossView>,
    pub pet: PetView,
    pub rune_options: Vec<Rune>,
    pub runes_picked: Vec<Rune>,
    /// Options currently offered by a Pet Charge draft (`PetChargeDraft`
    /// phase), empty otherwise -- same shape as `rune_options`.
    pub pet_charge_options: Vec<PetCharge>,
    /// Every Pet Charge the player has picked so far this run -- same shape
    /// as `runes_picked`.
    pub pet_charges_picked: Vec<PetCharge>,
    /// This wave's two field modifiers (`zone.rs`), visible for exactly as
    /// long as `wave_plan` is (see that field's own doc: filtered by
    /// `wave == self.wave`, not by phase) -- always 0 or 2 entries, one
    /// [`ZoneKind::TowerDamage`] and one [`ZoneKind::EnemySpeed`].
    pub field_zones: Vec<FieldZoneView>,
    /// Every board tile (`Board::build_zone_cells`), each carrying
    /// `None` if `Command::Place` may land a tower there right now, or
    /// `Some(reason)` if not -- lets a UI highlight legal build cells and
    /// explain the ones that aren't, without recomputing route/anchor/
    /// occupancy geometry itself.
    pub build_cells: Vec<BuildCellView>,
    /// The active combat wave's full spawn plan (composition and timing),
    /// exactly as `wave::generate_wave` built it before spawning started
    /// -- see `Simulation::begin_combat`. `Some` only while
    /// `phase == RunPhaseView::Combat` for that same wave; `None` during
    /// `Build`/draft/evolution phases, since the plan for the NEXT wave is
    /// not generated until its own Build countdown finishes (generating
    /// it early would consume the run's one shared RNG stream out of
    /// order, changing the run). A genuine NEXT-wave preview -- seeing a
    /// wave's composition before its Build countdown ends -- is not
    /// available from these rules as they stand; it would need the wave
    /// generator itself to grow a separate, RNG-order-safe preview path.
    pub wave_plan: Option<WavePlan>,
}
