//! Pet Bastion: Night Garden -- a deterministic tower-defense run.
//!
//! Role: pure, deterministic game rules on top of the `gate4agent-arcade`
//! engine contract. Owns the board, towers, enemies, bosses, the Living
//! Circuit, the eight-wave run schedule, rune drafts, pet evolutions, the
//! pet's own Pet Charge build (`pet::PetCharge`) and per-wave field
//! modifiers (`zone.rs`).
//!
//! Exports: [`sim::Simulation`] (implements [`contract::MiniGame`] and
//! [`contract::GameEntry`] -- the latter is what lets the arcade shell list
//! this game and negotiate its modal size, see `sim.rs`'s own `GameEntry`
//! impl for the terminal-cell sizing math), [`sim::PetBastionParams`],
//! [`command::Command`], [`event::SimEvent`], [`snapshot::SimulationSnapshot`],
//! [`wave::Difficulty`], [`contract::RunOutcome`], [`rng::EngineRng`].
//!
//! Imports: nothing beyond `std`. This crate depends on
//! `gate4agent-arcade-engine` (path dependency, default features only).
//! `contract.rs`, `hash.rs` and `rng.rs` are thin re-export shims over the
//! engine's own `game`/`hash`/`rng` modules (plus, in `rng.rs`, this
//! crate's own game-owned `shuffle` helper -- the engine's `EngineRng`
//! intentionally has no such method) -- see each module's own doc comment.
//!
//! Forbidden: no terminal, rendering, TUI, async, filesystem or network
//! type anywhere in this crate. No floats in any rule -- all state is
//! integer or fixed-point (see `geometry.rs`, `constants.rs`).

pub mod board;
pub mod boss;
pub mod command;
pub mod constants;
pub mod contract;
pub mod enemy;
pub mod event;
pub mod geometry;
pub mod hash;
pub mod ids;
pub mod pet;
pub mod rng;
pub mod rune;
pub mod sim;
pub mod snapshot;
pub mod status;
pub mod tower;
pub mod wave;
pub mod zone;

pub use command::Command;
pub use contract::{MiniGame, RunOutcome};
pub use event::SimEvent;
pub use rng::EngineRng;
pub use sim::{PetBastionParams, Simulation};
pub use snapshot::SimulationSnapshot;
pub use wave::Difficulty;

#[cfg(test)]
mod tests;
