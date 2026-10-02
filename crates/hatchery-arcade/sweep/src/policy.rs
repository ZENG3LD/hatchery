//! Four `Policy<Simulation>` implementations driving the headless balance
//! sweep: [`BaselinePolicy`] (dumb -- spams one cheap tower kind, never
//! moves the pet, never upgrades), [`GreedyPolicy`] (a simple,
//! deterministic, non-optimal heuristic -- diversifies its build, upgrades
//! once the board is full, and repositions the pet toward wherever the
//! Circuit currently covers the most attacking towers), [`CircuitPolicy`]
//! (builds like `GreedyPolicy` but actually spends Spark --
//! `PetPulse`/`Blink`/`FullCircuit` -- and positions the pet for whatever is
//! CURRENTLY able to hit the boss, reacting to Bellkeeper's bell-silence
//! cadence and its 75/50/25% escort thresholds, but only ever buys Needle),
//! and [`SlowStackPolicy`] (keeps `CircuitPolicy`'s own Circuit/Spark/
//! anchor play, but replaces its Needle-only build with a real
//! geometry-and-slow-aware value ranking across every kind and every
//! upgrade -- the deliberate test of whether stacking Bell's slow can
//! multiply a boss's contact time enough to close the damage gap; see its
//! own doc and the `slow_boss_probe` tests below for the measured answer).
//! None of the four claims to be optimal play; they exist to give the sweep
//! observably-different strategies to compare balance bands against, and
//! [`ActionCounters`] on all four makes the CONTRAST itself measurable --
//! not just asserted -- in the sweep report.
//!
//! All four policies MUST still resolve `RuneDraft`/`EvolutionChoice` --
//! `Simulation::advance` does not auto-progress out of either phase (see
//! `sim.rs`'s own `tick_build`/`tick_combat` dispatch, which is a no-op for
//! `RunPhase::RuneDraft`/`EvolutionChoice`); a policy that never answers a
//! draft would leave the run stuck at that wave forever, not merely play
//! badly.
//!
//! # `ActionCounters` correctness (issued == applied)
//!
//! Every counter increment in this module sits at a call site that has
//! ALREADY reproduced the exact precondition `Simulation::apply_command`
//! (see `games/pet-bastion/src/sim.rs`) checks before mutating state for
//! that command -- pad-occupancy + Sap for `Place`, valid-step + Sap for
//! upgrades, `spark >= cost` for `Blink`/`PetPulse`/`FullCircuit`, "target
//! differs from current anchor" for `MovePet`. [`CircuitPolicy`] additionally
//! never emits more than ONE Spark-spending command per tick (`Blink`,
//! `PetPulse`, `FullCircuit` all draw the same `pet.spark` pool, and
//! `Simulation::advance` applies a tick's commands sequentially against
//! LIVE, not snapshot, state -- a second spark command in the same list
//! would be checked against a balance the snapshot never showed as already
//! spent). Given both of those, "the policy decided to emit this command"
//! and "the command actually took effect" are the same fact here, so the
//! counters need no separate before/after snapshot diff in the sweep's own
//! run loop.

use hatchery_arcade_engine::sweep_api::Policy;
use hatchery_arcade_pet_bastion::board::{AnchorId, Board, Route, RouteId, RouteSegment, ANCHOR_COUNT, HEARTSEED};
use hatchery_arcade_pet_bastion::boss::BossKind;
use hatchery_arcade_pet_bastion::command::Command;
use hatchery_arcade_pet_bastion::constants::{
    BELLKEEPER_BELL_INTERVAL_TICKS, BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE, BELLKEEPER_SILENCE_TICKS, BLINK_COST,
    FIXED_SCALE, FULL_CIRCUIT_COST, LINK_BURST_COOLDOWN_TICKS, MAX_COMBINED_SLOW_PERMILLE,
    MOONWELL_LINGER_TICK_DAMAGE_PERMILLE, PET_PULSE_COST, PET_PULSE_RADIUS_FP, SPARK_CAP, TICKS_PER_SECOND,
};
use hatchery_arcade_pet_bastion::enemy::EnemyKind;
use hatchery_arcade_pet_bastion::geometry::{FixedPos, Tile};
use hatchery_arcade_pet_bastion::pet::{Evolution, PetCharge, PetState};
use hatchery_arcade_pet_bastion::rune::Rune;
use hatchery_arcade_pet_bastion::sim::Simulation;
use hatchery_arcade_pet_bastion::snapshot::{RunPhaseView, SimulationSnapshot};
use hatchery_arcade_pet_bastion::tower::{self, TowerKind, UpgradeBranch, UpgradeLevel};
use hatchery_arcade_pet_bastion::wave::is_boss_wave;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyKind {
    Baseline,
    Greedy,
    Circuit,
    SlowStack,
}

impl std::fmt::Display for PolicyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyKind::Baseline => write!(f, "baseline"),
            PolicyKind::Greedy => write!(f, "greedy"),
            PolicyKind::Circuit => write!(f, "circuit"),
            PolicyKind::SlowStack => write!(f, "slow_stack"),
        }
    }
}

/// Real, observable action counts for one run, incremented at the exact
/// point a policy DECIDES to emit a command (see the module doc's
/// issued-equals-applied argument). The sweep report prints these per
/// policy so a policy that never touches the Living Circuit shows it in
/// the numbers, not just in the source -- the diagnostic contrast this
/// whole sweep exists to produce.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActionCounters {
    pub towers_placed: u32,
    /// Of `towers_placed`, how many were specifically `TowerKind::Bell` --
    /// the one kind whose value depends entirely on its 35% slow (its raw
    /// dps/cost is the worst on the roster, see the module-level note above
    /// `ESCORT_RESPONSE_TICKS`), so this is the direct, per-run, per-policy
    /// readout of whether
    /// a policy actually chose to stack slow, not just whether its source
    /// code is capable of it.
    pub bell_placed: u32,
    /// The remaining four attacking kinds, broken out the same way --
    /// together with `bell_placed` this is a full per-kind build census, so
    /// "did this policy actually value Prism/Moonwell instead of only
    /// Needle" (the task's own ask) is a number in the report, not a claim
    /// about the source code.
    pub needle_placed: u32,
    pub prism_placed: u32,
    pub embernest_placed: u32,
    pub moonwell_placed: u32,
    pub upgrades_l2: u32,
    pub upgrades_l3: u32,
    pub move_pet: u32,
    pub blink: u32,
    pub pet_pulse: u32,
    pub full_circuit: u32,
}

impl ActionCounters {
    /// Records one `Command::Place` decision: `towers_placed` plus the
    /// per-kind breakdown (Relay is deliberately uncounted per-kind -- none
    /// of the four policies in this module ever place one).
    fn record_placement(&mut self, kind: TowerKind) {
        self.towers_placed += 1;
        match kind {
            TowerKind::Bell => self.bell_placed += 1,
            TowerKind::Needle => self.needle_placed += 1,
            TowerKind::Prism => self.prism_placed += 1,
            TowerKind::EmberNest => self.embernest_placed += 1,
            TowerKind::Moonwell => self.moonwell_placed += 1,
            TowerKind::Relay => {}
        }
    }
}

/// Occupied tile coordinates, in the exact `(i32, i32)` shape `Board::
/// open_build_tiles` compares against -- shared by every policy below that
/// needs to know which build-zone cells are already spoken for.
fn occupied_tiles(snapshot: &SimulationSnapshot) -> Vec<(i32, i32)> {
    snapshot.towers.iter().map(|t| t.position).collect()
}

/// Finds the lowest-`(y, x)`-ordered buildable tile not currently occupied
/// by any placed tower -- `Board::open_build_tiles`'s own row-major
/// enumeration order (`board.rs`'s `compute_near_route`), the free-
/// placement replacement for the old "lowest-numbered empty pad" rule.
fn first_empty_tile(snapshot: &SimulationSnapshot, board: &Board) -> Option<Tile> {
    board.open_build_tiles(&occupied_tiles(snapshot)).into_iter().next()
}

fn current_anchor(snapshot: &SimulationSnapshot) -> Option<AnchorId> {
    match snapshot.pet.state {
        PetState::AtAnchor(anchor) => Some(anchor),
        PetState::Moving { .. } => None,
    }
}

/// The dumb baseline: spends Sap on the cheapest tower kind (Needle) on the
/// lowest-`(y, x)`-ordered free buildable tile whenever affordable, never
/// upgrades, never sells, and never moves the pet away from its starting
/// anchor (anchor 0) -- any Circuit benefit it gets is purely incidental,
/// from towers that happen to land near the pet's starting position.
#[derive(Default)]
pub struct BaselinePolicy {
    pub counters: ActionCounters,
    board: Board,
}

impl Policy<Simulation> for BaselinePolicy {
    fn decide(&mut self, snapshot: &SimulationSnapshot, _tick_index: u64) -> Vec<Command> {
        match snapshot.phase {
            RunPhaseView::RuneDraft => {
                // Simplest possible rule: take whatever is offered first.
                return snapshot.rune_options.first().map(|&r| vec![Command::DraftRune(r)]).unwrap_or_default();
            }
            RunPhaseView::EvolutionChoice => {
                // A fixed, arbitrary, documented pick -- no reasoning applied.
                return vec![Command::ChooseEvolution(Evolution::Crab)];
            }
            RunPhaseView::PetChargeDraft => {
                // Same "take whatever is offered first" rule as its own
                // RuneDraft handling above -- no reasoning applied.
                return snapshot.pet_charge_options.first().map(|&c| vec![Command::DraftPetCharge(c)]).unwrap_or_default();
            }
            RunPhaseView::Victory | RunPhaseView::Defeat => return Vec::new(),
            RunPhaseView::Build { .. } | RunPhaseView::Combat => {}
        }

        let cost = TowerKind::Needle.base_stats().cost;
        if snapshot.sap < cost {
            return Vec::new();
        }
        match first_empty_tile(snapshot, &self.board) {
            Some(tile) => {
                self.counters.record_placement(TowerKind::Needle);
                vec![Command::Place { tile, kind: TowerKind::Needle }]
            }
            None => Vec::new(),
        }
    }
}

/// Build-order rotation: cheap pierce DPS first (Needle), then splash
/// (EmberNest) and slow/support (Bell) to handle swarms, then Prism's chain
/// damage, then the two priciest/most situational roles (Moonwell's heavy
/// hit, Relay's pure Circuit-extension with no direct damage) last. Ten
/// pads and a six-kind rotation means the first full board is
/// 2xNeedle/2xEmberNest/2xBell/2xPrism/1xMoonwell/1xRelay -- a deliberately
/// simple, deterministic diversification rule, not a cost/DPS-optimised
/// build order.
///
/// **This is a PREFERENCE order, not a hard gate**: [`choose_kind`] falls
/// back to the cheapest currently-affordable kind whenever the rotation's
/// own next pick is unaffordable, rather than leaving Sap idle. An earlier
/// version of this policy gated strictly on the rotation (never buying
/// anything but the exact next kind), which left Sap sitting unspent for
/// many ticks waiting for an expensive kind's exact cost -- an own-goal
/// found via this crate's headless sweep itself: `hatchery-arcade-pet-
/// bastion/src/tests.rs`'s own `boss_phase_transitions_fire_over_a_real_
/// wave_four_fight` test comment states plainly "full pad coverage is"
/// what beats a route-progress-first targeting boss, not kind diversity;
/// idling Sap directly starves coverage.
const BUILD_ROTATION: [TowerKind; 6] =
    [TowerKind::Needle, TowerKind::EmberNest, TowerKind::Bell, TowerKind::Prism, TowerKind::Moonwell, TowerKind::Relay];

/// Picks which kind to place THIS tick: the rotation's own next pick
/// (`towers.len() % BUILD_ROTATION.len()`) if affordable, otherwise the
/// cheapest kind in the rotation the current Sap balance can actually
/// afford. `None` only when nothing in the rotation is affordable at all.
fn choose_kind(snapshot: &SimulationSnapshot) -> Option<TowerKind> {
    let desired = BUILD_ROTATION[snapshot.towers.len() % BUILD_ROTATION.len()];
    if snapshot.sap >= desired.base_stats().cost {
        return Some(desired);
    }
    BUILD_ROTATION.iter().copied().filter(|k| snapshot.sap >= k.base_stats().cost).min_by_key(|k| k.base_stats().cost)
}

/// Squared tile distance from `tile` to the NEAREST already-placed tower
/// (`i64::MAX` when the board is still empty, so the very first placement
/// has no dispersion preference to break ties with).
fn dispersion_from_towers(tile: Tile, snapshot: &SimulationSnapshot) -> i64 {
    snapshot
        .towers
        .iter()
        .map(|t| {
            let dx = (t.position.0 - tile.x) as i64;
            let dy = (t.position.1 - tile.y) as i64;
            dx * dx + dy * dy
        })
        .min()
        .unwrap_or(i64::MAX)
}

