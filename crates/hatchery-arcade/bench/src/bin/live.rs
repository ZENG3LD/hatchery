//! Live-terminal SIXEL throughput bench. MUST run inside a real, on-screen
//! `wt.exe` window -- the terminal's own render/ingest pipeline IS the
//! system under measurement here, so redirecting stdout to a file or a
//! pipe (as `cargo run` alone would do) measures nothing the task cares
//! about. See `console.rs`'s own module doc for the round-trip technique
//! (`ESC[6n` Device Status Report probes) this binary uses to clock how
//! long Windows Terminal itself takes to ingest each frame, and its one
//! caveat (ingested by the VT parser is not literally "composited to the
//! screen yet" -- a separate, vsync-gated step downstream).
//!
//! Encoding is deliberately done OUTSIDE every timed section: all frames
//! a stage needs are pre-rasterized and pre-encoded with `icy_sixel`
//! BEFORE that stage's clock starts, so every number this binary reports
//! is channel/terminal cost only, never encode cost -- the task's own
//! "measure separately, the bottleneck could be either" instruction.
//! Encode cost lives in the sibling `encode` binary.
//!
//! Usage (inside `wt.exe`, release build only):
//!   live.exe bench --out <path> [--frames N]
//!   live.exe ghost --out <path>
//!
//! Never launches its own `wt.exe` window -- the caller starts
//! `wt.exe -w -1 ...` with this binary as that window's command.

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use hatchery_arcade_bench::console::{wait_for_dsr_reply, RawMode};
use hatchery_arcade_bench::{raster, sixel, stats};

const BOARD_COLS: u32 = 56;
const BOARD_ROWS: u32 = 14;
const BOARD_W: u32 = BOARD_COLS * raster::CELL_PX_W;
const BOARD_H: u32 = BOARD_ROWS * raster::CELL_PX_H;
/// Default palette size -- matches the TUI's current production
/// quantization (per the task's own framing: "TUI сейчас квантует иконки в
/// 32 цвета"). Overridable via `--colors N` (e.g. `--colors 256`) so this
/// binary can answer "does 256 colours still hold 60fps live, not just in
/// headless encode cost" without a second copy of this whole file.
const DEFAULT_MAX_COLORS: u16 = 32;
const DSR_TIMEOUT: Duration = Duration::from_secs(3);
const TARGET_FPS: f64 = 60.0;

struct FrameSample {
    write_us: f64,
    round_trip_us: f64,
    bytes: usize,
    timed_out: bool,
}

fn cup(row: u32, col: u32) -> Vec<u8> {
    format!("\x1b[{row};{col}H").into_bytes()
}

/// Writes `prefix ++ payload ++ ESC[6n` in one syscall-minimizing buffer,
/// times the write itself, then blocks on the terminal's own DSR reply.
fn draw_and_probe(rx: &Receiver<u8>, prefix: &[u8], payload: &[u8]) -> FrameSample {
    let mut buf = Vec::with_capacity(prefix.len() + payload.len() + 4);
    buf.extend_from_slice(prefix);
    buf.extend_from_slice(payload);
    buf.extend_from_slice(b"\x1b[6n");

    let mut stdout = std::io::stdout();
    let t0 = Instant::now();
    let write_ok = stdout.write_all(&buf).and_then(|_| stdout.flush()).is_ok();
    let write_us = t0.elapsed().as_secs_f64() * 1e6;
    if !write_ok {
        return FrameSample { write_us, round_trip_us: 0.0, bytes: buf.len(), timed_out: true };
    }
    match wait_for_dsr_reply(rx, DSR_TIMEOUT) {
        Some(t1) => FrameSample { write_us, round_trip_us: t1.duration_since(t0).as_secs_f64() * 1e6, bytes: buf.len(), timed_out: false },
        None => FrameSample { write_us, round_trip_us: DSR_TIMEOUT.as_secs_f64() * 1e6, bytes: buf.len(), timed_out: true },
    }
}

