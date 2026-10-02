//! The tier-agnostic rendering surface: the board-tile grid a game's own
//! render adapter mutates in place, and every `RenderBackend` reads from.

use crate::render::tiles::TileId;

/// 24-bit colour, tier-agnostic. The ONLY colour type a game ever
/// constructs; each backend converts it to whatever its own tier needs
/// (`uzor_tui::style::Color::Rgb(u8, u8, u8)` for every cell-based
/// backend).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A manifest-declared alt-state of one tile (Idle/Hit/Dead, NOT an
/// animation frame index -- see the plan's own Asset pipeline section for
/// why this is not a frame-sequence system).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileArt {
    pub tile: TileId,
    pub variant: u8,
}

/// One board-tile's COMPLETE visual contract. `glyph` is NEVER optional
/// and never derived from `art`'s presence -- this is the mechanical
/// enforcement of the readability contract: there is no way to construct
/// a `SurfaceCell` that carries sub-cell art but no glyph, because `glyph`
/// is a mandatory field, not an `Option`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SurfaceCell {
    pub glyph: char,
    pub fg: Rgb,
    /// `None` = board background shows through.
    pub bg: Option<Rgb>,
    /// Sub-cell/raster art a Tier2+ backend MAY use instead of the flat
    /// glyph cell; ignored by the glyph backend.
    pub art: Option<TileArt>,
}

impl SurfaceCell {
    /// A blank cell: a space glyph, black foreground, no background
    /// tint, no art. Used as [`Surface::get`]'s own out-of-bounds
    /// fallback and as a convenient starting fill cell.
    pub const BLANK: SurfaceCell = SurfaceCell { glyph: ' ', fg: Rgb(0, 0, 0), bg: None, art: None };
}

/// Board-tile grid, row-major, PERSISTENT across ticks -- a game's own
/// render adapter mutates this IN PLACE cell by cell, it is never thrown
/// away and rebuilt from scratch every tick. This is what makes sparse
/// dirty-region reporting meaningful: the coordinates a game touches via
/// `set` this call ARE the coordinates that changed, by construction, not
/// something a separate diff pass has to rediscover.
///
/// **Coordinate model**: coordinates are board-tile indices, never
/// terminal columns/rows. A host, once per frame, computes a
/// `TileFootprint` from the modal's current size and a `dest: Rect` (the
/// modal's inner content area, in real terminal cells), then hands
/// `(surface, footprint, dest, dirty)` to whichever `RenderBackend` the
/// owner's preference selected. Resize never touches the `Surface` --
/// only `footprint`/`dest` are recomputed.
#[derive(Clone)]
pub struct Surface {
    width: u16,
    height: u16,
    cells: Vec<SurfaceCell>,
}

impl Surface {
    pub fn new(width: u16, height: u16, fill: SurfaceCell) -> Self {
        let size = width as usize * height as usize;
        Self { width, height, cells: vec![fill; size] }
    }

    /// Clamps `(x, y)` into `[0, width) x [0, height)`. The plan leaves
    /// the exact clamp-vs-panic policy for out-of-bounds tile access to
    /// whoever writes the first real `MiniGame`; this pass picks CLAMP,
    /// not panic, matching the "never panic in library code" rule (a
    /// `Vec` index panic is still a panic, even though it cannot corrupt
    /// memory). `None` only for a genuinely zero-sized surface
    /// (`width == 0 || height == 0`), which has no cell to clamp into at
    /// all.
    fn index(&self, x: u16, y: u16) -> Option<usize> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let cx = x.min(self.width - 1);
        let cy = y.min(self.height - 1);
        Some(cy as usize * self.width as usize + cx as usize)
    }

    pub fn set(&mut self, x: u16, y: u16, cell: SurfaceCell) {
        if let Some(idx) = self.index(x, y) {
            self.cells[idx] = cell;
        }
    }

    pub fn get(&self, x: u16, y: u16) -> SurfaceCell {
        match self.index(x, y) {
            Some(idx) => self.cells[idx],
            None => SurfaceCell::BLANK,
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }
}

/// Terminal-cell footprint of ONE board tile, decided by the HOST from
/// the current modal size. Every backend must honour whatever footprint
/// it is given -- backend selection and footprint size are orthogonal
/// decisions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileFootprint {
    pub cells_w: u16,
    pub cells_h: u16,
}

/// Which board tiles a backend must actually repaint this call.
/// `Sparse(&[])` -- a tick where NOTHING changed -- is the correct,
/// expected, CHEAP case this exists to make possible, not a degenerate
/// edge case a backend merely tolerates.
pub enum DirtyHint<'a> {
    /// Repaint every tile -- first frame, entry into `Running`, after a
    /// resize, or a game with no cheaper subset to report.
    Full,
    /// Exactly these board-tile coordinates changed since the last call.
    /// MAY be empty.
    Sparse(&'a [(u16, u16)]),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_round_trip_within_bounds() {
        let mut surface = Surface::new(4, 3, SurfaceCell::BLANK);
        let cell = SurfaceCell { glyph: 'X', fg: Rgb(1, 2, 3), bg: Some(Rgb(4, 5, 6)), art: None };
        surface.set(2, 1, cell);
        assert_eq!(surface.get(2, 1), cell);
        assert_eq!(surface.get(0, 0), SurfaceCell::BLANK);
    }

    #[test]
    fn out_of_bounds_access_clamps_instead_of_panicking() {
        let mut surface = Surface::new(2, 2, SurfaceCell::BLANK);
        let cell = SurfaceCell { glyph: 'Z', fg: Rgb(9, 9, 9), bg: None, art: None };
        // Clamped to the last valid cell (1, 1) -- must not panic.
        surface.set(50, 50, cell);
        assert_eq!(surface.get(1, 1), cell);
    }

    #[test]
    fn zero_sized_surface_never_panics() {
        let surface = Surface::new(0, 0, SurfaceCell::BLANK);
        assert_eq!(surface.get(0, 0), SurfaceCell::BLANK);
    }

    #[test]
    fn width_and_height_accessors_report_construction_size() {
        let surface = Surface::new(5, 7, SurfaceCell::BLANK);
        assert_eq!(surface.width(), 5);
        assert_eq!(surface.height(), 7);
    }
}
