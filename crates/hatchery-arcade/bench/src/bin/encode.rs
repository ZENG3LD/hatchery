//! Headless `icy_sixel` encode-cost bench: no terminal involved, answers
//! "how many microseconds does one board frame cost to encode" across the
//! three board sizes and three palette sizes the task asked for. Safe to
//! run in any shell (redirected stdout is fine here -- unlike `live`,
//! nothing about encoding depends on a real terminal being on the other
//! end).
//!
//! Run: `cargo run --release -p gate4agent-arcade-bench --bin encode`

use gate4agent_arcade_bench::{raster, sixel, stats};

const SIZES: [(u32, u32, &str); 3] = [(560, 266, "560x266 (1x, = 56x14 cells)"), (1120, 532, "1120x532 (2x)"), (280, 133, "280x133 (0.5x, = 1/4 area of 1x)")];
const PALETTES: [u16; 3] = [16, 32, 256];
const WARMUP: usize = 8;
const SAMPLES: usize = 60;

fn main() {
    println!("board_size,max_colors,p50_us,p95_us,max_us,mean_us,bytes_p50,n,errors");
    for &(w, h, label) in &SIZES {
        for &colors in &PALETTES {
            let mut times_us = Vec::with_capacity(SAMPLES);
            let mut byte_sizes = Vec::with_capacity(SAMPLES);
            let mut errors = 0usize;
            for i in 0..(WARMUP + SAMPLES) {
                let frame = raster::synth_frame(w, h, i as u32);
                let t0 = std::time::Instant::now();
                let encoded = sixel::encode(frame, w, h, colors);
                let elapsed_us = t0.elapsed().as_secs_f64() * 1e6;
                if i < WARMUP {
                    continue;
                }
                match encoded {
                    Ok(bytes) => {
                        times_us.push(elapsed_us);
                        byte_sizes.push(bytes.len() as f64);
                    }
                    Err(e) => {
                        errors += 1;
                        eprintln!("encode error at {label} colors={colors}: {e}");
                    }
                }
            }
            let Some(time_stats) = stats::percentiles(&times_us) else {
                println!("{label},{colors},NO_SAMPLES,,,,,,{errors}");
                continue;
            };
            let byte_p50 = stats::percentiles(&byte_sizes).map(|s| s.p50).unwrap_or(0.0);
            println!(
                "{label},{colors},{:.1},{:.1},{:.1},{:.1},{:.0},{},{}",
                time_stats.p50, time_stats.p95, time_stats.max, time_stats.mean, byte_p50, time_stats.n, errors
            );
        }
    }
}
