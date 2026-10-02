//! `SextantBackend` -- NOT IMPLEMENTED this pass.
//!
//! The full design (per the plan's own Backends section) represents up to
//! 2x3 (sextant)/2x4 (octant) shape resolution, the same 2-colour-per-
//! glyph-cell ceiling as Braille/half-block. No `uzor` support exists for
//! the Unicode Symbols-for-Legacy-Computing block (`U+1FB00`) at all --
//! this backend would own its own small glyph-selection table (`fn
//! sextant_glyph(on_mask: u8) -> char`), not an external crate. It also
//! carries a real, undetectable-in-repo font-coverage risk (only correct
//! on a post-April-2024 Cascadia Mono build) -- see the plan's own
//! Backends section for the full reasoning.
//!
//! Defined here as a genuine, compiling stub, per this pass's own scope
//! -- `project` never touches `buf`/`surface` and always reports
//! [`TierStatus::NotImplemented`].

use uzor_tui::{buffer::TerminalBuffer, rect::Rect};

use crate::render::{DirtyHint, RenderBackend, Surface, TierStatus, TileFootprint};

#[derive(Default)]
pub struct SextantBackend;

impl RenderBackend for SextantBackend {
    type Output = TierStatus;

    fn project(
        &mut self,
        _surface: &Surface,
        _footprint: TileFootprint,
        _dest: Rect,
        _buf: &mut TerminalBuffer,
        _dragging: bool,
        _dirty: DirtyHint<'_>,
    ) -> TierStatus {
        TierStatus::NotImplemented
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::SurfaceCell;

    #[test]
    fn sextant_backend_reports_not_implemented_and_never_writes_the_buffer() {
        let surface = Surface::new(1, 1, SurfaceCell::BLANK);
        let mut buf = TerminalBuffer::new(1, 1);
        let clone = buf.clone();
        let mut backend = SextantBackend;
        let status = backend.project(
            &surface,
            TileFootprint { cells_w: 1, cells_h: 1 },
            Rect::new(0, 0, 1, 1),
            &mut buf,
            false,
            DirtyHint::Full,
        );
        assert_eq!(status, TierStatus::NotImplemented);
        assert!(buf.diff(&clone).is_empty());
    }
}
