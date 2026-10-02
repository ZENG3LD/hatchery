//! Synthetic "board" RGBA rasterizer -- stands in for a real
//! `RenderBackend::project` frame so `encode`/`live` can measure
//! `icy_sixel` cost without depending on `pet-bastion`'s sim or
//! `engine`'s own `backend_sixel` (this crate never touches either, per
//! the task's own "don't touch engine/games/sweep" boundary).
//!
//! Cell size is hand-synced to `engine::render::backend_sixel`'s own
//! `PX_PER_CELL_W`/`PX_PER_CELL_H` (10x19 px) so a board sized in cells
//! maps onto the pixel sizes the owner actually asked to measure (56x14
//! cells = 560x266 px, and its 2x/0.5x variants). Each cell gets a flat
//! base colour off a wide hue wheel (not a handful of repeated colours)
//! so the `max_colors` palette sweep in `encode` has real quantization
//! work to do at all three tested palette sizes, plus an inset shape in a
//! second colour -- matching `backend_sixel`'s own "flat procedural
//! shape, coloured by kind" visual style rather than a photograph, which
//! is what real tile content looks like and is why `diffusion: 0.0`
//! (no dithering) is the right setting to measure, not a photographic
//! gradient's own default.

pub const CELL_PX_W: u32 = 10;
pub const CELL_PX_H: u32 = 19;

/// Number of distinct base hues on the wheel -- comfortably above the
/// largest palette size under test (256) is not the goal (real boards
/// never use anywhere near 256 flat colours either); comfortably above
/// the SMALLEST (16) is, so the 16/32/256 sweep is actually quantizing
/// something down, not measuring a no-op.
const HUE_STEPS: u32 = 96;

fn put_pixel(buf: &mut [u8], px_w: u32, px_h: u32, x: i64, y: i64, rgba: [u8; 4]) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x >= px_w || y >= px_h {
        return;
    }
    let idx = ((y * px_w + x) * 4) as usize;
    buf[idx..idx + 4].copy_from_slice(&rgba);
}

fn fill_rect(buf: &mut [u8], px_w: u32, px_h: u32, x0: i64, y0: i64, x1: i64, y1: i64, rgba: [u8; 4]) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            put_pixel(buf, px_w, px_h, x, y, rgba);
        }
    }
}

fn fill_circle(buf: &mut [u8], px_w: u32, px_h: u32, cx: f64, cy: f64, radius: f64, rgba: [u8; 4]) {
    if radius <= 0.0 {
        return;
    }
    let r2 = radius * radius;
    let x0 = (cx - radius).floor() as i64;
    let x1 = (cx + radius).ceil() as i64;
    let y0 = (cy - radius).floor() as i64;
    let y1 = (cy + radius).ceil() as i64;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            if dx * dx + dy * dy <= r2 {
                put_pixel(buf, px_w, px_h, x, y, rgba);
            }
        }
    }
}

/// HSV (`h` in `[0, HUE_STEPS)`, fixed S/V) -> straight RGB, sRGB-naive
/// (this is a synthetic test pattern, not colour-managed content).
fn hue_rgb(h: u32, sat_v: (f64, f64)) -> (u8, u8, u8) {
    let (s, v) = sat_v;
    let h = (h % HUE_STEPS) as f64 / HUE_STEPS as f64 * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        (((r1 + m) * 255.0).round() as i32).clamp(0, 255) as u8,
        (((g1 + m) * 255.0).round() as i32).clamp(0, 255) as u8,
        (((b1 + m) * 255.0).round() as i32).clamp(0, 255) as u8,
    )
}

fn cell_hue(cx: u32, cy: u32) -> u32 {
    (cx.wrapping_mul(31).wrapping_add(cy.wrapping_mul(17))) % HUE_STEPS
}

/// One synthetic board frame, `px_w x px_h` RGBA (row-major, straight
/// alpha -- `icy_sixel`'s own expected input layout). `tick` shifts which
/// cells render a brighter "active" tint so consecutive frames genuinely
/// differ, the way a moving game's frames would (no encoder cache exists
/// to fool either way, but honest-looking input matters for the
/// live-terminal byte-size numbers).
pub fn synth_frame(px_w: u32, px_h: u32, tick: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (px_w as usize) * (px_h as usize) * 4];
    let cells_x = px_w.div_ceil(CELL_PX_W);
    let cells_y = px_h.div_ceil(CELL_PX_H);
    for cy in 0..cells_y {
        for cx in 0..cells_x {
            let hue = cell_hue(cx, cy);
            let active = (cx.wrapping_add(cy).wrapping_add(tick)) % 23 == 0;
            let (r, g, b) = if active { hue_rgb(hue, (0.85, 1.0)) } else { hue_rgb(hue, (0.55, 0.8)) };
            let x0 = (cx * CELL_PX_W) as i64;
            let y0 = (cy * CELL_PX_H) as i64;
            let x1 = x0 + CELL_PX_W as i64 - 1;
            let y1 = y0 + CELL_PX_H as i64 - 1;
            fill_rect(&mut buf, px_w, px_h, x0, y0, x1, y1, [r, g, b, 255]);

            // Inset shape in a second colour off the wheel -- keeps the
            // per-cell content closer to a real tile's "shape + wash"
            // look than a flat swatch, and adds another distinct colour
            // per cell for the palette sweep to actually quantize.
            let shape_hue = (hue + HUE_STEPS / 3) % HUE_STEPS;
            let (sr, sg, sb) = hue_rgb(shape_hue, (0.9, 0.9));
            let cx_px = x0 as f64 + CELL_PX_W as f64 / 2.0;
            let cy_px = y0 as f64 + CELL_PX_H as f64 / 2.0;
            let radius = (CELL_PX_W.min(CELL_PX_H) as f64) * 0.32;
            fill_circle(&mut buf, px_w, px_h, cx_px, cy_px, radius, [sr, sg, sb, 255]);
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_has_expected_len() {
        let buf = synth_frame(560, 266, 0);
        assert_eq!(buf.len(), 560 * 266 * 4);
    }

    #[test]
    fn frame_uses_more_than_thirty_two_colours() {
        let buf = synth_frame(560, 266, 0);
        let mut colours = std::collections::HashSet::new();
        for px in buf.chunks_exact(4) {
            colours.insert((px[0], px[1], px[2]));
        }
        assert!(colours.len() > 32, "expected >32 distinct colours, got {}", colours.len());
    }
}
