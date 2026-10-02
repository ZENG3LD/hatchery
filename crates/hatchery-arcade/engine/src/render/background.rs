//! The board's own CONTINUOUS static background -- ground, decor, routes,
//! and the water pool -- painted as one whole-frame [`Scene`] and rendered
//! ONCE into a cached, premultiplied [`Pixmap`] a host reuses verbatim
//! every later frame, instead of the old per-`SurfaceCell` painting
//! (`crate::render::backend_pixel`'s own `paint_static_layer`, now
//! `paint_overlay_layer`) that made every one of these 392 board tiles its
//! own separately-toned rectangle -- readable as a checkerboard no amount
//! of colour tuning could fix, since the boundary a viewer's eye locks
//! onto is the TILE GRID ITSELF, not any one tile's own colour choice.
//!
//! # What lives in the cache, what stays per-frame
//!
//! [`crate::render::TileId::is_board_environment`] is the exact split:
//! `Ground`/`Rock`/`Plant`/`WaterPool`/`Firefly`/`Path`/`Choke`/
//! `Heartseed`/`PetAnchor` are a PURE FUNCTION of the board's own fixed
//! layout -- they never change for the lifetime of a run -- and are
//! painted here, once. `BuildPad` (toggles with a drag gesture), every
//! `TileId::Tower*` (level/link/placement can change), and `CircuitLink`
//! (follows the pet) all stay in `backend_pixel`'s own per-frame overlay
//! pass instead, painted fresh from `Surface` every call exactly as
//! before -- see that module's own `paint_overlay_layer` doc comment.
//!
//! # No hard edge anywhere in this layer, by construction
//!
//! Every shape [`paint_background`] pushes is either a full-canvas flat
//! undercoat (covers everything, so nothing is ever left transparent) or a
//! fade-to-transparent gradient / a round-capped, round-joined stroke --
//! there is no flat-filled rectangle sized to one board tile anywhere in
//! this file. That is deliberate: a hard edge is what makes a grid line
//! visible in the first place, so a layer built entirely from soft edges
//! structurally CANNOT produce one, regardless of where any one shape's
//! own centre happens to fall relative to the tile grid underneath it.
//! Ground/decor blobs are placed on their own coarse grid, deliberately
//! NOT the board's 1-tile grid, each jittered by up to half its own pitch
//! (see [`unit_hash`]) so no blob boundary -- soft or otherwise -- ever
//! lines up with a tile edge even coincidentally.
//!
//! # Seed and cache-invalidation contract
//!
//! [`background_seed`] hashes exactly the `is_board_environment` cells a
//! `Surface` carries (position, kind, tone variant, colour, glyph) -- a
//! pure function of the BOARD's own static layout, never of wall-clock
//! time or render order, matching this pass's own "детерминированно...
//! чтобы он не мигал между кадрами" requirement. A host is responsible for
//! the actual caching: call [`background_seed`] each frame, and only call
//! [`build_background`] again when that seed changes from whatever
//! [`BoardBackground::seed`] it already holds (Pet Bastion's own board
//! layout never changes mid-run, so in practice a real host builds this
//! exactly once per run and never again). This module itself holds no
//! hidden static/thread-local cache -- same "no persistent backend state"
//! discipline `crate::render::backend_pixel`'s own module doc already
//! documents for every backend in this crate.
//!
//! **Feed this a TERRAIN-ONLY `Surface`, never a live one.** `Surface`
//! holds exactly one `art` per cell -- once a tower is placed on a tile
//! that used to carry a `Rock`/`Plant`/`Firefly` decor mark, that cell's
//! `art` is REPLACED, not layered. Computing [`background_seed`]/
//! [`build_background`] from a live, post-placement `Surface` would make
//! the seed drift (and force a full rebuild) every time a tower lands on
//! a decor tile -- exactly the per-tile repaint cost this whole cache
//! exists to eliminate. A game-specific adapter is expected to expose a
//! small, snapshot-independent "environment only" `Surface` builder for
//! this (e.g. `gate4agent-arcade-pet-bastion-render::terrain_surface`,
//! which is nothing more than that crate's own terrain-painting pass with
//! no snapshot-driven overlay ever applied on top) -- a HOST calls that
//! once, keeps the resulting `Surface` around purely to feed this module,
//! and never touches it again.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use uzor_urx_core::math::BezPath;
use uzor_urx_core::scene::{FillRule, Scene};
use uzor_urx_cpu::Pixmap;

