//! `SimulationSnapshot -> Surface` adapter and Pet Bastion's own tile/
//! colour catalog -- the ONE crate allowed to depend on BOTH `gate4agent-
//! arcade-pet-bastion` (sim rules, no terminal type anywhere -- see that
//! crate's own `lib.rs` "Forbidden" doc line) and `hatchery-arcade-
//! engine`'s `render` feature (terminal rendering types).
//!
//! # Why a third crate, not a feature on either existing one
//!
//! `pet-bastion`'s own `lib.rs` states its "no terminal, rendering, TUI,
//! ... type anywhere in this crate" invariant unconditionally -- not
//! "unless behind a feature flag" the way the engine's OWN render module
//! is carved out from ITS sim core. Growing a `#[cfg(feature = "render")]`
//! module inside `pet-bastion` would still put a rendering-typed module
//! source-file "in this crate", which is exactly what that doc line rules
//! out. And the engine's own `TileId` is deliberately a plain, generic
//! kind catalog (see `engine::render::tiles`'s own doc comment) -- it
//! does not itself know what a `TowerView`, an `EnemyKind`, or a
//! `SimulationSnapshot` is, and should not have to import a specific
//! mini-game's crate just to draw its board. A standalone adapter crate
//! is the only place both vocabularies can meet without violating either
//! side's own stated boundary -- exactly the "отдельный модуль-адаптер"
//! shape the task asked for.
//!
//! # What lives here
//!
//! [`snapshot_to_surface`] is the whole public API: one pure function,
//! `&SimulationSnapshot -> Surface`, called once per frame by whatever
//! render host (today: the `preview` binary; eventually, `gate4agent-tui`'s
//! own pet modal) wants to paint a Pet Bastion run. Every colour/glyph/
//! shape decision below is a deliberate, documented design choice, not a
//! placeholder -- see each `const`/function's own doc comment.
//!
//! # The "effects" tile family, honestly scoped
//!
//! `Surface` is one glyph per tile ([`SurfaceCell::glyph`] is mandatory,
//! never optional -- the engine's own readability contract). That means
//! a status effect on an OCCUPIED tile (an enemy's slow/stun) cannot be a
//! second, competing tile identity on the SAME cell -- it is encoded as
//! a colour tint (`bg`) on that enemy's own cell instead, plus a
//! `TileArt::variant` bump so [`hatchery_arcade_engine::SixelBackend`]
//! can draw it with real per-pixel structure too. [`TileId::CircuitLink`]
//! is the one effect that DOES get a real, standalone tile identity: a
//! small halo of tiles around the pet's current anchor, painted only over
//! cells that are still background (never overwriting a real terrain/
//! unit tile) whenever the pet currently has at least one linked tower.
//! A literal line traced from the anchor to every individual linked
//! tower was considered and rejected: `Surface` has no sub-cell line
//! primitive, so a traced line would have to either overwrite whatever
//! terrain tile it happens to cross, or silently skip it -- both read as
//! a rendering DEFECT, not a deliberate choice, on a board with real
//! terrain features (routes, pads) a straight line will often cross.

use std::collections::HashMap;

use hatchery_arcade_engine::{Rgb, Surface, SurfaceCell, TileArt, TileId};
use hatchery_arcade_pet_bastion::board::{AnchorId, Board, ANCHORS, ANCHOR_COUNT, HEARTSEED};
use hatchery_arcade_pet_bastion::boss::BossKind;
use hatchery_arcade_pet_bastion::constants::{BOARD_HEIGHT, BOARD_WIDTH, FIXED_SCALE, PET_MOVE_TICKS, PET_MOVE_TICKS_MOTH};
use hatchery_arcade_pet_bastion::enemy::EnemyKind;
use hatchery_arcade_pet_bastion::geometry::{FixedPos, Tile};
use hatchery_arcade_pet_bastion::pet::{Evolution, PetState};
use hatchery_arcade_pet_bastion::snapshot::{BossView, EnemyView, PetView, SimulationSnapshot, TowerView};
use hatchery_arcade_pet_bastion::tower::{TowerKind, UpgradeLevel};

/// Frame interpolation (fixed 20Hz sim step -> smooth 60Hz render) -- see
/// this module's own doc comment for why it lives here and not in the
/// engine.
pub mod interp;
/// Time-aged combat visual effects driven by `SimEvent` -- see this
/// module's own doc comment.
pub mod effects;

// ---------------------------------------------------------------------------
// Colour catalog -- one deliberate colour per kind, independent of level/
// status (those modulate brightness/bg, never the base hue).
// ---------------------------------------------------------------------------

