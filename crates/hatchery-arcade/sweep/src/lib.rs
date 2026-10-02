//! Library face of `hatchery-arcade-sweep`, alongside its own `main.rs`
//! binary -- exists so the calibrated `Policy<Simulation>` implementations
//! in [`policy`] (`BaselinePolicy`/`GreedyPolicy`/`CircuitPolicy`/
//! `SlowStackPolicy`) are reusable by another crate (today: the `preview`
//! binary, which drives `SlowStackPolicy` to capture a real mid-run frame
//! -- see `preview/src/main.rs`'s own doc comment) instead of being
//! re-implemented or duplicated. `main.rs` keeps its own `mod` tree over
//! the SAME source files (a standard colocated bin+lib crate shape) --
//! this file changes nothing about the CLI binary's own behaviour.
//!
//! This does not change `hatchery-arcade-sweep`'s own headless
//! guarantee: its `Cargo.toml` dependency edges are unchanged (still
//! `default-features = false` on both `engine` and `pet-bastion`), so a
//! consumer of this library face gets exactly the same terminal-free sim
//! types this binary always has -- a consumer that wants rendering pulls
//! `hatchery-arcade-engine`'s own `render` feature directly, the same
//! way `hatchery-arcade-pet-bastion-render` already does.

pub mod cli;
pub mod policy;
pub mod report;