/// Pre-rasterizes and pre-encodes `count` frames at `(w, h)` -- the timed
/// loops this feeds never pay encode cost.
fn preencode_sized(w: u32, h: u32, count: usize, colors: u16) -> Vec<Vec<u8>> {
    (0..count).map(|i| sixel::encode(raster::synth_frame(w, h, i as u32), w, h, colors).unwrap_or_default()).collect()
}

fn preencode_full_board(count: usize, colors: u16) -> Vec<Vec<u8>> {
    preencode_sized(BOARD_W, BOARD_H, count, colors)
}

/// A quarter of the board (bottom-right quadrant: half the columns, half
/// the rows), pre-encoded the same way.
fn preencode_quarter(count: usize, colors: u16) -> Vec<Vec<u8>> {
    let qw = (BOARD_COLS / 2) * raster::CELL_PX_W;
    let qh = (BOARD_ROWS / 2) * raster::CELL_PX_H;
    (0..count).map(|i| sixel::encode(raster::synth_frame(qw, qh, i as u32), qw, qh, colors).unwrap_or_default()).collect()
}

fn quarter_origin_cup() -> Vec<u8> {
    cup(BOARD_ROWS / 2 + 1, BOARD_COLS / 2 + 1)
}

struct StageReport {
    label: String,
    write_stats: Option<stats::Percentiles>,
    round_trip_stats: Option<stats::Percentiles>,
    realized_fps: f64,
    timeouts: usize,
    bytes_p50: f64,
    drift_first_third_us: f64,
    drift_last_third_us: f64,
}

fn run_stage(rx: &Receiver<u8>, label: &str, prefix: &[u8], frames: &[Vec<u8>]) -> StageReport {
    let mut samples = Vec::with_capacity(frames.len());
    let run_start = Instant::now();
    for payload in frames {
        samples.push(draw_and_probe(rx, prefix, payload));
    }
    let total_secs = run_start.elapsed().as_secs_f64();

    let timeouts = samples.iter().filter(|s| s.timed_out).count();
    let clean: Vec<&FrameSample> = samples.iter().filter(|s| !s.timed_out).collect();
    let write_us: Vec<f64> = clean.iter().map(|s| s.write_us).collect();
    let rt_us: Vec<f64> = clean.iter().map(|s| s.round_trip_us).collect();
    let byte_sizes: Vec<f64> = clean.iter().map(|s| s.bytes as f64).collect();

    let third = (rt_us.len() / 3).max(1);
    let drift_first: f64 = if rt_us.len() >= third { rt_us[..third].iter().sum::<f64>() / third as f64 } else { 0.0 };
    let drift_last: f64 = if rt_us.len() >= third { rt_us[rt_us.len() - third..].iter().sum::<f64>() / third as f64 } else { 0.0 };

    StageReport {
        label: label.to_string(),
        write_stats: stats::percentiles(&write_us),
        round_trip_stats: stats::percentiles(&rt_us),
        realized_fps: if total_secs > 0.0 { samples.len() as f64 / total_secs } else { 0.0 },
        timeouts,
        bytes_p50: stats::percentiles(&byte_sizes).map(|s| s.p50).unwrap_or(0.0),
        drift_first_third_us: drift_first,
        drift_last_third_us: drift_last,
    }
}

fn fmt_stage(out: &mut String, r: &StageReport) {
    out.push_str(&format!("== {} ==\n", r.label));
    if let Some(w) = r.write_stats {
        out.push_str(&format!("  write_us   p50={:.1} p95={:.1} max={:.1} mean={:.1} n={}\n", w.p50, w.p95, w.max, w.mean, w.n));
    } else {
        out.push_str("  write_us   NO SAMPLES\n");
    }
    if let Some(rt) = r.round_trip_stats {
        out.push_str(&format!("  round_trip_us p50={:.1} p95={:.1} max={:.1} mean={:.1} n={}\n", rt.p50, rt.p95, rt.max, rt.mean, rt.n));
    } else {
        out.push_str("  round_trip_us NO SAMPLES\n");
    }
    out.push_str(&format!("  realized_fps={:.2} timeouts={} bytes_p50={:.0}\n", r.realized_fps, r.timeouts, r.bytes_p50));
    out.push_str(&format!(
        "  drift: first-third round_trip mean={:.1}us, last-third mean={:.1}us, delta={:+.1}us\n",
        r.drift_first_third_us,
        r.drift_last_third_us,
        r.drift_last_third_us - r.drift_first_third_us
    ));
}

