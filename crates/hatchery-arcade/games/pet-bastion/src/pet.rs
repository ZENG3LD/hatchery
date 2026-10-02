//! The Living Circuit: the pet, its anchors, Spark economy, actions and
//! evolutions. Pure state + pure helpers; per-tick orchestration (applying
//! damage, emitting events) lives in `sim.rs`.

use crate::board::AnchorId;
use crate::constants::*;
use crate::geometry::FixedPos;
use crate::ids::EntityId;
use crate::rng::EngineRng;
use crate::tower::{Tower, TowerKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Evolution {
    Moth,
    Crab,
    Wisp,
}

/// One pet build pick, drafted through the Living Circuit -- the owner's
/// own ask ("пет ... имеет возможность выбора бонусов? скорость? урон?
/// сплеш? модификация типа урона?"), answered by attaching each pick to
/// what being LINKED is worth rather than to the pet directly: every one of
/// these four only ever affects a tower the Circuit currently links (see
/// `sim.rs`'s own `fire_tower`/`adjusted_interval`), so picking a build
/// still leaves "where do I put the pet" a live decision every time the
/// player moves it -- exactly the same reason the base +30% link speed
/// bonus already stays meaningful, not a permanent stat-sheet number that
/// would make the anchor choice matter less over a run.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PetCharge {
    /// Attack speed: an extra permille bonus on top of the Circuit's own
    /// link speed bonus ([`PET_CHARGE_SURGE_EXTRA_SPEED_PERMILLE`]).
    Surge,
    /// Damage: an extra permille bonus on a linked tower's hit
    /// ([`PET_CHARGE_FANG_DAMAGE_PERMILLE`]).
    Fang,
    /// Splash/chain reach: extra radius and one extra target slot for a
    /// linked tower's own splash/chain attack
    /// ([`PET_CHARGE_BLOOM_RADIUS_PERMILLE`]).
    Bloom,
    /// Damage family: a linked tower's outgoing damage family rotates
    /// through every family instead of staying fixed to its own kind's
    /// default -- a real tactical answer to Mirror's resistance (which only
    /// ever protects against the single family that hit it last), not a
    /// number. See `tower.rs`'s own `DamageFamily::ALL` and `sim.rs`'s
    /// `resolved_family`.
    Attune,
}

impl PetCharge {
    pub const ALL: [PetCharge; 4] = [PetCharge::Surge, PetCharge::Fang, PetCharge::Bloom, PetCharge::Attune];
}

/// Classic shuffle bag over [`PetCharge::ALL`] -- the exact same algorithm
/// `rune.rs`'s own `RuneShuffleBag` uses (see its own doc), duplicated
/// rather than genericised: this crate favours small, concrete types over
/// generics elsewhere (see e.g. `EnemyKind`/`TowerKind`/`Rune` all carrying
/// their own `ALL` const rather than a shared trait), and a bag over 4
/// items is not the same type as a bag over 5.
#[derive(Clone, Debug, Default)]
pub struct PetChargeShuffleBag {
    remaining: Vec<PetCharge>,
}

impl PetChargeShuffleBag {
    pub fn new() -> Self {
        Self { remaining: Vec::new() }
    }

    pub fn draw_options(&mut self, rng: &mut EngineRng, n: usize) -> Vec<PetCharge> {
        let mut result: Vec<PetCharge> = Vec::with_capacity(n);
        while result.len() < n {
            if self.remaining.is_empty() {
                let mut fresh: Vec<PetCharge> = PetCharge::ALL.into_iter().filter(|c| !result.contains(c)).collect();
                crate::rng::shuffle(rng, &mut fresh);
                self.remaining.extend(fresh);
            }
            result.push(self.remaining.remove(0));
        }
        result
    }
}

pub fn charge_draft_option_count() -> usize {
    PET_CHARGE_DRAFT_OPTIONS
}

/// The Pet Charges the player has actually picked this run -- same shape as
/// `rune.rs`'s own `RuneLoadout`: a pick is idempotent (drafting a charge
/// already held is a harmless no-op, exactly like re-offering an
/// already-picked rune), not a stacking counter. A flat, bounded set of
/// four independent on/off build axes is what keeps this from collapsing
/// into "always take damage" -- Fang cannot be stacked into a dominant
/// damage strategy by repeatedly picking it; the only way to grow it is a
/// second AXIS (Surge/Bloom/Attune), each valuable in a different situation
/// (see each variant's own doc).
#[derive(Clone, Debug, Default)]
pub struct PetChargeLoadout {
    picked: Vec<PetCharge>,
}

impl PetChargeLoadout {
    pub fn add(&mut self, charge: PetCharge) {
        if !self.picked.contains(&charge) {
            self.picked.push(charge);
        }
    }

    pub fn has(&self, charge: PetCharge) -> bool {
        self.picked.contains(&charge)
    }

