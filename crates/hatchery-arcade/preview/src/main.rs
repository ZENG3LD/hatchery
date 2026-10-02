//! Real-frame preview tool for Pet Bastion: Night Garden's render paths
//! -- NOT part of the product. A disposable dev binary that finds one
//! genuine, deep-run `slow_stack`-policy snapshot (wave 4's Bellkeeper
//! fight -- see `frame.rs`), renders it through each of them, and
//! rasterizes the result to a PNG so the owner can compare them by eye.
//!
//! There are four, not the three this comment claimed until the pixel
//! path was added and it was not updated (`311a342`, `dd0a9ce`): the
//! three `RenderTier` backends that implement `RenderBackend::project`
//! (glyph, half-block, sixel), plus the whole-board pixel path, which is
//! a SEPARATE API (`build_scene`/`render_scene`/`compose_frame`/
//! `encode_frame`) with no `RenderTier` variant and no `RenderBackend`
//! impl of its own -- see `engine/src/render/backend_pixel.rs`. This
//! binary renders an eight-frame interpolated motion sequence for that
//! path specifically, which none of the other three can produce.
//!
//! Every pixel in every PNG traces back to a real render call over a
//! real `Surface` built from a real simulation snapshot -- see
//! `raster.rs`'s own doc comments for exactly what each path does and
//! does not fabricate.
//!
//! Deliberately its OWN workspace member, not a new binary inside
//! `sweep` or `engine`: it depends on `hatchery-arcade-engine`'s
//! `render` feature (`uzor-tui`/`uzor-text`/`icy_sixel`) PLUS `image`/
//! `ab_glyph` (a font rasterizer, needed only to turn the glyph tier's
//! own text output into a PNG). Neither belongs in `hatchery-arcade-
//! sweep`'s own dependency graph, which must stay headless (see that
//! crate's own `Cargo.toml` doc comment), nor in `engine`'s normal
//! (non-`render`) graph -- `cargo tree -p hatchery-arcade-sweep` never
//! resolves this crate or any of ITS dependencies, since nothing in the
//! `sweep`/`engine`(default)/`pet-bastion` dependency edges points at
//! `preview` (dependencies only point one way, into the workspace's
//! sim-core crates -- never back out to a dev tool).

mod frame;
mod raster;

use std::path::Path;
use std::time::Instant;

use ab_glyph::FontRef;
use hatchery_arcade_engine::{
    build_background, compose_frame, encode_frame, DirtyHint, GlyphBackend, HalfBlockBackend, PixelFrameOutput, RenderBackend, SixelBackend, SixelOutput, Surface, TileFootprint,
};
use hatchery_arcade_pet_bastion::constants::TICK_MS;
use hatchery_arcade_pet_bastion::wave::Difficulty;
use hatchery_arcade_pet_bastion_render::effects::EffectsLayer;
use hatchery_arcade_pet_bastion_render::interp::{interpolated_dynamic_sprites, render_sim_time, sim_time};
use hatchery_arcade_pet_bastion_render::{snapshot_to_surface, terrain_surface};
use uzor_tui::{buffer::TerminalBuffer, rect::Rect};

/// Preview output directory (synthetic path; not a real operator home).
/// This tool is not part of the product; its output is not committed.
const OUT_DIR: &str = r"C:\Users\example\AppData\Local\Temp\hatchery-arcade-preview";
const FONT_PATH: &str = r"C:\Windows\Fonts\consola.ttf";
/// Chosen preview terminal-cell pixel size for the glyph/half-block PNGs
/// -- a plausible monospace cell aspect (roughly 1:2, matching a typical
/// terminal font), not a measurement of any specific real terminal
/// (there is no live font-metrics query available to this offline tool).
const CELL_PX_W: u32 = 12;
const CELL_PX_H: u32 = 24;
const MAX_SEEDS_TO_TRY: u64 = 60;
/// Number of PNG frames the pixel-tier motion sequence renders, at a fixed
/// 1/60s step each -- the owner's own explicit ask ("восемь кадров подряд
/// с шагом 1/60 секунды").
const PIXEL_SEQUENCE_FRAMES: usize = 8;
/// Real sim ticks the sequence must actually capture to cover
/// `PIXEL_SEQUENCE_FRAMES` frames at 1/60s each: the last frame sits at
/// `(PIXEL_SEQUENCE_FRAMES - 1) / 60` seconds, which spans more than two
/// full 50ms ticks (20Hz) -- 3 captured tick-to-tick transitions covers it
/// with room to spare, so no frame ever needs to clamp past the captured
/// window.
const PIXEL_SEQUENCE_TICKS: usize = 3;
/// How many ticks ahead of the already-found frame this tool will search
/// for a window that actually contains a `Shot` event, so the demo shows
/// real combat rather than an idle moment -- see `frame::
/// capture_best_combat_window`'s own doc comment.
const PIXEL_SEQUENCE_COMBAT_SEARCH_TICKS: u64 = 120;
/// Matches `hatchery-arcade-sweep`'s own CLI default ceiling
/// (`cli.rs::Cli::default`'s own `max_ticks` doc comment).
const MAX_TICKS: u64 = 30_000;