const PATH_COLOR: Rgb = Rgb(101, 88, 63);
const CHOKE_COLOR: Rgb = Rgb(150, 90, 50);
const BUILD_PAD_COLOR: Rgb = Rgb(110, 110, 130);
const HEARTSEED_COLOR: Rgb = Rgb(255, 215, 0);
const PET_ANCHOR_COLOR: Rgb = Rgb(120, 170, 220);
/// Night-garden ground: a dark, desaturated moss green -- the base every
/// other terrain colour above sits on top of. [`TileId::Ground`]'s own
/// `paint_tile` arm derives its handful of deterministic tone variants
/// straight off this one hue (darken/tint), so the whole board reads as
/// one textured surface, not a patchwork of unrelated colours.
const GROUND_COLOR: Rgb = Rgb(26, 42, 32);
/// Static decor -- see [`decor_for_tile`]'s own doc comment.
const ROCK_COLOR: Rgb = Rgb(94, 96, 104);
const PLANT_COLOR: Rgb = Rgb(58, 108, 68);
const WATER_POOL_COLOR: Rgb = Rgb(36, 66, 104);
/// A bright cyan glow -- deliberately a hue no enemy/Heartseed/Pet colour
/// in this catalog is anywhere near (Mite is green, Skitter/Heartseed are
/// warm gold, Mirror/Pet/CircuitLink are pale lavender, Husher is a muted
/// blue-purple). An earlier warmer firefly tone sat close enough to
/// Skitter's own warm-gold hue that a static decor mark and a live enemy
/// became hard to tell apart at a glance -- exactly the "не должны
/// сливаться" this pass's own brief asked to avoid, decor included.
const FIREFLY_COLOR: Rgb = Rgb(150, 240, 230);
pub(crate) const PET_COLOR: Rgb = Rgb(203, 166, 247);
/// Matches `gate4agent-tui`'s own `icons.rs::ACCENT_BG` mauve -- the same
/// accent colour that crate already uses for "this is selected/active",
/// reused here for the pet/Circuit family for visual continuity with the
/// TUI this preview stands in for.
pub(crate) const CIRCUIT_LINK_COLOR: Rgb = Rgb(203, 166, 247);
pub(crate) const LINK_BG: Rgb = Rgb(60, 40, 90);
pub(crate) const SLOW_BG: Rgb = Rgb(40, 90, 140);
pub(crate) const STUN_BG: Rgb = Rgb(160, 150, 40);

fn tower_glyph(kind: TowerKind) -> char {
    match kind {
        TowerKind::Needle => 'N',
        TowerKind::Bell => 'B',
        TowerKind::Prism => 'P',
        TowerKind::EmberNest => 'E',
        TowerKind::Moonwell => 'M',
        TowerKind::Relay => 'R',
    }
}

pub(crate) fn tower_base_color(kind: TowerKind) -> Rgb {
    match kind {
        TowerKind::Needle => Rgb(200, 200, 200),
        TowerKind::Bell => Rgb(150, 120, 255),
        TowerKind::Prism => Rgb(255, 140, 255),
        TowerKind::EmberNest => Rgb(255, 110, 40),
        TowerKind::Moonwell => Rgb(110, 200, 255),
        TowerKind::Relay => Rgb(180, 180, 80),
    }
}

pub(crate) fn tower_tile_id(kind: TowerKind) -> TileId {
    match kind {
        TowerKind::Needle => TileId::TowerNeedle,
        TowerKind::Bell => TileId::TowerBell,
        TowerKind::Prism => TileId::TowerPrism,
        TowerKind::EmberNest => TileId::TowerEmberNest,
        TowerKind::Moonwell => TileId::TowerMoonwell,
        TowerKind::Relay => TileId::TowerRelay,
    }
}

/// `TileArt::variant` for a tower: 0=Base, 1=L2, 2=L3 (either branch --
/// the Power/Utility split changes stats, not this preview's own visual
/// language).
pub(crate) fn tower_level_index(level: UpgradeLevel) -> u8 {
    match level {
        UpgradeLevel::Base => 0,
        UpgradeLevel::L2 => 1,
        UpgradeLevel::L3(_) => 2,
    }
}

/// Base=60%, L2=80%, L3=100% brightness -- a stronger tower reads
/// visually stronger, without changing its own base hue (hue alone still
/// carries kind identity, independent of level).
pub(crate) fn level_brightness_permille(level_index: u8) -> u32 {
    match level_index {
        0 => 600,
        1 => 800,
        _ => 1000,
    }
}

pub(crate) fn scale_color(color: Rgb, permille: u32) -> Rgb {
    let scale = |channel: u8| (((channel as u32) * permille) / 1000).min(255) as u8;
    Rgb(scale(color.0), scale(color.1), scale(color.2))
}

fn enemy_glyph(kind: EnemyKind) -> char {
    match kind {
        EnemyKind::Mite => 'm',
        EnemyKind::Skitter => 'k',
        EnemyKind::Shellback => 'b',
        EnemyKind::Splitter => 'x',
        EnemyKind::Husher => 'h',
        EnemyKind::Mirror => 'r',
    }
}

pub(crate) fn enemy_color(kind: EnemyKind) -> Rgb {
    match kind {
        EnemyKind::Mite => Rgb(170, 220, 120),
        EnemyKind::Skitter => Rgb(255, 220, 100),
        EnemyKind::Shellback => Rgb(140, 100, 60),
        EnemyKind::Splitter => Rgb(200, 80, 200),
        EnemyKind::Husher => Rgb(120, 120, 190),
        EnemyKind::Mirror => Rgb(210, 210, 255),
    }
}

pub(crate) fn enemy_tile_id(kind: EnemyKind) -> TileId {
    match kind {
        EnemyKind::Mite => TileId::EnemyMite,
        EnemyKind::Skitter => TileId::EnemySkitter,
        EnemyKind::Shellback => TileId::EnemyShellback,
        EnemyKind::Splitter => TileId::EnemySplitter,
        EnemyKind::Husher => TileId::EnemyHusher,
        EnemyKind::Mirror => TileId::EnemyMirror,
    }
}

fn boss_glyph(kind: BossKind) -> char {
    match kind {
        BossKind::Bellkeeper => '#',
        BossKind::NightMaw => '&',
    }
}

pub(crate) fn boss_color(kind: BossKind) -> Rgb {
    match kind {
        BossKind::Bellkeeper => Rgb(255, 80, 80),
        BossKind::NightMaw => Rgb(140, 40, 180),
    }
}

pub(crate) fn boss_bg(kind: BossKind) -> Rgb {
    match kind {
        BossKind::Bellkeeper => Rgb(80, 10, 10),
        BossKind::NightMaw => Rgb(40, 10, 60),
    }
}

