//! PNG rasterization for the three implemented render tiers -- dev-tool
//! code only (see `main.rs`'s own doc comment for why this lives in its
//! own workspace member rather than growing `engine`'s normal dependency
//! graph). Draws a monospace terminal-cell grid over a dark terminal
//! background with real per-cell colours -- as close to "what a real
//! terminal would show" as a still PNG can get for each tier.

use std::path::Path;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use icy_sixel::SixelImage;
use image::{Rgb, RgbImage};
use uzor_tui::{buffer::TerminalBuffer, style::Color};

use hatchery_arcade_engine::{PixelCanvas, SixelPlacement};

/// A plausible real dark terminal background -- hand-synced to
/// `gate4agent-tui`'s own `icons.rs::OVERRIDE_ACTIVE_BG`, so this preview
/// sits on the SAME "at rest" background colour the real TUI's pet modal
/// already uses, rather than an arbitrary placeholder grey.
pub const TERMINAL_BG: (u8, u8, u8) = (30, 30, 46);

fn color_or(color: Color, fallback: (u8, u8, u8)) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => fallback,
    }
}

fn fill_rect(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, color: (u8, u8, u8)) {
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            img.put_pixel(x, y, Rgb([color.0, color.1, color.2]));
        }
    }
}

fn blend_pixel(img: &mut RgbImage, x: u32, y: u32, color: (u8, u8, u8), coverage: f32) {
    if x >= img.width() || y >= img.height() || coverage <= 0.0 {
        return;
    }
    let coverage = coverage.clamp(0.0, 1.0);
    let existing = *img.get_pixel(x, y);
    let blend = |base: u8, top: u8| (base as f32 * (1.0 - coverage) + top as f32 * coverage).round() as u8;
    img.put_pixel(x, y, Rgb([blend(existing[0], color.0), blend(existing[1], color.1), blend(existing[2], color.2)]));
}

/// Renders the glyph tier's own `buf` as a monospace character grid: a
/// flat colour fill per cell (its own `bg`, or [`TERMINAL_BG`] for
/// `Color::Reset`), then the cell's own glyph drawn with `font` in its
/// own `fg`, horizontally centred on its own advance width and baselined
/// from the font's own scaled ascent. `cell_px_w`/`cell_px_h` are this
/// tool's own chosen preview cell size -- an arbitrary, documented choice
/// (there is no live terminal font-metrics query to read one from), NOT
/// the sixel tier's own separate `SIXEL_PX_PER_CELL_W`/`_H` assumption.
pub fn render_glyph_png(buf: &TerminalBuffer, font: &FontRef, cell_px_w: u32, cell_px_h: u32, out_path: &Path) -> image::ImageResult<()> {
    let cols = buf.width() as u32;
    let rows = buf.height() as u32;
    let mut img = RgbImage::new(cols * cell_px_w, rows * cell_px_h);

    for y in 0..rows {
        for x in 0..cols {
            let cell = buf.get(x as u16, y as u16);
            let bg = color_or(cell.style.bg, TERMINAL_BG);
            fill_rect(&mut img, x * cell_px_w, y * cell_px_h, (x + 1) * cell_px_w, (y + 1) * cell_px_h, bg);
        }
    }

    let scale = PxScale::from(cell_px_h as f32 * 0.82);
    let scaled = font.as_scaled(scale);
    let ascent = scaled.ascent();

    for y in 0..rows {
        for x in 0..cols {
            let cell = buf.get(x as u16, y as u16);
            let Some(ch) = cell.symbol.chars().next() else { continue };
            if ch == ' ' {
                continue;
            }
            let fg = color_or(cell.style.fg, (230, 230, 230));
            let glyph_id = font.glyph_id(ch);
            let advance = scaled.h_advance(glyph_id);
            let cell_left = (x * cell_px_w) as f32;
            let cell_top = (y * cell_px_h) as f32;
            let origin_x = cell_left + ((cell_px_w as f32 - advance) / 2.0).max(0.0);
            let origin_y = cell_top + ascent;
            let glyph = glyph_id.with_scale_and_position(scale, point(origin_x, origin_y));
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    let px = bounds.min.x + gx as f32;
                    let py = bounds.min.y + gy as f32;
                    if px < 0.0 || py < 0.0 {
                        return;
                    }
                    blend_pixel(&mut img, px as u32, py as u32, fg, coverage);
                });
            }
        }
    }

    img.save(out_path)
}