fn print_glyph_text(surface: &Surface) {
    for y in 0..surface.height() {
        let mut line = String::with_capacity(surface.width() as usize);
        for x in 0..surface.width() {
            line.push(surface.get(x, y).glyph);
        }
        println!("{line}");
    }
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("preview: {message}");
    std::process::exit(1);
}

fn main() {
    let found = frame::find_preview_frame(MAX_SEEDS_TO_TRY, MAX_TICKS)
        .unwrap_or_else(|| fail(format_args!("no seed among the first {MAX_SEEDS_TO_TRY} produced a winning run with a qualifying wave-4-Bellkeeper frame")));

    println!("hatchery-arcade preview");
    println!("seed={} tick={} score={} wave=4 boss=Bellkeeper", found.seed, found.tick_index, found.score);
    println!(
        "towers={} enemies={} circuit_linked={} sap={} integrity={}",
        found.snapshot.towers.len(),
        found.snapshot.enemies.len(),
        !found.snapshot.pet.linked_towers.is_empty(),
        found.snapshot.sap,
        found.snapshot.integrity,
    );
    println!();

    let surface = snapshot_to_surface(&found.snapshot);
    let footprint = TileFootprint { cells_w: 1, cells_h: 1 };
    let dest = Rect::new(0, 0, surface.width(), surface.height());

    println!("=== Glyph tier (text) ===");
    print_glyph_text(&surface);
    println!();

    let mut glyph_buf = TerminalBuffer::new(surface.width(), surface.height());
    let mut glyph_backend = GlyphBackend;
    glyph_backend.project(&surface, footprint, dest, &mut glyph_buf, false, DirtyHint::Full);

    let font_bytes = std::fs::read(FONT_PATH).unwrap_or_else(|err| fail(format_args!("failed to read font at {FONT_PATH}: {err}")));
    let font = FontRef::try_from_slice(&font_bytes).unwrap_or_else(|err| fail(format_args!("{FONT_PATH} is not a valid font: {err}")));

    let glyph_png = Path::new(OUT_DIR).join("arcade-preview-glyph.png");
    raster::render_glyph_png(&glyph_buf, &font, CELL_PX_W, CELL_PX_H, &glyph_png).unwrap_or_else(|err| fail(format_args!("failed writing {glyph_png:?}: {err}")));
    println!("wrote {glyph_png:?} ({}x{} px)", surface.width() as u32 * CELL_PX_W, surface.height() as u32 * CELL_PX_H);

    let mut halfblock_buf = TerminalBuffer::new(surface.width(), surface.height());
    let mut halfblock_backend = HalfBlockBackend;
    halfblock_backend.project(&surface, footprint, dest, &mut halfblock_buf, false, DirtyHint::Full);

    let halfblock_png = Path::new(OUT_DIR).join("arcade-preview-halfblock.png");
    raster::render_halfblock_png(&halfblock_buf, CELL_PX_W, CELL_PX_H, &halfblock_png)
        .unwrap_or_else(|err| fail(format_args!("failed writing {halfblock_png:?}: {err}")));
    println!("wrote {halfblock_png:?} ({}x{} px)", surface.width() as u32 * CELL_PX_W, surface.height() as u32 * CELL_PX_H);

    let mut sixel_scratch = TerminalBuffer::new(surface.width(), surface.height());
    let mut sixel_backend = SixelBackend;
    let sixel_output = sixel_backend.project(&surface, footprint, dest, &mut sixel_scratch, false, DirtyHint::Full);
    let placements = match sixel_output {
        SixelOutput::Placements(placements) => placements,
        SixelOutput::NotImplemented => {
            eprintln!("preview: SixelBackend reported NotImplemented -- reporting honestly, not fabricating a PNG");
            Vec::new()
        }
    };

    let sixel_png = Path::new(OUT_DIR).join("arcade-preview-sixel.png");
    match raster::render_sixel_png(surface.width() as u32, surface.height() as u32, &placements, &sixel_png) {
        Ok(()) => println!(
            "wrote {sixel_png:?} ({}x{} px, {} tiles encoded)",
            surface.width() as u32 * raster::SIXEL_PX_PER_CELL_W,
            surface.height() as u32 * raster::SIXEL_PX_PER_CELL_H,
            placements.len()
        ),
        Err(err) => fail(format_args!("sixel PNG render failed: {err}")),
    }

    println!();
    println!("=== Pixel tier ({PIXEL_SEQUENCE_FRAMES} frames, 1/60s step, interpolated motion) ===");
    let sequence = frame::capture_best_combat_window(found.seed, Difficulty::Standard, found.tick_index, PIXEL_SEQUENCE_COMBAT_SEARCH_TICKS, PIXEL_SEQUENCE_TICKS, MAX_TICKS)
        .unwrap_or_else(|| fail("the chosen seed's own run ended before the pixel-sequence capture window filled"));
    println!("pixel-sequence window starts at tick {} ({} ticks after the printed frame above, chosen to actually contain combat)", sequence.start_tick, sequence.start_tick.saturating_sub(found.tick_index));

    // The static board background -- ground, decor, routes, the water pool,
    // Heartseed/anchor markers -- built ONCE from a terrain-only Surface
    // (never a live snapshot's own, see `terrain_surface`'s own doc
    // comment for why) and reused verbatim by every frame below, including
    // the drag-mode frame further down. This is the one cost that would
    // otherwise be paid again every single frame.
    let background_start = Instant::now();
    let background = build_background(&terrain_surface());
    let background_build_us = background_start.elapsed().as_secs_f64() * 1e6;
    println!("static background built once: {background_build_us:.0}us ({}x{}px, seed={})", background.width_px(), background.height_px(), background.seed());

    let mut effects_layer = EffectsLayer::new();
    let mut ingested_ticks = 0usize;
    let mut compose_us = Vec::with_capacity(PIXEL_SEQUENCE_FRAMES);
    let mut encode_us = Vec::with_capacity(PIXEL_SEQUENCE_FRAMES);

    for i in 0..PIXEL_SEQUENCE_FRAMES {
        let t_secs = i as f64 / 60.0;
        let tick_f = t_secs * 1000.0 / TICK_MS as f64;
        let tick_lo = (tick_f.floor() as usize).min(sequence.snapshots.len().saturating_sub(2));
        let alpha = (tick_f - tick_lo as f64).clamp(0.0, 1.0);

        // Ingest every tick's own real events exactly once, in order, the
        // first time this frame's own render sweep reaches or passes it --
        // an effect born on tick `k` must exist for every LATER frame too
        // (it ages/fades across several 1/60s frames, not just one).
        while ingested_ticks <= tick_lo {
            let born_at = sim_time(sequence.start_tick + ingested_ticks as u64, 0.0);
            effects_layer.ingest(&sequence.events[ingested_ticks], &sequence.snapshots[ingested_ticks], born_at);
            ingested_ticks += 1;
        }

        let prev = &sequence.snapshots[tick_lo];
        let curr = &sequence.snapshots[tick_lo + 1];
        // `render_sim_time` IS this loop's own former `sim_time(sequence.
        // start_tick + tick_lo, alpha)` -- `capture_best_combat_window`'s
        // own doc comment fixes `snapshots[k]` at `start_tick + k`, so
        // `prev.tick_index` is that same number. Going through the shared
        // helper rather than re-deriving it here keeps exactly one copy of
        // the `prev`-not-`curr` rule in the workspace, which is the copy
        // the TUI's own pixel tier reads too.
        let now = render_sim_time(Some(prev), curr, alpha);
        effects_layer.age(now);

        let mut dynamic = interpolated_dynamic_sprites(Some(prev), curr, alpha);
        let (effect_sprites, effect_strokes) = effects_layer.sprites(now);
        dynamic.extend(effect_sprites);

        let frame_surface = snapshot_to_surface(curr);
        let compose_start = Instant::now();
        // `false`: this preview replays a captured combat window, never a
        // build-phase tower drag, so `BuildPad`'s own glow (gated on
        // `dragging`, see `backend_pixel::build_scene`'s own doc comment)
        // never applies here.
        let canvas = compose_frame(&background, &frame_surface, &dynamic, &effect_strokes, false);
        compose_us.push(compose_start.elapsed().as_secs_f64() * 1e6);

        let encode_start = Instant::now();
        let output = encode_frame(&canvas, Rect::new(0, 0, frame_surface.width(), frame_surface.height()));
        encode_us.push(encode_start.elapsed().as_secs_f64() * 1e6);
        let encoded_len = match &output {
            PixelFrameOutput::Frame(f) => f.encoded.len(),
            PixelFrameOutput::Empty => 0,
        };

        let png_path = Path::new(OUT_DIR).join(format!("arcade-preview-pixel-frame-{i}.png"));
        raster::render_pixel_canvas_png(&canvas, &png_path).unwrap_or_else(|err| fail(format_args!("failed writing {png_path:?}: {err}")));
        println!(
            "frame {i} (t={t_secs:.3}s tick={} alpha={alpha:.2}): {} dynamic sprites, {} strokes, compose={:.0}us encode={:.0}us sixel_bytes={encoded_len} -> {png_path:?}",
            sequence.start_tick + tick_lo as u64,
            dynamic.len(),
            effect_strokes.len(),
            compose_us[i],
            encode_us[i],
        );
    }

    let avg = |values: &[f64]| values.iter().sum::<f64>() / values.len().max(1) as f64;
    let max = |values: &[f64]| values.iter().cloned().fold(0.0f64, f64::max);
    println!();
    println!(
        "pixel tier per-frame cost (background already cached -- every frame above reused the SAME build_background call): compose avg={:.0}us max={:.0}us; encode avg={:.0}us max={:.0}us \
         (bench's own measured simple-frame baseline: 2850us p50 encode, 4780us p50 terminal ingest, within a 16700us/60fps budget)",
        avg(&compose_us),
        max(&compose_us),
        avg(&encode_us),
        max(&encode_us),
    );
    println!(
        "first-frame-ever cost (background build + one compose): {:.0}us; every later frame: ~{:.0}us (compose alone, background reused)",
        background_build_us + compose_us.first().copied().unwrap_or(0.0),
        avg(&compose_us),
    );

    println!();
    println!("=== Drag-mode frame (build-zone highlight, TileId::BuildPad painted only here) ===");
    // The SAME wave-4 combat snapshot as the tier PNGs above, so the drag
    // demo shows real occupied-vs-free contrast (already-placed towers
    // next to the free build zone), not an empty board -- but the static
    // Surface itself (`snapshot_to_surface`) still carries NO `BuildPad`
    // cell at all; `paint_build_zone_highlight` is the one opt-in call
    // that ever adds it, matching the real host-side contract (call it
    // only while the owner is actively dragging a tower out of the
    // palette).
    let mut drag_surface = snapshot_to_surface(&found.snapshot);
    hatchery_arcade_pet_bastion_render::paint_build_zone_highlight(&mut drag_surface, &found.snapshot);
    let drag_canvas = compose_frame(&background, &drag_surface, &[], &[], true);
    let drag_png = Path::new(OUT_DIR).join("arcade-preview-pixel-drag.png");
    raster::render_pixel_canvas_png(&drag_canvas, &drag_png).unwrap_or_else(|err| fail(format_args!("failed writing {drag_png:?}: {err}")));
    println!("wrote {drag_png:?} (560x266 px, dragging=true, build-zone highlight visible)");
}
