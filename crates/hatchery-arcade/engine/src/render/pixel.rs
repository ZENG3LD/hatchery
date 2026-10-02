//! Continuous, sub-cell pixel data -- additive to [`crate::render::Surface`],
//! never a replacement for it. `Surface` stays the tile-grid contract the
//! glyph/half-block/per-tile-sixel backends read (see this crate's own
//! readability contract: the glyph tier must keep working, standalone,
//! forever); this module is the pixel-precision data a richer tier
//! composites ON TOP OF a `Surface`'s own static (terrain/tower) content,
//! for anything that needs genuine sub-tile motion a `Surface` cell (one
//! glyph, one tile) can never represent -- a moving enemy, a boss body,
//! the pet, a flying projectile, a combat effect.
//!
//! # Why a sibling buffer, not a `Surface` extension
//!
//! `Surface` versus [`PixelCanvas`] is the same split as `SurfaceCell`'s own
//! mandatory `glyph` field versus its optional `art` (`surface.rs`'s own
//! doc comment): a tile-grid cell is fundamentally an INTEGER coordinate
//! concept (one glyph per `(x, y)` tile), and bolting a continuous
//! sub-pixel position onto that contract would either (a) break every
//! existing `Surface::get`/`set` caller's assumption that a cell IS the
//! unit of content, or (b) force `Surface` itself to grow a second,
//! parallel coordinate system it was never designed to carry, entangling
//! the floor tier's own simplicity with the richest tier's own needs. A
//! sibling [`PixelCanvas`], addressed in real pixels, keeps both contracts
//! exactly as simple as their own tier actually needs -- see
//! `crate::render::backend_pixel` for the backend that actually builds one.
//!
//! # No rasteriser lives here anymore
//!
//! This module used to own a from-scratch analytic-AA rasteriser (circles,
//! rings, diamonds, straight/gently-bulged strokes, a bilinear sprite
//! blitter) -- narrow, hand-rolled shape vocabulary with no real curves, no
//! gradients, no text, no raster images. That code is gone: URX
//! (`uzor-urx-core`/`uzor-urx-cpu`, see `crate::render::backend_pixel`'s own
//! module doc) now draws every pixel, with real bezier paths, gradients,
//! and group blend layers instead. [`PixelCanvas`] itself survives as a
//! plain, dumb output buffer -- the straight-alpha RGBA8 result
//! `backend_pixel::render_scene` hands back after converting URX's own
//! premultiplied `Pixmap`, and what `preview`'s own PNG dump and
//! `encode_frame`'s `icy_sixel` call both read pixels from. [`DynamicSprite`]
//! and [`DynamicStroke`] survive unchanged too -- they were always a plain
//! data contract (a continuously-positioned entity/stroke a caller wants
//! composited this frame), never rasteriser code themselves.

use super::Rgb;

/// A `width x height` RGBA8 pixel buffer, row-major, straight (non-
/// premultiplied) alpha -- the same layout `icy_sixel::SixelImage::
/// try_from_rgba` already expects (see `backend_sixel.rs`'s own
/// `rasterize_tile`), so a finished canvas encodes with zero reshaping.
/// Built by `crate::render::backend_pixel::render_scene` from URX's own
/// premultiplied `Pixmap` output -- see that function's own doc comment
/// for the conversion.
#[derive(Clone)]
pub struct PixelCanvas {
    pub width: u32,
    pub height: u32,
    /// `4 * width * height` bytes, `[r, g, b, a]` per pixel.
    pub rgba: Vec<u8>,
}