/// A tile's combined placement score: spread from already-placed towers
/// (`dispersion_from_towers`, so early towers don't cluster in one corner
/// of the map) MINUS its squared distance to `HEARTSEED` -- both routes
/// merge and every boss body always walks route 0 toward that exact tile
/// (`board.rs`'s own `Route::build` waypoints, confirmed: both routes'
/// last leg ends at `HEARTSEED`; `sim.rs`'s own `spawn_due_enemies` always
/// constructs `Boss::new(kind, id, RouteId(0), ...)`), so it is the one
/// point on the map every enemy AND every boss body is guaranteed to pass
/// near. Subtracting rewards tiles closer to that choke, which the
/// dispersion term alone does not know to prefer -- pure dispersion picks
/// the single farthest-apart layout, which can spread towers too thin near
/// the one location that matters most for actually killing a boss before
/// it reaches the Heartseed.
fn tile_placement_score(tile: Tile, snapshot: &SimulationSnapshot) -> i64 {
    let dx = (tile.x - HEARTSEED.x) as i64;
    let dy = (tile.y - HEARTSEED.y) as i64;
    let dist2_to_heartseed = dx * dx + dy * dy;
    dispersion_from_towers(tile, snapshot).saturating_sub(dist2_to_heartseed)
}

/// Picks the open build tile with the highest [`tile_placement_score`] --
/// spread out from the existing build, but biased toward the route-merge/
/// Heartseed choke every enemy and every boss body must pass near, instead
/// of filling the build zone in raw row-major order (which would otherwise
/// cluster every early tower in one corner of the 28x14 board, far from
/// both anchors and the choke) -- the free-placement replacement for the
/// old fixed-pad table's own `best_empty_pad`.
fn best_build_tile(snapshot: &SimulationSnapshot, board: &Board) -> Option<Tile> {
    board
        .open_build_tiles(&occupied_tiles(snapshot))
        .into_iter()
        .max_by_key(|&tile| tile_placement_score(tile, snapshot))
}

/// A simple, deterministic, non-optimal greedy policy: diversifies its
/// build via [`BUILD_ROTATION`], spends leftover Sap on upgrades once the
/// board is full, and repositions the pet toward whichever anchor
/// currently covers the most placed attacking towers by an
/// inverse-square-distance score (an approximation of the engine's own
/// nearest-N Circuit selection, not a reimplementation of it -- the
/// engine's `pet::compute_linked_towers` is the actual authority on which
/// towers end up linked; this score only decides where the POLICY chooses
/// to send the pet).
#[derive(Default)]
pub struct GreedyPolicy {
    pub counters: ActionCounters,
    board: Board,
}

/// Rune draft priority, most-preferred first: Overgrowth (avoids wasted
/// overkill on this policy's own splash/chain-heavy build), Symbiosis
/// (rewards a diversified roster, which `BUILD_ROTATION` always produces),
/// Phase (counters the armoured Shellback/Splitter/Husher/Mirror roster
/// from wave 5 on), Echo (single-target value only), Anchor (least useful
/// to a policy that repositions the pet every tick rather than settling).
const GREEDY_RUNE_PRIORITY: [Rune; 5] = [Rune::Overgrowth, Rune::Symbiosis, Rune::Phase, Rune::Echo, Rune::Anchor];

fn pick_greedy_rune(options: &[Rune]) -> Option<Rune> {
    GREEDY_RUNE_PRIORITY.iter().copied().find(|r| options.contains(r)).or_else(|| options.first().copied())
}

/// `GreedyPolicy`'s own Pet Charge priority, most-preferred first: Fang
/// (flat damage, the most universally useful of the four against this
/// policy's deliberately simple, kind-diversified build --
/// `BUILD_ROTATION`'s own doc), Bloom (splash/chain reach helps its own
/// Prism/Ember Nest/Moonwell share of the rotation), Surge (attack speed,
/// still useful but the least differentiated of the three numeric picks
/// for a build that is not concentrated on one kind), Attune (situational
/// -- Mirror only appears from wave 7 on, and this policy has no dedicated
/// Mirror response elsewhere either).
const GREEDY_PET_CHARGE_PRIORITY: [PetCharge; 4] = [PetCharge::Fang, PetCharge::Bloom, PetCharge::Surge, PetCharge::Attune];

/// `CircuitPolicy`'s own Pet Charge priority: Surge (compounds directly
/// with Needle's already-high base fire rate -- this policy's whole build
/// is one kind, so attack speed scales its entire investment at once),
/// Fang (flat damage, the same universal value Greedy ranks first), Attune
/// (defeats Mirror's resist from wave 7 on, real but late), Bloom (dead
/// weight for this policy -- Needle has no splash/chain for it to widen,
/// see [`PetCharge::Bloom`]'s own doc).
const CIRCUIT_PET_CHARGE_PRIORITY: [PetCharge; 4] = [PetCharge::Surge, PetCharge::Fang, PetCharge::Attune, PetCharge::Bloom];

/// `SlowStackPolicy`'s own Pet Charge priority: Bloom (amplifies its own
/// splash/chain-heavy build -- Prism/Ember Nest/Moonwell -- directly, the
/// one pick every other policy here ranks low or last), then the same
/// universal-then-situational order the other two diversified policies
/// use (Fang, Surge, Attune).
const SLOWSTACK_PET_CHARGE_PRIORITY: [PetCharge; 4] = [PetCharge::Bloom, PetCharge::Fang, PetCharge::Surge, PetCharge::Attune];

fn pick_pet_charge(priority: &[PetCharge], options: &[PetCharge]) -> Option<PetCharge> {
    priority.iter().copied().find(|c| options.contains(c)).or_else(|| options.first().copied())
}

/// Inverse-square-distance Circuit coverage score for one anchor: every
/// placed attacking tower contributes `100_000 / (1 + squared tile
/// distance)`, so nearer towers count for more without a hard, arbitrarily
/// chosen coverage radius.
fn anchor_coverage_score(snapshot: &SimulationSnapshot, anchor: AnchorId) -> i64 {
    let tile = Board::anchor_tile(anchor);
    snapshot
        .towers
        .iter()
        .filter(|t| t.kind.attacks())
        .map(|t| {
            let dx = (t.position.0 - tile.x) as i64;
            let dy = (t.position.1 - tile.y) as i64;
            100_000 / (1 + dx * dx + dy * dy)
        })
        .sum()
}

fn best_anchor(snapshot: &SimulationSnapshot) -> AnchorId {
    (0..ANCHOR_COUNT as u8)
        .map(AnchorId)
        .max_by_key(|&a| anchor_coverage_score(snapshot, a))
        .unwrap_or(AnchorId(0))
}

impl Policy<Simulation> for GreedyPolicy {
    fn decide(&mut self, snapshot: &SimulationSnapshot, _tick_index: u64) -> Vec<Command> {
        match snapshot.phase {
            RunPhaseView::RuneDraft => {
                return pick_greedy_rune(&snapshot.rune_options).map(|r| vec![Command::DraftRune(r)]).unwrap_or_default();
            }
            RunPhaseView::EvolutionChoice => {
                // Moth links one more tower than the base Circuit -- matches
                // this policy's own diversified, spread-out build.
                return vec![Command::ChooseEvolution(Evolution::Moth)];
            }
            RunPhaseView::PetChargeDraft => {
                return pick_pet_charge(&GREEDY_PET_CHARGE_PRIORITY, &snapshot.pet_charge_options)
                    .map(|c| vec![Command::DraftPetCharge(c)])
                    .unwrap_or_default();
            }
            RunPhaseView::Victory | RunPhaseView::Defeat => return Vec::new(),
            RunPhaseView::Build { .. } | RunPhaseView::Combat => {}
        }

        let mut commands = Vec::new();

        if let Some(tile) = best_build_tile(snapshot, &self.board) {
            if let Some(kind) = choose_kind(snapshot) {
                commands.push(Command::Place { tile, kind });
                self.counters.record_placement(kind);
            }
        } else if let Some(tower) = snapshot
            .towers
            .iter()
            .filter(|t| t.kind.attacks() && t.level == UpgradeLevel::Base)
            .min_by_key(|t| t.id)
        {
            let cost = UpgradeLevel::L2.step_cost(tower.kind.base_stats().cost);
            if snapshot.sap >= cost {
                commands.push(Command::UpgradeToL2 { tower: tower.id });
                self.counters.upgrades_l2 += 1;
            }
        } else if let Some(tower) =
            snapshot.towers.iter().filter(|t| t.kind.attacks() && t.level == UpgradeLevel::L2).min_by_key(|t| t.id)
        {
            let cost = UpgradeLevel::L3(UpgradeBranch::Power).step_cost(tower.kind.base_stats().cost);
            if snapshot.sap >= cost {
                commands.push(Command::UpgradeToL3 { tower: tower.id, branch: UpgradeBranch::Power });
                self.counters.upgrades_l3 += 1;
            }
        }

        if let Some(here) = current_anchor(snapshot) {
            let target = best_anchor(snapshot);
            if target != here {
                commands.push(Command::MovePet { anchor: target });
                self.counters.move_pet += 1;
            }
        }

        commands
    }
}

// ---------------------------------------------------------------------------
// CircuitPolicy
// ---------------------------------------------------------------------------

/// Rune draft priority shared by [`CircuitPolicy`] and [`SlowStackPolicy`]
/// (both call [`pick_circuit_rune`]), most-preferred first: Echo (a free
/// bonus attack from another same-kind tower every 4th hit -- applies to
/// boss damage too, since `fire_tower`'s Echo trigger does not check
/// `linked`/target-kind), Overgrowth (transfers a killed boss's own
/// overkill into the next chain/splash target, and does the same for
/// regular enemies -- `fire_tower`'s `TargetRef::Boss` arm now computes
/// this exactly like its `TargetRef::Enemy` sibling; genuinely valuable for
/// [`SlowStackPolicy`]'s Prism/Ember Nest/Moonwell build, though dead
/// weight for [`CircuitPolicy`] specifically, whose Needle-only build
/// ([`best_circuit_build_action`]) never has a chain/splash target to
/// transfer into either way -- ranked here for the shared list's better-informed
/// user), Phase (armour-ignoring hits, relevant from wave 5's armoured
/// roster on), Anchor (this policy repositions deliberately, not every
/// tick, so lingering buffs after a move are actually worth something).
/// Symbiosis is LAST on purpose, not by omission: its adjacency check
/// requires two attacking towers within 1.5 tiles (`SYMBIOSIS_ADJACENCY_
/// FP`), and neither this policy's own build ranking
/// ([`best_circuit_build_action`]) nor [`SlowStackPolicy`]'s
/// ([`best_build_action`]) ever scores adjacency itself -- both rank
/// candidate tiles purely by expected damage-per-Sap, so any Symbiosis
/// trigger is incidental (two high-value tiles happening to land within
/// 1.5 tiles of each other), never a deliberate goal either build ranking
/// pursues. Since free placement (unlike the old fixed pad table, whose
/// every pair sat at least 3 tiles apart) makes that incidental case
/// genuinely reachable, Symbiosis is no longer a guaranteed dead pick --
/// just still the one option here neither policy's own value ranking
/// actively seeks out, so it stays last among the two that ARE always
/// live for this build ([`Rune::Echo`], [`Rune::Overgrowth`]) and the two
/// pet-side ones ([`Rune::Phase`], [`Rune::Anchor`]).
const CIRCUIT_RUNE_PRIORITY: [Rune; 5] = [Rune::Echo, Rune::Overgrowth, Rune::Phase, Rune::Anchor, Rune::Symbiosis];

fn pick_circuit_rune(options: &[Rune]) -> Option<Rune> {
    CIRCUIT_RUNE_PRIORITY.iter().copied().find(|r| options.contains(r)).or_else(|| options.first().copied())
}

// Needle's own case for staying `CircuitPolicy`'s only kind: it wins the
// RAW single-target damage-per-second-per-Sap race at base stats (11.76
// DPS / 60 Sap = 0.196, vs EmberNest 12.3/70 = 0.176, Prism 15/85 = 0.176,
// Moonwell 13.3/100 = 0.133, Bell 3/50 = 0.06) -- cheaper AND more
// efficient per hit, which also means more total copies (and more,
// cheaper, per-copy upgrades) from the same fixed pre-wave-4 Sap budget,
// AND the single LARGEST Link Burst payout of the four boss-reaching
// kinds (3 full-damage shots, vs one primary hit for Prism or one field
// tick for Ember Nest/Moonwell). A diagnostic trace against a real
// Standard-difficulty wave-4 build (`hatchery-arcade-sweep`'s own
// `probe_diag.rs`, not shipped) showed `GreedyPolicy`'s own `choose_kind`
// falling back to Bell (its own cheapest-affordable fallback, usually the
// single worst boss-DPS-per-Sap kind on the roster) for 3 of 5 built
// towers; this policy just never buys it for boss purposes, keeping it
// the deliberately SIMPLE Circuit-using policy contrasted against
// `SlowStackPolicy`'s real geometry-and-value-ranked, multi-kind build
// (`best_build_action`).

