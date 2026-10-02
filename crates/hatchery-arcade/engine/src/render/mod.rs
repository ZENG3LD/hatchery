//! Feature-gated multi-tier terminal rendering surface -- the ONLY part
//! of `hatchery-arcade-engine` that may import `uzor_tui`/`uzor_text`/
//! `icy_sixel`. Nothing in `render/` ever imports `hatchery-tui`,
//! `gate4agent-*`, or any TUI/async/filesystem/network type; nothing in
//! the sim core (`crate::game`, `crate::runner`, `crate::shell`, ...)
//! ever imports from here.
//!
//! Tier ladder: [`RenderTier::Glyph`] is the always-available floor tier
//! (see [`backend_glyph`]), [`RenderTier::HalfBlock`] (see
//! [`backend_halfblock`]) and [`RenderTier::Sixel`] (see
//! [`backend_sixel`]) are both real, fully painting implementations.
//! `Sextant` is still a genuine "not implemented yet" compiling stub
//! (see its own module doc comment for exactly what a future pass still
//! owes -- no `uzor` support for the Unicode Symbols-for-Legacy-Computing
//! block exists to build it on, and it carries its own font-coverage
//! risk independent of that).

pub mod background;
pub mod backend_glyph;
pub mod backend_halfblock;
pub mod backend_pixel;
pub mod backend_sextant;
pub mod backend_sixel;
pub mod interp;
pub mod pixel;
pub mod sprites;
pub mod surface;
pub mod tiles;

pub use background::{background_seed, build_background, BoardBackground};
pub use backend_glyph::GlyphBackend;
pub use backend_halfblock::HalfBlockBackend;
pub use backend_pixel::{build_scene, compose_frame, encode_frame, render_over_background, render_scene, PixelFrame, PixelFrameOutput};
pub use backend_sextant::SextantBackend;
pub use backend_sixel::{SixelBackend, SixelOutput, SixelPlacement};
pub use interp::tick_alpha;
pub use pixel::{DynamicSprite, DynamicStroke, PixelCanvas};
pub use surface::{DirtyHint, Rgb, Surface, SurfaceCell, TileArt, TileFootprint};
pub use tiles::TileId;

/// Which sub-cell rendering technique currently paints the board. A
/// manual, host-stored preference with a conservative default (`Glyph`)
/// -- never automatic probing (this crate has no font-coverage/
/// sixel-support probe to lean on for a smarter default).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderTier {
    Glyph,
    HalfBlock,
    Sextant,
    Sixel,
}

/// Whether a [`RenderBackend::project`] call actually painted a frame, or
/// the tier is a documented stub with no implementation yet this pass.
/// Distinguishes "did nothing because the frame was already correct" (a
/// real backend's `DirtyHint::Sparse(&[])` no-op) from "cannot paint at
/// all" -- a stub backend must never be mistaken for a correctly-idle
/// real one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TierStatus {
    Painted,
    NotImplemented,
}

/// One rendering backend for one [`RenderTier`]. An associated `Output`
/// (not a shared return type) because exactly one backend -- Sixel --
/// needs to hand data back to the host; every cell-based backend writes
/// directly into `buf` and returns a status instead.
pub trait RenderBackend {
    type Output;

    fn project(
        &mut self,
        surface: &Surface,
        footprint: TileFootprint,
        dest: uzor_tui::rect::Rect,
        buf: &mut uzor_tui::buffer::TerminalBuffer,
        dragging: bool,
        dirty: DirtyHint<'_>,
    ) -> Self::Output;
}
