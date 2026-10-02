//! `hatchery-arcade-engine` -- a deterministic mini-game engine plus an
//! optional multi-tier terminal renderer for the `hatchery-tui` pet
//! modal arcade. See
//! `docs/gate4agent/plans/hatchery-arcade-engine-2026-08-26.md` for the
//! full design. This file only re-exports; all real code lives in the
//! named modules below.
//!
//! Dependency direction: the sim core (`game`, `runner`, `shell`,
//! `cadence`, `rng`, `hash`, `admission`, `replay`, `sweep_api`) never
//! imports anything from `render`. `render` is the ONLY part of this
//! crate that may import `uzor_tui`/`uzor_text`, and is entirely absent
//! from a build with `default-features = false` (e.g.
//! `hatchery-arcade-sweep`'s own dependency edge).

pub mod admission;
pub mod cadence;
pub mod game;
pub mod hash;
pub mod replay;
pub mod rng;
pub mod runner;
pub mod shell;
pub mod sweep_api;

#[cfg(feature = "render")]
pub mod render;

#[cfg(test)]
mod test_support;

pub use admission::{AdmissionCredit, AdmissionError, AdmissionSource};
pub use cadence::{tightest_wake, Cadence};
pub use game::{MiniGame, RunOutcome};
pub use hash::StableHasher;
pub use replay::{ReplayError, ReplayV1, MAX_REPLAY_COMMANDS};
pub use rng::EngineRng;
pub use runner::{RecordedTick, Runner, MAX_CATCHUP_TICKS};
pub use shell::{ArcadeOccupant, ArcadeShell, CellArea, GameCatalogEntry, GameEntry, GameScreen};
pub use sweep_api::{simulate, Policy, SimulationReport};

#[cfg(feature = "render")]
pub use render::{
    background_seed, build_background, build_scene, compose_frame, encode_frame, render_over_background, render_scene, tick_alpha,
    BoardBackground, DirtyHint, DynamicSprite, DynamicStroke, GlyphBackend, HalfBlockBackend, PixelCanvas, PixelFrame, PixelFrameOutput,
    Rgb, RenderBackend, RenderTier, SextantBackend, SixelBackend, SixelOutput, SixelPlacement, Surface, SurfaceCell, TierStatus, TileArt,
    TileFootprint, TileId,
};