/// How many ticks after a freshly-crossed Bellkeeper escort threshold
/// (75/50/25% HP -- `BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE`) this policy
/// blends enemy coverage into its anchor choice, on top of boss coverage --
/// long enough for the 3 escort Skitters (18_000 fp/s = 1.8 tiles/s, board
/// board.rs's `EnemyKind::Skitter` speed) to cross a meaningful stretch of
/// the opposite route, short enough that the policy snaps back to
/// boss-only coverage well before the run naturally revisits this branch.
const ESCORT_RESPONSE_TICKS: u32 = 100;

/// Minimum number of Needles [`best_circuit_build_action`]'s own
/// lane-parity gate guarantees on route 1 once route 0 has already
/// received at least this many -- see that function's own doc for the
/// empirical calibration between "no floor" (leak-drained losses) and "a
/// floor that matches route 0 1:1" (undamaged-boss losses) this value sits
/// between.
const CIRCUIT_ROUTE1_MIN_NEEDLES: usize = 1;
/// Route 0 gets priority for its first this-many Needles (its own core
/// boss cluster, pads 0/1/4/5) before the lane-parity gate ever engages --
/// see [`best_circuit_build_action`]'s own doc for the empirical trade-off
/// this sits on.
const CIRCUIT_ROUTE0_PRIORITY_NEEDLES: usize = 3;

/// Bellkeeper's bell-timer, re-derived from `boss.rs`'s own
/// `run_boss_abilities` Bellkeeper branch and confirmed empirically (a
/// one-off trace against a real run, matching this crate's own working
/// discipline of tracing before trusting a hand-derivation): the cooldown
/// reset happens on the SAME call that fires silence -- that call does NOT
/// also decrement the freshly-reset cooldown -- so one full silence cycle
/// is `BELLKEEPER_BELL_INTERVAL_TICKS + 1` boss-ability calls long, not
/// `BELLKEEPER_BELL_INTERVAL_TICKS`, and the FIRST silence-triggering call
/// is call number `BELLKEEPER_BELL_INTERVAL_TICKS + 1` (not
/// `BELLKEEPER_BELL_INTERVAL_TICKS`). Traced via a Moonwell tower
/// deliberately kept outside the Circuit (long range, so it has the boss
/// in sight for nearly its whole route; unlinked, so it goes fully silent
/// -- not just slower -- for exactly `BELLKEEPER_SILENCE_TICKS`): its
/// `cooldown_ticks` froze at a constant value for exactly 40 consecutive
/// ticks starting at boss-ability call 603 = `201 * 3`, i.e. exactly the
/// THIRD multiple of the 201-call cycle this derivation predicts.
const BELL_CYCLE_CALLS: i64 = BELLKEEPER_BELL_INTERVAL_TICKS as i64 + 1;
const BELL_SILENCE_START_CALL: i64 = BELL_CYCLE_CALLS;
/// How many boss-ability calls ahead of a predicted silence window this
/// policy starts trying to cast Full Circuit. `FULL_CIRCUIT_TICKS` (80)
/// comfortably outlasts `BELLKEEPER_SILENCE_TICKS` (40) even cast this
/// early, and [`bellkeeper_silence_window_active`] stays true for the
/// whole lead-in + silence span, so the policy keeps retrying every tick
/// the window is open rather than needing to land on one exact tick.
const BELL_PRECAST_LEAD_CALLS: i64 = 15;

/// Maps a `decide()`-visible `tick_index` to the boss-ability "call
/// number" (1-indexed, relative to the boss's spawn) that `Simulation::
/// advance` will run THIS tick: the tick at which this policy first
/// observed the boss (`first_seen_tick`) is always exactly ONE tick after
/// the boss's actual spawn tick (`Simulation::snapshot` is read BEFORE
/// that tick's own `advance`, and the boss is created and gets its first
/// ability call inside the SAME `advance` call that starts combat), and
/// the boss's own FIRST ability call happens on its spawn tick itself --
/// so call number `n(t) = t - first_seen_tick + 2`.
fn bellkeeper_call_number(tick_index: u64, first_seen_tick: u64) -> i64 {
    tick_index as i64 - first_seen_tick as i64 + 2
}

/// True while now is either inside a predicted Bellkeeper silence window
/// or within [`BELL_PRECAST_LEAD_CALLS`] calls of one starting.
fn bellkeeper_silence_window_active(tick_index: u64, first_seen_tick: u64) -> bool {
    let n = bellkeeper_call_number(tick_index, first_seen_tick);
    let phase = (n - BELL_SILENCE_START_CALL).rem_euclid(BELL_CYCLE_CALLS);
    let silence_ticks = BELLKEEPER_SILENCE_TICKS as i64;
    phase < silence_ticks || phase >= BELL_CYCLE_CALLS - BELL_PRECAST_LEAD_CALLS
}

fn hp_permille(hp: i32, max_hp: i32) -> i64 {
    if max_hp <= 0 {
        0
    } else {
        (hp as i64 * 1000) / max_hp as i64
    }
}

/// True if a Pet Pulse cast from `anchor` right now would land on at least
/// one enemy or boss body -- the "meaningful moment" gate the task asks
/// for, instead of casting on cooldown regardless of whether anything is
/// actually in the 2.5-tile radius.
fn pulse_would_land(snapshot: &SimulationSnapshot, anchor: AnchorId) -> bool {
    let origin = Board::anchor_tile(anchor).to_fixed();
    let radius2 = PET_PULSE_RADIUS_FP * PET_PULSE_RADIUS_FP;
    let hits_enemy = snapshot.enemies.iter().any(|e| origin.dist2(e.position) <= radius2);
    let hits_boss = snapshot
        .boss
        .as_ref()
        .map(|b| b.bodies.iter().any(|body| origin.dist2(body.position) <= radius2))
        .unwrap_or(false);
    hits_enemy || hits_boss
}

/// Inverse-square-distance coverage score for `anchor` over exactly the
/// attacking towers that could CURRENTLY hit at least one of `targets`
/// (their own effective range/min-range, recomputed per tower kind+level
/// via `tower::effective_stats` -- not a flat radius guess). Zero when no
/// placed tower is currently capable of reaching any target.
fn anchor_target_coverage_score(snapshot: &SimulationSnapshot, anchor: AnchorId, targets: &[FixedPos]) -> i64 {
    if targets.is_empty() {
        return 0;
    }
    let anchor_pos = Board::anchor_tile(anchor).to_fixed();
    snapshot
        .towers
        .iter()
        .filter(|t| t.kind.attacks())
        .filter_map(|t| {
            let stats = tower::effective_stats(t.kind, t.level);
            let tower_pos = Tile::new(t.position.0, t.position.1).to_fixed();
            let range2 = stats.range_fp * stats.range_fp;
            let min2 = stats.min_range_fp.map(|m| m * m);
            let in_range = targets.iter().any(|&p| {
                let d2 = tower_pos.dist2(p);
                d2 <= range2 && min2.map(|m| d2 >= m).unwrap_or(true)
            });
            if in_range {
                Some(100_000 / (1 + anchor_pos.dist2(tower_pos)))
            } else {
                None
            }
        })
        .sum()
}

/// Anchor selection for [`CircuitPolicy`]: while a boss is present, cover
/// whatever is CURRENTLY able to hit it -- and, for a short window after a
/// fresh escort threshold ([`ESCORT_RESPONSE_TICKS`]), blend in coverage of
/// every live enemy too, since that is the concrete moment Skitters start
/// approaching from the opposite entrance (`sim.rs`'s own
/// `run_boss_abilities`: `opposite = self.board.opposite_route(boss.
/// bodies[0].route)`, always route 1 since a boss always spawns on route
/// 0). Falls back to [`best_anchor`]'s general tower-coverage score
/// whenever nothing is currently in range of a target (early in a boss
/// wave, or entirely outside a boss wave) -- never leaves the pet
/// stranded on a zero-coverage anchor just because a boss target list was
/// momentarily empty.
/// Updates `last_seen_anchor`/`ticks_since_arrival` from the pet's CURRENT
/// snapshot state -- resets the dwell counter on every genuine arrival (the
/// anchor the pet is AT differs from the last one recorded), and just
/// counts ticks while parked. Left untouched while the pet is mid-`Moving`
/// (`current_anchor` returns `None` then): the dwell counter only needs to
/// be accurate once the pet is somewhere to dwell at again, and a stale
/// value from before the move is about to be overwritten by the fresh
/// arrival anyway.
fn track_anchor_dwell(snapshot: &SimulationSnapshot, last_seen_anchor: &mut Option<AnchorId>, ticks_since_arrival: &mut u32) {
    if let Some(here) = current_anchor(snapshot) {
        if *last_seen_anchor == Some(here) {
            *ticks_since_arrival = ticks_since_arrival.saturating_add(1);
        } else {
            *last_seen_anchor = Some(here);
            *ticks_since_arrival = 0;
        }
    }
}

/// While a boss is present, Link Burst only fires again on a genuine NEW
/// arrival (`sim.rs`'s own `on_pet_arrival` -- the `+30%` attack-speed bonus
/// is recomputed every tick with no arrival needed, but the burst itself is
/// not), so a policy that settles on one best-coverage anchor for a whole
/// boss fight only ever bursts that anchor's towers ONCE. Once the
/// 6-second cooldown ([`LINK_BURST_COOLDOWN_TICKS`]) has had time to clear
/// since the pet last arrived at `here`, this returns a DIFFERENT anchor to
/// step to instead of the natural (unchanged) target -- a deliberate
/// "leave and come back", the plan's own explicit purpose for the Living
/// Circuit ("Its purpose is to test Circuit movement, not only total
/// damage"). The step away is short: one `MovePet` there, then the normal
/// coverage-scoring logic naturally walks the pet straight back once it
/// arrives (the real target anchor's own coverage score has not changed),
/// landing a genuinely fresh `PetArrived` -- hence a fresh Link Burst -- on
/// that return trip.
fn link_burst_refresh_target(
    snapshot: &SimulationSnapshot,
    here: AnchorId,
    natural_target: AnchorId,
    ticks_since_arrival: u32,
) -> AnchorId {
    if snapshot.boss.is_some() && natural_target == here && ticks_since_arrival >= LINK_BURST_COOLDOWN_TICKS as u32 {
        AnchorId((here.0 + 1) % ANCHOR_COUNT as u8)
    } else {
        natural_target
    }
}

fn pick_circuit_anchor(snapshot: &SimulationSnapshot, include_enemy_coverage: bool) -> AnchorId {
    let mut targets: Vec<FixedPos> = Vec::new();
    if let Some(boss) = &snapshot.boss {
        targets.extend(boss.bodies.iter().map(|body| body.position));
    }
    if include_enemy_coverage {
        targets.extend(snapshot.enemies.iter().map(|e| e.position));
    }
    if !targets.is_empty() {
        if let Some((anchor, score)) = (0..ANCHOR_COUNT as u8)
            .map(AnchorId)
            .map(|a| (a, anchor_target_coverage_score(snapshot, a, &targets)))
            .max_by_key(|&(_, score)| score)
        {
            if score > 0 {
                return anchor;
            }
        }
    }
    best_anchor(snapshot)
}

/// Plays the Living Circuit as designed: builds a real expected-damage-
/// per-Sap-ranked, Needle-only DPS core ([`best_circuit_build_action`] --
/// see its own doc for why this replaced an earlier hard
/// place-before-upgrade, boss-value-only precedence, and the Standard-
/// difficulty leak-drain losses that precedence left on the table), and
/// actually spends Spark --
/// `FullCircuit` timed against Bellkeeper's bell-silence
/// cadence (and eagerly during Night Maw's final phase, where the base
/// Circuit shrinks to 2 slots), `PetPulse` whenever it would actually land
/// (reserved during a Bellkeeper fight so it never starves Full Circuit's
/// own 5-Spark threshold), `Blink` for urgent boss-coverage repositioning
/// -- and moves the pet to cover whatever is presently hitting the boss
/// rather than a boss-agnostic tower-coverage score.
#[derive(Default)]
pub struct CircuitPolicy {
    pub counters: ActionCounters,
    board: Board,
    bellkeeper_first_seen_tick: Option<u64>,
    escort_thresholds_seen: [bool; 3],
    escort_response_ticks_remaining: u32,
    /// Link Burst refresh tracking -- see `link_burst_refresh_target`'s own
    /// doc for why this exists at all.
    last_seen_anchor: Option<AnchorId>,
    ticks_since_arrival: u32,
}

