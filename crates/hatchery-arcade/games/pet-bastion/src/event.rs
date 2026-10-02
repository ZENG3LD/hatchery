//! Transient, one-tick-lifetime notifications for the renderer/replay log.
//! Never read back by `Simulation::advance` on a later tick -- persistent
//! effects live in state, not in this list.

use crate::board::AnchorId;
use crate::boss::BossKind;
use crate::enemy::EnemyKind;
use crate::ids::EntityId;
use crate::pet::{Evolution, PetCharge};
use crate::rune::Rune;
use crate::zone::WaveZones;

#[derive(Clone, Debug)]
pub enum SimEvent {
    Shot { tower: EntityId, target: EntityId },
    Impact { target: EntityId, damage: i32 },
    Kill { target: EntityId, kind: EnemyKind },
    /// An enemy reached the Heartseed and cost Integrity. It is NOT
    /// removed for this (`enemy::Enemy::walk`'s own doc) -- the same unit
    /// loops back to its own route start and keeps going, so this fires
    /// again for the same `enemy` id every lap it completes undefended.
    Leak { enemy: EntityId, integrity_remaining: i32 },
    /// A boss (any of its bodies) reached the Heartseed and cost Integrity
    /// instead of ending the run outright -- `sim.rs`'s own
    /// `BOSS_LAP_INTEGRITY_DAMAGE` doc has the full reasoning. Fired at
    /// most once per tick even if more than one body arrives that same
    /// tick (`advance_enemy_movement`'s own doc).
    BossLap { kind: BossKind, integrity_remaining: i32 },
    LinkBurst { tower: EntityId },
    PetArrived { anchor: AnchorId },
    SparkGenerated { total: u32 },
    WaveStarted { wave: u32 },
    WaveCompleted { wave: u32 },
    RuneOffered { options: Vec<Rune> },
    RuneChosen { rune: Rune },
    EvolutionOffered,
    EvolutionChosen { evolution: Evolution },
    PetChargeOffered { options: Vec<PetCharge> },
    PetChargeChosen { charge: PetCharge },
    /// Fired once per wave, the same tick `WaveStarted` fires -- this
    /// wave's own field modifiers, just drawn (`sim.rs`'s own
    /// `begin_combat`).
    FieldZonesRevealed { zones: WaveZones },
    BossEscortTriggered { threshold_index: usize },
    BossSplit,
    BossFinalPhase,
    BossDefeated { kind: BossKind },
    RunWon,
    RunLost,
}
