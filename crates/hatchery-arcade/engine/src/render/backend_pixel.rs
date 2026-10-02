//! The richest tier: one CONTINUOUS whole-board pixel canvas per frame,
//! drawn by URX (`uzor-urx-core`/`uzor-urx-cpu` -- uzor's own CPU scanline
//! rasteriser, no GPU/window/event loop) and encoded to a single SIXEL
//! image -- the exact recipe the `bench` crate's own live-terminal run
//! measured holding 60fps (full 560x266 board, 32 colours, ONE image, one
//! cursor jump to the board's own origin; a quarter-board redraw via
//! cursor repositioning was 3.37x faster still, but that is a STATIC-
//! redraw saving, not a "how many small images can I emit instead" one --
//! see this module's own "Why one whole-board image, never per-tile
//! placements" doc section).
//!
//! Unlike [`crate::render::backend_sixel::SixelBackend`] (one independent
//! [`crate::render::SixelPlacement`] per tile, each tile-cursor-anchored
//! and so structurally unable to place content that spans a tile boundary
//! -- see that module's own doc comment), this backend paints [`Surface`]'s
//! own static content (terrain, towers, build pads, the Circuit halo --
//! anything for which [`TileId::is_dynamic_entity`] is `false`) into ONE
//! shared URX [`Scene`], then every caller-supplied [`DynamicSprite`]/
//! [`DynamicStroke`] on top at its own CONTINUOUS pixel position, and
//! encodes the whole board as a single image. This is what makes genuine
//! sub-tile motion possible at all: an enemy at tile `x = 3.4` is one real
//! pixel position in one shared scene, not a value `Surface`'s own
//! tile-integer coordinates could ever represent.
//!
//! # Why URX, not a hand-rolled rasteriser
//!
//! This backend used to build its own `PixelCanvas` by hand, painting
//! every shape through `pixel.rs`'s own from-scratch analytic-AA
//! primitives (circles, rings, diamonds, straight/gently-bulged strokes) --
//! a narrow vocabulary (no real curves, no gradients, no text, no raster
//! images, no group blend layers) that read as "an ASCII diagram made of
//! pixels" rather than real graphics. `uzor-urx-cpu::CpuBackend::render`
//! (`docs/gate4agent/research/urx-pixel-path-for-terminal-2026-08-27.md`
//! is the research pass that confirmed this) is a pure function -- `&Scene,
//! &mut Pixmap -> Result<(), RenderError>` -- with no window, no GPU device,
//! no event loop, that already draws real bezier paths, linear/radial/sweep
//! gradients, rounded rects, dashed/round-joined strokes, and group blend
//! layers. [`build_scene`] is the adapter: it walks the SAME `Surface`/
//! `DynamicSprite`/`DynamicStroke` inputs the old hand-rolled version read,
//! and emits `uzor_urx_core::scene::DrawCommand`s (via
//! `crate::render::sprites::paint_tile`) instead of plotting pixels
//! directly.
//!
//! # Premultiplied vs. straight alpha
//!
//! `uzor_urx_cpu::Pixmap` is premultiplied RGBA8 (`r/g/b` already scaled by
//! `a`); `icy_sixel::SixelImage::try_from_rgba` (and this backend's own
//! `PixelCanvas`) expect STRAIGHT alpha. [`render_scene`] does the one
//! un-premultiply pass this conversion needs -- see
//! [`straight_alpha_from_premultiplied`]'s own doc comment.
//!
//! # Why one whole-board image, never per-tile placements
//!
//! A moving scene touches tiles scattered across the whole board almost
//! every frame (an enemy on each route, the boss, the pet, in-flight
//! effects) -- there is no small "quarter of the board" a 60Hz combat
//! frame could redraw INSTEAD of the full board the way a mostly-static
//! screen could. The owner's own measured numbers back a full-board,
//! single-image emit at 32 colours (2.85ms encode + 4.78ms terminal
//! ingest of the SAME 16.7ms budget the owner's own live 240-frame run
//! held at 60.18fps) -- that is the one recipe this backend reproduces
//! exactly, not a per-tile placement scheme this crate never measured at
//! 60fps with a moving scene.
//!
//! # No persistent backend state -- except the background a HOST persists
//!
//! Same convention `backend_sixel.rs`'s own `SixelBackend` documents: a
//! fresh [`build_scene`]/[`render_scene`] call always re-paints and
//! re-encodes exactly what it is handed, with no hidden mutable state of
//! its own. There is still no dirty-tracking here (unlike the cell-based
//! backends' `DirtyHint`) -- see this module's own doc section above for
//! why a moving scene has no cheap static subset to track in the first
//! place. The one deliberate exception: [`super::background::
//! BoardBackground`] is a cache a HOST builds once (`super::background::
//! build_background`) and holds onto across many frames, reused via
//! [`render_over_background`]'s own cheap `Pixmap` clone -- see that
//! module's own top-level doc comment for the full contract. That cache
//! lives entirely in the CALLER's own hands (a plain, host-owned value,
//! never a static/thread-local this crate manages internally), so this
//! backend itself is still exactly as stateless as every other one in
//! this crate -- it just now accepts one more piece of already-painted
//! input instead of repainting everything from `Surface` every call.