impl Policy<Simulation> for CircuitPolicy {
    fn decide(&mut self, snapshot: &SimulationSnapshot, tick_index: u64) -> Vec<Command> {
        match snapshot.phase {
            RunPhaseView::RuneDraft => {
                return pick_circuit_rune(&snapshot.rune_options).map(|r| vec![Command::DraftRune(r)]).unwrap_or_default();
            }
            RunPhaseView::EvolutionChoice => {
                // Moth's extra permanent link slot (4 vs base 3) keeps more
                // towers linked ALL the time, for free -- more valuable to
                // this policy's boss-focused play than Crab's per-arrival
                // shield or Wisp's free-but-rare Blinks.
                return vec![Command::ChooseEvolution(Evolution::Moth)];
            }
            RunPhaseView::PetChargeDraft => {
                return pick_pet_charge(&CIRCUIT_PET_CHARGE_PRIORITY, &snapshot.pet_charge_options)
                    .map(|c| vec![Command::DraftPetCharge(c)])
                    .unwrap_or_default();
            }
            RunPhaseView::Victory | RunPhaseView::Defeat => return Vec::new(),
            RunPhaseView::Build { .. } | RunPhaseView::Combat => {}
        }

        let mut commands = Vec::new();

        // 1. Build: rank every affordable Needle placement AND every
        // already-placed Needle's next upgrade step by real expected
        // damage-per-Sap (boss contact PLUS minion contact --
        // best_circuit_build_action's own doc) instead of a hard
        // place-before-upgrade, boss-value-only precedence. That precedence
        // left two real defects on the table: a marginal, far-from-the-boss
        // pad could keep winning over upgrading an already-well-positioned
        // Needle regardless of which was actually worth more Sap, AND a
        // pure boss-route pad score never once considered route 1 -- this
        // policy's own `DIAG_DEATH_CAUSE` trace found that was the REAL
        // reason it lost 198/200 Standard seeds: Integrity drained to
        // exactly 0/-1 from unguarded route-1 leaks, not the boss reaching
        // the Heartseed (its own near-zero HP at those deaths was a
        // correlation, not the cause -- see best_circuit_build_action's own
        // doc for the full trace).
        let boss_speed_fp =
            snapshot.boss.as_ref().map(|b| b.kind.speed_fp()).unwrap_or_else(|| BossKind::Bellkeeper.speed_fp());
        if let Some(candidate) = best_circuit_build_action(snapshot, &self.board, boss_speed_fp) {
            let placed_kind = match &candidate.command {
                Command::Place { kind, .. } => Some(*kind),
                _ => None,
            };
            commands.push(candidate.command);
            if let Some(kind) = placed_kind {
                self.counters.record_placement(kind);
            } else if candidate.is_l3 {
                self.counters.upgrades_l3 += 1;
            } else {
                self.counters.upgrades_l2 += 1;
            }
        }

        // 2. Track Bellkeeper's first-seen tick (needed for the silence
        // prediction below) and escort-threshold crossings. The policy has
        // no access to the sim's own private `Boss` fields, so this
        // mirrors `Boss::newly_crossed_escort_threshold` from the HP the
        // snapshot DOES expose.
        let mut new_threshold_crossed = false;
        match &snapshot.boss {
            Some(boss) if boss.kind == BossKind::Bellkeeper => {
                if self.bellkeeper_first_seen_tick.is_none() {
                    self.bellkeeper_first_seen_tick = Some(tick_index);
                }
                let pm = hp_permille(boss.hp, boss.max_hp);
                for (i, &threshold) in BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE.iter().enumerate() {
                    if !self.escort_thresholds_seen[i] && pm <= threshold {
                        self.escort_thresholds_seen[i] = true;
                        new_threshold_crossed = true;
                    }
                }
            }
            _ => {
                // No Bellkeeper this tick (before/after its one wave, or
                // this run never reaches it) -- keep the tracking honestly
                // per-encounter rather than stale from a previous wave.
                self.bellkeeper_first_seen_tick = None;
                self.escort_thresholds_seen = [false; 3];
            }
        }
        self.escort_response_ticks_remaining = if new_threshold_crossed {
            ESCORT_RESPONSE_TICKS
        } else {
            self.escort_response_ticks_remaining.saturating_sub(1)
        };

        // 3. Anchor selection: cover whatever is presently hitting the
        // boss (blended with general enemy coverage right after a fresh
        // escort threshold). Also update the Link Burst refresh dwell
        // tracker (see `link_burst_refresh_target`'s own doc) -- reads the
        // pet's CURRENT position, so this must happen before movement is
        // decided below, not after.
        let target_anchor = pick_circuit_anchor(snapshot, self.escort_response_ticks_remaining > 0);
        track_anchor_dwell(snapshot, &mut self.last_seen_anchor, &mut self.ticks_since_arrival);

        // 4. Pet ability: at most ONE Spark-spending command per tick (see
        // the module doc's issued-equals-applied argument).
        let mut spark_action: Option<Command> = None;

        if let Some(boss) = &snapshot.boss {
            let full_circuit_worth_it = match boss.kind {
                BossKind::Bellkeeper => self
                    .bellkeeper_first_seen_tick
                    .map(|first_seen| bellkeeper_silence_window_active(tick_index, first_seen))
                    .unwrap_or(false),
                // Night Maw's final phase forces the base Circuit down to
                // 2 slots regardless of evolution -- Full Circuit is the
                // only way back to full tower coverage while it holds.
                BossKind::NightMaw => boss.final_phase,
            };
            if full_circuit_worth_it && snapshot.pet.spark >= FULL_CIRCUIT_COST {
                spark_action = Some(Command::FullCircuit);
                self.counters.full_circuit += 1;
            }
        }

        if spark_action.is_none() && snapshot.pet.spark >= PET_PULSE_COST {
            if let Some(anchor) = current_anchor(snapshot) {
                // While fighting Bellkeeper specifically, only spend Spark
                // on Pet Pulse once the bar is AT the cap: spending the
                // 3-cost from a full 8 still leaves 5 -- exactly enough to
                // still cast Full Circuit immediately if a silence window
                // is open -- so this never actually starves the
                // reservation Full Circuit needs. Spending any EARLIER
                // does: an earlier version of this policy cast Pet Pulse
                // as soon as it would land, which kept draining the bar
                // below 5 before the narrow pre-silence window ever
                // opened -- caught via this sweep's own action counters:
                // `full_circuit=0` over all 200 Standard-difficulty seeds
                // while Cozy/Wild both showed nonzero casts (same bell
                // timing, same formula, so a real difference in outcome
                // had to mean a real difference in whether Spark was ever
                // actually available when the window opened).
                let fighting_bellkeeper = matches!(&snapshot.boss, Some(b) if b.kind == BossKind::Bellkeeper);
                let reserve_satisfied = !fighting_bellkeeper || snapshot.pet.spark == SPARK_CAP;
                if reserve_satisfied && pulse_would_land(snapshot, anchor) {
                    spark_action = Some(Command::PetPulse);
                    self.counters.pet_pulse += 1;
                }
            }
        }

        // 5. Movement: Blink (spends the one remaining Spark-action slot)
        // when a boss is present and repositioning matters RIGHT NOW;
        // otherwise the free `MovePet` walk. `link_burst_refresh_target`
        // overrides an otherwise-unchanged target with a deliberate step
        // away once the pet has dwelled at the current (correctly-scored)
        // anchor past Link Burst's own cooldown -- see its own doc.
        if let Some(here) = current_anchor(snapshot) {
            let move_target = link_burst_refresh_target(snapshot, here, target_anchor, self.ticks_since_arrival);
            if move_target != here {
                if spark_action.is_none() && snapshot.boss.is_some() && snapshot.pet.spark >= BLINK_COST {
                    spark_action = Some(Command::Blink { anchor: move_target });
                    self.counters.blink += 1;
                } else {
                    commands.push(Command::MovePet { anchor: move_target });
                    self.counters.move_pet += 1;
                }
            }
        }

        if let Some(action) = spark_action {
            commands.push(action);
        }

        commands
    }
}

// ---------------------------------------------------------------------------
// Boss-route geometry and build-value ranking, shared by CircuitPolicy
// ([`best_circuit_build_action`]) and SlowStackPolicy ([`best_build_action`])
// ---------------------------------------------------------------------------

/// Boss route -- `sim.rs`'s own `spawn_due_enemies` always constructs a boss
/// on `RouteId(0)`; every boss-facing geometry calculation below is scored
/// against exactly this route.
const BOSS_ROUTE: RouteId = RouteId(0);

/// Integer square root via Newton's method. Every caller here passes a
/// non-negative fixed-point squared-distance well inside `i64` (tower
/// ranges are at most ~10 tiles, so `range_fp^2` stays far under `2^62`),
/// so there is no overflow case to guard.
fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Fixed-point length of the portion of one axis-aligned route segment that
/// lies within `[min_range_fp, range_fp]` of `origin` -- exact integer
/// geometry, no floats. Because `board.rs`'s own `Route::build` guarantees
/// every segment is axis-aligned, squared distance to `origin` is a convex
/// quadratic in the ONE varying coordinate, so "within radius R" is always
/// a single contiguous interval on the segment; the min-range exclusion is
/// that same construction for the smaller radius, subtracted back out of
/// the outer interval.
fn segment_length_in_annulus(origin: FixedPos, range_fp: i64, min_range_fp: Option<i64>, seg: &RouteSegment) -> i64 {
    let (u_from, u_to, origin_u, v_const, origin_v) = if seg.from.y == seg.to.y {
        (seg.from.x as i64 * FIXED_SCALE, seg.to.x as i64 * FIXED_SCALE, origin.x, seg.from.y as i64 * FIXED_SCALE, origin.y)
    } else {
        (seg.from.y as i64 * FIXED_SCALE, seg.to.y as i64 * FIXED_SCALE, origin.y, seg.from.x as i64 * FIXED_SCALE, origin.x)
    };
    let lo = u_from.min(u_to);
    let hi = u_from.max(u_to);
    let dv = v_const - origin_v;
    let dv2 = dv * dv;
    let range2 = range_fp * range_fp;
    if range2 < dv2 {
        return 0;
    }
    let half_outer = isqrt(range2 - dv2);
    let outer_lo = (origin_u - half_outer).max(lo);
    let outer_hi = (origin_u + half_outer).min(hi);
    if outer_lo >= outer_hi {
        return 0;
    }
    let outer_len = outer_hi - outer_lo;
    let Some(min_range_fp) = min_range_fp else {
        return outer_len;
    };
    let min2 = min_range_fp * min_range_fp;
    if min2 <= dv2 {
        return outer_len;
    }
    let half_inner = isqrt(min2 - dv2);
    let inner_lo = (origin_u - half_inner).max(outer_lo);
    let inner_hi = (origin_u + half_inner).min(outer_hi);
    let inner_len = if inner_lo < inner_hi { inner_hi - inner_lo } else { 0 };
    outer_len - inner_len
}

/// Total fixed-point route length within a tower's `[min_range, range]`
/// annulus of `origin`, summed over every segment of `route`.
fn route_length_in_annulus(origin: FixedPos, range_fp: i64, min_range_fp: Option<i64>, route: &Route) -> i64 {
    route.segments.iter().map(|seg| segment_length_in_annulus(origin, range_fp, min_range_fp, seg)).sum()
}

/// Expected damage a tower at `pad_pos` lands on a boss body over ONE full
/// walk of `route` at `boss_speed_fp` -- `dps x contact time`, using REAL
/// route geometry for contact time instead of a flat dps/cost guess. This
/// is the gap `CircuitPolicy`'s own Needle-only build
/// ([`best_circuit_build_action`]) leaves: Prism's shorter 3.0 range and
/// Moonwell's 2.0-tile dead zone both change how much of a pad's
/// theoretical range ever actually overlaps the
/// boss's walk, which a flat dps/cost number cannot see, and a pad near the
/// route's one 20-tile leg can give a longer-range tower far more total
/// contact than its raw dps/cost ranking implies.
///
/// Deliberately stays UNDILATED -- plain geometry-only contact time at the
/// boss's base `boss_speed_fp`, for every kind including Bell. Slow DOES
/// reach boss movement (`BossBody::advance_movement` shares
/// `Enemy::advance_movement`'s own combined-slow math via
/// `crate::status::SlowState`, and `fire_tower`'s `TargetRef::Boss` branch
/// calls `BossBody::apply_slow` exactly like its `TargetRef::Enemy` sibling
/// branch always has -- see this module's own `slow_boss_probe` tests below
/// for the measured confirmation), but modelling that per-pad here would
/// double-count it: [`bell_boss_dilation_value`] scores Bell's OWN marginal
/// boss-contact gain from its own slow separately, the same way
/// [`bell_minion_value`] scores its minion-side gain separately from this
/// function's plain baseline.
fn expected_boss_damage_per_pass(pad_pos: FixedPos, kind: TowerKind, level: UpgradeLevel, route: &Route, boss_speed_fp: i64) -> i64 {
    if !kind.attacks() {
        return 0;
    }
    let stats = tower::effective_stats(kind, level);
    let contact_fp = route_length_in_annulus(pad_pos, stats.range_fp, stats.min_range_fp, route);
    if contact_fp <= 0 {
        return 0;
    }
    let contact_ticks = contact_fp * TICKS_PER_SECOND / boss_speed_fp.max(1);
    let shots = contact_ticks / stats.interval_ticks.max(1) as i64;
    shots * stats.damage as i64
}

