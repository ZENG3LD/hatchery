//! The richest tier: a real per-pixel raster per tile, encoded to SIXEL
//! via `icy_sixel` -- the SAME encoder `hatchery-tui` already bakes its
//! own icons with (`crates/hatchery-tui/src/icons.rs`'s own
//! `SixelImage::try_from_rgba` / `EncodeOptions` usage), so this backend
//! matches that crate's own already-hardened conventions rather than
//! inventing a second sixel-encoding style: transparent background
//! (`BackgroundMode::Transparent`, never composited against a guessed
//! terminal colour), a small explicit palette with dithering switched off
//! (see [`encode_options`]).
//!
//! There is still no baked bitmap asset catalog this pass (see
//! `tiles/mod.rs`'s own doc comment) -- every tile's own raster is drawn
//! PROCEDURALLY, in [`rasterize_tile`], from nothing but its
//! [`SurfaceCell`]'s own `fg`/`bg`/`art` fields: a `TileId`'s CATEGORY
//! (terrain/tower/enemy/boss/pet/effect) picks a shape, `fg` colours it,
//! `bg` (when set -- a tower's Living Circuit link tint, an enemy's
//! slow/stun tint, a boss/pet's evolution tint) paints a wash behind it,
//! and a tower/boss's own `TileArt::variant` scales the shape (tower
//! level; boss remaining-HP tenths). Individual KIND (Needle vs Bell,
//! Mite vs Skitter, ...) is carried by colour alone, matching every other
//! tier -- six hand-authored shapes per category is real, honest
//! per-pixel art within this pass's own scope; six DIFFERENT shapes per
//! individual tower/enemy kind is the `tools/bake_tiles.py` asset
//! pipeline's job, not a from-scratch procedural generator's.
//!
//! Each tile is encoded as its OWN [`SixelPlacement`] (never one
//! whole-board sixel blob) so a host can dedupe/cache per tile via
//! [`SixelPlacement::identity`] (an [`crate::hash::StableHasher`] digest
//! of that tile's own visual content) -- an unchanged tile between two
//! `DirtyHint::Sparse` calls never needs re-encoding by the host, even
//! though THIS backend itself holds no cache (a fresh call always
//! re-rasterizes+re-encodes whatever `dirty` names, same "no persistent
//! backend state" contract [`crate::render::backend_halfblock::
//! HalfBlockBackend`] already keeps).
//!
//! Failure mode: encoding a small, fixed-size, in-memory RGBA buffer to
//! SIXEL is not expected to ever fail in practice (`hatchery-tui`'s own
//! `build_sixel_sized` treats the equivalent call as infallible via
//! `.expect(...)`), but this crate's own "never panic in library code"
//! rule means a genuine (if unreachable in testing) encoder error here
//! degrades that ONE tile to "no placement emitted" rather than a panic
//! or a fabricated placeholder image -- see [`rasterize_tile_placement`].

use icy_sixel::{BackgroundMode, EncodeOptions, SixelImage};
use uzor_tui::{buffer::TerminalBuffer, rect::Rect};

use crate::hash::StableHasher;
use crate::render::{DirtyHint, RenderBackend, Rgb, Surface, SurfaceCell, TileFootprint, TileId};

/// Assumed terminal-cell pixel size, hand-synced to `hatchery-tui`'s own
/// `icons.rs::ASSUMED_CELL_WIDTH_PX`/`ASSUMED_CELL_HEIGHT_PX` -- the same
/// measured Cascadia Mono / Consolas cell aspect that crate's own baked
/// icons already assume. There is no live font-metrics query on this
/// crate's own side (the render module owns no terminal I/O at all -- see
/// `render/mod.rs`'s own doc comment) to derive this number from instead.
const PX_PER_CELL_W: u32 = 10;
const PX_PER_CELL_H: u32 = 19;

/// One rendered sixel placement -- data-only, defined now so a future
/// implementation and this backend's own `Output` type can exist without
/// a breaking signature change later; not constructed anywhere in this
/// pass.
#[derive(Clone, Debug)]
pub struct SixelPlacement {
    pub rect: Rect,
    pub encoded: Vec<u8>,
    pub identity: u64,
    pub force_reemit: bool,
}

