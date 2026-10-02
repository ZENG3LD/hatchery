//! Headless URX CPU-render cost bench: how long does `uzor-urx-cpu::
//! CpuBackend::render` (via `hatchery-arcade-engine`'s own public
//! `build_scene`/`render_over_background`) cost on a scene of this crate's
//! own real complexity, at the full `560x266` board `bench`'s own `live`/
//! `encode` binaries already proved the terminal holds at 60fps
//! (`BOARD_COLS=56, BOARD_ROWS=14` at `10x19` px/cell there; here `28x14`
//! real Pet Bastion board tiles at `20x19` px/cell -- same `560x266`
//! total, see `hatchery-arcade-engine/src/render/backend_pixel.rs`'s own
//! `PX_PER_CELL_W` doc comment for why `20`, not `10`). No terminal
//! involved -- this is the OTHER half of the 16.7ms/60fps frame budget
//! those binaries measure (encode + terminal ingest is the sixel side);
//! this binary measures the drawing cost itself.
//!
//! Two distinct costs, measured separately, matching the engine's own
//! background/overlay split (`hatchery-arcade-engine::render::
//! background`'s own module doc comment):
//!
//! - `build_background` -- the continuous ground/decor/route/pool/marker
//!   layer, rasterised ONCE from a terrain-only `Surface` into a cached
//!   `Pixmap`. Paid exactly once per board (the FIRST frame a real host
//!   ever draws, or whenever the board's own layout genuinely changes --
//!   never once per tick).
//! - `build_scene` + `render_over_background` -- every SUBSEQUENT frame's
//!   real cost: towers/`BuildPad`/`CircuitLink` overlay content, every
//!   dynamic sprite/stroke, the vignette, composited onto a cheap clone of
//!   the already-cached background. This is the number that actually has
//!   to fit the 60fps budget, every single tick.
//!
//! Run: `cargo run --release -p hatchery-arcade-bench --bin urx_render`

use std::time::Instant;

use hatchery_arcade_bench::stats;
use hatchery_arcade_engine::{build_background, build_scene, render_over_background, BoardBackground, DynamicSprite, DynamicStroke, Rgb, Surface, SurfaceCell, TileArt, TileId};

const BOARD_W: u16 = 28;
const BOARD_H: u16 = 14;
const PX_PER_CELL_W: u32 = 20;
const PX_PER_CELL_H: u32 = 19;
const WARMUP: usize = 8;
const SAMPLES: usize = 120;
/// The sixel-side cost this crate's own `live`/`encode` binaries already
/// measured for a full `560x266` board (`backend_pixel.rs`'s own module
/// doc: "2.85ms encode + 4.78ms terminal ingest" at 32 colours, a
/// 60.18fps-holding live run). `backend_pixel.rs`'s own `MAX_COLORS` is now
/// 256, not 32 (a separate live 240-frame run at 256 colours, same board
/// size, held 60.07fps with 98.8% of frames inside budget -- only ~1ms
/// costlier overall than the 32-colour run); +1000us here is a
/// conservative, explicitly-approximate carry of that owner-reported delta
/// onto the 32-colour figure above, not a second independent live
/// measurement of the 256-colour sixel side alone.
const SIXEL_SIDE_US: f64 = 2850.0 + 4780.0 + 1000.0;
const FRAME_BUDGET_US: f64 = 16_700.0;

fn terrain_cell(glyph: char, fg: Rgb, tile: TileId, variant: u8) -> SurfaceCell {
    SurfaceCell { glyph, fg, bg: None, art: Some(TileArt { tile, variant }) }
}

/// Cheap deterministic hash, the SAME finalizer shape `hatchery-arcade-
/// pet-bastion-render::tile_hash` uses (see that function's own doc
/// comment) -- reproduced here, not imported, so this bench stays a leaf
/// that never depends on `hatchery-arcade-pet-bastion`'s own simulation
/// (this module's own doc comment).
fn tile_hash(x: u16, y: u16, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1);
    h ^= (y as u32).wrapping_mul(0x85EB_CA77).rotate_left(13);
    h ^= salt.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// Whether `is_route(x, y)` -- a two-lane path merging at `x=14`, the same