/// Expected EXTRA damage a tower at `pad_pos` lands on a boss body via its
/// own Link Burst, over one full walk of `route` at `boss_speed_fp` --
/// [`expected_boss_damage_per_pass`]'s own regular-attack contact-time
/// model, scored against `apply_link_burst`'s boss branch instead
/// (`sim.rs`): Needle/Prism use the tower's own normal range/min-range
/// (their burst picks a primary target exactly like a regular attack
/// does); Ember Nest/Moonwell use their burst's own field radius
/// (`splash_radius_fp`, no min-range -- the field is centred on the tower,
/// not gated by the "too close" exclusion their primary attack has). One
/// use is credited per [`LINK_BURST_COOLDOWN_TICKS`] the boss spends in
/// that reach (plus one more for any leftover contact, however short --
/// the very first arrival after placement almost always lands at least one
/// free burst) -- optimistic in that it assumes a policy keeps
/// re-triggering a fresh arrival that often, which `CircuitPolicy`'s and
/// `SlowStackPolicy`'s own `link_burst_refresh_target` now deliberately
/// do, but bounded by the SAME real contact-time geometry the regular-
/// attack model uses, not fabricated. Bell is excluded on purpose: its
/// Link Burst is a group stun, deliberately excluded from ever touching a
/// boss (`sim.rs`'s own `apply_link_burst` doc), so it has no boss-side
/// burst value to add here -- see [`bell_boss_dilation_value`] for its
/// own, separate, slow-based boss value instead.
fn expected_link_burst_damage_per_pass(pad_pos: FixedPos, kind: TowerKind, level: UpgradeLevel, route: &Route, boss_speed_fp: i64) -> i64 {
    let stats = tower::effective_stats(kind, level);
    let (radius_fp, min_range_fp, per_use): (i64, Option<i64>, i64) = match kind {
        TowerKind::Needle => (stats.range_fp, stats.min_range_fp, 3 * stats.damage as i64),
        TowerKind::Prism => (stats.range_fp, stats.min_range_fp, tower::prism_jump_damage(stats.damage, 0) as i64),
        TowerKind::EmberNest => {
            let radius_fp = stats.splash_radius_fp.unwrap_or(stats.range_fp);
            (radius_fp, None, tower::ember_splash_damage(stats.damage, true) as i64)
        }
        TowerKind::Moonwell => {
            let radius_fp = stats.splash_radius_fp.unwrap_or(0);
            (radius_fp, None, (stats.damage as i64 * MOONWELL_LINGER_TICK_DAMAGE_PERMILLE) / 1000)
        }
        TowerKind::Bell | TowerKind::Relay => return 0,
    };
    let contact_fp = route_length_in_annulus(pad_pos, radius_fp, min_range_fp, route);
    if contact_fp <= 0 {
        return 0;
    }
    let contact_ticks = contact_fp * TICKS_PER_SECOND / boss_speed_fp.max(1);
    let bursts = 1 + contact_ticks / LINK_BURST_COOLDOWN_TICKS as i64;
    bursts * per_use
}

/// Real, spec-formula combined slow (`combined_slow = 1 - product(1 -
/// slow_i)`, capped at [`MAX_COMBINED_SLOW_PERMILLE`] -- the plan's own
/// formula) contributed by every ALREADY-PLACED Bell tower whose range
/// currently reaches `point`.
fn combined_slow_at_point(point: FixedPos, snapshot: &SimulationSnapshot) -> i64 {
    let mut remaining = 1000i64;
    for t in snapshot.towers.iter().filter(|t| t.kind == TowerKind::Bell) {
        let stats = tower::effective_stats(t.kind, t.level);
        let Some(slow) = stats.slow_permille else { continue };
        let tower_pos = Tile::new(t.position.0, t.position.1).to_fixed();
        if tower_pos.dist2(point) <= stats.range_fp * stats.range_fp {
            remaining = remaining * (1000 - slow) / 1000;
        }
    }
    (1000 - remaining).min(MAX_COMBINED_SLOW_PERMILLE)
}

/// Combines two independent slow sources by the same multiplicative
/// formula, capped the same way -- used to fold one NEW Bell's own slow
/// into the ambient [`combined_slow_at_point`] already on the board.
fn combine_slow_permille(a: i64, b: i64) -> i64 {
    let remaining = (1000 - a) * (1000 - b) / 1000;
    (1000 - remaining).min(MAX_COMBINED_SLOW_PERMILLE)
}

/// Average base speed across the enemy roster (`EnemyKind::ALL`) -- a
/// representative minion speed for [`bell_minion_value`]'s contact-time
/// estimate. The wave generator mixes kinds per-wave and this heuristic has
/// no cheap way to know the exact upcoming mix before a wave starts, so it
/// uses the roster average rather than guessing one kind.
fn representative_minion_speed_fp() -> i64 {
    let sum: i64 = EnemyKind::ALL.iter().map(|k| k.base_stats().speed_fp).sum();
    sum / EnemyKind::ALL.len() as i64
}

/// Bell's minion-side value: this scores Bell by its own expected damage
/// against a minion crossing ITS OWN range once, inflated by the genuine
/// `1 / (1 - combined_slow)` time-dilation factor `enemy.rs`'s own
/// `advance_movement` produces (`effective = per_tick * (1000 - slow) /
/// 1000`, so time to cross a fixed distance scales by the reciprocal) --
/// using the board's OTHER already-placed Bells to compute the ambient
/// slow this new Bell would stack onto via [`combined_slow_at_point`], so
/// a Bell placed once the 60% ceiling is already saturated correctly
/// scores no better than an unslowed one (the marginal `after - ambient`
/// gain collapses to zero at the cap). Scoped to Bell's OWN hits only, not
/// neighbouring towers' -- modelling full cross-tower synergy needs
/// per-pad range-overlap bookkeeping this heuristic does not attempt, so
/// this UNDERSTATES slow's full board-wide value; it exists to give Bell a
/// fair, honestly-grounded (not zero, not invented) chance to win a pad
/// when its real minion role is worth more than another kind's real boss
/// role there. See [`bell_boss_dilation_value`] for the same idea applied
/// to the boss instead of a representative minion.
fn bell_minion_value(pad_pos: FixedPos, level: UpgradeLevel, board: &Board, snapshot: &SimulationSnapshot) -> i64 {
    let stats = tower::effective_stats(TowerKind::Bell, level);
    let Some(own_slow) = stats.slow_permille else { return 0 };
    let minion_speed = representative_minion_speed_fp();
    let contact_fp = [RouteId(0), RouteId(1)]
        .into_iter()
        .map(|r| route_length_in_annulus(pad_pos, stats.range_fp, stats.min_range_fp, board.route(r)))
        .max()
        .unwrap_or(0);
    if contact_fp <= 0 {
        return 0;
    }
    let ambient = combined_slow_at_point(pad_pos, snapshot);
    let after = combine_slow_permille(ambient, own_slow);
    let contact_ticks = contact_fp * TICKS_PER_SECOND / minion_speed.max(1);
    let dilated_ticks = contact_ticks * 1000 / (1000 - after).max(1);
    let shots = dilated_ticks / stats.interval_ticks.max(1) as i64;
    shots * stats.damage as i64
}

/// Bell's boss-side value: the marginal EXTRA expected damage Bell's own
/// slow earns Bell itself against the boss, over the plain undilated
/// baseline [`expected_boss_damage_per_pass`] already credits every kind
/// (including Bell) with. Mirrors [`bell_minion_value`] exactly (same
/// ambient-plus-own combine via [`combined_slow_at_point`]/
/// [`combine_slow_permille`], same `1 / (1 - combined_slow)` dilation), but
/// against [`BOSS_ROUTE`] at `boss_speed_fp` instead of a representative
/// minion route/speed, and returns only the DELTA the dilation adds (not
/// the full dilated total) so adding this to `expected_boss_damage_per_pass`'s
/// own undilated call does not double-count the undilated portion. Scoped to
/// Bell's OWN contact only -- like `bell_minion_value`'s own scoping note,
/// this does not attempt to value the dilation bonus every OTHER tower near
/// this pad also gets from a slower boss, so it UNDERSTATES Bell's full
/// board-wide boss value.
fn bell_boss_dilation_value(
    pad_pos: FixedPos,
    level: UpgradeLevel,
    board: &Board,
    snapshot: &SimulationSnapshot,
    boss_speed_fp: i64,
) -> i64 {
    let stats = tower::effective_stats(TowerKind::Bell, level);
    let Some(own_slow) = stats.slow_permille else { return 0 };
    let contact_fp = route_length_in_annulus(pad_pos, stats.range_fp, stats.min_range_fp, board.route(BOSS_ROUTE));
    if contact_fp <= 0 {
        return 0;
    }
    let ambient = combined_slow_at_point(pad_pos, snapshot);
    let after = combine_slow_permille(ambient, own_slow);
    let undilated_ticks = contact_fp * TICKS_PER_SECOND / boss_speed_fp.max(1);
    let dilated_ticks = undilated_ticks * 1000 / (1000 - after).max(1);
    let extra_ticks = (dilated_ticks - undilated_ticks).max(0);
    let extra_shots = extra_ticks / stats.interval_ticks.max(1) as i64;
    extra_shots * stats.damage as i64
}

/// Expected damage a tower at `pad_pos` lands on a REPRESENTATIVE minion
/// (`representative_minion_speed_fp`'s roster-average speed) over one pass
/// of whichever of route 0/1 it covers better -- the same "dps x contact
/// time" engine [`expected_boss_damage_per_pass`] uses against the boss,
/// scored against a minion route/speed instead so a pad's value ranking
/// stops being boss-only. An earlier version of [`best_circuit_build_action`]
/// (`CircuitPolicy`'s own, deliberately simpler, Needle-only ranking) never
/// scored this term at all, picking every pad purely by boss-route
/// contact; a full trace of its own resulting builds showed why that
/// starves the other lane outright: pads 0/1/4/5 (route 0's own 20-tile
/// leg) and 8/9 (the merge) are ALL closer to route 0 than EVERY
/// route-1-only pad (2/3/6/7), so a 4-6-tower Sap budget (this sweep's own
/// observed build size at wave 4) never reached route 1 at all -- every
/// minion sent through the OTHER entrance leaked for free, an Integrity
/// drain this crate's own `DIAG_DEATH_CAUSE` trace confirmed was the REAL
/// cause of 198/200 of that version's Standard losses (`integrity` at
/// exactly 0 or -1 at death, not the positive value a genuine "boss
/// reached the Heartseed" loss would leave), not the boss's own near-zero
/// HP at those same deaths -- a correlation, not the cause. Generalises
/// [`bell_minion_value`]'s own already-working pattern (previously scoped
/// to Bell alone, since only Bell got scored for minion value at all) to
/// every attacking kind, so both [`best_build_action`]'s ranking AND
/// [`best_circuit_build_action`]'s own now weigh "this pad barely reaches
/// the boss but comfortably covers the undefended lane" against "this pad
/// is a marginally better boss pad" instead of ignoring the first option
/// outright.
fn expected_minion_damage_per_pass(pad_pos: FixedPos, kind: TowerKind, level: UpgradeLevel, board: &Board, minion_speed_fp: i64) -> i64 {
    if !kind.attacks() {
        return 0;
    }
    let stats = tower::effective_stats(kind, level);
    [RouteId(0), RouteId(1)]
        .into_iter()
        .map(|r| {
            let contact_fp = route_length_in_annulus(pad_pos, stats.range_fp, stats.min_range_fp, board.route(r));
            if contact_fp <= 0 {
                return 0;
            }
            let contact_ticks = contact_fp * TICKS_PER_SECOND / minion_speed_fp.max(1);
            let shots = contact_ticks / stats.interval_ticks.max(1) as i64;
            shots * stats.damage as i64
        })
        .max()
        .unwrap_or(0)
}

/// One candidate build action considered this tick by [`SlowStackPolicy`]:
/// a Sap cost and the marginal expected value it buys (boss damage per
/// pass for every kind, plus [`bell_minion_value`] for Bell specifically),
/// used to rank EVERY affordable new placement AND EVERY affordable
/// upgrade on the SAME scale.
struct BuildCandidate {
    command: Command,
    cost: i32,
    value: i64,
    is_l3: bool,
}