use super::backend_pixel::{render_scene_premultiplied, PX_PER_CELL_H, PX_PER_CELL_W};
use super::sprites::{self, circle_path, darken, linear, push_fill, push_rect, push_stroke_brush, push_stroke_path, radial, stop, tint};
use super::{Rgb, Surface, TileId};
use crate::render::sprites::paint_tile;

/// Coarse-grid pitch (in board tiles) for the larger, low-frequency ground
/// blob octave -- "несколько клеток" per blob, several tiles wide, never
/// one.
const GROUND_OCTAVE_A_PITCH_TILES: f64 = 6.0;
const GROUND_OCTAVE_A_ALPHA: f32 = 0.5;
/// A second, finer octave layered on top for texture richness -- still
/// comfortably multi-tile (well over a 2-tile-diameter blob), never
/// sub-tile.
const GROUND_OCTAVE_B_PITCH_TILES: f64 = 3.0;
const GROUND_OCTAVE_B_ALPHA: f32 = 0.28;
/// Blob radius as a fraction of its own grid pitch -- deliberately well
/// under `1.0` (adjacent blobs need not touch): a gap between two blobs
/// simply shows the flat, full-canvas undercoat [`paint_ground`] already
/// laid down underneath everything, which is itself one uniform,
/// non-grid-aligned fill -- there is no hard edge for a gap to expose
/// either way (see this module's own "No hard edge anywhere in this
/// layer" doc section). Kept LOW because CPU rasterisation cost here is
/// dominated by total pixel area covered, which scales with the SQUARE of
/// this ratio regardless of pitch (`bench`'s own `urx_render` binary
/// measured the original, much richer `0.72` ratio at ~33ms p50 for
/// `build_background` alone -- most of it this one multiplier; a large
/// PITCH, on the other hand, is free -- it only reduces the blob COUNT,
/// which lowers per-blob fixed overhead without changing total pixel-area
/// cost at all, since fewer/bigger blobs at the same ratio cover the same
/// total area).
const GROUND_BLOB_RADIUS_RATIO: f64 = 0.32;
/// How far a decor mark's own painted centre may drift from its owning
/// tile's centre, as a fraction of one cell -- "по непрерывным
/// координатам кадра, а не по центрам клеток".
const DECOR_JITTER_FRACTION: f64 = 0.85;
/// A gentler jitter for the water pool -- enough to soften its own
/// otherwise perfectly diamond-shaped Chebyshev-ring footprint without
/// losing the "ringing the Heartseed" shape a player reads at a glance.
const POOL_JITTER_FRACTION: f64 = 0.28;
/// Route ribbon width, as a fraction of the shorter cell dimension --
/// deliberately close to the OLD per-tile inset margin's own effective
/// width (`sprites::push_road`'s own `0.19` margin leaves `1 - 2*0.19 =
/// 0.62` of the tile), so the route reads the same visual weight, just
/// with a continuous, round-jointed edge instead of a stepped one.
const ROUTE_WIDTH_FRACTION: f64 = 0.62;

fn tile_center_px(x: u16, y: u16) -> (f64, f64) {
    ((x as f64 + 0.5) * PX_PER_CELL_W as f64, (y as f64 + 0.5) * PX_PER_CELL_H as f64)
}