pub(crate) fn boss_tile_id(kind: BossKind) -> TileId {
    match kind {
        BossKind::Bellkeeper => TileId::BossBellkeeper,
        BossKind::NightMaw => TileId::BossNightMaw,
    }
}

pub(crate) fn evolution_bg(evolution: Option<Evolution>) -> Option<Rgb> {
    match evolution {
        None => None,
        Some(Evolution::Moth) => Some(Rgb(30, 60, 40)),
        Some(Evolution::Crab) => Some(Rgb(70, 40, 20)),
        Some(Evolution::Wisp) => Some(Rgb(20, 50, 70)),
    }
}

pub(crate) fn evolution_variant(evolution: Option<Evolution>) -> u8 {
    match evolution {
        None => 0,
        Some(Evolution::Moth) => 1,
        Some(Evolution::Crab) => 2,
        Some(Evolution::Wisp) => 3,
    }
}

// ---------------------------------------------------------------------------
// Tile coordinate helpers
// ---------------------------------------------------------------------------

/// Rounds a continuous fixed-point position to the nearest board tile
/// (integer arithmetic only -- `(value + FIXED_SCALE/2) / FIXED_SCALE` is
/// the standard round-to-nearest-via-half-up-bias trick; every coordinate
/// this crate ever rounds is non-negative, so there is no
/// away-from-zero-vs-toward-zero ambiguity to guard against).
fn round_to_tile(pos: FixedPos) -> (i32, i32) {
    let x = (pos.x + FIXED_SCALE / 2) / FIXED_SCALE;
    let y = (pos.y + FIXED_SCALE / 2) / FIXED_SCALE;
    (x as i32, y as i32)
}

/// Clamps a tile coordinate into the board's own bounds -- the same
/// "clamp, never panic" policy `engine::render::Surface::set`/`get`
/// already apply, kept consistent here so a value one tile off due to
/// integer rounding at a board edge degrades to the nearest real tile
/// rather than silently disappearing into an out-of-bounds `Surface::set`
/// no-op.
fn clamp_tile(x: i32, y: i32) -> (u16, u16) {
    let cx = x.clamp(0, BOARD_WIDTH - 1) as u16;
    let cy = y.clamp(0, BOARD_HEIGHT - 1) as u16;
    (cx, cy)
}

/// Every integer tile on one axis-aligned route segment, both endpoints
/// inclusive (`board.rs`'s own routing invariant guarantees every segment
/// shares an x or a y, so this never needs a Bresenham walk).
fn walk_segment_tiles(from: Tile, to: Tile) -> Vec<(i32, i32)> {
    if from.y == to.y {
        let (lo, hi) = (from.x.min(to.x), from.x.max(to.x));
        (lo..=hi).map(|x| (x, from.y)).collect()
    } else {
        let (lo, hi) = (from.y.min(to.y), from.y.max(to.y));
        (lo..=hi).map(|y| (from.x, y)).collect()
    }
}

/// The pet's own current tile: its anchor while `AtAnchor`, or a linear
/// interpolation between the `from`/`to` anchors while `Moving` (this
/// preview's own presentation-layer approximation of in-flight position --
/// the sim itself only tracks discrete anchors + a tick countdown, never a
/// continuous position, so any in-between rendering is necessarily an
/// adapter-side interpolation, not a sim fact).
fn pet_tile(pet: &PetView) -> (u16, u16) {
    match pet.state {
        PetState::AtAnchor(anchor) => {
            let tile = Board::anchor_tile(anchor);
            clamp_tile(tile.x, tile.y)
        }
        PetState::Moving { from, to, ticks_remaining } => {
            let total: i64 = match pet.evolution {
                Some(Evolution::Moth) => PET_MOVE_TICKS_MOTH as i64,
                _ => PET_MOVE_TICKS as i64,
            };
            let elapsed = (total - ticks_remaining as i64).clamp(0, total.max(1));
            let from_tile = Board::anchor_tile(from).to_fixed();
            let to_tile = Board::anchor_tile(to).to_fixed();
            let denom = total.max(1);
            let x = from_tile.x + (to_tile.x - from_tile.x) * elapsed / denom;
            let y = from_tile.y + (to_tile.y - from_tile.y) * elapsed / denom;
            let (tx, ty) = round_to_tile(FixedPos { x, y });
            clamp_tile(tx, ty)
        }
    }
}

// ---------------------------------------------------------------------------
// Painting passes -- ground, then static decor, then routes (shaped by real
// neighbour connectivity), then the water pool ringing the Heartseed, then
// anchors/Heartseed themselves, then the Circuit-link halo (only onto
// non-important tiles), then units -- each pass painting over whatever the
// previous one left on that exact tile, so a unit's own tile always wins.
// `TileId::BuildPad` is deliberately NOT one of these passes any more: see
// [`paint_build_zone_highlight`]'s own doc comment.
// ---------------------------------------------------------------------------

/// Cheap deterministic 32-bit hash of a board tile coordinate plus a `salt`
/// (a distinct constant per independent decision made off the same
/// coordinate -- "which ground tone" and "which decor kind" must never
/// correlate) -- the entire mechanism [`ground_variant`]/[`decor_for_tile`]
/// scatter static decor from. No RNG stream, no crate dependency: a pure
/// function of position, so the SAME tile paints the SAME decor on every
/// call (`snapshot_to_surface` runs once per frame) -- this pass's own
/// "детерминированно... чтобы он не мигал между кадрами" requirement, and
/// -- just as importantly -- entirely independent of `Simulation`'s own
/// seeded PRNG stream, so painting a frame can never perturb run-affecting
/// randomness.
fn tile_hash(x: i32, y: i32, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1);
    h ^= (y as u32).wrapping_mul(0x85EB_CA77).rotate_left(13);
    h ^= salt.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// One of [`TileId::Ground`]'s own four deterministic tone variants -- see