/// Renders the half-block tier's own `buf` by interpreting each cell's
/// symbol DIRECTLY as the two-tone pixel block it represents (`█`/`▀`/
/// `▄`/` `) -- flat rectangle fills, not a font-rasterized glyph. This is
/// deliberately MORE faithful to a real terminal than routing these four
/// characters through [`render_glyph_png`]'s own outline rasterizer: a
/// real terminal special-cases block-drawing characters to fill the
/// exact cell box with no anti-aliased edge, which a flat rectangle fill
/// reproduces exactly and a generic glyph outline (built for
/// proportional letterforms, with its own internal padding) would not.
pub fn render_halfblock_png(buf: &TerminalBuffer, cell_px_w: u32, cell_px_h: u32, out_path: &Path) -> image::ImageResult<()> {
    let cols = buf.width() as u32;
    let rows = buf.height() as u32;
    let mut img = RgbImage::new(cols * cell_px_w, rows * cell_px_h);
    let half_h = cell_px_h / 2;

    for y in 0..rows {
        for x in 0..cols {
            let cell = buf.get(x as u16, y as u16);
            let fg = color_or(cell.style.fg, TERMINAL_BG);
            let bg = color_or(cell.style.bg, TERMINAL_BG);
            let (x0, y0, x1, y1) = (x * cell_px_w, y * cell_px_h, (x + 1) * cell_px_w, (y + 1) * cell_px_h);
            match cell.symbol.as_str() {
                "█" => fill_rect(&mut img, x0, y0, x1, y1, fg),
                "▀" => {
                    fill_rect(&mut img, x0, y0, x1, y0 + half_h, fg);
                    fill_rect(&mut img, x0, y0 + half_h, x1, y1, bg);
                }
                "▄" => {
                    fill_rect(&mut img, x0, y0, x1, y0 + half_h, bg);
                    fill_rect(&mut img, x0, y0 + half_h, x1, y1, fg);
                }
                _ => fill_rect(&mut img, x0, y0, x1, y1, TERMINAL_BG),
            }
        }
    }

    img.save(out_path)
}

/// Must match `hatchery-arcade-engine`'s own `render::backend_sixel`
/// `PX_PER_CELL_W`/`_H` exactly -- those are private to that module (only
/// [`SixelPlacement::rect`], in TERMINAL-CELL space, is public), so this
/// preview tool keeps its own documented copy rather than growing that
/// module a `pub` constant whose only consumer would ever be this
/// dev-only tool.
pub const SIXEL_PX_PER_CELL_W: u32 = 10;
pub const SIXEL_PX_PER_CELL_H: u32 = 19;

/// Decodes every real, wire-format SIXEL placement `SixelBackend`
/// produced and composites them onto one flat [`TERMINAL_BG`] canvas --
/// this shows what a real terminal would display after actually parsing
/// those exact DCS byte sequences (post colour-quantization, since sixel
/// is a genuinely paletted wire format), not a re-render of the
/// pre-encode RGBA buffer this tool never had access to in the first
/// place (`rasterize_tile` is private to `backend_sixel`).
pub fn render_sixel_png(cols: u32, rows: u32, placements: &[SixelPlacement], out_path: &Path) -> Result<(), String> {
    let mut img = RgbImage::from_pixel(cols * SIXEL_PX_PER_CELL_W, rows * SIXEL_PX_PER_CELL_H, Rgb([TERMINAL_BG.0, TERMINAL_BG.1, TERMINAL_BG.2]));

    for placement in placements {
        let decoded = SixelImage::decode(&placement.encoded).map_err(|err| format!("sixel decode failed for tile at {:?}: {err}", placement.rect))?;
        let ox = placement.rect.x as u32 * SIXEL_PX_PER_CELL_W;
        let oy = placement.rect.y as u32 * SIXEL_PX_PER_CELL_H;
        for py in 0..decoded.height as u32 {
            for px in 0..decoded.width as u32 {
                let idx = ((py * decoded.width as u32 + px) * 4) as usize;
                let Some(chunk) = decoded.pixels.get(idx..idx + 4) else { continue };
                let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
                if a < 128 {
                    continue;
                }
                let (dx, dy) = (ox + px, oy + py);
                if dx < img.width() && dy < img.height() {
                    img.put_pixel(dx, dy, Rgb([r, g, b]));
                }
            }
        }
    }

    img.save(out_path).map_err(|err| err.to_string())
}

/// Renders the NEW pixel tier's own already-composed [`PixelCanvas`]
/// straight to PNG, composited over [`TERMINAL_BG`] -- unlike
/// [`render_sixel_png`], this reads the RGBA canvas
/// `hatchery_arcade_engine::compose_frame` actually produced directly,
/// with no sixel encode/decode round trip in between, so what this PNG
/// shows is exactly what `encode_frame` was handed (the round trip is
/// exercised and timed separately, see `main.rs`'s own cost-measurement
/// section, but this function's own job is showing genuine motion, not
/// re-proving the encoder).
pub fn render_pixel_canvas_png(canvas: &PixelCanvas, out_path: &Path) -> image::ImageResult<()> {
    let mut img = RgbImage::from_pixel(canvas.width, canvas.height, Rgb([TERMINAL_BG.0, TERMINAL_BG.1, TERMINAL_BG.2]));
    for y in 0..canvas.height {
        for x in 0..canvas.width {
            let px = canvas.get(x, y);
            if px[3] == 0 {
                continue;
            }
            blend_pixel(&mut img, x, y, (px[0], px[1], px[2]), px[3] as f32 / 255.0);
        }
    }
    img.save(out_path)
}