/// Cheap deterministic 32-bit hash of an integer coordinate pair plus a
/// `salt` (a distinct constant per independent decision made off the same
/// coordinate) -- this module's OWN copy of the same avalanche-mix shape
/// `gate4agent-arcade-pet-bastion-render::tile_hash`/`gate4agent-arcade-
/// bench::urx_render::tile_hash` already use (see either one's own doc
/// comment for why each crate keeps its own copy rather than importing
/// one): `engine` never depends on a specific game crate just to scatter
/// its own background texture -- the wrong dependency direction this
/// crate's own top-level doc comment already rules out.
fn coord_hash(x: i64, y: i64, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1);
    h ^= (y as u32).wrapping_mul(0x85EB_CA77).rotate_left(13);
    h ^= salt.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// [`coord_hash`], normalised to `[0.0, 1.0)`.
fn unit_hash(x: i64, y: i64, salt: u32) -> f64 {
    coord_hash(x, y, salt) as f64 / (u32::MAX as f64 + 1.0)
}

/// The colour every ground blob tones off of -- the first
/// [`TileId::Ground`] cell's own `fg` found scanning row-major, so this
/// module never invents its own hardcoded hue (that colour choice belongs
/// entirely to the game-specific adapter that built `surface`, exactly as
/// `sprites::paint_tile`'s own `TileId::Ground` arm already reads `fg`
/// from the caller instead of a baked-in constant). A board with no real
/// `Ground` cell at all (never true for a real Pet Bastion board, but
/// `Surface` places no such guarantee on a caller) degrades to a plain
/// dark neutral rather than panicking.
fn reference_ground_color(surface: &Surface) -> Rgb {
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            if surface.get(x, y).art.map(|a| a.tile) == Some(TileId::Ground) {
                return surface.get(x, y).fg;
            }
        }
    }
    Rgb(32, 40, 34)
}

/// One octave of large, soft, overlapping ground blobs on a grid pitched
/// `pitch_tiles` apart -- deliberately INDEPENDENT of the board's own
/// 1-tile grid (see this module's own "No hard edge anywhere in this
/// layer" doc section), each centre jittered by up to half its own pitch.
/// Every blob is a single fade-to-transparent radial gradient -- there is
/// no hard edge here for a grid line to ever coincide with.
fn paint_ground_octave(scene: &mut Scene, px_w: f64, px_h: f64, base: Rgb, pitch_tiles: f64, alpha: f32, salt: u32) {
    let pitch_x = pitch_tiles * PX_PER_CELL_W as f64;
    let pitch_y = pitch_tiles * PX_PER_CELL_H as f64;
    if pitch_x <= 0.0 || pitch_y <= 0.0 {
        return;
    }
    let radius = pitch_x.max(pitch_y) * GROUND_BLOB_RADIUS_RATIO;
    let cols = (px_w / pitch_x).ceil() as i64 + 2;
    let rows = (px_h / pitch_y).ceil() as i64 + 2;
    for gy in -1..rows {
        for gx in -1..cols {
            let jitter_x = (unit_hash(gx, gy, salt) - 0.5) * pitch_x;
            let jitter_y = (unit_hash(gx, gy, salt.wrapping_add(1)) - 0.5) * pitch_y;
            let cx = (gx as f64 + 0.5) * pitch_x + jitter_x;
            let cy = (gy as f64 + 0.5) * pitch_y + jitter_y;
            if cx < -radius || cx > px_w + radius || cy < -radius || cy > px_h + radius {
                continue;
            }
            let toned = match coord_hash(gx, gy, salt.wrapping_add(2)) % 4 {
                0 => darken(base, 780),
                1 => tint(base, 90),
                2 => darken(base, 900),
                _ => base,
            };
            let brush = radial(vec![stop(0.0, toned, alpha), stop(1.0, toned, 0.0)], cx, cy, radius);
            push_fill(scene, circle_path(cx, cy, radius), FillRule::NonZero, brush);
        }
    }
}