/// [`SixelBackend::project`]'s return value. A bare `Vec<SixelPlacement>`
/// cannot distinguish "the tier is unimplemented" from "an idle
/// `Sparse(&[])` tick correctly produced nothing to re-encode" -- both
/// would otherwise be an empty `Vec`. This enum keeps those two states
/// distinct at the type level, matching [`crate::render::TierStatus`]'s
/// own reasoning for the other stub backends.
#[derive(Clone, Debug)]
pub enum SixelOutput {
    Placements(Vec<SixelPlacement>),
    NotImplemented,
}

fn encode_options() -> EncodeOptions {
    // Same "small explicit palette, no dithering" recipe `hatchery-tui`
    // already settled on for its own flat-colour UI glyphs (`icons.rs::
    // icon_encode_options`'s own doc comment) -- every tile this backend
    // draws is a handful of flat procedural shapes, never a photograph,
    // so Floyd-Steinberg dithering would only add speckle noise.
    EncodeOptions { max_colors: 48, diffusion: 0.0, ..EncodeOptions::default() }
}

fn to_rgba(rgb: Rgb, alpha: u8) -> [u8; 4] {
    [rgb.0, rgb.1, rgb.2, alpha]
}

fn darken(rgb: Rgb, permille: u32) -> Rgb {
    let scale = |c: u8| ((c as u32 * permille) / 1000) as u8;
    Rgb(scale(rgb.0), scale(rgb.1), scale(rgb.2))
}

fn put_pixel(buf: &mut [u8], px_w: u32, px_h: u32, x: i64, y: i64, color: [u8; 4]) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x >= px_w || y >= px_h {
        return;
    }
    let idx = ((y * px_w + x) * 4) as usize;
    buf[idx..idx + 4].copy_from_slice(&color);
}

/// Fills the axis-aligned rectangle `[x0, x1] x [y0, y1]` (both ends
/// inclusive, pixel coordinates), clipped to the buffer.
fn fill_rect(buf: &mut [u8], px_w: u32, px_h: u32, x0: i64, y0: i64, x1: i64, y1: i64, color: [u8; 4]) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            put_pixel(buf, px_w, px_h, x, y, color);
        }
    }
}

/// A rectangle covering `frac` (0.0-1.0) of the tile's own width/height,
/// centred -- the terrain/link-halo/wash shapes' shared "inset square"
/// primitive.
fn fill_rect_inset(buf: &mut [u8], px_w: u32, px_h: u32, frac: f64, color: [u8; 4]) {
    let margin_x = (px_w as f64 * (1.0 - frac) / 2.0).round() as i64;
    let margin_y = (px_h as f64 * (1.0 - frac) / 2.0).round() as i64;
    fill_rect(buf, px_w, px_h, margin_x, margin_y, px_w as i64 - 1 - margin_x, px_h as i64 - 1 - margin_y, color);
}

fn fill_circle(buf: &mut [u8], px_w: u32, px_h: u32, cx: f64, cy: f64, radius: f64, color: [u8; 4]) {
    if radius <= 0.0 {
        return;
    }
    let r2 = radius * radius;
    for y in 0..px_h {
        for x in 0..px_w {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            if dx * dx + dy * dy <= r2 {
                put_pixel(buf, px_w, px_h, x as i64, y as i64, color);
            }
        }
    }
}

fn fill_ring(buf: &mut [u8], px_w: u32, px_h: u32, cx: f64, cy: f64, outer: f64, inner: f64, color: [u8; 4]) {
    let (outer2, inner2) = (outer * outer, inner * inner);
    for y in 0..px_h {
        for x in 0..px_w {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let d2 = dx * dx + dy * dy;
            if d2 <= outer2 && d2 >= inner2 {
                put_pixel(buf, px_w, px_h, x as i64, y as i64, color);
            }
        }
    }
}

/// A diamond (rotated square, Manhattan/L1 ball) centred at `(cx, cy)`
/// with half-width `rx` and half-height `ry` -- the shared "enemy"/
/// "unoccupied anchor" marker shape.
fn fill_diamond(buf: &mut [u8], px_w: u32, px_h: u32, cx: f64, cy: f64, rx: f64, ry: f64, color: [u8; 4]) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    for y in 0..px_h {
        for x in 0..px_w {
            let dx = (x as f64 + 0.5 - cx).abs() / rx;
            let dy = (y as f64 + 0.5 - cy).abs() / ry;
            if dx + dy <= 1.0 {
                put_pixel(buf, px_w, px_h, x as i64, y as i64, color);
            }
        }
    }
}