fn run_paced_stage(rx: &Receiver<u8>, label: &str, prefix: &[u8], frames: &[Vec<u8>], target_fps: f64) -> (StageReport, usize, usize) {
    let budget = Duration::from_secs_f64(1.0 / target_fps);
    let anchor = Instant::now();
    let mut samples = Vec::with_capacity(frames.len());
    let mut on_time = 0usize;
    for (i, payload) in frames.iter().enumerate() {
        let scheduled = anchor + budget * i as u32;
        let now = Instant::now();
        if scheduled > now {
            std::thread::sleep(scheduled - now);
        }
        let sample = draw_and_probe(rx, prefix, payload);
        if !sample.timed_out && Duration::from_secs_f64(sample.round_trip_us / 1e6) <= budget {
            on_time += 1;
        }
        samples.push(sample);
    }
    let total_secs = anchor.elapsed().as_secs_f64();

    let timeouts = samples.iter().filter(|s| s.timed_out).count();
    let clean: Vec<&FrameSample> = samples.iter().filter(|s| !s.timed_out).collect();
    let write_us: Vec<f64> = clean.iter().map(|s| s.write_us).collect();
    let rt_us: Vec<f64> = clean.iter().map(|s| s.round_trip_us).collect();
    let byte_sizes: Vec<f64> = clean.iter().map(|s| s.bytes as f64).collect();
    let third = (rt_us.len() / 3).max(1);
    let drift_first: f64 = if rt_us.len() >= third { rt_us[..third].iter().sum::<f64>() / third as f64 } else { 0.0 };
    let drift_last: f64 = if rt_us.len() >= third { rt_us[rt_us.len() - third..].iter().sum::<f64>() / third as f64 } else { 0.0 };

    let report = StageReport {
        label: label.to_string(),
        write_stats: stats::percentiles(&write_us),
        round_trip_stats: stats::percentiles(&rt_us),
        realized_fps: if total_secs > 0.0 { samples.len() as f64 / total_secs } else { 0.0 },
        timeouts,
        bytes_p50: stats::percentiles(&byte_sizes).map(|s| s.p50).unwrap_or(0.0),
        drift_first_third_us: drift_first,
        drift_last_third_us: drift_last,
    };
    (report, on_time, frames.len())
}

fn erase_rect_payload(top_row: u32, left_col: u32, cols: u32, rows: u32) -> Vec<u8> {
    let spaces = " ".repeat(cols as usize);
    let mut out = Vec::new();
    for r in 0..rows {
        out.extend_from_slice(&cup(top_row + r, left_col));
        out.extend_from_slice(spaces.as_bytes());
    }
    out
}

struct Cli {
    mode: String,
    out: PathBuf,
    frames: usize,
    colors: u16,
}

fn parse_cli() -> Result<Cli, String> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or("expected a mode: 'bench' or 'ghost'")?;
    let mut out: Option<PathBuf> = None;
    let mut frames: usize = 240;
    let mut colors: u16 = DEFAULT_MAX_COLORS;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(args.next().ok_or("--out needs a path")?)),
            "--frames" => frames = args.next().ok_or("--frames needs a number")?.parse().map_err(|e| format!("bad --frames: {e}"))?,
            "--colors" => colors = args.next().ok_or("--colors needs a number")?.parse().map_err(|e| format!("bad --colors: {e}"))?,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(Cli { mode, out: out.ok_or("missing --out <path>")?, frames, colors })
}