/// A full-canvas opaque undercoat, then two overlapping octaves of soft
/// tonal blobs several tiles wide -- see this module's own top-level doc
/// comment for why this replaces the old per-tile `TileId::Ground`
/// painting entirely.
fn paint_ground(scene: &mut Scene, surface: &Surface, px_w: f64, px_h: f64) {
    let base = reference_ground_color(surface);
    push_rect(scene, 0.0, 0.0, px_w, px_h, 0.0, base, 1.0);
    paint_ground_octave(scene, px_w, px_h, base, GROUND_OCTAVE_A_PITCH_TILES, GROUND_OCTAVE_A_ALPHA, 101);
    paint_ground_octave(scene, px_w, px_h, base, GROUND_OCTAVE_B_PITCH_TILES, GROUND_OCTAVE_B_ALPHA, 211);
}

/// Every `Rock`/`Plant`/`Firefly` decor mark `surface` carries, repainted
/// at a CONTINUOUS pixel position jittered off its own owning tile's
/// centre (never the tile centre itself) -- this pass's own "декор...
/// раскладывай по непрерывным координатам кадра, а не по центрам клеток"
/// requirement. Reuses [`sprites::paint_tile`]'s own existing shape for
/// each kind (rock/plant/firefly), just at a shifted centre, rather than
/// re-implementing those shapes here.
fn paint_decor_scatter(scene: &mut Scene, surface: &Surface) {
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            let cell = surface.get(x, y);
            let Some(art) = cell.art else { continue };
            if !matches!(art.tile, TileId::Rock | TileId::Plant | TileId::Firefly) {
                continue;
            }
            let (base_cx, base_cy) = tile_center_px(x, y);
            let jx = (unit_hash(x as i64, y as i64, 401) - 0.5) * PX_PER_CELL_W as f64 * DECOR_JITTER_FRACTION;
            let jy = (unit_hash(x as i64, y as i64, 402) - 0.5) * PX_PER_CELL_H as f64 * DECOR_JITTER_FRACTION;
            paint_tile(scene, art.tile, art.variant, cell.fg, None, base_cx + jx, base_cy + jy, PX_PER_CELL_W as f64, PX_PER_CELL_H as f64, 1.0, false);
        }
    }
}

/// The still garden pool ringing the Heartseed -- one soft outer glow plus
/// one solid-ish body blob per `WaterPool` tile, each nudged by a gentle
/// jitter and sized to overlap its own neighbours, so the whole cluster
/// reads as ONE continuous puddle with a soft rim instead of a chain of
/// separately-inset tiles.
fn paint_water_pool(scene: &mut Scene, surface: &Surface) {
    let half = PX_PER_CELL_W.min(PX_PER_CELL_H) as f64 / 2.0;
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            let cell = surface.get(x, y);
            let Some(art) = cell.art else { continue };
            if art.tile != TileId::WaterPool {
                continue;
            }
            let (base_cx, base_cy) = tile_center_px(x, y);
            let jx = (unit_hash(x as i64, y as i64, 501) - 0.5) * PX_PER_CELL_W as f64 * POOL_JITTER_FRACTION;
            let jy = (unit_hash(x as i64, y as i64, 502) - 0.5) * PX_PER_CELL_H as f64 * POOL_JITTER_FRACTION;
            let cx = base_cx + jx;
            let cy = base_cy + jy;
            let glow = radial(vec![stop(0.0, cell.fg, 0.4), stop(1.0, cell.fg, 0.0)], cx, cy, half * 2.2);
            push_fill(scene, circle_path(cx, cy, half * 2.2), FillRule::NonZero, glow);
            let deep = radial(vec![stop(0.0, darken(cell.fg, 500), 0.9), stop(0.7, cell.fg, 0.85), stop(1.0, tint(cell.fg, 200), 0.55)], cx, cy, half * 1.18);
            push_fill(scene, circle_path(cx, cy, half * 1.18), FillRule::NonZero, deep);
        }
    }
}

