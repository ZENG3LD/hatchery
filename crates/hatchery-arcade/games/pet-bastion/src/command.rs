//! Player/host input. Pause/speed are a `Runner`-level concern (per the
//! engine contract, see `contract.rs`) and are deliberately absent here --
//! this crate only defines commands that mutate the RUN's own state.

use crate::board::AnchorId;
use crate::geometry::Tile;
use crate::ids::EntityId;
use crate::pet::{Evolution, PetCharge};
use crate::rune::Rune;
use crate::tower::{TowerKind, UpgradeBranch};

#[derive(Clone, Debug)]
pub enum Command {
    /// Places a new tower at `tile` -- any board cell `Board::
    /// static_build_reason` reports as buildable and that is not currently
    /// occupied by another tower (`Simulation::place_tower` checks both).
    Place { tile: Tile, kind: TowerKind },
    UpgradeToL2 { tower: EntityId },
    UpgradeToL3 { tower: EntityId, branch: UpgradeBranch },
    Sell { tower: EntityId },
    MovePet { anchor: AnchorId },
    Blink { anchor: AnchorId },
    PetPulse,
    FullCircuit,
    /// Ends the current build phase immediately ("the player may launch
    /// early").
    StartWave,
    DraftRune(Rune),
    ChooseEvolution(Evolution),
    DraftPetCharge(PetCharge),
}