impl PixelCanvas {
    /// A fully transparent canvas (every pixel `[0, 0, 0, 0]`) -- the
    /// zero-sized/degraded-output case (`encode_frame`'s own "never
    /// fabricate a frame" fallback, `render_scene`'s own render-error
    /// fallback).
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, rgba: vec![0u8; width as usize * height as usize * 4] }
    }

    /// Wraps an already-straight-alpha RGBA8 buffer -- the constructor
    /// `backend_pixel::render_scene` actually uses once it has converted
    /// URX's premultiplied `Pixmap` output. `rgba.len()` must already equal
    /// `4 * width * height`; a caller-side invariant (both call sites
    /// build `rgba` from a same-sized `Pixmap`), not re-validated here to
    /// avoid a redundant length check on every frame.
    pub fn from_straight_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self { width, height, rgba }
    }

    fn index(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    /// The raw `[r, g, b, a]` at `(x, y)`, or fully transparent black for an
    /// out-of-bounds query -- matches [`crate::render::Surface::get`]'s own
    /// clamp-never-panic policy.
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let idx = self.index(x, y);
        [self.rgba[idx], self.rgba[idx + 1], self.rgba[idx + 2], self.rgba[idx + 3]]
    }
}

/// One continuously-positioned entity a caller wants composited onto a
/// [`crate::render::backend_pixel`] frame this render call -- everything a
/// `Surface` cell cannot represent (see this module's own doc comment): an
/// enemy mid-route, a boss body, the pet, or a transient combat effect.
/// Data-only; actually drawing it is `crate::render::backend_pixel::
/// build_scene`'s job, via `crate::render::sprites::paint_tile`.
#[derive(Clone, Copy, Debug)]
pub struct DynamicSprite {
    pub tile: super::TileId,
    pub variant: u8,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    /// Centre position, in fractional BOARD TILES (e.g. `3.42`) -- the same
    /// tile-coordinate convention [`crate::render::Surface`] uses; the
    /// pixel backend owns the tile-to-pixel scale.
    pub tile_x: f64,
    pub tile_y: f64,
    /// `1.0` = this kind's own native baked size; an effect may grow or
    /// shrink over its own lifetime (a splash ring expanding, an impact
    /// flash shrinking) by varying this from frame to frame.
    pub scale: f32,
    /// `0.0..=1.0` -- an effect fading in or out.
    pub alpha: f32,
}

/// One anti-aliased stroke a caller wants drawn this frame -- a flying
/// projectile's own trail, or a Prism chain's own visible arc between two
/// already-resolved hit positions. Both endpoints are fractional board
/// tiles, the same convention [`DynamicSprite::tile_x`]/`tile_y` use.
#[derive(Clone, Copy, Debug)]
pub struct DynamicStroke {
    pub from_tile: (f64, f64),
    pub to_tile: (f64, f64),
    pub color: Rgb,
    /// Stroke width, in PIXELS (not tiles) -- a stroke's own visual
    /// thickness does not scale with board size the way a sprite's own
    /// tile footprint does.
    pub width_px: f32,
    /// A non-zero value bows this stroke's own real quadratic Bezier
    /// through a control point offset perpendicular to the straight
    /// from-to line, as a fraction of the segment's own length -- `0.0` is
    /// a dead-straight line. See `crate::render::backend_pixel`'s own
    /// stroke-building code for the actual curve construction.
    pub bulge: f32,
    pub alpha: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_canvas_is_fully_transparent() {
        let canvas = PixelCanvas::new(3, 2);
        assert_eq!(canvas.rgba.len(), 3 * 2 * 4);
        assert_eq!(canvas.get(1, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn get_out_of_bounds_is_transparent_black_not_a_panic() {
        let canvas = PixelCanvas::new(2, 2);
        assert_eq!(canvas.get(5, 5), [0, 0, 0, 0]);
    }

    #[test]
    fn from_straight_rgba_round_trips_the_supplied_bytes() {
        let rgba = vec![10, 20, 30, 255, 40, 50, 60, 128];
        let canvas = PixelCanvas::from_straight_rgba(2, 1, rgba);
        assert_eq!(canvas.get(0, 0), [10, 20, 30, 255]);
        assert_eq!(canvas.get(1, 0), [40, 50, 60, 128]);
    }
}