use icy_sixel::{BackgroundMode, EncodeOptions, SixelImage};
use uzor_tui::rect::Rect;
use uzor_urx_core::scene::Scene;
use uzor_urx_cpu::{CpuBackend, Pixmap};

use super::background::BoardBackground;
use super::pixel::{DynamicSprite, DynamicStroke, PixelCanvas};
use super::sprites::{paint_tile, paint_vignette, push_ambient_glow, push_soft_shadow};
use super::Surface;

/// Assumed terminal-cell pixel size -- kept as this module's own documented
/// copy, hand-synced to `backend_sixel.rs`'s own `PX_PER_CELL_W`/`_H` (see
/// that module's own doc comment for why there is no live font-metrics
/// query to derive this from instead). `PX_PER_CELL_W` is doubled from the
/// cell-based tiers' own `10` -- Pet Bastion's own board is `28` tiles
/// wide (`pet-bastion::constants::BOARD_WIDTH`), and `bench`'s own live-
/// terminal proof was measured at a `56`-cell-wide, `10px`-per-cell board
/// (`560px` total, see `bench/src/bin/live.rs`'s own `BOARD_COLS`/
/// `CELL_PX_W`); at the OLD `10px` this backend painted a `280px`-wide
/// board -- exactly half the width the terminal is actually proven to
/// hold at 60fps, for no reason other than an unexamined hand-sync. `20px`
/// closes that gap (`28 * 20 = 560`) without touching Pet Bastion's own
/// tile count (`games/pet-bastion/src/constants.rs` stays untouched, per
/// this pass's own scope boundary) and without touching
/// `preview/src/raster.rs`'s own SEPARATE `SIXEL_PX_PER_CELL_W`/`_H`
/// (that constant belongs to the older per-tile sixel tier, not this one).
pub const PX_PER_CELL_W: u32 = 20;
pub const PX_PER_CELL_H: u32 = 19;

/// Matches `bench`'s own live-terminal-validated `MAX_COLORS` (`bench/src/
/// bin/live.rs --colors 256`) -- NOT `backend_sixel.rs`'s own
/// `encode_options`, which uses 48 (a per-tile-image palette never measured
/// live at 60fps with a moving scene). 256, not 32: a live 240-frame,
/// paced-at-60fps run at this exact board size confirmed 256 colours holds
/// 60.07fps with 98.8% of frames landing inside the 16.7ms budget, only
/// ~1ms more expensive than 32 -- real headroom this backend's own richer
/// night-garden gradients/shadows genuinely need (32 colours forces heavy
/// dithering-free banding on every gradient in `crate::render::sprites`;
/// 256 lets each one actually read as a smooth ramp).
const MAX_COLORS: u16 = 256;