/// One route (`Path`/`Choke`) tile's own connecting edges to its already-
/// scanned East/South neighbour, if any -- painting each edge exactly
/// once (a route tile's own North/West edge is always covered by that
/// neighbour's own East/South scan instead) as a straight, round-capped
/// stroke between the two tile centres. A solid colour when both ends
/// share one, otherwise a linear gradient between the two -- the one
/// place a route's own colour genuinely changes (the `Path`-to-`Choke`
/// seam), so the transition itself is smooth rather than a hard seam.
fn paint_route_segment(scene: &mut Scene, surface: &Surface, x0: u16, y0: u16, x1: u16, y1: u16, width: f64) {
    let a = surface.get(x0, y0);
    let b = surface.get(x1, y1);
    let (ax, ay) = tile_center_px(x0, y0);
    let (bx, by) = tile_center_px(x1, y1);
    let mut path = BezPath::new();
    path.move_to((ax, ay));
    path.line_to((bx, by));
    if a.fg == b.fg {
        push_stroke_path(scene, path, width, a.fg, 1.0, None);
    } else {
        let brush = linear(vec![stop(0.0, a.fg, 1.0), stop(1.0, b.fg, 1.0)], ax, ay, bx, by);
        push_stroke_brush(scene, path, width, brush, None);
    }
}

/// Every route tile's own connectivity mask (`ROAD_NORTH`/`ROAD_EAST`/
/// `ROAD_SOUTH`/`ROAD_WEST`, carried in `TileArt::variant` -- the same
/// bits `sprites::push_road`'s own doc comment documents) rebuilt as one
/// continuous ribbon: a filled, round cap at every route tile's own centre
/// (guarantees full coverage at dead ends and junctions regardless of how
/// the stroker below joins independently-pushed segments) plus one
/// straight, round-capped/round-joined stroke per connecting edge. The
/// union of round caps and round joins is what removes the OLD per-tile
/// design's own stepped, grid-following edge at every turn -- this pass's
/// own "край дорожки не должен быть ступенчатым по клеткам" requirement --
/// while every tile centre a route actually visits stays EXACTLY where it
/// always was, so the route stays exactly as readable/predictable as
/// before ("тропы... должны оставаться читаемыми").
fn paint_routes(scene: &mut Scene, surface: &Surface) {
    let ribbon = PX_PER_CELL_W.min(PX_PER_CELL_H) as f64 * ROUTE_WIDTH_FRACTION;
    let cap_r = ribbon / 2.0;
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            let cell = surface.get(x, y);
            let Some(art) = cell.art else { continue };
            if !matches!(art.tile, TileId::Path | TileId::Choke) {
                continue;
            }
            let (cx, cy) = tile_center_px(x, y);
            sprites::push_circle(scene, cx, cy, cap_r, cell.fg, 1.0);
            let mask = art.variant;
            if mask & sprites::ROAD_EAST != 0 && x + 1 < surface.width() {
                paint_route_segment(scene, surface, x, y, x + 1, y, ribbon);
            }
            if mask & sprites::ROAD_SOUTH != 0 && y + 1 < surface.height() {
                paint_route_segment(scene, surface, x, y, x, y + 1, ribbon);
            }
        }
    }
}

/// [`TileId::Heartseed`]'s own glow and every [`TileId::PetAnchor`] marker
/// -- both gameplay-meaningful LANDMARK positions a player reads
/// precisely, so unlike decor these stay painted exactly at their own tile
/// centre (no jitter): baked in here since neither one ever moves or
/// changes appearance for the lifetime of a run.
fn paint_static_markers(scene: &mut Scene, surface: &Surface) {
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            let cell = surface.get(x, y);
            let Some(art) = cell.art else { continue };
            if !matches!(art.tile, TileId::Heartseed | TileId::PetAnchor) {
                continue;
            }
            let (cx, cy) = tile_center_px(x, y);
            paint_tile(scene, art.tile, art.variant, cell.fg, None, cx, cy, PX_PER_CELL_W as f64, PX_PER_CELL_H as f64, 1.0, false);
        }
    }
}