/// Picks the single best-value-per-Sap build action across every open
/// build tile x affordable kind AND every eligible tower's next upgrade
/// step -- NOT a hard placement-before-upgrade precedence. This is what
/// actually fixes the trap `GreedyPolicy`'s own `if best_build_tile(..)
/// {..} else if ..upgrade..} else if ..upgrade..}` still has (see its own
/// doc): that chain never reaches an upgrade branch while ANY build tile
/// is still open, even once nothing left is affordable there, which
/// strands Sap the rest of the run instead of spending it on a real
/// upgrade. Comparing every
/// candidate's `value * other.cost` cross-product (instead of dividing,
/// which would need floats for a fair fractional comparison) ranks by
/// value-per-Sap without ever leaving an affordable, positive-value action
/// on the table just because a differently-shaped action was checked
/// first.
fn best_build_action(snapshot: &SimulationSnapshot, board: &Board, boss_speed_fp: i64) -> Option<BuildCandidate> {
    let minion_speed_fp = representative_minion_speed_fp();
    let mut best: Option<BuildCandidate> = None;
    let mut consider = |candidate: BuildCandidate| {
        let better = match &best {
            None => true,
            Some(b) => candidate.value * b.cost as i64 > b.value * candidate.cost as i64,
        };
        if better {
            best = Some(candidate);
        }
    };

    let occupied = occupied_tiles(snapshot);
    for tile in board.open_build_tiles(&occupied) {
        let pad_pos = tile.to_fixed();
        for &kind in TowerKind::ALL.iter() {
            if !kind.attacks() {
                continue;
            }
            let cost = kind.base_stats().cost;
            if snapshot.sap < cost {
                continue;
            }
            let mut value =
                expected_boss_damage_per_pass(pad_pos, kind, UpgradeLevel::Base, board.route(BOSS_ROUTE), boss_speed_fp)
                    + expected_link_burst_damage_per_pass(
                        pad_pos,
                        kind,
                        UpgradeLevel::Base,
                        board.route(BOSS_ROUTE),
                        boss_speed_fp,
                    );
            if kind == TowerKind::Bell {
                // `bell_minion_value` already covers Bell's own (dilated)
                // minion value in full -- adding the generic, undilated
                // `expected_minion_damage_per_pass` term below on top would
                // double-count the undilated portion.
                value += bell_minion_value(pad_pos, UpgradeLevel::Base, board, snapshot);
                value += bell_boss_dilation_value(pad_pos, UpgradeLevel::Base, board, snapshot, boss_speed_fp);
            } else {
                value += expected_minion_damage_per_pass(pad_pos, kind, UpgradeLevel::Base, board, minion_speed_fp);
            }
            consider(BuildCandidate { command: Command::Place { tile, kind }, cost, value, is_l3: false });
        }
    }

    for tower in snapshot.towers.iter().filter(|t| t.kind.attacks()) {
        let pad_pos = Tile::new(tower.position.0, tower.position.1).to_fixed();
        let base_cost = tower.kind.base_stats().cost;
        match tower.level {
            UpgradeLevel::Base => {
                let cost = UpgradeLevel::L2.step_cost(base_cost);
                if snapshot.sap < cost {
                    continue;
                }
                let before = expected_boss_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::Base, board.route(BOSS_ROUTE), boss_speed_fp)
                    + expected_link_burst_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::Base, board.route(BOSS_ROUTE), boss_speed_fp);
                let after = expected_boss_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::L2, board.route(BOSS_ROUTE), boss_speed_fp)
                    + expected_link_burst_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::L2, board.route(BOSS_ROUTE), boss_speed_fp);
                let mut value = after - before;
                if tower.kind == TowerKind::Bell {
                    let before_m = bell_minion_value(pad_pos, UpgradeLevel::Base, board, snapshot);
                    let after_m = bell_minion_value(pad_pos, UpgradeLevel::L2, board, snapshot);
                    value += after_m - before_m;
                    let before_b = bell_boss_dilation_value(pad_pos, UpgradeLevel::Base, board, snapshot, boss_speed_fp);
                    let after_b = bell_boss_dilation_value(pad_pos, UpgradeLevel::L2, board, snapshot, boss_speed_fp);
                    value += after_b - before_b;
                } else {
                    let before_min = expected_minion_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::Base, board, minion_speed_fp);
                    let after_min = expected_minion_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::L2, board, minion_speed_fp);
                    value += after_min - before_min;
                }
                consider(BuildCandidate { command: Command::UpgradeToL2 { tower: tower.id }, cost, value, is_l3: false });
            }
            UpgradeLevel::L2 => {
                let cost = UpgradeLevel::L3(UpgradeBranch::Power).step_cost(base_cost);
                if snapshot.sap < cost {
                    continue;
                }
                let before = expected_boss_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::L2, board.route(BOSS_ROUTE), boss_speed_fp)
                    + expected_link_burst_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::L2, board.route(BOSS_ROUTE), boss_speed_fp);
                let after = expected_boss_damage_per_pass(
                    pad_pos,
                    tower.kind,
                    UpgradeLevel::L3(UpgradeBranch::Power),
                    board.route(BOSS_ROUTE),
                    boss_speed_fp,
                ) + expected_link_burst_damage_per_pass(
                    pad_pos,
                    tower.kind,
                    UpgradeLevel::L3(UpgradeBranch::Power),
                    board.route(BOSS_ROUTE),
                    boss_speed_fp,
                );
                let before_min = expected_minion_damage_per_pass(pad_pos, tower.kind, UpgradeLevel::L2, board, minion_speed_fp);
                let after_min = expected_minion_damage_per_pass(
                    pad_pos,
                    tower.kind,
                    UpgradeLevel::L3(UpgradeBranch::Power),
                    board,
                    minion_speed_fp,
                );
                consider(BuildCandidate {
                    command: Command::UpgradeToL3 { tower: tower.id, branch: UpgradeBranch::Power },
                    cost,
                    value: after - before + after_min - before_min,
                    is_l3: true,
                });
            }
            UpgradeLevel::L3(_) => {}
        }
    }

    best
}

/// Classifies `pad_pos` by which route a Needle placed there mostly
/// defends -- whichever of route 0/route 1 its own Needle-range annulus
/// ([`route_length_in_annulus`], the same real geometry
/// [`expected_minion_damage_per_pass`] uses) covers more of. A tie
/// (neither route reachable from this pad at Needle's own range) defaults
/// to route 0, [`BOSS_ROUTE`] -- this policy's own primary objective, and
/// never actually reached in practice: every candidate tile is within
/// `BUILD_RADIUS_FP` (2.0 tiles) of SOME route by construction (`board.
/// rs`'s own `Board::near_route_cells`), and Needle's own 3.5-tile range
/// comfortably exceeds that radius, so at least one of `c0`/`c1` is always
/// positive.
fn pad_lane(pad_pos: FixedPos, board: &Board) -> RouteId {
    let stats = tower::effective_stats(TowerKind::Needle, UpgradeLevel::Base);
    let c0 = route_length_in_annulus(pad_pos, stats.range_fp, stats.min_range_fp, board.route(RouteId(0)));
    let c1 = route_length_in_annulus(pad_pos, stats.range_fp, stats.min_range_fp, board.route(RouteId(1)));
    if c1 > c0 {
        RouteId(1)
    } else {
        RouteId(0)
    }
}

/// [`CircuitPolicy`]'s own build ranking -- the same cross-multiply
/// value-per-Sap comparison [`best_build_action`] uses for
/// [`SlowStackPolicy`], scoped to Needle only (see the module-level note
/// above [`ESCORT_RESPONSE_TICKS`] for why Needle stays this policy's only
/// kind), but -- unlike an earlier version of this function -- NOT scoped
/// to boss contact value alone: it also adds
/// [`expected_minion_damage_per_pass`], exactly the term
/// [`best_build_action`] already credits every non-Bell kind with.
///
/// This is not a cosmetic addition. A first pass at this function (boss
/// contact value only, otherwise identical) moved `hatchery-arcade-
/// sweep --difficulty=standard --seeds=200`'s own reading of "how close
/// this policy gets" from Bellkeeper at 1.0% HP remaining to 0.3% --
/// closer, but the win rate itself stayed 0/200. A `DIAG_DEATH_CAUSE`
/// trace of the SAME 200 seeds' own `SimulationSnapshot::integrity` at
/// death told the real story: 198 of those 200 losses ended with
/// `integrity` at exactly 0 or -1 (the leak-drain signature -- `sim.rs`'s
/// own `advance_enemy_movement` ends a run the instant `integrity <= 0`),
/// not a positive value the way a genuine "boss reached the Heartseed
/// while Integrity was still fine" loss would. Bellkeeper's own near-zero
/// HP at those same deaths was a correlation, not the cause: a purely
/// boss-value pad ranking never once considers route 1 (the boss always
/// spawns on route 0 -- see [`BOSS_ROUTE`]'s own doc), so every minion the
/// wave-4 generator ever sends through the OTHER entrance (`wave.rs`'s own
/// `generate_wave` alternates `RouteId((i % 2) as u8)` spawn-to-spawn)
/// leaked for free, bleeding Standard's 20-point Integrity budget to zero
/// well before the fight against the boss itself was actually decided.
/// Adding the SAME minion term [`best_build_action`] already uses was NOT
/// enough on its own, though: pads 0/1/4/5 (route 0's own 20-tile leg)
/// score BOTH boss value AND minion value (route 0 minions walk the same
/// stretch the boss does), so their combined value stays far above any
/// route-1-only pad's minion-only value for as long as any of the four
/// remain empty -- a per-Sap value ranking alone still never once builds
/// on route 1 while a dual-purpose route-0 pad is still available, which
/// this crate's own `DIAG_DEATH_CAUSE` trace confirmed (still 190/200
/// Standard losses at `integrity` 0/-1 with the minion term added alone).
/// [`pad_lane`]'s own lane-parity gate below is what actually closes it:
/// a hard floor, not a value comparison, that a value ranking has no way
/// to express on its own (route 1 is not "a slightly worse boss pad", it
/// is "the entire other half of the map this build otherwise never
/// touches at all"). Placement still never buys anything but Needle,
/// keeping this policy's own build-diversity contrast with
/// [`SlowStackPolicy`] intact.
fn best_circuit_build_action(snapshot: &SimulationSnapshot, board: &Board, boss_speed_fp: i64) -> Option<BuildCandidate> {
    let minion_speed_fp = representative_minion_speed_fp();
    let value_at = |pad_pos: FixedPos, level: UpgradeLevel| -> i64 {
        expected_boss_damage_per_pass(pad_pos, TowerKind::Needle, level, board.route(BOSS_ROUTE), boss_speed_fp)
            + expected_link_burst_damage_per_pass(pad_pos, TowerKind::Needle, level, board.route(BOSS_ROUTE), boss_speed_fp)
            + expected_minion_damage_per_pass(pad_pos, TowerKind::Needle, level, board, minion_speed_fp)
    };

    let cost = TowerKind::Needle.base_stats().cost;
    let occupied = occupied_tiles(snapshot);

    // Lane-parity gate: guarantee at least [`CIRCUIT_ROUTE1_MIN_NEEDLES`]
    // Needle(s) on route 1 (the boss's own lane, route 0, is the ONLY lane
    // a plain value ranking ever picks -- see this function's own doc) once
    // this build has already committed at least that many towers to route
    // 0. A genuine floor, not another vote in a per-Sap comparison route 0
    // always wins on raw value: this MINIMUM (not parity -- route 0 still
    // gets every tower beyond the floor, since it stays the higher-value
    // lane) is calibrated empirically against `hatchery-arcade-sweep
    // --difficulty=standard`'s own two failure signatures -- a floor of 0
    // (no gate) left 190-198/200 losses at `integrity` 0/-1 (leak-drained,
    // `DIAG_DEATH_CAUSE`); a floor that matched route 0's own count 1:1
    // (strict alternation) overcorrected to 0 leak deaths but only ~25%
    // mean boss HP lost (half the Needle count halves boss DPS); a floor
    // of exactly [`CIRCUIT_ROUTE1_MIN_NEEDLES`] is the smallest floor that
    // still drove leak deaths to (near) zero. Skipped once no open
    // route-1 tile remains (then falls through to the ordinary ranking
    // below, same as any other exhausted lane).
    if snapshot.sap >= cost && is_boss_wave(snapshot.wave) {
        let route0_count = snapshot
            .towers
            .iter()
            .filter(|t| t.kind == TowerKind::Needle && pad_lane(Tile::new(t.position.0, t.position.1).to_fixed(), board) == RouteId(0))
            .count();
        let route1_count = snapshot.towers.iter().filter(|t| t.kind == TowerKind::Needle).count() - route0_count;
        if route1_count < CIRCUIT_ROUTE1_MIN_NEEDLES && route0_count >= CIRCUIT_ROUTE0_PRIORITY_NEEDLES {
            let best_route1_tile = board
                .open_build_tiles(&occupied)
                .into_iter()
                .filter(|&tile| pad_lane(tile.to_fixed(), board) == RouteId(1))
                .max_by_key(|&tile| value_at(tile.to_fixed(), UpgradeLevel::Base));
            if let Some(tile) = best_route1_tile {
                let value = value_at(tile.to_fixed(), UpgradeLevel::Base);
                return Some(BuildCandidate { command: Command::Place { tile, kind: TowerKind::Needle }, cost, value, is_l3: false });
            }
        }
    }

    let mut best: Option<BuildCandidate> = None;
    let mut consider = |candidate: BuildCandidate| {
        let better = match &best {
            None => true,
            Some(b) => candidate.value * b.cost as i64 > b.value * candidate.cost as i64,
        };
        if better {
            best = Some(candidate);
        }
    };

    if snapshot.sap >= cost {
        for tile in board.open_build_tiles(&occupied) {
            let value = value_at(tile.to_fixed(), UpgradeLevel::Base);
            consider(BuildCandidate { command: Command::Place { tile, kind: TowerKind::Needle }, cost, value, is_l3: false });
        }
    }

    for tower in snapshot.towers.iter().filter(|t| t.kind == TowerKind::Needle) {
        let pad_pos = Tile::new(tower.position.0, tower.position.1).to_fixed();
        let base_cost = tower.kind.base_stats().cost;
        match tower.level {
            UpgradeLevel::Base => {
                let step_cost = UpgradeLevel::L2.step_cost(base_cost);
                if snapshot.sap < step_cost {
                    continue;
                }
                let before = value_at(pad_pos, UpgradeLevel::Base);
                let after = value_at(pad_pos, UpgradeLevel::L2);
                consider(BuildCandidate {
                    command: Command::UpgradeToL2 { tower: tower.id },
                    cost: step_cost,
                    value: after - before,
                    is_l3: false,
                });
            }
            UpgradeLevel::L2 => {
                let step_cost = UpgradeLevel::L3(UpgradeBranch::Power).step_cost(base_cost);
                if snapshot.sap < step_cost {
                    continue;
                }
                let before = value_at(pad_pos, UpgradeLevel::L2);
                let after = value_at(pad_pos, UpgradeLevel::L3(UpgradeBranch::Power));
                consider(BuildCandidate {
                    command: Command::UpgradeToL3 { tower: tower.id, branch: UpgradeBranch::Power },
                    cost: step_cost,
                    value: after - before,
                    is_l3: true,
                });
            }
            UpgradeLevel::L3(_) => {}
        }
    }

    best
}