/// [`tile_hash`]'s own doc comment.
fn ground_variant(x: i32, y: i32) -> u8 {
    (tile_hash(x, y, 1) % 4) as u8
}

/// The static decor kind (if any) scattered onto one background tile:
/// `None` for most tiles (bare [`TileId::Ground`] only), or one of
/// [`TileId::Rock`]/[`TileId::Plant`]/[`TileId::Firefly`] with its own
/// glyph/colour/`TileArt::variant`. Roughly 7% rock, 9% plant, 4% firefly,
/// 80% bare ground -- dense enough that the board reads as a real garden
/// (the "растительность, камни... светлячки" this pass's own brief asked
/// for) without becoming a second "ковёр" of identical marks. Every later
/// painting pass in [`paint_terrain`] (routes, the water pool, anchors,
/// Heartseed) freely overrides whatever this pass placed, so this function
/// itself never needs to know which tiles are "important" -- overwrite
/// order alone keeps decor off real terrain.
fn decor_for_tile(x: i32, y: i32) -> Option<(TileId, u8, Rgb, char)> {
    let bucket = tile_hash(x, y, 2) % 100;
    let variant_salt = tile_hash(x, y, 3);
    match bucket {
        0..=6 => Some((TileId::Rock, (variant_salt % 6) as u8, ROCK_COLOR, 'O')),
        7..=15 => Some((TileId::Plant, (variant_salt % 6) as u8, PLANT_COLOR, '"')),
        16..=19 => Some((TileId::Firefly, (variant_salt % 9) as u8, FIREFLY_COLOR, ':')),
        _ => None,
    }
}

/// [`TileId::Path`]/[`TileId::Choke`]'s own `TileArt::variant` bit for "the
/// tile immediately north/east/south/west is also a route tile" -- matches
/// `hatchery-arcade-engine`'s own private `ROAD_NORTH`/`ROAD_EAST`/
/// `ROAD_SOUTH`/`ROAD_WEST` constants (`render::sprites`) bit for bit; kept
/// as a plain literal here (not re-exported) since only this one producer
/// and that one consumer ever need to agree on the encoding.
const ROAD_NORTH: u8 = 1;
const ROAD_EAST: u8 = 2;
const ROAD_SOUTH: u8 = 4;
const ROAD_WEST: u8 = 8;

fn paint_terrain(surface: &mut Surface) {
    let board = Board::new();

    // Pass 1: ground everywhere -- see `TileId::Ground`'s own doc comment
    // for why a dense scene needs a real base layer under every tile, not
    // just the ones that carry gameplay meaning.
    for y in 0..BOARD_HEIGHT {
        for x in 0..BOARD_WIDTH {
            let (cx, cy) = clamp_tile(x, y);
            surface.set(cx, cy, SurfaceCell { glyph: '`', fg: GROUND_COLOR, bg: None, art: Some(TileArt { tile: TileId::Ground, variant: ground_variant(x, y) }) });
        }
    }

    // Pass 2: static decor scatter, painted straight over Ground -- every
    // later pass below overrides whatever a tile picked here, so no
    // exclusion bookkeeping is needed (see `decor_for_tile`'s own doc
    // comment).
    for y in 0..BOARD_HEIGHT {
        for x in 0..BOARD_WIDTH {
            if let Some((tile, variant, color, glyph)) = decor_for_tile(x, y) {
                let (cx, cy) = clamp_tile(x, y);
                surface.set(cx, cy, SurfaceCell { glyph, fg: color, bg: None, art: Some(TileArt { tile, variant }) });
            }
        }
    }

    // Pass 3: routes -- every route/choke tile's own kind, keyed by tile
    // coordinate (a tile the shared final leg walks via BOTH routes must
    // stay `Choke` even if an earlier route's own `Path` pass claimed it
    // first).
    let mut route_kind: HashMap<(i32, i32), TileId> = HashMap::new();
    for route in &board.routes {
        let last_index = route.segments.len().saturating_sub(1);
        for (index, segment) in route.segments.iter().enumerate() {
            let tile_id = if index == last_index { TileId::Choke } else { TileId::Path };
            for (x, y) in walk_segment_tiles(segment.from, segment.to) {
                route_kind
                    .entry((x, y))
                    .and_modify(|kind| {
                        if tile_id == TileId::Choke {
                            *kind = TileId::Choke;
                        }
                    })
                    .or_insert(tile_id);
            }
        }
    }
    // Shaped by real neighbour connectivity (`push_road`'s own doc comment
    // in `render::sprites`) so a whole route reads as one continuous
    // ribbon, not a chain of separately-inset squares.
    for (&(x, y), &tile_id) in &route_kind {
        let mut mask = 0u8;
        for (dx, dy, bit) in [(0i32, -1i32, ROAD_NORTH), (1, 0, ROAD_EAST), (0, 1, ROAD_SOUTH), (-1, 0, ROAD_WEST)] {
            if route_kind.contains_key(&(x + dx, y + dy)) {
                mask |= bit;
            }
        }
        let (color, glyph) = if tile_id == TileId::Choke { (CHOKE_COLOR, '+') } else { (PATH_COLOR, '.') };
        let (cx, cy) = clamp_tile(x, y);
        surface.set(cx, cy, SurfaceCell { glyph, fg: color, bg: None, art: Some(TileArt { tile: tile_id, variant: mask }) });
    }

    // Pass 4: a still garden pool ringing the Heartseed -- the "сток"
    // (drain) every route visually empties into before the Heartseed
    // marker itself, not a bare tile floating in open ground. Every
    // background tile within Chebyshev distance 1-2 of the Heartseed that
    // is not itself a route/anchor tile.
    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            let cheby = dx.abs().max(dy.abs());
            if cheby == 0 || cheby > 2 {
                continue;
            }
            let tile = Tile::new(HEARTSEED.x + dx, HEARTSEED.y + dy);
            if !Board::bounds_contain(tile) || route_kind.contains_key(&(tile.x, tile.y)) || ANCHORS.contains(&tile) {
                continue;
            }
            let (cx, cy) = clamp_tile(tile.x, tile.y);
            surface.set(cx, cy, SurfaceCell { glyph: '=', fg: WATER_POOL_COLOR, bg: None, art: Some(TileArt { tile: TileId::WaterPool, variant: 0 }) });
        }
    }

    // Pass 5: anchors and the Heartseed itself.
    for anchor in 0..ANCHOR_COUNT as u8 {
        let tile = Board::anchor_tile(AnchorId(anchor));
        let (cx, cy) = clamp_tile(tile.x, tile.y);
        surface.set(
            cx,
            cy,
            SurfaceCell { glyph: 'a', fg: PET_ANCHOR_COLOR, bg: None, art: Some(TileArt { tile: TileId::PetAnchor, variant: 0 }) },
        );
    }
    let (hx, hy) = clamp_tile(HEARTSEED.x, HEARTSEED.y);
    surface.set(hx, hy, SurfaceCell { glyph: '*', fg: HEARTSEED_COLOR, bg: None, art: Some(TileArt { tile: TileId::Heartseed, variant: 0 }) });
}