/// synthetic route shape this bench has always used, kept unchanged so the
/// route/choke footprint stays comparable across this file's own history.
fn is_route(x: i32, y: i32) -> bool {
    y == 6 || y == 7 || (x == 14 && (2..12).contains(&y))
}

/// Builds a `28x14` terrain-only [`Surface`] representative of a mid-wave
/// Pet Bastion night-garden board: a `Ground`-covered board (every tile,
/// deterministic tone variant), a static decor scatter (rocks/plants/
/// fireflies, ~20% of tiles -- the same density `hatchery-arcade-pet-
/// bastion-render::decor_for_tile` produces), a two-lane path shaped by
/// real neighbour connectivity, a small water pool ringing the Heartseed,
/// and pet anchors -- the same `TileId::is_board_environment` content
/// `hatchery-arcade-pet-bastion-render::terrain_surface` would paint
/// from the real board, hand-built here so `bench` never has to depend on
/// `hatchery-arcade-pet-bastion`'s own simulation (this bench stays a
/// leaf that only depends on `engine`'s `render` feature, exactly like
/// `preview` already does). No tower, no `BuildPad`, no dynamic content --
/// exactly what [`build_background`] is meant to be fed, matching
/// `hatchery-arcade-engine::render::background`'s own "feed this a
/// TERRAIN-ONLY Surface" doc requirement.
fn representative_terrain_surface() -> Surface {
    let mut surface = Surface::new(BOARD_W, BOARD_H, SurfaceCell::BLANK);
    let ground_color = Rgb(26, 42, 32);
    let path_color = Rgb(101, 88, 63);
    let choke_color = Rgb(150, 90, 50);
    let rock_color = Rgb(94, 96, 104);
    let plant_color = Rgb(58, 108, 68);
    let water_color = Rgb(36, 66, 104);
    let firefly_color = Rgb(150, 240, 230);

    // Pass 1: ground everywhere.
    for y in 0..BOARD_H {
        for x in 0..BOARD_W {
            let variant = (tile_hash(x, y, 1) % 4) as u8;
            surface.set(x, y, terrain_cell('`', ground_color, TileId::Ground, variant));
        }
    }
    // Pass 2: static decor scatter.
    for y in 0..BOARD_H {
        for x in 0..BOARD_W {
            let bucket = tile_hash(x, y, 2) % 100;
            let vs = tile_hash(x, y, 3);
            match bucket {
                0..=6 => surface.set(x, y, terrain_cell('O', rock_color, TileId::Rock, (vs % 6) as u8)),
                7..=15 => surface.set(x, y, terrain_cell('"', plant_color, TileId::Plant, (vs % 6) as u8)),
                16..=19 => surface.set(x, y, terrain_cell(':', firefly_color, TileId::Firefly, (vs % 9) as u8)),
                _ => {}
            }
        }
    }
    // Pass 3: the route itself, shaped by real neighbour connectivity (bit
    // 1=north, 2=east, 4=south, 8=west -- matches `render::sprites`'s own
    // private `ROAD_*` constants).
    for y in 0..BOARD_H as i32 {
        for x in 0..BOARD_W as i32 {
            if !is_route(x, y) {
                continue;
            }
            let tile_id = if (x, y) == (14, 7) { TileId::Choke } else { TileId::Path };
            let mut mask = 0u8;
            for (dx, dy, bit) in [(0i32, -1i32, 1u8), (1, 0, 2u8), (0, 1, 4u8), (-1, 0, 8u8)] {
                if is_route(x + dx, y + dy) {
                    mask |= bit;
                }
            }
            let (color, glyph) = if tile_id == TileId::Choke { (choke_color, '+') } else { (path_color, '.') };
            surface.set(x as u16, y as u16, terrain_cell(glyph, color, tile_id, mask));
        }
    }
    // Pass 4: a small water pool ringing the Heartseed at (14, 6).
    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            let cheby = dx.abs().max(dy.abs());
            if cheby == 0 || cheby > 2 {
                continue;
            }
            let (hx, hy) = (14i32 + dx, 6i32 + dy);
            if hx < 0 || hy < 0 || hx >= BOARD_W as i32 || hy >= BOARD_H as i32 || is_route(hx, hy) {
                continue;
            }
            surface.set(hx as u16, hy as u16, terrain_cell('=', water_color, TileId::WaterPool, 0));
        }
    }
    surface.set(14, 6, terrain_cell('*', Rgb(255, 215, 0), TileId::Heartseed, 0));
    for &(x, y) in &[(6u16, 5u16), (6, 8), (20, 5), (20, 8)] {
        surface.set(x, y, terrain_cell('a', Rgb(120, 170, 220), TileId::PetAnchor, 0));
    }
    surface
}