/// Procedurally rasterizes one tile's own `px_w x px_h` RGBA buffer
/// (row-major, straight, non-premultiplied alpha -- `icy_sixel`'s own
/// expected input layout) -- see this module's own doc comment for the
/// shape-per-category / colour-per-kind split.
fn rasterize_tile(px_w: u32, px_h: u32, cell: &SurfaceCell) -> Vec<u8> {
    let mut buf = vec![0u8; (px_w as usize) * (px_h as usize) * 4];
    let Some(art) = cell.art else {
        return buf;
    };
    let fg = to_rgba(cell.fg, 255);
    let bg = cell.bg.map(|c| to_rgba(c, 255));
    let cx = px_w as f64 / 2.0;
    let cy = px_h as f64 / 2.0;
    let half = (px_w.min(px_h) as f64) / 2.0;

    match art.tile {
        TileId::Ground => {
            fill_rect_inset(&mut buf, px_w, px_h, 1.0, fg);
        }
        TileId::Rock => {
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.3, fg);
        }
        TileId::Plant => {
            fill_diamond(&mut buf, px_w, px_h, cx, cy, half * 0.26, half * 0.4, fg);
        }
        TileId::WaterPool => {
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.55, fg);
        }
        TileId::Firefly => {
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.14, fg);
        }
        TileId::Path | TileId::Choke => {
            fill_rect_inset(&mut buf, px_w, px_h, 0.86, fg);
        }
        TileId::BuildPad => {
            fill_ring(&mut buf, px_w, px_h, cx, cy, half * 0.55, half * 0.36, fg);
        }
        TileId::Heartseed => {
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.5, fg);
        }
        TileId::PetAnchor => {
            fill_diamond(&mut buf, px_w, px_h, cx, cy, half * 0.42, half * 0.42, fg);
        }
        TileId::TowerNeedle
        | TileId::TowerBell
        | TileId::TowerPrism
        | TileId::TowerEmberNest
        | TileId::TowerMoonwell
        | TileId::TowerRelay => {
            if let Some(bg) = bg {
                fill_rect_inset(&mut buf, px_w, px_h, 0.86, bg);
            }
            let level = art.variant.min(2) as f64;
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * (0.30 + 0.09 * level), fg);
        }
        TileId::EnemyMite
        | TileId::EnemySkitter
        | TileId::EnemyShellback
        | TileId::EnemySplitter
        | TileId::EnemyHusher
        | TileId::EnemyMirror => {
            if let Some(bg) = bg {
                fill_rect_inset(&mut buf, px_w, px_h, 0.8, bg);
            }
            fill_diamond(&mut buf, px_w, px_h, cx, cy, half * 0.42, half * 0.42, fg);
        }
        TileId::BossBellkeeper | TileId::BossNightMaw => {
            if let Some(bg) = bg {
                fill_rect(&mut buf, px_w, px_h, 0, 0, px_w as i64 - 1, px_h as i64 - 1, bg);
            }
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.48, fg);
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.17, to_rgba(darken(cell.fg, 400), 255));
        }
        TileId::Pet => {
            if let Some(bg) = bg {
                fill_rect_inset(&mut buf, px_w, px_h, 0.86, bg);
            }
            fill_rect(&mut buf, px_w, px_h, (cx - half * 0.12) as i64, (cy - half * 0.42) as i64, (cx + half * 0.12) as i64, (cy + half * 0.42) as i64, fg);
            fill_rect(&mut buf, px_w, px_h, (cx - half * 0.42) as i64, (cy - half * 0.12) as i64, (cx + half * 0.42) as i64, (cy + half * 0.12) as i64, fg);
        }
        TileId::CircuitLink => {
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.18, fg);
        }
        // Combat effects: this per-tile backend has no continuous-position
        // concept at all (see `render::pixel`'s own doc comment for why
        // that lives in a sibling module) -- these arms exist only so this
        // exhaustive match keeps compiling now that `TileId` carries them,
        // and draw the same honest small marker every other single-glyph
        // effect kind gets here, never a placeholder/panic.
        TileId::Projectile => {
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.16, fg);
        }
        TileId::ImpactFlash => {
            fill_ring(&mut buf, px_w, px_h, cx, cy, half * 0.5, half * 0.28, fg);
        }
        TileId::DeathBurst => {
            fill_ring(&mut buf, px_w, px_h, cx, cy, half * 0.62, half * 0.42, fg);
        }
        TileId::SplashRing => {
            fill_ring(&mut buf, px_w, px_h, cx, cy, half * 0.7, half * 0.5, fg);
        }
        TileId::LinkPulse => {
            fill_ring(&mut buf, px_w, px_h, cx, cy, half * 0.5, half * 0.34, fg);
            fill_circle(&mut buf, px_w, px_h, cx, cy, half * 0.15, fg);
        }
    }
    buf
}