fn encode_options() -> EncodeOptions {
    EncodeOptions { max_colors: MAX_COLORS, diffusion: 0.0, ..EncodeOptions::default() }
}

/// One encoded whole-board frame, ready for a host to place at `rect`'s own
/// origin.
#[derive(Clone, Debug)]
pub struct PixelFrame {
    pub rect: Rect,
    pub encoded: Vec<u8>,
}

/// Mirrors [`crate::render::backend_sixel::SixelOutput`]'s own reasoning:
/// keeps "genuinely nothing to paint" (a zero-sized board) distinct from a
/// real, if trivially small, encoded frame.
#[derive(Clone, Debug)]
pub enum PixelFrameOutput {
    Frame(PixelFrame),
    Empty,
}

fn tile_to_px(tile_x: f64, tile_y: f64) -> (f64, f64) {
    (tile_x * PX_PER_CELL_W as f64, tile_y * PX_PER_CELL_H as f64)
}

/// Paints `surface`'s own BOARD OVERLAY content -- everything that is
/// neither a [`super::TileId::is_dynamic_entity`] sprite NOR
/// [`super::TileId::is_board_environment`] terrain -- into `scene`, tile-
/// aligned: `BuildPad`, every `Tower*`, and `CircuitLink`, the three kinds
/// whose STATE can still change mid-run even though their own position
/// never moves (a drag gesture, a tower level-up/link, the pet's own
/// current anchor). Every `is_board_environment` tile was already painted
/// ONCE into the cached [`super::background::BoardBackground`] this
/// scene composites on top of via [`render_over_background`] -- painting
/// it again here every frame is exactly the redundant per-tile repaint
/// this pass's own background cache exists to eliminate (see
/// `super::background`'s own top-level doc comment for the full split).
///
/// Also drops a soft ambient light pool plus a soft contact shadow under
/// every placed tower before its own sprite -- this pass's own
/// "подсветка у источников света" / "мягкие тени под объектами" depth
/// requirements for the one kind of overlay content that reads as a
/// physical, lit object sitting on the ground.
fn paint_overlay_layer(scene: &mut Scene, surface: &Surface, dragging: bool) {
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            let cell = surface.get(x, y);
            let Some(art) = cell.art else { continue };
            if art.tile.is_dynamic_entity() || art.tile.is_board_environment() {
                continue;
            }
            let (cx, cy) = tile_to_px(x as f64 + 0.5, y as f64 + 0.5);
            if art.tile.is_tower() {
                push_ambient_glow(scene, cx, cy, PX_PER_CELL_W.max(PX_PER_CELL_H) as f64 * 0.85, cell.fg, 0.16);
                push_soft_shadow(scene, cx, cy + PX_PER_CELL_H as f64 * 0.22, PX_PER_CELL_W as f64 * 0.5, PX_PER_CELL_H as f64 * 0.24, 0.30);
            }
            paint_tile(scene, art.tile, art.variant, cell.fg, cell.bg, cx, cy, PX_PER_CELL_W as f64, PX_PER_CELL_H as f64, 1.0, dragging);
        }
    }
}