/// [`representative_terrain_surface`] plus a dozen towers spanning all six
/// kinds and every level -- the same OVERLAY content (`TileId::is_tower`,
/// never `is_board_environment`) `hatchery-arcade-pet-bastion-render::
/// paint_tower` would add on top of the terrain from a real snapshot. This
/// is the `Surface` [`build_scene`]'s own per-frame overlay pass reads --
/// the background layer underneath it is composited separately, from a
/// SEPARATE [`representative_terrain_surface`] call, exactly matching a
/// real host's own split.
///
/// [`TileId::BuildPad`] is deliberately ABSENT here -- a normal frame
/// never paints it any more (this pass's own "убери слоты из постоянной
/// отрисовки" requirement); see [`representative_surface_dragging`] for
/// the one scenario that still exercises it.
fn representative_surface() -> Surface {
    let mut surface = representative_terrain_surface();
    let towers: [(u16, u16, TileId, Rgb, u8); 12] = [
        (5, 4, TileId::TowerNeedle, Rgb(200, 200, 200), 2),
        (9, 5, TileId::TowerNeedle, Rgb(200, 200, 200), 1),
        (13, 4, TileId::TowerBell, Rgb(150, 120, 255), 2),
        (17, 5, TileId::TowerBell, Rgb(150, 120, 255), 0),
        (21, 4, TileId::TowerPrism, Rgb(255, 140, 255), 2),
        (24, 6, TileId::TowerPrism, Rgb(255, 140, 255), 1),
        (5, 9, TileId::TowerEmberNest, Rgb(255, 110, 40), 2),
        (9, 10, TileId::TowerEmberNest, Rgb(255, 110, 40), 0),
        (13, 10, TileId::TowerMoonwell, Rgb(110, 200, 255), 2),
        (17, 9, TileId::TowerMoonwell, Rgb(110, 200, 255), 1),
        (21, 10, TileId::TowerRelay, Rgb(180, 180, 80), 2),
        (24, 9, TileId::TowerRelay, Rgb(180, 180, 80), 0),
    ];
    for &(x, y, tile, color, level) in &towers {
        surface.set(x, y, SurfaceCell { glyph: 'T', fg: color, bg: Some(Rgb(60, 40, 90)), art: Some(TileArt { tile, variant: level }) });
    }
    surface
}

/// [`representative_surface`] plus [`TileId::BuildPad`] painted over every
/// tile that is not already route/water/anchor/Heartseed/tower terrain --
/// the one scenario that still exercises `BuildPad` at all, matching
/// `hatchery-arcade-pet-bastion-render::paint_build_zone_highlight`'s own
/// opt-in, drag-mode-only overlay (`board::Board::near_route_cells` marks
/// roughly half a real board's own tiles buildable, so this is a real,
/// dense worst case, not a token handful).
fn representative_surface_dragging() -> Surface {
    let mut surface = representative_surface();
    let pad_color = Rgb(110, 110, 130);
    let blocks_highlight = |tile: TileId| {
        matches!(
            tile,
            TileId::Path
                | TileId::Choke
                | TileId::Heartseed
                | TileId::PetAnchor
                | TileId::WaterPool
                | TileId::TowerNeedle
                | TileId::TowerBell
                | TileId::TowerPrism
                | TileId::TowerEmberNest
                | TileId::TowerMoonwell
                | TileId::TowerRelay
        )
    };
    for y in 0..BOARD_H {
        for x in 0..BOARD_W {
            let blocked = surface.get(x, y).art.is_some_and(|a| blocks_highlight(a.tile));
            if !blocked {
                surface.set(x, y, terrain_cell('o', pad_color, TileId::BuildPad, 0));
            }
        }
    }
    surface
}