fn run_bench(cli: &Cli) -> Result<String, String> {
    let raw = RawMode::enable().map_err(|e| format!("failed to enable raw console mode: {e}"))?;
    let rx = raw.spawn_reader();

    // Drain any stale bytes sitting in the input buffer from before this
    // process started (window-creation noise, a stray keypress, ...)
    // before the channel actually established by this process's own
    // queries is trusted.
    while wait_for_dsr_reply(&rx, Duration::from_millis(50)).is_some() {}

    let mut report = String::new();

    // Stage 0: baseline round-trip latency, no SIXEL payload at all --
    // the channel's own floor (write + ConPTY + WT parse + reply path),
    // to see how much of a real frame's round trip is "just IO" versus
    // "decoding this specific image".
    let empty_frames: Vec<Vec<u8>> = (0..30).map(|_| Vec::new()).collect();
    let baseline = run_stage(&rx, "baseline (no sixel, ESC[6n only)", &[], &empty_frames);
    fmt_stage(&mut report, &baseline);
    if baseline.round_trip_stats.is_none() {
        return Err("baseline DSR probe never got a reply -- is this really running inside wt.exe with a live console?".to_string());
    }
    report.push('\n');

    // Stage 1: full board, as fast as this process can push it.
    let full_frames = preencode_full_board(cli.frames, cli.colors);
    let origin = cup(1, 1);
    let full_stage = run_stage(&rx, &format!("full board {BOARD_W}x{BOARD_H} @ {} colors, sustained ({} frames)", cli.colors, cli.frames), &origin, &full_frames);
    fmt_stage(&mut report, &full_stage);
    report.push('\n');

    // Stage 2: full board, paced at the target fps -- does it hold?
    let paced_frames = preencode_full_board(cli.frames, cli.colors);
    let (paced_stage, on_time, total) = run_paced_stage(&rx, &format!("full board @ {} colors, paced at {TARGET_FPS:.0}fps ({} frames)", cli.colors, cli.frames), &origin, &paced_frames, TARGET_FPS);
    fmt_stage(&mut report, &paced_stage);
    report.push_str(&format!("  on-time-within-budget: {on_time}/{total} ({:.1}%)\n\n", 100.0 * on_time as f64 / total as f64));

    // Stage 3: quarter-subregion only, board already painted (stage 1
    // left a full board on screen) -- does redrawing less help?
    let quarter_frames = preencode_quarter(cli.frames, cli.colors);
    let quarter_prefix = quarter_origin_cup();
    let quarter_stage = run_stage(&rx, &format!("quarter subregion redraw only, sustained ({} frames)", cli.frames), &quarter_prefix, &quarter_frames);
    fmt_stage(&mut report, &quarter_stage);
    report.push_str(&format!("  fps ratio quarter/full = {:.2}x\n\n", quarter_stage.realized_fps / full_stage.realized_fps.max(0.001)));

    // Stage 4: cost of erasing the quarter subregion's cell rect (plain
    // spaces, no SIXEL) before a redraw, standalone.
    let erase_payload = erase_rect_payload(BOARD_ROWS / 2 + 1, BOARD_COLS / 2 + 1, BOARD_COLS / 2, BOARD_ROWS / 2);
    let erase_frames: Vec<Vec<u8>> = (0..30).map(|_| erase_payload.clone()).collect();
    let erase_stage = run_stage(&rx, "erase quarter cell-rect (plain spaces, no sixel)", &[], &erase_frames);
    fmt_stage(&mut report, &erase_stage);
    report.push('\n');

    // Stage 5: double-size board (1120x532), fewer frames -- headless
    // encoding already showed this size is where encode cost alone can
    // exceed a 60fps budget; check whether the terminal's own ingest cost
    // scales the same way.
    let double_frames_n = (cli.frames / 2).max(40);
    let double_frames = preencode_sized(BOARD_W * 2, BOARD_H * 2, double_frames_n, cli.colors);
    let double_stage = run_stage(&rx, &format!("double board {}x{} @ {} colors, sustained ({double_frames_n} frames)", BOARD_W * 2, BOARD_H * 2, cli.colors), &origin, &double_frames);
    fmt_stage(&mut report, &double_stage);
    report.push('\n');

    report.push_str(&format!(
        "answer inputs: baseline_p50_us={:.1} full_p50_us={:.1} full_p95_us={:.1} full_realized_fps={:.2} paced_on_time_pct={:.1} quarter_p50_us={:.1} quarter_realized_fps={:.2} erase_p50_us={:.1} double_p50_us={:.1} double_realized_fps={:.2}\n",
        baseline.round_trip_stats.map(|s| s.p50).unwrap_or(0.0),
        full_stage.round_trip_stats.map(|s| s.p50).unwrap_or(0.0),
        full_stage.round_trip_stats.map(|s| s.p95).unwrap_or(0.0),
        full_stage.realized_fps,
        100.0 * on_time as f64 / total as f64,
        quarter_stage.round_trip_stats.map(|s| s.p50).unwrap_or(0.0),
        quarter_stage.realized_fps,
        erase_stage.round_trip_stats.map(|s| s.p50).unwrap_or(0.0),
        double_stage.round_trip_stats.map(|s| s.p50).unwrap_or(0.0),
        double_stage.realized_fps,
    ));

    Ok(report)
}

