//! Thin re-export of the engine's stable hash accumulator. See
//! `hatchery_arcade_engine::hash` for the FNV-1a-over-explicit-bytes
//! discipline `Simulation::stable_hash` (`sim.rs`) follows.

pub use hatchery_arcade_engine::StableHasher;