/// Two dozen enemies across all six kinds (a third carrying a slow/stun
/// status tint), a three-body boss, the pet plus its Circuit-link halo,
/// two in-flight projectiles, and one of each combat-effect pip
/// (impact flash, death burst, splash ring, Link Burst pulse) -- plus a
/// two-jump Prism chain and a Link Burst charge line as [`DynamicStroke`]s.
/// "террейн, десяток башен, два десятка врагов, босс, пет, снаряды и
/// эффекты", per this pass's own brief.
fn representative_dynamic() -> (Vec<DynamicSprite>, Vec<DynamicStroke>) {
    let enemy_kinds = [
        (TileId::EnemyMite, Rgb(170, 220, 120)),
        (TileId::EnemySkitter, Rgb(255, 220, 100)),
        (TileId::EnemyShellback, Rgb(140, 100, 60)),
        (TileId::EnemySplitter, Rgb(200, 80, 200)),
        (TileId::EnemyHusher, Rgb(120, 120, 190)),
        (TileId::EnemyMirror, Rgb(210, 210, 255)),
    ];
    let mut sprites = Vec::with_capacity(32);
    for i in 0..24u32 {
        let (tile, color) = enemy_kinds[(i as usize) % enemy_kinds.len()];
        let (bg, variant) = match i % 5 {
            0 => (Some(Rgb(40, 90, 140)), 1u8),
            1 => (Some(Rgb(160, 150, 40)), 2u8),
            _ => (None, 0u8),
        };
        let tile_x = 1.5 + (i as f64 * 1.13) % (BOARD_W as f64 - 3.0);
        let tile_y = 1.5 + (i as f64 * 0.71) % (BOARD_H as f64 - 3.0);
        sprites.push(DynamicSprite { tile, variant, fg: color, bg, tile_x, tile_y, scale: 1.0, alpha: 1.0 });
    }

    for (i, (dx, dy)) in [(0.0, 0.0), (1.2, 0.4), (-1.1, 0.6)].into_iter().enumerate() {
        sprites.push(DynamicSprite {
            tile: TileId::BossBellkeeper,
            variant: 6,
            fg: Rgb(255, 80, 80),
            bg: Some(Rgb(80, 10, 10)),
            tile_x: 14.0 + dx,
            tile_y: 7.0 + dy,
            scale: if i == 0 { 1.6 } else { 1.0 },
            alpha: 1.0,
        });
    }

    sprites.push(DynamicSprite { tile: TileId::CircuitLink, variant: 0, fg: Rgb(203, 166, 247), bg: None, tile_x: 14.0, tile_y: 6.0, scale: 2.2, alpha: 0.55 });
    sprites.push(DynamicSprite { tile: TileId::Pet, variant: 3, fg: Rgb(203, 166, 247), bg: Some(Rgb(20, 50, 70)), tile_x: 14.0, tile_y: 6.0, scale: 1.0, alpha: 1.0 });

    sprites.push(DynamicSprite { tile: TileId::Projectile, variant: 0, fg: Rgb(200, 200, 200), bg: None, tile_x: 7.0, tile_y: 5.0, scale: 1.0, alpha: 0.9 });
    sprites.push(DynamicSprite { tile: TileId::Projectile, variant: 0, fg: Rgb(255, 110, 40), bg: None, tile_x: 18.0, tile_y: 9.0, scale: 1.0, alpha: 0.9 });
    sprites.push(DynamicSprite { tile: TileId::ImpactFlash, variant: 0, fg: Rgb(255, 255, 255), bg: None, tile_x: 9.5, tile_y: 5.5, scale: 1.3, alpha: 0.7 });
    sprites.push(DynamicSprite { tile: TileId::DeathBurst, variant: 0, fg: Rgb(170, 220, 120), bg: None, tile_x: 12.0, tile_y: 8.0, scale: 1.8, alpha: 0.5 });
    sprites.push(DynamicSprite { tile: TileId::SplashRing, variant: 0, fg: Rgb(255, 110, 40), bg: None, tile_x: 18.0, tile_y: 9.0, scale: 1.5, alpha: 0.6 });
    sprites.push(DynamicSprite { tile: TileId::LinkPulse, variant: 0, fg: Rgb(203, 166, 247), bg: None, tile_x: 21.0, tile_y: 10.0, scale: 1.4, alpha: 0.8 });

    let strokes = vec![
        DynamicStroke { from_tile: (21.0, 4.0), to_tile: (19.0, 6.0), color: Rgb(255, 140, 255), width_px: 1.4, bulge: 0.35, alpha: 0.8 },
        DynamicStroke { from_tile: (19.0, 6.0), to_tile: (17.0, 5.5), color: Rgb(255, 140, 255), width_px: 1.4, bulge: 0.35, alpha: 0.7 },
        DynamicStroke { from_tile: (14.0, 6.0), to_tile: (21.0, 10.0), color: Rgb(203, 166, 247), width_px: 1.4, bulge: 0.18, alpha: 0.6 },
        DynamicStroke { from_tile: (7.0, 4.0), to_tile: (7.0, 5.0), color: Rgb(200, 200, 200), width_px: 1.6, bulge: 0.0, alpha: 0.7 },
    ];
    (sprites, strokes)
}

