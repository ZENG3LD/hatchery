//! The floor rendering tier: 1x1 px/cell (whole cell = one glyph), two
//! independent 24-bit colours (fg/bg). The one tier every game must be
//! fully legible on alone.
//!
//! Drives `uzor_text::ascii::AsciiGrid`/`CellShader` directly via
//! `AsciiGrid::for_each`, NOT via `uzor_tui::ascii::blit_ascii_grid` --
//! `blit_ascii_grid` builds `Style::default().fg(...)` and never sets
//! `bg` (every painted cell keeps `Style::default()`'s `bg: Color::
//! Reset`), because `uzor_text::ascii::Cell` itself carries no `bg` field
//! at all (only `ch`/`color`/`alpha`/`scale` -- it was built for
//! icon-overlay compositing, not a board). A `SurfaceCell` needs an
//! independent `bg` per tile (terrain vs. path vs. selection tint), so
//! this backend reads `bg` straight off the `Surface` itself (which DOES
//! carry it) at the same time it reads the shaded `ch`/`fg` off
//! `AsciiGrid`'s own per-cell output, and writes its own `uzor_tui::Cell::
//! styled(ch, Style::default().fg(fg).bg(bg))` per cell.
//!
//! `DirtyHint::Sparse` bypasses `AsciiGrid` entirely: `uzor_text::ascii::
//! AsciiGrid::step` has no partial-update mode (its nested loop always
//! re-evaluates every cell), so honouring a genuinely sparse update means
//! never calling `step` at all for that path -- reading `Surface::get`
//! directly for exactly the named coordinates instead. `Sparse(&[])`
//! therefore performs literally zero writes to `buf`, leaving its
//! per-row dirty tracking untouched.

use uzor_text::ascii::{AsciiGrid, Cell as AsciiCell, CellShader, Coord, Cursor, GridContext};
use uzor_tui::{
    buffer::TerminalBuffer,
    cell::Cell,
    rect::Rect,
    style::{Color, Style},
};

use crate::render::{DirtyHint, RenderBackend, Rgb, Surface, TileFootprint};

/// A `CellShader` that is a pure pass-through over a `Surface`'s own
/// `glyph`/`fg` -- reused purely to drive `AsciiGrid`'s existing frame/
/// cursor/timing bookkeeping and `for_each` walk, not because this
/// backend performs any dynamic per-frame shading today (`SurfaceCell`
/// carries no time-varying field for a shader to react to yet).
struct SurfaceCellShader<'a> {
    surface: &'a Surface,
}

impl CellShader for SurfaceCellShader<'_> {
    fn main(&self, coord: Coord, _ctx: &GridContext, _cursor: &Cursor) -> AsciiCell {
        let cell = self.surface.get(coord.x as u16, coord.y as u16);
        AsciiCell { ch: cell.glyph, color: [cell.fg.0, cell.fg.1, cell.fg.2], alpha: 1.0, scale: 1.0 }
    }
}

fn to_color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

/// Paints one board tile's own `footprint.cells_w x footprint.cells_h`
/// terminal-cell block, clipped to `dest`. The tile's own top-left
/// sub-cell carries the real glyph+fg+bg; every OTHER sub-cell in a
/// footprint bigger than 1x1 is filled with a `bg`-only blank -- "a
/// bigger footprint at this tier buys nothing but whitespace padding," the
/// honest, correct degenerate behaviour for a tier with no sub-cell
/// resolution of its own.
fn paint_tile(
    buf: &mut TerminalBuffer,
    dest: Rect,
    footprint: TileFootprint,
    tile_x: u16,
    tile_y: u16,
    glyph: char,
    fg: Rgb,
    bg: Option<Rgb>,
) {
    let base_x = dest.x.saturating_add(tile_x.saturating_mul(footprint.cells_w));
    let base_y = dest.y.saturating_add(tile_y.saturating_mul(footprint.cells_h));
    let bg_color = bg.map(to_color).unwrap_or(Color::Reset);

    for ly in 0..footprint.cells_h {
        for lx in 0..footprint.cells_w {
            let dx = base_x.saturating_add(lx);
            let dy = base_y.saturating_add(ly);
            if !dest.contains(dx, dy) {
                continue;
            }
            let cell = if lx == 0 && ly == 0 {
                Cell::styled(glyph.to_string(), Style::default().fg(to_color(fg)).bg(bg_color))
            } else {
                Cell::styled(" ", Style::default().bg(bg_color))
            };
            buf.set(dx, dy, cell);
        }
    }
}

/// The always-available floor tier. Failure mode: none -- if `footprint`
/// is larger than 1x1, the extra cells degrade to whitespace padding (see
/// [`paint_tile`]); there is no input this backend cannot render SOME
/// legible cell for.
#[derive(Default)]
pub struct GlyphBackend;

impl RenderBackend for GlyphBackend {
    type Output = ();

