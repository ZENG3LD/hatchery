//! The universal sub-cell tier: 1x2 px/cell, two independent 24-bit
//! colours per terminal cell (`▀`/`▄`/`█`, top pixel = `fg`, bottom pixel
//! = `bg`) -- works in any terminal that can print Unicode block
//! characters, no sixel/graphics protocol support required.
//!
//! Drives `uzor_tui::canvas::{PixelCanvas, CanvasMode::HalfBlock}`
//! directly, one small `PixelCanvas` sized to exactly ONE tile's own
//! `footprint.cells_w x footprint.cells_h` terminal cells at a time (not
//! one canvas for the whole board): `PixelCanvas` exposes no public
//! per-pixel reader, only [`PixelCanvas::flush`], so painting tile by
//! tile through a small scratch [`TerminalBuffer`] is how this backend
//! reads back the exact glyph/colour `flush`'s own collapsing rule (solid
//! block when both pixels match, `Reset` background when a pixel was
//! left unset) picked for a given `(fg, bg)` pair, without duplicating
//! that logic here. Every sub-cell ROW of a tile's own footprint gets the
//! SAME `(fg, bg)` pair -- there is no baked per-pixel bitmap asset this
//! pass (see `tiles/mod.rs`'s own doc comment), so a footprint taller/
//! wider than 1x1 paints as a solid two-tone block, not a 1x1 tile
//! upscaled with padding the way [`crate::render::backend_glyph`]
//! degrades a bigger footprint at the glyph tier.
//!
//! `DirtyHint::Sparse` repaints only the NAMED tiles, same contract as
//! every other tier -- there is no persistent canvas state to invalidate
//! (a fresh scratch `PixelCanvas` is built per tile, per call), so a
//! sparse call touches exactly, and only, the coordinates it is given.

use uzor_tui::{
    buffer::TerminalBuffer,
    canvas::{CanvasMode, PixelCanvas},
    rect::Rect,
    style::Color,
};

use crate::render::{DirtyHint, RenderBackend, Rgb, Surface, TileFootprint};

fn to_color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

/// Paints one board tile's own `footprint.cells_w x footprint.cells_h`
/// terminal-cell block, clipped to `dest`, via a fresh per-tile
/// [`PixelCanvas`] -- see this module's own doc comment for why a
/// per-tile scratch canvas, not one whole-board canvas.
fn paint_tile(buf: &mut TerminalBuffer, dest: Rect, footprint: TileFootprint, tile_x: u16, tile_y: u16, fg: Rgb, bg: Option<Rgb>) {
    if footprint.cells_w == 0 || footprint.cells_h == 0 {
        return;
    }
    let mut canvas = PixelCanvas::new(CanvasMode::HalfBlock, footprint.cells_w, footprint.cells_h);
    let fg_color = to_color(fg);
    let bg_color = bg.map(to_color);
    for ly in 0..footprint.cells_h as i64 {
        for lx in 0..footprint.cells_w as i64 {
            canvas.set_pixel(lx, ly * 2, fg_color);
            if let Some(c) = bg_color {
                canvas.set_pixel(lx, ly * 2 + 1, c);
            }
        }
    }

    let mut scratch = TerminalBuffer::new(footprint.cells_w, footprint.cells_h);
    canvas.flush(Rect::new(0, 0, footprint.cells_w, footprint.cells_h), &mut scratch);

    let base_x = dest.x.saturating_add(tile_x.saturating_mul(footprint.cells_w));
    let base_y = dest.y.saturating_add(tile_y.saturating_mul(footprint.cells_h));
    for ly in 0..footprint.cells_h {
        for lx in 0..footprint.cells_w {
            let dx = base_x.saturating_add(lx);
            let dy = base_y.saturating_add(ly);
            if !dest.contains(dx, dy) {
                continue;
            }
            buf.set(dx, dy, scratch.get(lx, ly).clone());
        }
    }
}

/// The universal, always-legible-in-colour sub-cell tier. Failure mode:
/// none -- every `(fg, bg)` pair a [`Surface`] can carry paints SOME
/// two-tone block, there is no input this backend cannot render.
#[derive(Default)]
pub struct HalfBlockBackend;