fn fmt_percentiles(label: &str, samples: &[f64]) {
    match stats::percentiles(samples) {
        Some(p) => println!("{label} p50={:.1} p95={:.1} max={:.1} mean={:.1} n={}", p.p50, p.p95, p.max, p.mean, p.n),
        None => println!("{label} NO SAMPLES"),
    }
}

/// Runs `WARMUP + SAMPLES` `build_background` cycles and prints its own
/// percentile report -- the FIRST-FRAME-EVER cost (or "board changed"
/// cost) a real host pays exactly once, never per tick. Looped only to
/// get stable percentile statistics on that one-time cost, exactly the
/// "p50 и p95" the owner's own brief asked this bench to report for it --
/// a real host still calls this exactly once per board, per
/// `hatchery-arcade-engine::render::background`'s own module doc.
fn measure_background_build(terrain_surface: &Surface) {
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut px_w = 0u32;
    let mut px_h = 0u32;
    for i in 0..(WARMUP + SAMPLES) {
        let t0 = Instant::now();
        let background = build_background(terrain_surface);
        let us = t0.elapsed().as_secs_f64() * 1e6;
        px_w = background.width_px();
        px_h = background.height_px();
        if i >= WARMUP {
            samples.push(us);
        }
    }
    println!("-- static background build (paid ONCE per board, cached and reused every later frame) --");
    println!("background canvas: {px_w}x{px_h}px");
    fmt_percentiles("  build_background_us  ", &samples);
    println!();
}