/// A stable per-tile content digest -- see this module's own doc comment
/// for why this exists (host-side cache/dedupe key, not read by this
/// backend itself).
fn tile_identity(tile_x: u16, tile_y: u16, cell: &SurfaceCell) -> u64 {
    let mut hasher = StableHasher::new();
    hasher.write_u64(tile_x as u64);
    hasher.write_u64(tile_y as u64);
    hasher.write(cell.glyph.to_string().as_bytes());
    hasher.write(&[cell.fg.0, cell.fg.1, cell.fg.2]);
    match cell.bg {
        Some(bg) => hasher.write(&[1, bg.0, bg.1, bg.2]),
        None => hasher.write(&[0]),
    }
    match cell.art {
        Some(art) => hasher.write(&[1, art.tile as u8, art.variant]),
        None => hasher.write(&[0]),
    }
    hasher.finish()
}

/// Rasterizes and encodes exactly one tile, or `None` when it has nothing
/// visible to paint (a pure-background cell -- no `art`, no `bg`) or the
/// encoder genuinely fails (see this module's own doc comment's Failure
/// mode section).
fn rasterize_tile_placement(surface: &Surface, footprint: TileFootprint, dest: Rect, tile_x: u16, tile_y: u16, dragging: bool) -> Option<SixelPlacement> {
    let cell = surface.get(tile_x, tile_y);
    if cell.art.is_none() && cell.bg.is_none() {
        return None;
    }
    if footprint.cells_w == 0 || footprint.cells_h == 0 {
        return None;
    }
    let px_w = footprint.cells_w as u32 * PX_PER_CELL_W;
    let px_h = footprint.cells_h as u32 * PX_PER_CELL_H;
    let rgba = rasterize_tile(px_w, px_h, &cell);
    let image = SixelImage::try_from_rgba(rgba, px_w as usize, px_h as usize).ok()?;
    let encoded = image.with_background_mode(BackgroundMode::Transparent).encode_with(&encode_options()).ok()?.into_bytes();

    let base_x = dest.x.saturating_add(tile_x.saturating_mul(footprint.cells_w));
    let base_y = dest.y.saturating_add(tile_y.saturating_mul(footprint.cells_h));
    let rect = Rect::new(base_x, base_y, footprint.cells_w, footprint.cells_h).intersect(dest);
    if rect.is_empty() {
        return None;
    }
    Some(SixelPlacement { rect, encoded, identity: tile_identity(tile_x, tile_y, &cell), force_reemit: dragging })
}

/// The richest, real-per-pixel tier -- see this module's own doc comment.
#[derive(Default)]
pub struct SixelBackend;

impl RenderBackend for SixelBackend {
    type Output = SixelOutput;