    pub fn picked(&self) -> &[PetCharge] {
        &self.picked
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PetAbility {
    Blink,
    PetPulse,
    FullCircuit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PetState {
    AtAnchor(AnchorId),
    Moving {
        from: AnchorId,
        to: AnchorId,
        ticks_remaining: u32,
    },
}

#[derive(Clone, Debug)]
pub struct Pet {
    pub state: PetState,
    pub spark: u32,
    pub linked_kill_credit: u32,
    pub evolution: Option<Evolution>,
    pub full_circuit_ticks_remaining: u32,
    pub wisp_free_blink_ticks_remaining: u32,
    pub crab_shield: i32,
    /// Anchor rune: the linked-tower set from the anchor just left, and how
    /// many ticks its buffs still linger for.
    pub lingering: Option<(Vec<EntityId>, u32)>,
    /// The pet's own build, drafted over the run through the Living Circuit
    /// -- see [`PetCharge`]'s own doc.
    pub charges: PetChargeLoadout,
}

impl Pet {
    pub fn new(start: AnchorId) -> Self {
        Pet {
            state: PetState::AtAnchor(start),
            spark: 0,
            linked_kill_credit: 0,
            evolution: None,
            full_circuit_ticks_remaining: 0,
            wisp_free_blink_ticks_remaining: 0,
            crab_shield: 0,
            lingering: None,
            charges: PetChargeLoadout::default(),
        }
    }

    pub fn current_anchor(&self) -> Option<AnchorId> {
        match self.state {
            PetState::AtAnchor(a) => Some(a),
            PetState::Moving { .. } => None,
        }
    }

    pub fn base_slots(&self, night_maw_final_phase: bool) -> usize {
        if night_maw_final_phase {
            return CIRCUIT_NIGHT_MAW_FINAL_SLOTS;
        }
        match self.evolution {
            None | Some(Evolution::Crab) => CIRCUIT_BASE_SLOTS,
            Some(Evolution::Moth) => CIRCUIT_MOTH_SLOTS,
            Some(Evolution::Wisp) => CIRCUIT_WISP_SLOTS,
        }
    }

    pub fn move_duration_ticks(&self) -> u32 {
        match self.evolution {
            Some(Evolution::Moth) => PET_MOVE_TICKS_MOTH as u32,
            _ => PET_MOVE_TICKS as u32,
        }
    }

    /// Starts travelling to `to`. No-op if already there or already moving
    /// there. `has_anchor_rune` decides whether the anchor just left keeps
    /// lingering buffs.
    pub fn start_move(&mut self, to: AnchorId, linked_before_move: &[EntityId], has_anchor_rune: bool) {
        let from = match self.state {
            PetState::AtAnchor(a) => {
                if a == to {
                    return;
                }
                a
            }
            PetState::Moving { to: current_to, .. } => {
                if current_to == to {
                    return;
                }
                current_to
            }
        };
        if has_anchor_rune {
            self.lingering = Some((linked_before_move.to_vec(), ANCHOR_RUNE_LINGER_TICKS as u32));
        }
        self.state = PetState::Moving {
            from,
            to,
            ticks_remaining: self.move_duration_ticks(),
        };
    }

    /// Blink: arrives immediately. Returns the Spark cost actually charged
    /// (0 if a Wisp free Blink was consumed).
    pub fn blink(&mut self, to: AnchorId) -> u32 {
        let free = self.evolution == Some(Evolution::Wisp) && self.wisp_free_blink_ticks_remaining == 0;
        if free {
            self.wisp_free_blink_ticks_remaining = WISP_FREE_BLINK_INTERVAL_TICKS as u32;
        }
        self.state = PetState::AtAnchor(to);
        if free {
            0
        } else {
            BLINK_COST
        }
    }

    pub fn start_full_circuit(&mut self) {
        self.full_circuit_ticks_remaining = FULL_CIRCUIT_TICKS as u32;
    }

    pub fn on_arrival_shield(&mut self) {
        if self.evolution == Some(Evolution::Crab) {
            self.crab_shield = (self.crab_shield + CRAB_SHIELD_ON_ARRIVAL).min(CRAB_SHIELD_CAP);
        }
    }

    pub fn pet_pulse_damage(&self) -> i32 {
        if self.evolution == Some(Evolution::Crab) {
            ((PET_PULSE_DAMAGE as i64 * CRAB_PET_PULSE_DAMAGE_PERMILLE) / 1000) as i32
        } else {
            PET_PULSE_DAMAGE
        }
    }

    /// Credits a linked kill toward Spark generation, doubled during Night
    /// Maw's final phase.
    pub fn credit_linked_kill(&mut self, doubled: bool) {
        self.linked_kill_credit += if doubled {
            NIGHT_MAW_FINAL_PHASE_KILL_CREDIT
        } else {
            1
        };
        while self.linked_kill_credit >= SPARK_PER_KILLS {
            self.linked_kill_credit -= SPARK_PER_KILLS;
            self.spark = (self.spark + 1).min(SPARK_CAP);
        }
    }

    /// Advances one tick. Returns `Some(anchor)` if the pet just arrived
    /// somewhere this tick (a Move completing).
    pub fn tick(&mut self) -> Option<AnchorId> {
        self.full_circuit_ticks_remaining = self.full_circuit_ticks_remaining.saturating_sub(1);
        self.wisp_free_blink_ticks_remaining = self.wisp_free_blink_ticks_remaining.saturating_sub(1);
        if let Some((_, ticks)) = &mut self.lingering {
            *ticks = ticks.saturating_sub(1);
            if *ticks == 0 {
                self.lingering = None;
            }
        }
        if let PetState::Moving {
            to,
            ticks_remaining,
            ..
        } = &mut self.state
        {
            if *ticks_remaining == 0 {
                let arrived = *to;
                self.state = PetState::AtAnchor(arrived);
                return Some(arrived);
            }
            *ticks_remaining -= 1;
            if *ticks_remaining == 0 {
                let arrived = *to;
                self.state = PetState::AtAnchor(arrived);
                return Some(arrived);
            }
        }
        None
    }
}

/// Computes which attacking towers are linked from `anchor_pos`: the
/// `base_slots` nearest attacking towers, plus one extra slot per placed
/// Relay tower (Relay itself is never linked -- it has no attack to speed
/// up). Ties broken by lowest stable entity id.
pub fn compute_linked_towers(anchor_pos: FixedPos, towers: &[Tower], base_slots: usize) -> Vec<EntityId> {
    let relay_count = towers.iter().filter(|t| t.kind == TowerKind::Relay).count();
    let total_slots = base_slots + relay_count;
    let mut attacking: Vec<(i64, EntityId)> = towers
        .iter()
        .filter(|t| t.kind.attacks())
        .map(|t| (anchor_pos.dist2(t.position_fp()), t.id))
        .collect();
    attacking.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    attacking.into_iter().take(total_slots).map(|(_, id)| id).collect()
}

/// All attacking tower ids, for Full Circuit.
pub fn all_attacking_towers(towers: &[Tower]) -> Vec<EntityId> {
    towers.iter().filter(|t| t.kind.attacks()).map(|t| t.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Tile;
    use crate::ids::EntityIdAllocator;

    #[test]
    fn nearest_three_towers_are_linked_by_default() {
        let mut alloc = EntityIdAllocator::default();
        let towers = vec![
            Tower::new(alloc.next(), Tile::new(4, 1), TowerKind::Needle),
            Tower::new(alloc.next(), Tile::new(4, 5), TowerKind::Needle),
            Tower::new(alloc.next(), Tile::new(14, 1), TowerKind::Needle),
            Tower::new(alloc.next(), Tile::new(23, 2), TowerKind::Needle),
        ];
        let anchor_pos = crate::board::Board::anchor_tile(AnchorId(0)).to_fixed(); // (10,2)
        let linked = compute_linked_towers(anchor_pos, &towers, CIRCUIT_BASE_SLOTS);
        assert_eq!(linked.len(), CIRCUIT_BASE_SLOTS);
        // The farthest tower (23,2) must not be linked when there are three
        // closer ones available.
        assert!(!linked.contains(&towers[3].id));
    }

    #[test]
    fn a_relay_tower_extends_capacity_by_one() {
        let mut alloc = EntityIdAllocator::default();
        let towers = vec![
            Tower::new(alloc.next(), Tile::new(4, 1), TowerKind::Needle),
            Tower::new(alloc.next(), Tile::new(4, 5), TowerKind::Needle),
            Tower::new(alloc.next(), Tile::new(14, 1), TowerKind::Needle),
            Tower::new(alloc.next(), Tile::new(23, 2), TowerKind::Needle),
            Tower::new(alloc.next(), Tile::new(23, 9), TowerKind::Relay),
        ];
        let anchor_pos = crate::board::Board::anchor_tile(AnchorId(0)).to_fixed();
        let linked = compute_linked_towers(anchor_pos, &towers, CIRCUIT_BASE_SLOTS);
        assert_eq!(linked.len(), CIRCUIT_BASE_SLOTS + 1);
        assert!(!linked.contains(&towers[4].id));
    }

    #[test]
    fn spark_caps_at_eight() {
        let mut pet = Pet::new(AnchorId(0));
        for _ in 0..500 {
            pet.credit_linked_kill(false);
        }
        assert_eq!(pet.spark, SPARK_CAP);
    }

    #[test]
    fn one_spark_per_five_linked_kills() {
        let mut pet = Pet::new(AnchorId(0));
        for _ in 0..4 {
            pet.credit_linked_kill(false);
        }
        assert_eq!(pet.spark, 0);
        pet.credit_linked_kill(false);
        assert_eq!(pet.spark, 1);
    }
}