/// Runs `WARMUP + SAMPLES` build_scene+render_over_background cycles for
/// one `dragging` mode against an ALREADY-BUILT `background` and prints
/// its own percentile report -- this is the real per-tick cost, the
/// SUBSEQUENT-frame number that has to fit the 60fps budget every single
/// tick. See `main`'s own doc comment for why both modes get measured (a
/// real frame is `dragging=false` almost all the time; `dragging=true` --
/// every `BuildPad` glowing at once -- is this scene's own worst case, not
/// its typical one).
fn measure(label: &str, background: &BoardBackground, surface: &Surface, dynamic: &[DynamicSprite], strokes: &[DynamicStroke], dragging: bool) {
    let mut build_samples = Vec::with_capacity(SAMPLES);
    let mut render_samples = Vec::with_capacity(SAMPLES);
    let mut command_count = 0usize;
    let mut canvas_bytes = 0usize;

    for i in 0..(WARMUP + SAMPLES) {
        let t0 = Instant::now();
        let scene = build_scene(surface, dynamic, strokes, dragging);
        let build_us = t0.elapsed().as_secs_f64() * 1e6;

        let t1 = Instant::now();
        let canvas = render_over_background(background, &scene);
        let render_us = t1.elapsed().as_secs_f64() * 1e6;

        command_count = scene.commands.len();
        canvas_bytes = canvas.rgba.len();
        if i >= WARMUP {
            build_samples.push(build_us);
            render_samples.push(render_us);
        }
    }

    println!("-- {label} (dragging={dragging}) --");
    println!(
        "scene: {command_count} URX draw commands from {} dynamic sprites + {} strokes (overlay only -- the environment layer is reused from the already-cached background, not rebuilt)",
        dynamic.len(),
        strokes.len()
    );
    println!("canvas: {canvas_bytes} bytes ({} px)", canvas_bytes / 4);
    fmt_percentiles("  build_scene_us              ", &build_samples);
    fmt_percentiles("  render_over_background_us   ", &render_samples);
    let total: Vec<f64> = build_samples.iter().zip(render_samples.iter()).map(|(b, r)| b + r).collect();
    fmt_percentiles("  total draw_us (subsequent frame, background cached)", &total);
    if let Some(p) = stats::percentiles(&total) {
        let remaining_after_sixel = FRAME_BUDGET_US - SIXEL_SIDE_US;
        println!(
            "  budget check: frame={FRAME_BUDGET_US:.0}us, sixel side (this crate's own live proof)={SIXEL_SIDE_US:.0}us, \
             draw budget remaining={remaining_after_sixel:.0}us, this scene's own draw p50={:.0}us p95={:.0}us -> {}",
            p.p50,
            p.p95,
            if p.p95 <= remaining_after_sixel { "FITS at p95" } else { "OVER BUDGET at p95" }
        );
    }
    println!();
}

fn main() {
    let terrain_surface = representative_terrain_surface();
    let surface = representative_surface();
    let dragging_surface = representative_surface_dragging();
    let (dynamic, strokes) = representative_dynamic();
    let px_w = BOARD_W as u32 * PX_PER_CELL_W;
    let px_h = BOARD_H as u32 * PX_PER_CELL_H;
    assert_eq!((px_w, px_h), (560, 266), "this bench's own board must land on the exact size bench/live.rs already proved the terminal holds at 60fps");

    println!("hatchery-arcade-bench: urx_render");
    println!("board {px_w}x{px_h}px ({BOARD_W}x{BOARD_H} cells, {PX_PER_CELL_W}x{PX_PER_CELL_H}px/cell), 12 towers, night-garden ground+decor+road terrain, BuildPad absent from the normal frame");
    println!();

    // First-frame-ever cost: build the continuous background once.
    measure_background_build(&terrain_surface);

    // Every measurement below reuses the SAME already-built background --
    // exactly what a real host does after the first frame (or after the
    // one time the board's own layout ever changes).
    let background = build_background(&terrain_surface);

    // `dragging=false` is the common case (any frame that is not actively
    // mid-drag over the build UI, `representative_surface`'s own night-
    // garden static content -- no `BuildPad` anywhere on it at all).
    // `dragging=true` against `representative_surface_dragging` -- every
    // `BuildPad` on the board painting its own glow -- is the worst case a
    // real session only hits while the owner is physically dragging a
    // tower out of the palette.
    measure("typical frame (no build-zone highlight)", &background, &surface, &dynamic, &strokes, false);
    measure("worst case: dragging a tower (build-zone highlight, every BuildPad glowing)", &background, &dragging_surface, &dynamic, &strokes, true);
}