fn run_ghost(cli: &Cli) -> Result<String, String> {
    let raw = RawMode::enable().map_err(|e| format!("failed to enable raw console mode: {e}"))?;
    let rx = raw.spawn_reader();
    while wait_for_dsr_reply(&rx, Duration::from_millis(50)).is_some() {}

    let mut report = String::new();
    let origin = cup(1, 1);
    // A 20x10-cell solid block (fully opaque, no transparency) -- shape A.
    let big = sixel::encode(raster::synth_frame(20 * raster::CELL_PX_W, 10 * raster::CELL_PX_H, 0), 20 * raster::CELL_PX_W, 10 * raster::CELL_PX_H, cli.colors)?;
    let big_sample = draw_and_probe(&rx, &origin, &big);
    report.push_str(&format!("step1 big shape drawn, round_trip_us={:.1}\n", big_sample.round_trip_us));
    std::thread::sleep(Duration::from_secs(9));

    // A smaller 8x4-cell shape at the SAME origin, Transparent mode, NO
    // erase first -- if BackgroundMode::Transparent leaves stale pixels
    // outside its own footprint untouched, the rest of shape A's
    // now-uncovered area should still show on screen.
    let small = sixel::encode(raster::synth_frame(8 * raster::CELL_PX_W, 4 * raster::CELL_PX_H, 1), 8 * raster::CELL_PX_W, 4 * raster::CELL_PX_H, cli.colors)?;
    let small_sample = draw_and_probe(&rx, &origin, &small);
    report.push_str(&format!("step2 small shape drawn WITHOUT erase, round_trip_us={:.1}\n", small_sample.round_trip_us));
    std::thread::sleep(Duration::from_secs(9));

    // Erase shape A's full footprint, then redraw the small shape.
    let erase = erase_rect_payload(1, 1, 20, 10);
    let erase_sample = draw_and_probe(&rx, &[], &erase);
    let redraw_sample = draw_and_probe(&rx, &origin, &small);
    report.push_str(&format!(
        "step3 erased (round_trip_us={:.1}) then redrew small shape (round_trip_us={:.1})\n",
        erase_sample.round_trip_us, redraw_sample.round_trip_us
    ));
    std::thread::sleep(Duration::from_secs(9));

    report.push_str(&format!("mode: bg=Transparent, board origin=row1 col1, out={}\n", cli.out.display()));
    Ok(report)
}

fn main() {
    let cli = match parse_cli() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("live: {e}");
            std::process::exit(2);
        }
    };
    let result = match cli.mode.as_str() {
        "bench" => run_bench(&cli),
        "ghost" => run_ghost(&cli),
        other => Err(format!("unknown mode '{other}' (expected 'bench' or 'ghost')")),
    };
    let text = match result {
        Ok(text) => text,
        Err(e) => format!("ERROR: {e}\n"),
    };
    if let Err(e) = std::fs::write(&cli.out, &text) {
        eprintln!("live: failed to write report to {}: {e}", cli.out.display());
    }
    println!("\n{text}");
}