/// Paints the board's own complete continuous background into `scene`:
/// ground, decor, routes, the water pool, then the Heartseed/anchor
/// markers on top -- the same painter's-order the old per-tile
/// `paint_terrain` pass used (ground, decor, routes, pool, markers), kept
/// identical here so a route/pool edge is never visually clipped by a
/// stray decor jitter painted after it.
pub(crate) fn paint_background(scene: &mut Scene, surface: &Surface, px_w: f64, px_h: f64) {
    paint_ground(scene, surface, px_w, px_h);
    paint_decor_scatter(scene, surface);
    paint_routes(scene, surface);
    paint_water_pool(scene, surface);
    paint_static_markers(scene, surface);
}

/// Hashes exactly `surface`'s own [`TileId::is_board_environment`] cells
/// (position, kind, tone variant, colour, glyph), row-major, plus the
/// board's own width/height -- see this module's own "Seed and cache-
/// invalidation contract" doc section for the full contract a host relies
/// on this for. Built on [`DefaultHasher`] (SipHash): its exact algorithm
/// is not guaranteed stable ACROSS Rust versions/builds, which is
/// irrelevant here -- this seed is only ever compared within one running
/// process against a [`BoardBackground`] that process itself built moments
/// earlier, never persisted or compared across restarts.
pub fn background_seed(surface: &Surface) -> u64 {
    let mut hasher = DefaultHasher::new();
    surface.width().hash(&mut hasher);
    surface.height().hash(&mut hasher);
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            let cell = surface.get(x, y);
            let Some(art) = cell.art else { continue };
            if art.tile.is_dynamic_entity() || !art.tile.is_board_environment() {
                continue;
            }
            (x, y, art.tile, art.variant, cell.glyph, cell.fg.0, cell.fg.1, cell.fg.2).hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// A fully-painted, continuous board background -- ground, decor, routes,
/// the water pool, and the Heartseed/anchor markers -- rendered ONCE from
/// a board's own [`TileId::is_board_environment`] tiles and cached
/// premultiplied, ready for [`super::backend_pixel::render_over_background`]
/// to clone-and-composite every later frame's own overlay/dynamic content
/// on top, instead of repainting several hundred board-tile shapes every
/// single frame. See this module's own top-level doc comment for the
/// full cache/seed contract a HOST (not this struct) is responsible for.
pub struct BoardBackground {
    pixmap: Pixmap,
    px_w: u32,
    px_h: u32,
    seed: u64,
}

impl BoardBackground {
    /// The [`background_seed`] this background was built from -- a host
    /// compares this against a freshly-computed `background_seed(surface)`
    /// each frame to decide whether this cache is still valid.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn width_px(&self) -> u32 {
        self.px_w
    }

    pub fn height_px(&self) -> u32 {
        self.px_h
    }

    /// The cached premultiplied pixmap itself -- crate-internal only (see
    /// `crate::render::backend_pixel::render_over_background`'s own doc
    /// comment for the one caller); `Pixmap` is an internal URX type this
    /// crate never re-exports, exactly like `crate::render::backend_pixel`'s
    /// own module doc explains for `PixelCanvas` staying the one pixel
    /// buffer type this crate exposes across its own boundary.
    pub(crate) fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }
}