/// A fresh `Surface` carrying ONLY [`paint_terrain`]'s own output --
/// ground, decor, routes, the water pool, anchors, the Heartseed -- and
/// nothing else: no tower, no enemy, no pet, no `BuildPad`, ever. Pure
/// board layout (`Board::new()`), entirely independent of any
/// [`SimulationSnapshot`] -- calling this twice, or calling it before vs.
/// after towers have been placed, always produces byte-identical output.
///
/// This is the ONE correct input to `hatchery_arcade_engine::
/// build_background`/`background_seed`: [`snapshot_to_surface`]'s own
/// output later gets towers/enemies/the pet painted ON TOP of this exact
/// same terrain, and `Surface` holds exactly one `art` per cell -- so a
/// tower placed on a tile that used to carry a `Rock`/`Plant`/`Firefly`
/// decor mark REPLACES that cell's `art` in a live, post-placement
/// `Surface`. Hashing/painting the background from THAT surface instead
/// of this one would make the cached background's own seed drift every
/// time a tower gets placed on top of a decor tile -- forcing a full
/// background rebuild mid-run, exactly the per-tile repaint cost this
/// whole cache exists to eliminate. A host builds the background from
/// THIS function's own output once (Pet Bastion's board layout never
/// changes mid-run), never from a live snapshot's own `Surface`.
pub fn terrain_surface() -> Surface {
    let mut surface = Surface::new(BOARD_WIDTH as u16, BOARD_HEIGHT as u16, SurfaceCell::BLANK);
    paint_terrain(&mut surface);
    surface
}

/// Paints [`TileId::BuildPad`] onto every currently-buildable tile
/// (`snapshot.build_cells`, `reason == None`) -- the ONLY place any frame
/// this crate produces ever grows the old "carpet of identical circles"
/// back. Call this ONLY while the owner is actively dragging a tower out
/// of the palette; an ordinary combat/build-phase frame must never call
/// it. `snapshot.build_cells` (not `Board::build_zone_cells` directly) is
/// the right source here: it already layers live tower occupancy on top
/// of the board's own static route/anchor rule, so an occupied buildable
/// tile correctly does NOT get highlighted a second time underneath the
/// tower already standing on it.
pub fn paint_build_zone_highlight(surface: &mut Surface, snapshot: &SimulationSnapshot) {
    for cell in &snapshot.build_cells {
        if cell.reason.is_some() {
            continue;
        }
        let (cx, cy) = clamp_tile(cell.tile.0, cell.tile.1);
        surface.set(cx, cy, SurfaceCell { glyph: 'o', fg: BUILD_PAD_COLOR, bg: None, art: Some(TileArt { tile: TileId::BuildPad, variant: 0 }) });
    }
}

/// Whether `tile` is meaningful terrain the Circuit-link halo must never
/// paint over. Every other static tile ([`TileId::Ground`] and its own
/// decor scatter) is fair game -- decor carries no gameplay meaning of its
/// own, so losing one rock/plant/firefly mark under a Circuit halo tile is
/// a non-issue, unlike silently erasing a route/anchor/Heartseed/water-pool
/// tile a player actually reads for information.
fn blocks_circuit_halo(tile: TileId) -> bool {
    matches!(tile, TileId::Path | TileId::Choke | TileId::Heartseed | TileId::PetAnchor | TileId::WaterPool)
}

/// Paints [`TileId::CircuitLink`] on the pet's own orthogonal neighbour
/// tiles, but never over meaningful terrain (see [`blocks_circuit_halo`]'s
/// own doc comment) -- see this crate's own doc comment for why a halo,
/// not a traced line, and why it must never overwrite a real terrain tile.
fn paint_circuit_link_halo(surface: &mut Surface, pet: &PetView) {
    if pet.linked_towers.is_empty() {
        return;
    }
    let (px, py) = pet_tile(pet);
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = px as i32 + dx;
        let ny = py as i32 + dy;
        if nx < 0 || ny < 0 || nx >= BOARD_WIDTH || ny >= BOARD_HEIGHT {
            continue;
        }
        let (nx, ny) = (nx as u16, ny as u16);
        let blocked = surface.get(nx, ny).art.is_some_and(|art| blocks_circuit_halo(art.tile));
        if !blocked {
            surface.set(
                nx,
                ny,
                SurfaceCell { glyph: '~', fg: CIRCUIT_LINK_COLOR, bg: None, art: Some(TileArt { tile: TileId::CircuitLink, variant: 0 }) },
            );
        }
    }
}