/// Plays like [`CircuitPolicy`] for the Living Circuit and Spark (sections
/// 2-5 below are copied from its own `decide`, unchanged -- see its own
/// docs for the Bellkeeper-silence prediction, anchor-coverage and
/// Spark-reservation reasoning, none of which depends on build strategy),
/// but replaces its Needle-only, pad-then-upgrade build with
/// [`best_build_action`]'s honest, geometry-and-slow-aware value ranking --
/// the deliberate test of the "stack Bell's slow to multiply boss contact
/// time" hypothesis this policy exists for. See [`bell_minion_value`] for
/// the bounded minion-side value Bell gets scored on and
/// [`bell_boss_dilation_value`] for the matching boss-side value (both
/// scoped to Bell's OWN contact only, so both UNDERSTATE its full
/// board-wide effect -- see each function's own doc for why).
#[derive(Default)]
pub struct SlowStackPolicy {
    pub counters: ActionCounters,
    board: Board,
    bellkeeper_first_seen_tick: Option<u64>,
    escort_thresholds_seen: [bool; 3],
    escort_response_ticks_remaining: u32,
    /// Link Burst refresh tracking -- see `link_burst_refresh_target`'s own
    /// doc for why this exists at all.
    last_seen_anchor: Option<AnchorId>,
    ticks_since_arrival: u32,
}