    fn project(
        &mut self,
        surface: &Surface,
        footprint: TileFootprint,
        dest: Rect,
        buf: &mut TerminalBuffer,
        _dragging: bool,
        dirty: DirtyHint<'_>,
    ) {
        match dirty {
            DirtyHint::Full => {
                let mut grid = AsciiGrid::new(surface.width() as usize, surface.height() as usize);
                let shader = SurfaceCellShader { surface };
                // `time = 0.0`: this backend has no animated shader today
                // (no wall clock reaches into `render/`); kept as an
                // explicit, documented zero rather than a fabricated
                // source. `aspect = 1.0`: no SDF/circle math here, this
                // shader never reads `ctx.aspect`.
                grid.step(&shader, 0.0, 1.0);
                grid.for_each(|x, y, ascii_cell| {
                    let tile = surface.get(x as u16, y as u16);
                    paint_tile(buf, dest, footprint, x as u16, y as u16, ascii_cell.ch, tile.fg, tile.bg);
                });
            }
            DirtyHint::Sparse(coords) => {
                for &(x, y) in coords {
                    let tile = surface.get(x, y);
                    paint_tile(buf, dest, footprint, x, y, tile.glyph, tile.fg, tile.bg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::SurfaceCell;

    fn cell(glyph: char, fg: (u8, u8, u8), bg: Option<(u8, u8, u8)>) -> SurfaceCell {
        SurfaceCell { glyph, fg: Rgb(fg.0, fg.1, fg.2), bg: bg.map(|(r, g, b)| Rgb(r, g, b)), art: None }
    }

    #[test]
    fn glyph_backend_renders_every_occupied_tile_with_its_own_distinct_glyph() {
        let mut surface = Surface::new(2, 1, SurfaceCell::BLANK);
        surface.set(0, 0, cell('A', (255, 0, 0), Some((10, 10, 10))));
        surface.set(1, 0, cell('B', (0, 255, 0), None));

        let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
        let dest = Rect::new(0, 0, 2, 1);
        let mut buf = TerminalBuffer::new(2, 1);
        let mut backend = GlyphBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);

        assert_eq!(buf.get(0, 0).symbol, "A");
        assert_eq!(buf.get(0, 0).style.fg, Color::Rgb(255, 0, 0));
        assert_eq!(buf.get(0, 0).style.bg, Color::Rgb(10, 10, 10));

        assert_eq!(buf.get(1, 0).symbol, "B");
        assert_eq!(buf.get(1, 0).style.fg, Color::Rgb(0, 255, 0));
        assert_eq!(buf.get(1, 0).style.bg, Color::Reset);
    }

    #[test]
    fn glyph_backend_bigger_footprint_pads_with_bg_only_blanks() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, cell('#', (1, 2, 3), Some((9, 8, 7))));

        let footprint = TileFootprint { cells_w: 2, cells_h: 2 };
        let dest = Rect::new(0, 0, 2, 2);
        let mut buf = TerminalBuffer::new(2, 2);
        let mut backend = GlyphBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);

        assert_eq!(buf.get(0, 0).symbol, "#");
        for &(x, y) in &[(1u16, 0u16), (0, 1), (1, 1)] {
            assert_eq!(buf.get(x, y).symbol, " ");
            assert_eq!(buf.get(x, y).style.bg, Color::Rgb(9, 8, 7));
        }
    }

    #[test]
    fn glyph_backend_sparse_dirty_hint_with_empty_slice_writes_zero_cells() {
        let mut surface = Surface::new(3, 3, SurfaceCell::BLANK);
        for y in 0..3u16 {
            for x in 0..3u16 {
                surface.set(x, y, cell('#', (1, 1, 1), Some((2, 2, 2))));
            }
        }
        let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
        let dest = Rect::new(0, 0, 3, 3);
        let mut buf = TerminalBuffer::new(3, 3);
        let mut backend = GlyphBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);
        buf.clear_dirty();
        let clone = buf.clone();

        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Sparse(&[]));

        assert!(buf.diff(&clone).is_empty(), "an empty Sparse hint must write literally zero cells");
        // `diff()` alone only proves no ROW was even scanned (both sides'
        // `dirty_rows` stayed false); assert byte-identity directly too,
        // cell by cell, so this test does not rely solely on `diff`'s own
        // correctness.
        for y in 0..3u16 {
            for x in 0..3u16 {
                assert_eq!(buf.get(x, y), clone.get(x, y), "cell ({x},{y}) must be untouched");
            }
        }
    }

    #[test]
    fn glyph_backend_sparse_dirty_hint_only_touches_the_named_tiles() {
        let mut surface = Surface::new(3, 3, SurfaceCell::BLANK);
        for y in 0..3u16 {
            for x in 0..3u16 {
                surface.set(x, y, cell('#', (1, 1, 1), Some((2, 2, 2))));
            }
        }
        let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
        let dest = Rect::new(0, 0, 3, 3);
        let mut buf = TerminalBuffer::new(3, 3);
        let mut backend = GlyphBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);
        buf.clear_dirty();
        let clone = buf.clone();

        // Change exactly 3 tiles' own content, and report exactly those 3
        // coordinates as dirty.
        let changed = [(0u16, 0u16), (1, 1), (2, 2)];
        for &(x, y) in &changed {
            surface.set(x, y, cell('X', (200, 0, 0), Some((0, 0, 200))));
        }
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Sparse(&changed));

        let diff = buf.diff(&clone);
        assert_eq!(diff.len(), changed.len(), "exactly the named tiles must differ, nothing else");
        for &(x, y) in &changed {
            assert!(diff.iter().any(|&(dx, dy, _)| dx == x && dy == y), "changed tile ({x},{y}) must appear in the diff");
        }
    }
}