fn paint_tower(surface: &mut Surface, tower: &TowerView) {
    let (cx, cy) = clamp_tile(tower.position.0, tower.position.1);
    let level_index = tower_level_index(tower.level);
    let fg = scale_color(tower_base_color(tower.kind), level_brightness_permille(level_index));
    let bg = tower.linked.then_some(LINK_BG);
    surface.set(
        cx,
        cy,
        SurfaceCell { glyph: tower_glyph(tower.kind), fg, bg, art: Some(TileArt { tile: tower_tile_id(tower.kind), variant: level_index }) },
    );
}

/// `(status background tint, `TileArt::variant`)` for one enemy -- shared
/// between the tile-grid `paint_enemy` pass below and
/// `hatchery-arcade-pet-bastion-render`'s own pixel-tier `interp` module,
/// so "slowed"/"stunned" reads as the identical tint on both paths rather
/// than two independently-maintained copies of the same three-way rule
/// (this pass's own "readable замедление" requirement).
pub(crate) fn enemy_status_bg_variant(enemy: &EnemyView) -> (Option<Rgb>, u8) {
    if enemy.stunned {
        (Some(STUN_BG), 2u8)
    } else if enemy.slow_permille > 0 {
        (Some(SLOW_BG), 1u8)
    } else {
        (None, 0u8)
    }
}

fn paint_enemy(surface: &mut Surface, enemy: &EnemyView) {
    let (x, y) = round_to_tile(enemy.position);
    let (cx, cy) = clamp_tile(x, y);
    let (bg, variant) = enemy_status_bg_variant(enemy);
    surface.set(
        cx,
        cy,
        SurfaceCell { glyph: enemy_glyph(enemy.kind), fg: enemy_color(enemy.kind), bg, art: Some(TileArt { tile: enemy_tile_id(enemy.kind), variant }) },
    );
}

/// `TileArt::variant` = remaining HP in tenths (0..=10) -- lets
/// [`hatchery_arcade_engine::SixelBackend`]'s own procedural boss shape
/// react to how close the fight is, without this adapter needing to know
/// anything about sixel rendering itself.
pub(crate) fn boss_hp_tenths(boss: &BossView) -> u8 {
    if boss.max_hp <= 0 {
        return 0;
    }
    let permille = (boss.hp.max(0) as i64 * 1000) / boss.max_hp as i64;
    ((permille + 50) / 100).clamp(0, 10) as u8
}

fn paint_boss(surface: &mut Surface, boss: &BossView) {
    let variant = boss_hp_tenths(boss);
    for body in &boss.bodies {
        let (x, y) = round_to_tile(body.position);
        let (cx, cy) = clamp_tile(x, y);
        surface.set(
            cx,
            cy,
            SurfaceCell {
                glyph: boss_glyph(boss.kind),
                fg: boss_color(boss.kind),
                bg: Some(boss_bg(boss.kind)),
                art: Some(TileArt { tile: boss_tile_id(boss.kind), variant }),
            },
        );
    }
}

fn paint_pet(surface: &mut Surface, pet: &PetView) {
    let (cx, cy) = pet_tile(pet);
    surface.set(
        cx,
        cy,
        SurfaceCell { glyph: '@', fg: PET_COLOR, bg: evolution_bg(pet.evolution), art: Some(TileArt { tile: TileId::Pet, variant: evolution_variant(pet.evolution) }) },
    );
}