impl RenderBackend for HalfBlockBackend {
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
                for y in 0..surface.height() {
                    for x in 0..surface.width() {
                        let tile = surface.get(x, y);
                        paint_tile(buf, dest, footprint, x, y, tile.fg, tile.bg);
                    }
                }
            }
            DirtyHint::Sparse(coords) => {
                for &(x, y) in coords {
                    let tile = surface.get(x, y);
                    paint_tile(buf, dest, footprint, x, y, tile.fg, tile.bg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::SurfaceCell;

    fn cell(fg: (u8, u8, u8), bg: Option<(u8, u8, u8)>) -> SurfaceCell {
        SurfaceCell { glyph: '#', fg: Rgb(fg.0, fg.1, fg.2), bg: bg.map(|(r, g, b)| Rgb(r, g, b)), art: None }
    }

    #[test]
    fn half_block_backend_paints_top_fg_bottom_bg_for_a_single_cell_footprint() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, cell((255, 0, 0), Some((0, 0, 255))));
        let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
        let dest = Rect::new(0, 0, 1, 1);
        let mut buf = TerminalBuffer::new(1, 1);
        let mut backend = HalfBlockBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);

        // A single-terminal-cell footprint only has ONE (top, bottom)
        // pixel pair, so `PixelCanvas::flush`'s own "different colours"
        // rule (`▀`, fg=top, bg=bottom) applies directly.
        assert_eq!(buf.get(0, 0).symbol, "▀");
        assert_eq!(buf.get(0, 0).style.fg, Color::Rgb(255, 0, 0));
        assert_eq!(buf.get(0, 0).style.bg, Color::Rgb(0, 0, 255));
    }

    #[test]
    fn half_block_backend_with_no_bg_leaves_the_bottom_pixel_unset() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, cell((10, 20, 30), None));
        let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
        let dest = Rect::new(0, 0, 1, 1);
        let mut buf = TerminalBuffer::new(1, 1);
        let mut backend = HalfBlockBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);

        // No `bg` -> the bottom pixel is never set -> `▀` with `bg: Reset`
        // (`PixelCanvas::flush`'s own "only top set" rule).
        assert_eq!(buf.get(0, 0).symbol, "▀");
        assert_eq!(buf.get(0, 0).style.fg, Color::Rgb(10, 20, 30));
        assert_eq!(buf.get(0, 0).style.bg, Color::Reset);
    }

    #[test]
    fn half_block_backend_same_fg_and_bg_collapses_to_a_solid_block() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, cell((7, 7, 7), Some((7, 7, 7))));
        let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
        let dest = Rect::new(0, 0, 1, 1);
        let mut buf = TerminalBuffer::new(1, 1);
        let mut backend = HalfBlockBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);

        assert_eq!(buf.get(0, 0).symbol, "█");
        assert_eq!(buf.get(0, 0).style.fg, Color::Rgb(7, 7, 7));
    }

    #[test]
    fn half_block_backend_bigger_footprint_paints_every_sub_cell_the_same_two_tone_block() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, cell((1, 2, 3), Some((4, 5, 6))));
        let footprint = TileFootprint { cells_w: 2, cells_h: 2 };
        let dest = Rect::new(0, 0, 2, 2);
        let mut buf = TerminalBuffer::new(2, 2);
        let mut backend = HalfBlockBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);

        for y in 0..2u16 {
            for x in 0..2u16 {
                assert_eq!(buf.get(x, y).symbol, "▀", "cell ({x},{y}) must be painted, not left blank");
                assert_eq!(buf.get(x, y).style.fg, Color::Rgb(1, 2, 3));
                assert_eq!(buf.get(x, y).style.bg, Color::Rgb(4, 5, 6));
            }
        }
    }

    #[test]
    fn half_block_backend_sparse_dirty_hint_only_touches_the_named_tiles() {
        let mut surface = Surface::new(3, 3, SurfaceCell::BLANK);
        for y in 0..3u16 {
            for x in 0..3u16 {
                surface.set(x, y, cell((1, 1, 1), Some((2, 2, 2))));
            }
        }
        let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
        let dest = Rect::new(0, 0, 3, 3);
        let mut buf = TerminalBuffer::new(3, 3);
        let mut backend = HalfBlockBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);
        buf.clear_dirty();
        let clone = buf.clone();

        let changed = [(0u16, 0u16), (2, 2)];
        for &(x, y) in &changed {
            surface.set(x, y, cell((200, 0, 0), Some((0, 0, 200))));
        }
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Sparse(&changed));

        let diff = buf.diff(&clone);
        assert_eq!(diff.len(), changed.len());
        for &(x, y) in &changed {
            assert!(diff.iter().any(|&(dx, dy, _)| dx == x && dy == y));
        }
    }

    #[test]
    fn half_block_backend_sparse_empty_slice_writes_zero_cells() {
        let mut surface = Surface::new(2, 2, SurfaceCell::BLANK);
        for y in 0..2u16 {
            for x in 0..2u16 {
                surface.set(x, y, cell((1, 1, 1), Some((2, 2, 2))));
            }
        }
        let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
        let dest = Rect::new(0, 0, 2, 2);
        let mut buf = TerminalBuffer::new(2, 2);
        let mut backend = HalfBlockBackend;
        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Full);
        buf.clear_dirty();
        let clone = buf.clone();

        backend.project(&surface, footprint, dest, &mut buf, false, DirtyHint::Sparse(&[]));
        assert!(buf.diff(&clone).is_empty());
    }
}
