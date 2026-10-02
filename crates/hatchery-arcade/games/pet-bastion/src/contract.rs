//! Thin re-export of the engine's [`MiniGame`]/[`GameEntry`] contracts and
//! [`RunOutcome`] result type. See `gate4agent_arcade_engine::game` and
//! `gate4agent_arcade_engine::shell` for the trait definitions this crate's
//! own `impl MiniGame for Simulation` and `impl GameEntry for Simulation`
//! (`sim.rs`) satisfy. `CellArea`/`GameEntry` are core engine types (no
//! render feature required) -- re-exporting them here does not pull any
//! terminal/rendering dependency into this crate.

pub use gate4agent_arcade_engine::{CellArea, GameEntry, MiniGame, RunOutcome};