/// Builds a fresh [`BoardBackground`] from `surface`'s own board-
/// environment tiles -- sizes the canvas from `surface`'s own tile
/// dimensions times [`PX_PER_CELL_W`]/[`PX_PER_CELL_H`], paints
/// [`paint_background`] into a scene, and rasterises it exactly once via
/// [`render_scene_premultiplied`]. Expensive relative to a single overlay
/// frame (this is the whole cost this pass's own caching exists to pay
/// only once) -- a caller holds onto the result and calls this again only
/// when [`background_seed`] changes (see this module's own top-level doc
/// comment).
pub fn build_background(surface: &Surface) -> BoardBackground {
    let px_w = surface.width() as u32 * PX_PER_CELL_W;
    let px_h = surface.height() as u32 * PX_PER_CELL_H;
    let mut scene = Scene::new();
    paint_background(&mut scene, surface, px_w as f64, px_h as f64);
    let pixmap = render_scene_premultiplied(&scene, px_w, px_h);
    BoardBackground { pixmap, px_w, px_h, seed: background_seed(surface) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{SurfaceCell, TileArt};

    fn env_cell(tile: TileId, variant: u8, fg: Rgb) -> SurfaceCell {
        SurfaceCell { glyph: '.', fg, bg: None, art: Some(TileArt { tile, variant }) }
    }

    fn small_surface() -> Surface {
        let mut surface = Surface::new(3, 3, SurfaceCell::BLANK);
        for y in 0..3 {
            for x in 0..3 {
                surface.set(x, y, env_cell(TileId::Ground, ((x + y) % 4) as u8, Rgb(20, 40, 30)));
            }
        }
        surface.set(1, 1, env_cell(TileId::Heartseed, 0, Rgb(255, 215, 0)));
        surface
    }

    /// A `small_surface` widened by one BLANK (no `art` at all) column --
    /// the one position [`background_seed_ignores_overlay_only_content`]
    /// paints a tower onto, so that test genuinely exercises "overlay
    /// content at a position with no environment content of its own"
    /// rather than the different, and legitimately seed-changing, case
    /// [`background_seed_changes_when_environment_content_changes`] covers
    /// (a tower REPLACING an environment tile that used to be there -- see
    /// `gate4agent_arcade_pet_bastion_render::terrain_surface`'s own doc
    /// comment for why a real host must never hand this module a `Surface`
    /// where that can happen).
    fn surface_with_a_blank_gutter_column() -> Surface {
        let mut surface = Surface::new(4, 3, SurfaceCell::BLANK);
        for y in 0..3 {
            for x in 0..3 {
                surface.set(x, y, env_cell(TileId::Ground, ((x + y) % 4) as u8, Rgb(20, 40, 30)));
            }
        }
        surface.set(1, 1, env_cell(TileId::Heartseed, 0, Rgb(255, 215, 0)));
        surface
    }

    #[test]
    fn background_seed_is_stable_across_repeated_calls_on_the_same_board() {
        let surface = small_surface();
        assert_eq!(background_seed(&surface), background_seed(&surface), "the same board must hash to the same seed every call, never depending on time or call order");
    }

    #[test]
    fn background_seed_changes_when_environment_content_changes() {
        let mut surface = small_surface();
        let base = background_seed(&surface);
        surface.set(0, 0, env_cell(TileId::Rock, 2, Rgb(94, 96, 104)));
        assert_ne!(background_seed(&surface), base, "changing a board-environment tile's own kind must change the seed");
    }

    #[test]
    fn background_seed_ignores_overlay_only_content() {
        let mut surface = surface_with_a_blank_gutter_column();
        let base = background_seed(&surface);
        surface.set(3, 0, SurfaceCell { glyph: 'T', fg: Rgb(200, 200, 200), bg: None, art: Some(TileArt { tile: TileId::TowerNeedle, variant: 1 }) });
        assert_eq!(background_seed(&surface), base, "placing a tower on a position with no environment content of its own must never change the cached background's own seed");
    }

    #[test]
    fn build_background_sizes_the_pixmap_from_the_surface_and_the_cell_pixel_size() {
        let surface = small_surface();
        let background = build_background(&surface);
        assert_eq!(background.width_px(), 3 * PX_PER_CELL_W);
        assert_eq!(background.height_px(), 3 * PX_PER_CELL_H);
        assert_eq!(background.seed(), background_seed(&surface));
    }

    #[test]
    fn build_background_actually_paints_visible_pixels() {
        let surface = small_surface();
        let background = build_background(&surface);
        assert!(background.pixmap().pixels().chunks_exact(4).any(|px| px[3] > 0), "a real board must produce at least one non-transparent pixel");
    }

    #[test]
    fn build_background_is_deterministic_pixel_for_pixel_across_two_independent_builds() {
        let surface = small_surface();
        let a = build_background(&surface);
        let b = build_background(&surface);
        assert_eq!(a.pixmap().pixels(), b.pixmap().pixels(), "the same board must rasterise to byte-identical pixels every time -- no flicker between frames");
    }
}