    fn project(
        &mut self,
        surface: &Surface,
        footprint: TileFootprint,
        dest: Rect,
        _buf: &mut TerminalBuffer,
        dragging: bool,
        dirty: DirtyHint<'_>,
    ) -> SixelOutput {
        let mut placements = Vec::new();
        match dirty {
            DirtyHint::Full => {
                for y in 0..surface.height() {
                    for x in 0..surface.width() {
                        if let Some(placement) = rasterize_tile_placement(surface, footprint, dest, x, y, dragging) {
                            placements.push(placement);
                        }
                    }
                }
            }
            DirtyHint::Sparse(coords) => {
                for &(x, y) in coords {
                    if let Some(placement) = rasterize_tile_placement(surface, footprint, dest, x, y, dragging) {
                        placements.push(placement);
                    }
                }
            }
        }
        SixelOutput::Placements(placements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Rgb as RenderRgb, TileArt};

    fn art_cell(tile: TileId, variant: u8, fg: (u8, u8, u8), bg: Option<(u8, u8, u8)>) -> SurfaceCell {
        SurfaceCell {
            glyph: '#',
            fg: RenderRgb(fg.0, fg.1, fg.2),
            bg: bg.map(|(r, g, b)| RenderRgb(r, g, b)),
            art: Some(TileArt { tile, variant }),
        }
    }

    #[test]
    fn a_pure_background_tile_never_produces_a_placement() {
        let surface = Surface::new(1, 1, SurfaceCell::BLANK);
        let mut buf = TerminalBuffer::new(1, 1);
        let mut backend = SixelBackend;
        let output = backend.project(
            &surface,
            TileFootprint { cells_w: 1, cells_h: 1 },
            Rect::new(0, 0, 1, 1),
            &mut buf,
            false,
            DirtyHint::Full,
        );
        match output {
            SixelOutput::Placements(placements) => assert!(placements.is_empty()),
            SixelOutput::NotImplemented => panic!("SixelBackend must be genuinely implemented, not a stub"),
        }
    }

    #[test]
    fn a_real_tile_produces_exactly_one_non_empty_placement_covering_its_own_footprint() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, art_cell(TileId::TowerNeedle, 0, (200, 200, 200), Some((40, 20, 60))));
        let mut buf = TerminalBuffer::new(2, 3);
        let mut backend = SixelBackend;
        let output = backend.project(
            &surface,
            TileFootprint { cells_w: 2, cells_h: 3 },
            Rect::new(0, 0, 2, 3),
            &mut buf,
            false,
            DirtyHint::Full,
        );
        let SixelOutput::Placements(placements) = output else {
            panic!("expected real placements");
        };
        assert_eq!(placements.len(), 1);
        let placement = &placements[0];
        assert_eq!(placement.rect, Rect::new(0, 0, 2, 3));
        assert!(placement.encoded.starts_with(b"\x1b"), "sixel output must start with the DCS introducer ESC");
        assert!(!placement.force_reemit);
    }

    #[test]
    fn dragging_forces_reemit_on_every_placement() {
        let mut surface = Surface::new(1, 1, SurfaceCell::BLANK);
        surface.set(0, 0, art_cell(TileId::EnemyMite, 0, (170, 220, 120), None));
        let mut buf = TerminalBuffer::new(1, 1);
        let mut backend = SixelBackend;
        let output = backend.project(
            &surface,
            TileFootprint { cells_w: 1, cells_h: 1 },
            Rect::new(0, 0, 1, 1),
            &mut buf,
            true,
            DirtyHint::Full,
        );
        let SixelOutput::Placements(placements) = output else {
            panic!("expected real placements");
        };
        assert_eq!(placements.len(), 1);
        assert!(placements[0].force_reemit);
    }

    #[test]
    fn sparse_dirty_hint_only_considers_the_named_tiles() {
        let mut surface = Surface::new(2, 1, SurfaceCell::BLANK);
        surface.set(0, 0, art_cell(TileId::Heartseed, 0, (255, 215, 0), None));
        surface.set(1, 0, art_cell(TileId::Heartseed, 0, (255, 215, 0), None));
        let mut buf = TerminalBuffer::new(2, 1);
        let mut backend = SixelBackend;
        let output = backend.project(
            &surface,
            TileFootprint { cells_w: 1, cells_h: 1 },
            Rect::new(0, 0, 2, 1),
            &mut buf,
            false,
            DirtyHint::Sparse(&[(1, 0)]),
        );
        let SixelOutput::Placements(placements) = output else {
            panic!("expected real placements");
        };
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].rect, Rect::new(1, 0, 1, 1));
    }

    #[test]
    fn identical_tiles_produce_identical_identity_and_differing_tiles_diverge() {
        let cell_a = art_cell(TileId::TowerBell, 0, (1, 2, 3), None);
        let cell_b = art_cell(TileId::TowerBell, 0, (1, 2, 3), None);
        let cell_c = art_cell(TileId::TowerBell, 1, (1, 2, 3), None);
        assert_eq!(tile_identity(0, 0, &cell_a), tile_identity(0, 0, &cell_b));
        assert_ne!(tile_identity(0, 0, &cell_a), tile_identity(0, 0, &cell_c));
        assert_ne!(tile_identity(0, 0, &cell_a), tile_identity(1, 0, &cell_a));
    }
}