/// Converts one [`SimulationSnapshot`] into a fresh `BOARD_WIDTH x
/// BOARD_HEIGHT` [`Surface`] -- terrain, then the Circuit-link halo, then
/// every live enemy/boss body/tower/the pet itself, each pass painting
/// over whatever the previous one left on that exact tile. See this
/// crate's own doc comment for the full colour/glyph/shape catalog and
/// the reasoning behind the "effects" tile family's scope.
pub fn snapshot_to_surface(snapshot: &SimulationSnapshot) -> Surface {
    let mut surface = Surface::new(BOARD_WIDTH as u16, BOARD_HEIGHT as u16, SurfaceCell::BLANK);

    paint_terrain(&mut surface);
    paint_circuit_link_halo(&mut surface, &snapshot.pet);
    for enemy in &snapshot.enemies {
        paint_enemy(&mut surface, enemy);
    }
    if let Some(boss) = &snapshot.boss {
        paint_boss(&mut surface, boss);
    }
    for tower in &snapshot.towers {
        paint_tower(&mut surface, tower);
    }
    paint_pet(&mut surface, &snapshot.pet);

    surface
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatchery_arcade_pet_bastion::snapshot::{BossBodyView, RunPhaseView};
    use hatchery_arcade_pet_bastion::tower;
    use hatchery_arcade_pet_bastion::wave::Difficulty;

    fn empty_snapshot() -> SimulationSnapshot {
        SimulationSnapshot {
            tick_index: 0,
            difficulty: Difficulty::Standard,
            wave: 1,
            phase: RunPhaseView::Build { ticks_remaining: 10 },
            sap: 0,
            integrity: 20,
            crab_shield: 0,
            towers: Vec::new(),
            enemies: Vec::new(),
            boss: None,
            pet: PetView { state: PetState::AtAnchor(AnchorId(0)), spark: 0, evolution: None, linked_towers: Vec::new() },
            rune_options: Vec::new(),
            runes_picked: Vec::new(),
            pet_charge_options: Vec::new(),
            pet_charges_picked: Vec::new(),
            field_zones: Vec::new(),
            wave_plan: None,
            build_cells: Vec::new(),
        }
    }

    #[test]
    fn surface_dimensions_match_the_board() {
        let surface = snapshot_to_surface(&empty_snapshot());
        assert_eq!(surface.width(), BOARD_WIDTH as u16);
        assert_eq!(surface.height(), BOARD_HEIGHT as u16);
    }

    #[test]
    fn terrain_surface_matches_every_environment_cell_a_fresh_snapshot_surface_carries() {
        // `terrain_surface`'s whole reason to exist: it must carry EXACTLY
        // the same environment tiles `snapshot_to_surface` paints from an
        // otherwise-empty snapshot, with the ONE expected exception of the
        // pet's own current anchor tile -- `empty_snapshot`'s own pet is
        // always `AtAnchor(0)`, and `paint_pet` always overwrites that one
        // cell with `TileId::Pet` even on an otherwise pristine board (see
        // `paint_pet`'s own call in `snapshot_to_surface`). Every OTHER
        // cell must be byte-identical, since a real host relies on
        // `terrain_surface` being indistinguishable from the terrain layer
        // any live run's own Surface shows before the first tower is ever
        // placed.
        let terrain = terrain_surface();
        let from_snapshot = snapshot_to_surface(&empty_snapshot());
        assert_eq!(terrain.width(), from_snapshot.width());
        assert_eq!(terrain.height(), from_snapshot.height());
        let pet_anchor_tile = Board::anchor_tile(AnchorId(0));
        let (pax, pay) = clamp_tile(pet_anchor_tile.x, pet_anchor_tile.y);
        for y in 0..terrain.height() {
            for x in 0..terrain.width() {
                if (x, y) == (pax, pay) {
                    continue;
                }
                assert_eq!(terrain.get(x, y), from_snapshot.get(x, y), "terrain_surface must match snapshot_to_surface at ({x},{y}) for an empty (no towers/enemies) snapshot");
            }
        }
    }

    #[test]
    fn terrain_surface_is_deterministic_across_independent_calls() {
        assert_eq!(terrain_surface().width(), terrain_surface().width());
        let a = terrain_surface();
        let b = terrain_surface();
        for y in 0..a.height() {
            for x in 0..a.width() {
                assert_eq!(a.get(x, y), b.get(x, y), "the board's own fixed layout must paint byte-identical terrain every call");
            }
        }
    }

    #[test]
    fn terrain_surface_never_carries_buildpad_tower_or_dynamic_content() {
        let terrain = terrain_surface();
        for y in 0..terrain.height() {
            for x in 0..terrain.width() {
                if let Some(art) = terrain.get(x, y).art {
                    assert!(!art.tile.is_dynamic_entity(), "terrain_surface must never carry a dynamic-entity tile at ({x},{y})");
                    assert!(art.tile.is_board_environment(), "terrain_surface must only ever carry board-environment tiles, found {:?} at ({x},{y})", art.tile);
                }
            }
        }
    }

    #[test]
    fn every_single_cell_is_populated_no_empty_space_in_a_normal_frame() {
        // The whole point of the night-garden `Ground` base layer: a
        // normal frame must never leave a genuinely empty (fully
        // background) tile anywhere on the board -- see `TileId::Ground`'s
        // own doc comment. Every cell must carry real `art` and a
        // non-space glyph, not merely "at least one populated cell", the
        // old, much weaker version of this test.
        let surface = snapshot_to_surface(&empty_snapshot());
        for y in 0..surface.height() {
            for x in 0..surface.width() {
                let cell = surface.get(x, y);
                assert!(cell.art.is_some(), "tile ({x},{y}) is empty background in a normal frame -- the ground layer must cover every tile");
                assert_ne!(cell.glyph, ' ', "tile ({x},{y}) carries a space glyph despite being populated");
            }
        }
    }

    #[test]
    fn build_pads_never_appear_in_a_normal_frame() {
        // Item 1 of this pass's own brief: free build slots are not part
        // of the ordinary render at all any more -- only
        // `paint_build_zone_highlight` (opt-in, drag-mode only) ever
        // produces a `TileId::BuildPad` cell.
        let surface = snapshot_to_surface(&empty_snapshot());
        for y in 0..surface.height() {
            for x in 0..surface.width() {
                assert_ne!(surface.get(x, y).art.map(|a| a.tile), Some(TileId::BuildPad), "tile ({x},{y}) carries BuildPad in a normal (non-dragging) frame");
            }
        }
    }

    #[test]
    fn paint_build_zone_highlight_marks_every_currently_buildable_tile() {
        let mut snapshot = empty_snapshot();
        // `empty_snapshot`'s own `build_cells` is deliberately empty (no
        // snapshot has been taken from a real `Simulation`) -- build the
        // real per-tile view directly off `Board`, the same source
        // `Simulation::snapshot` itself reads (`sim.rs`'s own
        // `build_zone_cells`-derived `build_cells`).
        snapshot.build_cells = Board::new()
            .build_zone_cells()
            .iter()
            .map(|&(tile, reason)| hatchery_arcade_pet_bastion::snapshot::BuildCellView { tile: (tile.x, tile.y), reason })
            .collect();
        let mut surface = snapshot_to_surface(&snapshot);
        let buildable_count = snapshot.build_cells.iter().filter(|c| c.reason.is_none()).count();
        assert!(buildable_count > 0, "an empty board must have at least one buildable tile to highlight");
        paint_build_zone_highlight(&mut surface, &snapshot);
        let mut painted = 0usize;
        for cell in &snapshot.build_cells {
            if cell.reason.is_some() {
                continue;
            }
            let (cx, cy) = clamp_tile(cell.tile.0, cell.tile.1);
            if surface.get(cx, cy).art.map(|a| a.tile) == Some(TileId::BuildPad) {
                painted += 1;
            }
        }
        assert_eq!(painted, buildable_count, "every currently-buildable tile must carry BuildPad once the highlight is painted");
    }

    #[test]
    fn heartseed_tile_is_painted_at_the_real_board_position() {
        let surface = snapshot_to_surface(&empty_snapshot());
        let (hx, hy) = clamp_tile(HEARTSEED.x, HEARTSEED.y);
        let cell = surface.get(hx, hy);
        assert_eq!(cell.glyph, '*');
        assert_eq!(cell.art.map(|a| a.tile), Some(TileId::Heartseed));
    }

    #[test]
    fn pet_tile_overrides_the_anchor_terrain_underneath_it() {
        let surface = snapshot_to_surface(&empty_snapshot());
        let anchor_tile = Board::anchor_tile(AnchorId(0));
        let (cx, cy) = clamp_tile(anchor_tile.x, anchor_tile.y);
        let cell = surface.get(cx, cy);
        assert_eq!(cell.glyph, '@');
        assert_eq!(cell.art.map(|a| a.tile), Some(TileId::Pet));
    }

    #[test]
    fn a_placed_tower_overrides_its_own_build_cell_terrain() {
        let mut snapshot = empty_snapshot();
        // A real buildable cell under the free-placement rule -- 2 tiles
        // off route 0's own first (0,3)-(20,3) leg, exactly the old fixed
        // pad table's own `PADS[0]` position.
        let tile = Tile::new(4, 1);
        let stats = tower::effective_stats(TowerKind::Bell, UpgradeLevel::L2);
        let base_cost = TowerKind::Bell.base_stats().cost;
        snapshot.towers.push(TowerView {
            id: hatchery_arcade_pet_bastion::ids::EntityIdAllocator::default().next(),
            kind: TowerKind::Bell,
            level: UpgradeLevel::L2,
            position: (tile.x, tile.y),
            linked: true,
            cooldown_ticks: 0,
            stats,
            next_upgrade_cost: tower::next_upgrade_cost(UpgradeLevel::L2, base_cost),
            sell_price: tower::sell_price(base_cost),
        });
        let surface = snapshot_to_surface(&snapshot);
        let (cx, cy) = clamp_tile(tile.x, tile.y);
        let cell = surface.get(cx, cy);
        assert_eq!(cell.glyph, 'B');
        assert_eq!(cell.bg, Some(LINK_BG), "a linked tower must carry the Circuit-link background tint");
        assert_eq!(cell.art, Some(TileArt { tile: TileId::TowerBell, variant: 1 }));
    }

    #[test]
    fn circuit_link_halo_never_overwrites_a_real_terrain_tile() {
        let mut snapshot = empty_snapshot();
        snapshot.pet.linked_towers = vec![hatchery_arcade_pet_bastion::ids::EntityIdAllocator::default().next()];
        let surface = snapshot_to_surface(&snapshot);
        let anchor_tile = Board::anchor_tile(AnchorId(0));

        // Anchor 0 sits at (10, 2); its own north neighbour (10, 3) is a
        // genuine Path tile (route 0's own (0,3)-(20,3) leg) -- the halo
        // must never overwrite it.
        let (px, py) = clamp_tile(anchor_tile.x, anchor_tile.y + 1);
        let path_neighbour = surface.get(px, py);
        assert_eq!(path_neighbour.art.map(|a| a.tile), Some(TileId::Path), "a genuine route tile next to the anchor must survive the halo untouched");

        // At least one of the anchor's own OTHER (non-route) neighbours
        // must legitimately pick up the halo -- proving this is a real
        // opt-out, not a halo that silently never paints anything.
        let mut saw_halo = false;
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1)] {
            let nx = anchor_tile.x + dx;
            let ny = anchor_tile.y + dy;
            if nx < 0 || ny < 0 || nx >= BOARD_WIDTH || ny >= BOARD_HEIGHT {
                continue;
            }
            let (cx, cy) = clamp_tile(nx, ny);
            let cell = surface.get(cx, cy);
            if cell.art.map(|a| a.tile) == Some(TileId::CircuitLink) {
                assert_eq!(cell.glyph, '~', "a CircuitLink tile must always carry its own glyph, never a leftover terrain glyph");
                saw_halo = true;
            }
        }
        assert!(saw_halo, "at least one non-route anchor neighbour must legitimately carry the Circuit-link halo");
    }

    #[test]
    fn boss_variant_reflects_remaining_hp_in_tenths() {
        let mut snapshot = empty_snapshot();
        snapshot.boss = Some(BossView {
            kind: BossKind::Bellkeeper,
            hp: 500,
            max_hp: 1000,
            hp_permille: 500,
            bodies: vec![BossBodyView { id: hatchery_arcade_pet_bastion::ids::EntityIdAllocator::default().next(), position: FixedPos::new(0, 0), slow_permille: 0 }],
            final_phase: false,
            split_triggered: false,
            escort_triggered: [false; 3],
        });
        let surface = snapshot_to_surface(&snapshot);
        let (cx, cy) = clamp_tile(0, 0);
        let cell = surface.get(cx, cy);
        assert_eq!(cell.art, Some(TileArt { tile: TileId::BossBellkeeper, variant: 5 }));
    }
}