impl Policy<Simulation> for SlowStackPolicy {
    fn decide(&mut self, snapshot: &SimulationSnapshot, tick_index: u64) -> Vec<Command> {
        match snapshot.phase {
            RunPhaseView::RuneDraft => {
                return pick_circuit_rune(&snapshot.rune_options).map(|r| vec![Command::DraftRune(r)]).unwrap_or_default();
            }
            RunPhaseView::EvolutionChoice => {
                return vec![Command::ChooseEvolution(Evolution::Moth)];
            }
            RunPhaseView::PetChargeDraft => {
                return pick_pet_charge(&SLOWSTACK_PET_CHARGE_PRIORITY, &snapshot.pet_charge_options)
                    .map(|c| vec![Command::DraftPetCharge(c)])
                    .unwrap_or_default();
            }
            RunPhaseView::Victory | RunPhaseView::Defeat => return Vec::new(),
            RunPhaseView::Build { .. } | RunPhaseView::Combat => {}
        }

        let mut commands = Vec::new();

        // 1. Build: rank every affordable placement AND upgrade by real
        // expected-boss-damage-per-Sap instead of a hard precedence chain
        // (see `best_build_action`'s own doc).
        let boss_speed_fp =
            snapshot.boss.as_ref().map(|b| b.kind.speed_fp()).unwrap_or_else(|| BossKind::Bellkeeper.speed_fp());
        if let Some(candidate) = best_build_action(snapshot, &self.board, boss_speed_fp) {
            let placed_kind = match &candidate.command {
                Command::Place { kind, .. } => Some(*kind),
                _ => None,
            };
            commands.push(candidate.command);
            if let Some(kind) = placed_kind {
                self.counters.record_placement(kind);
            } else if candidate.is_l3 {
                self.counters.upgrades_l3 += 1;
            } else {
                self.counters.upgrades_l2 += 1;
            }
        }

        // 2. Track Bellkeeper's first-seen tick (needed for the silence
        // prediction below) and escort-threshold crossings. The policy has
        // no access to the sim's own private `Boss` fields, so this
        // mirrors `Boss::newly_crossed_escort_threshold` from the HP the
        // snapshot DOES expose.
        let mut new_threshold_crossed = false;
        match &snapshot.boss {
            Some(boss) if boss.kind == BossKind::Bellkeeper => {
                if self.bellkeeper_first_seen_tick.is_none() {
                    self.bellkeeper_first_seen_tick = Some(tick_index);
                }
                let pm = hp_permille(boss.hp, boss.max_hp);
                for (i, &threshold) in BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE.iter().enumerate() {
                    if !self.escort_thresholds_seen[i] && pm <= threshold {
                        self.escort_thresholds_seen[i] = true;
                        new_threshold_crossed = true;
                    }
                }
            }
            _ => {
                self.bellkeeper_first_seen_tick = None;
                self.escort_thresholds_seen = [false; 3];
            }
        }
        self.escort_response_ticks_remaining = if new_threshold_crossed {
            ESCORT_RESPONSE_TICKS
        } else {
            self.escort_response_ticks_remaining.saturating_sub(1)
        };

        // 3. Anchor selection: cover whatever is presently hitting the
        // boss (blended with general enemy coverage right after a fresh
        // escort threshold). Also update the Link Burst refresh dwell
        // tracker (see `link_burst_refresh_target`'s own doc) -- reads the
        // pet's CURRENT position, so this must happen before movement is
        // decided below, not after.
        let target_anchor = pick_circuit_anchor(snapshot, self.escort_response_ticks_remaining > 0);
        track_anchor_dwell(snapshot, &mut self.last_seen_anchor, &mut self.ticks_since_arrival);

        // 4. Pet ability: at most ONE Spark-spending command per tick (see
        // `CircuitPolicy`'s own module-doc issued-equals-applied argument).
        let mut spark_action: Option<Command> = None;

        if let Some(boss) = &snapshot.boss {
            let full_circuit_worth_it = match boss.kind {
                BossKind::Bellkeeper => self
                    .bellkeeper_first_seen_tick
                    .map(|first_seen| bellkeeper_silence_window_active(tick_index, first_seen))
                    .unwrap_or(false),
                BossKind::NightMaw => boss.final_phase,
            };
            if full_circuit_worth_it && snapshot.pet.spark >= FULL_CIRCUIT_COST {
                spark_action = Some(Command::FullCircuit);
                self.counters.full_circuit += 1;
            }
        }

        if spark_action.is_none() && snapshot.pet.spark >= PET_PULSE_COST {
            if let Some(anchor) = current_anchor(snapshot) {
                let fighting_bellkeeper = matches!(&snapshot.boss, Some(b) if b.kind == BossKind::Bellkeeper);
                let reserve_satisfied = !fighting_bellkeeper || snapshot.pet.spark == SPARK_CAP;
                if reserve_satisfied && pulse_would_land(snapshot, anchor) {
                    spark_action = Some(Command::PetPulse);
                    self.counters.pet_pulse += 1;
                }
            }
        }

        // 5. Movement: Blink (spends the one remaining Spark-action slot)
        // when a boss is present and repositioning matters RIGHT NOW;
        // otherwise the free `MovePet` walk. `link_burst_refresh_target`
        // overrides an otherwise-unchanged target with a deliberate step
        // away once the pet has dwelled at the current (correctly-scored)
        // anchor past Link Burst's own cooldown -- see its own doc.
        if let Some(here) = current_anchor(snapshot) {
            let move_target = link_burst_refresh_target(snapshot, here, target_anchor, self.ticks_since_arrival);
            if move_target != here {
                if spark_action.is_none() && snapshot.boss.is_some() && snapshot.pet.spark >= BLINK_COST {
                    spark_action = Some(Command::Blink { anchor: move_target });
                    self.counters.blink += 1;
                } else {
                    commands.push(Command::MovePet { anchor: move_target });
                    self.counters.move_pet += 1;
                }
            }
        }

        if let Some(action) = spark_action {
            commands.push(action);
        }

        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatchery_arcade_engine::sweep_api::simulate;
    use hatchery_arcade_pet_bastion::geometry::{dist2_to_axis_aligned_segment, tiles_to_fixed};
    use hatchery_arcade_pet_bastion::sim::PetBastionParams;
    use hatchery_arcade_pet_bastion::snapshot::PetView;
    use hatchery_arcade_pet_bastion::wave::Difficulty;

    #[test]
    fn isqrt_matches_known_squares() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(10_000), 100);
        assert_eq!(isqrt(99), 9); // floor(sqrt(99)) == 9, not a perfect square.
    }

    #[test]
    fn segment_annulus_length_matches_a_hand_worked_chord() {
        // Horizontal segment y=3, x in [0,20] tiles; origin at (4,1) tiles,
        // range 3.5 tiles -> perpendicular distance 2 tiles, half-chord =
        // sqrt(3.5^2 - 2^2) tiles ~= 2.87228 tiles.
        let seg = RouteSegment {
            from: Tile::new(0, 3),
            to: Tile::new(20, 3),
            length_fp: tiles_to_fixed(20, 0),
            cumulative_before_fp: 0,
        };
        let origin = Tile::new(4, 1).to_fixed();
        let range_fp = tiles_to_fixed(3, 5);
        let len = segment_length_in_annulus(origin, range_fp, None, &seg);
        let expected_fp = (2.0 * (3.5f64 * 3.5 - 2.0 * 2.0).sqrt() * FIXED_SCALE as f64) as i64;
        assert!((len - expected_fp).abs() <= 2, "len={len} expected~={expected_fp}");
    }

    #[test]
    fn segment_annulus_length_is_zero_when_the_whole_row_is_out_of_range() {
        // Same segment, origin now 10 tiles away perpendicular -- outside
        // even Moonwell's 6-tile range.
        let seg = RouteSegment {
            from: Tile::new(0, 3),
            to: Tile::new(20, 3),
            length_fp: tiles_to_fixed(20, 0),
            cumulative_before_fp: 0,
        };
        let origin = Tile::new(4, 13).to_fixed();
        let range_fp = tiles_to_fixed(6, 0);
        assert_eq!(segment_length_in_annulus(origin, range_fp, None, &seg), 0);
    }

    #[test]
    fn moonwells_dead_zone_removes_the_centre_of_its_own_reach() {
        // Origin sitting ON the segment: min_range 2.0 carves out a 4-tile
        // hole in the middle of the 12-tile (2*6.0) outer reach.
        let seg = RouteSegment {
            from: Tile::new(0, 3),
            to: Tile::new(20, 3),
            length_fp: tiles_to_fixed(20, 0),
            cumulative_before_fp: 0,
        };
        let origin = Tile::new(10, 3).to_fixed();
        let range_fp = tiles_to_fixed(6, 0);
        let min_range_fp = Some(tiles_to_fixed(2, 0));
        let with_dead_zone = segment_length_in_annulus(origin, range_fp, min_range_fp, &seg);
        let without = segment_length_in_annulus(origin, range_fp, None, &seg);
        assert!(without - with_dead_zone >= tiles_to_fixed(4, 0) - 2);
    }

    #[test]
    fn combined_slow_caps_at_the_spec_ceiling() {
        // Three independent 35% slows compound to 1-0.65^3=0.7254 -- the
        // same worked case `enemy.rs`'s own
        // `slow_combination_hits_the_ceiling` test uses, capped at 600.
        let a = combine_slow_permille(0, 350);
        let b = combine_slow_permille(a, 350);
        let c = combine_slow_permille(b, 350);
        assert_eq!(c, MAX_COMBINED_SLOW_PERMILLE);
    }

    #[test]
    fn bell_boss_dilation_value_is_positive_for_a_route_adjacent_pad_with_no_ambient_slow() {
        // A pad within Bell's own range of route 0's first leg, with no
        // other Bell already placed (ambient slow == 0): the marginal
        // EXTRA boss-contact damage Bell's OWN slow earns over the
        // undilated baseline must be strictly positive -- the concrete
        // proof that `SlowStackPolicy`'s value ranking now credits Bell
        // for boss-side slow at all. (See this function's own doc for why
        // this credit alone does not flip every pad's winner on this exact
        // board: Bell's raw 3 damage/hit still loses the value-per-Sap
        // race against Needle's/Moonwell's much higher raw output -- the
        // dilation credit is real, just not large enough to overturn that
        // by itself.)
        let board = Board::new();
        let pad_pos = Tile::new(4, 1).to_fixed();
        let boss_speed_fp = BossKind::Bellkeeper.speed_fp();
        let snapshot = SimulationSnapshot {
            tick_index: 0,
            difficulty: Difficulty::Standard,
            wave: 4,
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
        };

        let value = bell_boss_dilation_value(pad_pos, UpgradeLevel::Base, &board, &snapshot, boss_speed_fp);
        assert!(value > 0, "a Bell within range of the boss route must earn positive boss-side dilation value");
    }

    #[test]
    fn combined_slow_below_the_ceiling_matches_the_spec_formula() {
        // A single 35% slow: combined_slow == its own magnitude, uncapped.
        assert_eq!(combine_slow_permille(0, 350), 350);
        // Two 35% slows: 1-0.65^2=0.5775 -> ~577/578 permille (integer
        // truncation), which proves the multiplicative formula, not an
        // additive one (which would wrongly give 700).
        let two = combine_slow_permille(350, 350);
        assert!((577..=578).contains(&two), "two-slow combine={two}, expected ~577-578");
    }

    /// Tiles whose whole range sits well within Bell's 2.4-tile reach of
    /// route 0's own first (and longest, 20-tile) leg -- `(4,1)`/`(4,5)`/
    /// `(14,1)`/`(14,5)`, each exactly 2 tiles perpendicular from that leg
    /// (`board::Route::build`'s own `(0,3)-(20,3)` first waypoint pair) --
    /// the old fixed pad table's own former `PADS[0]`/`[1]`/`[4]`/`[5]`
    /// positions, still buildable under free placement (`board.rs`'s own
    /// module doc -- anywhere is buildable now, this particular spot
    /// included).
    /// Four Bells there combine to `1-0.65^4=0.8215`, capped at the spec's
    /// own 60% ceiling -- the best-case scenario for the "slow multiplies
    /// boss contact time" hypothesis.
    const PROBE_RESERVED_TILES: [Tile; 4] = [Tile::new(4, 1), Tile::new(4, 5), Tile::new(14, 1), Tile::new(14, 5)];

    /// Which of the two routes' own segments sits closest to `tile` --
    /// used by [`first_empty_non_reserved_tile`] to keep the probe's own
    /// pre-wave-4 defense from clustering both of its Needles on the SAME
    /// lane. Free placement (`board.rs`'s own module doc) means
    /// `Board::open_build_tiles` is no longer bounded to a single lane's
    /// own proximity band the way the old build radius implicitly kept
    /// it, so a naive "take the first N in row-major order" pick can land
    /// both Needles near route 0 and leave route 1 completely undefended.
    /// With Heartseed looping (`sim.rs`'s own `advance_enemy_movement`) a
    /// wave only ends once every spawned enemy is actually KILLED, so an
    /// undefended lane no longer just leaks itself clear -- it loops
    /// forever and drains Integrity until the run is lost on wave 1,
    /// before this probe ever reaches the wave-4 Bellkeeper it exists to
    /// measure.
    fn nearest_route(board: &Board, tile: Tile) -> RouteId {
        let p = tile.to_fixed();
        let dist2_to = |route: &Route| {
            route
                .segments
                .iter()
                .map(|seg| dist2_to_axis_aligned_segment(p, seg.from.to_fixed(), seg.to.to_fixed()))
                .min()
                .unwrap_or(i64::MAX)
        };
        if dist2_to(&board.routes[1]) < dist2_to(&board.routes[0]) {
            RouteId(1)
        } else {
            RouteId(0)
        }
    }

    /// The lowest-`(y, x)`-ordered open, non-reserved tile nearest
    /// `route`'s own path -- `first_empty_non_reserved_tile`'s own per-lane
    /// half, so its caller can request one tile per route instead of
    /// letting raw row-major order decide which lane gets covered.
    fn first_empty_non_reserved_tile_on_route(snapshot: &SimulationSnapshot, board: &Board, route: RouteId) -> Option<Tile> {
        let occupied = occupied_tiles(snapshot);
        board
            .open_build_tiles(&occupied)
            .into_iter()
            .filter(|&tile| !PROBE_RESERVED_TILES.contains(&tile))
            .find(|&tile| nearest_route(board, tile) == route)
    }

    /// One tile per route, alternating by `already_placed` (even -> route
    /// 0, odd -> route 1) -- see [`nearest_route`]'s own doc for why this
    /// probe cannot just take `Board::open_build_tiles`'s first N tiles
    /// under free placement any more.
    fn first_empty_non_reserved_tile(snapshot: &SimulationSnapshot, board: &Board, already_placed: usize) -> Option<Tile> {
        let route = RouteId((already_placed % 2) as u8);
        first_empty_non_reserved_tile_on_route(snapshot, board, route)
    }

    /// A minimal, fully deterministic policy built ONLY to isolate one
    /// variable for `bell_slow_never_lengthens_the_bosss_route_traversal`:
    /// whether `PROBE_RESERVED_TILES` carry Bell or Needle. Every OTHER
    /// tick's build decision is identical between both runs (a fixed
    /// two-Needle pre-wave-4 defense, just enough to survive the leak
    /// budget without spending Sap the reserved cluster needs), so waves
    /// 1-3 -- and therefore wave 4's own spawn plan, generated from the
    /// SAME `EngineRng` stream both runs consume identically up to that
    /// point -- are bit-identical; only the reserved tiles' kind, filled
    /// the moment wave 4 starts, differs. Every draft/evolution prompt
    /// takes a fixed, arbitrary first pick -- this probe measures
    /// movement, not play quality.
    struct SlowBossProbe {
        reserved_kind: TowerKind,
        board: Board,
        boss_spawn_tick: Option<u64>,
        boss_leg_cross_tick: Option<u64>,
    }

    impl SlowBossProbe {
        fn new(reserved_kind: TowerKind) -> Self {
            Self { reserved_kind, board: Board::new(), boss_spawn_tick: None, boss_leg_cross_tick: None }
        }
    }

    impl Policy<Simulation> for SlowBossProbe {
        fn decide(&mut self, snapshot: &SimulationSnapshot, tick_index: u64) -> Vec<Command> {
            let mut commands = Vec::new();
            match snapshot.phase {
                RunPhaseView::RuneDraft => {
                    return snapshot.rune_options.first().map(|&r| vec![Command::DraftRune(r)]).unwrap_or_default();
                }
                RunPhaseView::EvolutionChoice => return vec![Command::ChooseEvolution(Evolution::Crab)],
                RunPhaseView::PetChargeDraft => {
                    return snapshot.pet_charge_options.first().map(|&c| vec![Command::DraftPetCharge(c)]).unwrap_or_default();
                }
                RunPhaseView::Victory | RunPhaseView::Defeat => return Vec::new(),
                RunPhaseView::Build { .. } | RunPhaseView::Combat => {}
            }

            if snapshot.wave >= 4 {
                for &tile in &PROBE_RESERVED_TILES {
                    let occupied = snapshot.towers.iter().any(|t| t.position == (tile.x, tile.y));
                    if !occupied && snapshot.sap >= self.reserved_kind.base_stats().cost {
                        commands.push(Command::Place { tile, kind: self.reserved_kind });
                    }
                }
            } else {
                let non_reserved_count = snapshot
                    .towers
                    .iter()
                    .filter(|t| !PROBE_RESERVED_TILES.iter().any(|p| (p.x, p.y) == t.position))
                    .count();
                if non_reserved_count < 2 {
                    if let Some(tile) = first_empty_non_reserved_tile(snapshot, &self.board, non_reserved_count) {
                        if snapshot.sap >= TowerKind::Needle.base_stats().cost {
                            commands.push(Command::Place { tile, kind: TowerKind::Needle });
                        }
                    }
                }
            }

            if let Some(boss) = &snapshot.boss {
                if self.boss_spawn_tick.is_none() {
                    self.boss_spawn_tick = Some(tick_index);
                }
                if self.boss_leg_cross_tick.is_none() && boss.bodies[0].position.x >= tiles_to_fixed(20, 0) {
                    self.boss_leg_cross_tick = Some(tick_index);
                }
            }

            commands
        }
    }

    /// THE empirical proof behind [`bell_boss_dilation_value`]'s own doc
    /// claim (and the regression guard for the defect its history fixed --
    /// `fire_tower`'s `TargetRef::Boss` branch used to never call
    /// `BossBody::apply_slow`, the way its `TargetRef::Enemy` sibling
    /// branch always has): runs the SAME seed through two builds that are
    /// bit-identical except for `PROBE_RESERVED_TILES`' kind (Needle in the
    /// control, Bell in the test, saturating the spec's own 60% slow
    /// ceiling for as long as a Bell keeps landing hits), and measures the
    /// tick count for the Bellkeeper boss body to walk route 0's first
    /// 20-tile leg. Slow affects boss movement exactly the way it affects
    /// regular enemies (`BossBody::advance_movement` shares
    /// `Enemy::advance_movement`'s own combined-slow math via
    /// `crate::status::SlowState`), so the Bell run needs measurably MORE
    /// ticks than the Needle run to cross the exact same stretch. Real
    /// contact is imperfect -- staggered Bell cooldowns, the slow's own 2s
    /// decay against Bell's 1s reattack interval, and wave-4 minions
    /// competing for the same towers' targeting all leave gaps short of the
    /// full ~2.5x ceiling (`1 / (1 - 0.60)`) a continuously-saturated
    /// stretch would need -- so this asserts a concrete floor with headroom
    /// below the actual measurement (666 control ticks / 842 test ticks on
    /// this exact seed, a ~1.26x ratio), not the measurement itself: at
    /// least 15% more ticks than the control.
    #[test]
    fn bell_slow_measurably_lengthens_the_bosss_route_traversal() {
        let seed = 0;
        let params = || PetBastionParams::new(Difficulty::Standard);

        let mut control = SlowBossProbe::new(TowerKind::Needle);
        simulate::<Simulation>(seed, params(), &mut control, 30_000);
        let mut test = SlowBossProbe::new(TowerKind::Bell);
        simulate::<Simulation>(seed, params(), &mut test, 30_000);

        let control_spawn =
            control.boss_spawn_tick.expect("seed 0 / Standard with a 2-Needle pre-wave-4 defense must reach the wave-4 Bellkeeper");
        let control_cross =
            control.boss_leg_cross_tick.expect("Bellkeeper must cross route 0's first leg within the 30_000-tick ceiling");
        let test_spawn =
            test.boss_spawn_tick.expect("seed 0 / Standard with a 2-Needle pre-wave-4 defense must reach the wave-4 Bellkeeper");
        let test_cross =
            test.boss_leg_cross_tick.expect("Bellkeeper must cross route 0's first leg within the 30_000-tick ceiling");

        let control_ticks = control_cross - control_spawn;
        let test_ticks = test_cross - test_spawn;
        println!(
            "slow_boss_probe: control(Needle x4 reserved)={control_ticks} ticks, test(Bell x4 reserved, ~60% combined slow)={test_ticks} ticks"
        );
        assert!(
            test_ticks > control_ticks,
            "Bell's slow must measurably lengthen the boss's route-0 traversal (control={control_ticks}, test={test_ticks})"
        );
        assert!(
            test_ticks * 100 >= control_ticks * 115,
            "test_ticks={test_ticks} must be at least 15% above control_ticks={control_ticks}"
        );
    }
}