/// Pushes one [`DynamicStroke`] into `scene` as a real `StrokePath` -- a
/// straight line for `bulge == 0.0`, otherwise a genuine quadratic Bezier
/// curve through a control point offset perpendicular to the straight
/// from-to line (replacing the old two-straight-segment polyline
/// approximation through an offset midpoint -- this is now the actual
/// curve that approximation used to only gesture at).
fn push_dynamic_stroke(scene: &mut Scene, stroke: &DynamicStroke) {
    use uzor_urx_core::math::{Affine, BezPath, Brush, Color};
    use uzor_urx_core::scene::{DrawCommand, LineCap, LineJoin, Stroke};

    if stroke.width_px <= 0.0 || stroke.alpha <= 0.0 {
        return;
    }
    let (x0, y0) = tile_to_px(stroke.from_tile.0, stroke.from_tile.1);
    let (x1, y1) = tile_to_px(stroke.to_tile.0, stroke.to_tile.1);

    let mut path = BezPath::new();
    path.move_to((x0, y0));
    if stroke.bulge == 0.0 {
        path.line_to((x1, y1));
    } else {
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt();
        let (bx, by) = if len > 0.001 { (mx - dy / len * stroke.bulge as f64 * len, my + dx / len * stroke.bulge as f64 * len) } else { (mx, my) };
        path.quad_to((bx, by), (x1, y1));
    }

    let color = Color::from_rgba8(stroke.color.0, stroke.color.1, stroke.color.2, (stroke.alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
    let kstroke = Stroke { width: stroke.width_px, cap: LineCap::Round, join: LineJoin::Round, ..Stroke::default() };
    scene.push(DrawCommand::StrokePath { path, stroke: kstroke, brush: Brush::Solid(color), transform: Affine::IDENTITY });
}

/// Builds the OVERLAY-ONLY URX [`Scene`] for one frame -- `surface`'s own
/// board overlay layer (`paint_overlay_layer`'s own doc comment), then
/// every `stroke`, then every `dynamic` sprite (same painter's-order as
/// before: a stroke's own trail sits under the sprite riding along it),
/// then the whole-canvas vignette, last. Deliberately does NOT paint
/// `surface`'s own `is_board_environment` terrain any more -- that content
/// lives in a separately-cached [`super::background::BoardBackground`]
/// this scene is meant to be composited on top of via
/// [`render_over_background`], not repainted here every call. A caller
/// that only needs the CPU rasterisation cost of an already-built scene,
/// separately from the cost of building it, still has that split via
/// [`render_scene`]/[`render_over_background`] taking `&Scene` directly
/// (`bench`'s own URX-cost measurement).
///
/// `dragging` matches [`RenderBackend::project`](super::RenderBackend)'s
/// own parameter of the same name -- forwarded only to `Surface`'s overlay
/// layer (see `crate::render::sprites::paint_tile`'s own doc comment for
/// why: only `TileId::BuildPad` reads it, to gate its own glow). Every
/// dynamic sprite/stroke is an enemy/boss/pet/effect, never a build pad,
/// so the dynamic loop below never needs to pass it.
pub fn build_scene(surface: &Surface, dynamic: &[DynamicSprite], strokes: &[DynamicStroke], dragging: bool) -> Scene {
    let mut scene = Scene::new();
    paint_overlay_layer(&mut scene, surface, dragging);
    for stroke in strokes {
        push_dynamic_stroke(&mut scene, stroke);
    }
    for sprite in dynamic {
        let (cx, cy) = tile_to_px(sprite.tile_x, sprite.tile_y);
        let w = PX_PER_CELL_W as f64 * sprite.scale as f64;
        let h = PX_PER_CELL_H as f64 * sprite.scale as f64;
        // This pass's own "мягкие тени под объектами" depth requirement --
        // every grounded creature (never a transient effect pip, see
        // `TileId::casts_ground_shadow`'s own doc comment) drops a soft
        // contact shadow before its own sprite paints.
        if sprite.tile.casts_ground_shadow() {
            push_soft_shadow(&mut scene, cx, cy + h * 0.28, w * 0.42, h * 0.20, 0.28 * sprite.alpha);
        }
        paint_tile(&mut scene, sprite.tile, sprite.variant, sprite.fg, sprite.bg, cx, cy, w, h, sprite.alpha, false);
    }
    // Last, strictly on top of every tile/sprite/stroke above: the
    // whole-canvas night-garden vignette (`paint_vignette`'s own doc
    // comment) -- one extra `FillRect` regardless of board size or how
    // many entities this frame carries. Applied here (not baked into the
    // background) so it darkens the FULL composite -- background AND
    // overlay AND dynamic content -- every frame, exactly as it always
    // darkened the full board before this pass's own background cache
    // existed.
    let px_w = surface.width() as f64 * PX_PER_CELL_W as f64;
    let px_h = surface.height() as f64 * PX_PER_CELL_H as f64;
    paint_vignette(&mut scene, px_w, px_h);
    scene
}

/// Un-premultiplies `pixmap`'s own RGBA8 buffer -- `uzor_urx_cpu::Pixmap`
/// is premultiplied (`r/g/b` already scaled by `a`, per that crate's own
/// doc comment), `icy_sixel::SixelImage::try_from_rgba` expects STRAIGHT
/// alpha. `straight = premul * 255 / a` (rounded, clamped), `a` unchanged.
/// A fully transparent pixel (`a == 0`) has no recoverable original colour
/// -- it degrades to plain transparent black rather than dividing by zero,
/// matching [`PixelCanvas::new`]'s own all-zero convention. `pub(crate)`:
/// `super::background::BoardBackground`'s own doc comment names this as
/// the one other caller, converting a cached background's own cloned-and-
/// composited pixmap the identical way.
pub(crate) fn straight_alpha_from_premultiplied(pixmap: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixmap.pixels().len());
    for px in pixmap.pixels().chunks_exact(4) {
        let a = px[3];
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let unpremultiply = |c: u8| -> u8 { (((c as u32) * 255 + (a as u32) / 2) / (a as u32)).min(255) as u8 };
        out.extend_from_slice(&[unpremultiply(px[0]), unpremultiply(px[1]), unpremultiply(px[2]), a]);
    }
    out
}

/// Rasterises an already-built `scene` at `px_w x px_h` via URX's CPU
/// backend onto a FRESH, blank premultiplied [`Pixmap`], returning it
/// as-is (still premultiplied) -- the shared core [`render_scene`] and
/// `super::background::build_background` both call, so a fresh board
/// background is rasterised through the exact same code path an ordinary
/// (non-cached) scene always was. `pub(crate)`: an internal building
/// block, not part of this crate's own public API (a caller outside this
/// crate never needs the raw premultiplied `Pixmap`, only [`render_scene`]/
/// `render_over_background`'s own straight-alpha [`PixelCanvas`]).
///
/// A fresh [`CpuBackend`] every call -- see this module's own "No
/// persistent backend state" doc section; `CpuBackend` itself is
/// stateless config data, cheap to construct. `CpuBackend::render`'s only
/// documented error (`RenderError::ClipUnderflow`, an unbalanced
/// `PushClipRect`/`PopClip` pair) is unreachable from this module's own
/// scenes -- neither [`build_scene`] nor `crate::render::sprites` nor
/// `super::background::paint_background` ever pushes a clip -- but this
/// crate's own "never panic in library code" rule means a genuine (if
/// unreachable in testing) render error degrades to a blank pixmap rather
/// than a panic or a fabricated frame.
pub(crate) fn render_scene_premultiplied(scene: &Scene, px_w: u32, px_h: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(px_w, px_h);
    let _ = CpuBackend::new().render(scene, &mut pixmap);
    pixmap
}

/// Rasterises an already-built `scene` at `px_w x px_h` via URX's CPU
/// backend, converts the result to straight alpha, and returns it as a
/// [`PixelCanvas`] -- the pure, terminal-independent half of this backend
/// (see [`encode_frame`] for the SIXEL-encoding half; kept separate so a
/// caller such as `preview` can dump the raw RGBA canvas straight to a PNG
/// without paying an encode-then-decode round trip just to inspect what
/// was actually drawn, and so `bench` can time this call alone against
/// [`build_scene`]'s own separate cost).
pub fn render_scene(scene: &Scene, px_w: u32, px_h: u32) -> PixelCanvas {
    let pixmap = render_scene_premultiplied(scene, px_w, px_h);
    PixelCanvas::from_straight_rgba(px_w, px_h, straight_alpha_from_premultiplied(&pixmap))
}

/// Composites `scene` on top of `background`'s own already-rendered
/// [`BoardBackground`] -- a cheap `Pixmap` clone, then the SAME
/// `CpuBackend::render` call [`render_scene`] itself uses, just onto a
/// pre-populated destination instead of a blank one (`uzor-urx-cpu`'s own
/// `Pixmap::blend_pixel` -- what every fill/stroke primitive in this
/// crate's own scenes ultimately calls -- is a real source-over blend
/// against whatever is already in the destination pixmap, so painting
/// `scene` onto a clone of an already-painted background composites
/// correctly, it does not overwrite it). This is the whole point of
/// [`super::background::BoardBackground`]'s own cache: `scene` here only
/// ever carries [`build_scene`]'s own overlay/dynamic/vignette content,
/// never the several-hundred-tile environment layer `background` already
/// paid to rasterise once.
///
/// On a genuine (untested-reachable) `CpuBackend::render` error, this
/// degrades to the background ALONE (skipping whatever `scene` would have
/// added) rather than [`render_scene`]'s own blank-canvas fallback --
/// discarding a real, already-rendered background to fabricate a blank
/// frame would be the worse degrade of the two here.
pub fn render_over_background(background: &BoardBackground, scene: &Scene) -> PixelCanvas {
    let mut pixmap = background.pixmap().clone();
    let _ = CpuBackend::new().render(scene, &mut pixmap);
    PixelCanvas::from_straight_rgba(background.width_px(), background.height_px(), straight_alpha_from_premultiplied(&pixmap))
}

/// Builds `surface`'s own overlay-only scene ([`build_scene`]) and
/// composites it on top of `background`'s own already-cached
/// [`BoardBackground`] ([`render_over_background`]) -- the whole-frame
/// entry point a host calls every tick once it holds a `background`
/// built (and kept fresh) via `super::background::build_background`.
/// `dragging` is [`build_scene`]'s own parameter of the same name.
pub fn compose_frame(background: &BoardBackground, surface: &Surface, dynamic: &[DynamicSprite], strokes: &[DynamicStroke], dragging: bool) -> PixelCanvas {
    render_over_background(background, &build_scene(surface, dynamic, strokes, dragging))
}

/// Encodes an already-composed `canvas` to SIXEL, as ONE whole-board image
/// -- see this module's own doc comment for why never per-tile. Failure
/// mode matches `backend_sixel.rs`'s own documented one: encoding a small,
/// fixed-size, in-memory RGBA buffer is not expected to fail in practice,
/// but this crate's "never panic in library code" rule means a genuine (if
/// unreachable in testing) encoder error degrades to
/// [`PixelFrameOutput::Empty`] rather than a panic or a fabricated frame.
pub fn encode_frame(canvas: &PixelCanvas, rect: Rect) -> PixelFrameOutput {
    if canvas.width == 0 || canvas.height == 0 || rect.is_empty() {
        return PixelFrameOutput::Empty;
    }
    let Ok(image) = SixelImage::try_from_rgba(canvas.rgba.clone(), canvas.width as usize, canvas.height as usize) else {
        return PixelFrameOutput::Empty;
    };
    let Ok(encoded_text) = image.with_background_mode(BackgroundMode::Transparent).encode_with(&encode_options()) else {
        return PixelFrameOutput::Empty;
    };
    PixelFrameOutput::Frame(PixelFrame { rect, encoded: encoded_text.into_bytes() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::background::build_background;
    use crate::render::{Rgb, SurfaceCell, TileArt, TileId};

    #[test]
    fn compose_frame_produces_a_canvas_sized_from_the_surface_and_the_cell_pixel_size() {
        let surface = Surface::new(3, 2, SurfaceCell::BLANK);
        let background = build_background(&surface);
        let canvas = compose_frame(&background, &surface, &[], &[], false);
        assert_eq!(canvas.width, 3 * PX_PER_CELL_W);
        assert_eq!(canvas.height, 2 * PX_PER_CELL_H);
    }

    #[test]
    fn a_dynamic_tile_kind_on_the_surface_is_never_painted_by_the_overlay_layer() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, SurfaceCell { glyph: 'm', fg: Rgb(170, 220, 120), bg: None, art: Some(TileArt { tile: TileId::EnemyMite, variant: 0 }) });
        let blank_surface = Surface::new(1, 1, SurfaceCell::BLANK);
        let empty = compose_frame(&build_background(&blank_surface), &blank_surface, &[], &[], false);
        let with_enemy_on_surface = compose_frame(&build_background(&surface), &surface, &[], &[], false);
        assert_eq!(empty.rgba, with_enemy_on_surface.rgba, "an enemy TileId left on the Surface must be skipped by the overlay layer");
    }

    #[test]
    fn a_dynamic_sprite_actually_paints_something() {
        let surface = Surface::new(4, 4, SurfaceCell::BLANK);
        let background = build_background(&surface);
        let sprite = DynamicSprite { tile: TileId::EnemyMite, variant: 0, fg: Rgb(170, 220, 120), bg: None, tile_x: 2.0, tile_y: 2.0, scale: 1.0, alpha: 1.0 };
        let canvas = compose_frame(&background, &surface, &[sprite], &[], false);
        assert!(canvas.rgba.chunks_exact(4).any(|px| px[3] > 0), "a dynamic sprite in range must paint at least one visible pixel");
    }

    #[test]
    fn a_stroke_actually_paints_something() {
        let surface = Surface::new(4, 4, SurfaceCell::BLANK);
        let background = build_background(&surface);
        let stroke = DynamicStroke { from_tile: (0.5, 0.5), to_tile: (3.5, 3.5), color: Rgb(255, 0, 0), width_px: 2.0, bulge: 0.0, alpha: 1.0 };
        let canvas = compose_frame(&background, &surface, &[], &[stroke], false);
        assert!(canvas.rgba.chunks_exact(4).any(|px| px[3] > 0));
    }

    #[test]
    fn a_bulged_stroke_actually_paints_something() {
        let surface = Surface::new(6, 6, SurfaceCell::BLANK);
        let background = build_background(&surface);
        let stroke = DynamicStroke { from_tile: (0.5, 0.5), to_tile: (5.5, 0.5), color: Rgb(0, 255, 0), width_px: 2.0, bulge: 0.35, alpha: 1.0 };
        let canvas = compose_frame(&background, &surface, &[], &[stroke], false);
        assert!(canvas.rgba.chunks_exact(4).any(|px| px[3] > 0), "a bulged (quadratic-curve) stroke must still paint visible pixels");
    }

    #[test]
    fn dragging_adds_a_glow_to_build_pads_that_a_non_dragging_frame_never_pays_for() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, SurfaceCell { glyph: 'o', fg: Rgb(110, 110, 130), bg: None, art: Some(TileArt { tile: TileId::BuildPad, variant: 0 }) });
        let background = build_background(&surface);
        let quiet = compose_frame(&background, &surface, &[], &[], false);
        let glowing = compose_frame(&background, &surface, &[], &[], true);
        assert_ne!(quiet.rgba, glowing.rgba, "a build pad must only glow while the caller reports a drag in progress");
    }

    #[test]
    fn encode_frame_of_a_real_composed_frame_starts_with_the_sixel_dcs_introducer() {
        let mut surface = Surface::new(2, 2, SurfaceCell::BLANK);
        surface.set(0, 0, SurfaceCell { glyph: '.', fg: Rgb(70, 80, 65), bg: None, art: Some(TileArt { tile: TileId::Path, variant: 0 }) });
        let background = build_background(&surface);
        let canvas = compose_frame(&background, &surface, &[], &[], false);
        let output = encode_frame(&canvas, Rect::new(0, 0, 2, 2));
        let PixelFrameOutput::Frame(frame) = output else { panic!("expected a real frame") };
        assert!(frame.encoded.starts_with(b"\x1b"), "sixel output must start with the DCS introducer ESC");
    }

    #[test]
    fn encode_frame_of_a_zero_sized_canvas_reports_empty_not_a_fabricated_frame() {
        let canvas = PixelCanvas::new(0, 0);
        let output = encode_frame(&canvas, Rect::new(0, 0, 0, 0));
        assert!(matches!(output, PixelFrameOutput::Empty));
    }

    #[test]
    fn straight_alpha_conversion_recovers_the_original_colour_from_a_premultiplied_pixel() {
        // A pixel that was originally opaque-ish red (255, 0, 0) at 50%
        // alpha, stored premultiplied: r = 255*128/255 rounds to 128,
        // g/b stay 0, a stays 128.
        let mut pixmap = Pixmap::new(1, 1);
        pixmap.set_pixel(0, 0, [128, 0, 0, 128]);
        let straight = straight_alpha_from_premultiplied(&pixmap);
        assert_eq!(straight[3], 128, "alpha itself must pass through unchanged");
        // Un-premultiplying 128 at alpha 128 recovers ~255 (128*255/128),
        // within integer-rounding tolerance of the original 255.
        assert!(straight[0] >= 253, "expected the red channel to recover to ~255, got {}", straight[0]);
    }

    #[test]
    fn straight_alpha_conversion_of_a_fully_transparent_pixel_is_transparent_black_not_a_divide_by_zero_panic() {
        let pixmap = Pixmap::new(1, 1);
        let straight = straight_alpha_from_premultiplied(&pixmap);
        assert_eq!(straight, vec![0, 0, 0, 0]);
    }

    #[test]
    fn compose_frame_matches_render_over_background_of_the_same_build_scene_call() {
        let mut surface = Surface::new(4, 3, SurfaceCell::BLANK);
        surface.set(1, 1, SurfaceCell { glyph: 'T', fg: Rgb(200, 200, 200), bg: None, art: Some(TileArt { tile: TileId::TowerNeedle, variant: 0 }) });
        let background = build_background(&surface);
        let via_compose_frame = compose_frame(&background, &surface, &[], &[], false);
        let via_manual_split = render_over_background(&background, &build_scene(&surface, &[], &[], false));
        assert_eq!(via_compose_frame.rgba, via_manual_split.rgba, "compose_frame must be exactly build_scene + render_over_background, nothing more");
    }

    #[test]
    fn render_over_background_never_discards_the_background_when_the_overlay_scene_is_empty() {
        let mut surface = Surface::new(3, 3, SurfaceCell::BLANK);
        surface.set(1, 1, SurfaceCell { glyph: '.', fg: Rgb(101, 88, 63), bg: None, art: Some(TileArt { tile: TileId::Path, variant: 0 }) });
        let background = build_background(&surface);
        let empty_scene = Scene::new();
        let canvas = render_over_background(&background, &empty_scene);
        assert!(canvas.rgba.chunks_exact(4).any(|px| px[3] > 0), "a background with real content composited with an empty overlay scene must still show that background");
    }

    #[test]
    fn a_tower_gets_an_ambient_glow_and_shadow_the_cached_background_alone_never_paints() {
        let mut surface = Surface::new(3, 3, SurfaceCell::BLANK);
        surface.set(1, 1, SurfaceCell { glyph: 'T', fg: Rgb(200, 200, 200), bg: None, art: Some(TileArt { tile: TileId::TowerNeedle, variant: 0 }) });
        let background = build_background(&surface);
        let background_alone = render_over_background(&background, &Scene::new());
        let with_tower = compose_frame(&background, &surface, &[], &[], false);
        assert_ne!(background_alone.rgba, with_tower.rgba, "a placed tower's own glow/shadow/body must paint something the background cache alone never does");
    }
}
