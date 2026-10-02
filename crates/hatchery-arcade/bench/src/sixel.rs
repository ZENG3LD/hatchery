//! The one `icy_sixel` call this whole crate measures -- kept in a single
//! place so `encode` (headless) and `live` (real terminal) time the exact
//! same recipe: `EncodeOptions` matches `engine::render::backend_sixel`'s
//! own `encode_options()` (no dithering -- `diffusion: 0.0`, flat
//! procedural content has no gradients for Floyd-Steinberg to smooth),
//! and `BackgroundMode::Transparent` matches that backend's own choice
//! (undrawn/alpha<128 pixels keep whatever was already on screen, so a
//! redraw never needs a same-size opaque wash first).

use icy_sixel::{BackgroundMode, EncodeOptions, SixelImage};

pub fn encode_options(max_colors: u16) -> EncodeOptions {
    EncodeOptions { max_colors, diffusion: 0.0, ..EncodeOptions::default() }
}

/// Encodes one `px_w x px_h` RGBA frame to SIXEL bytes. Takes ownership of
/// `rgba` (never clones it) so the measured cost matches what a real
/// per-frame rasterize-then-encode call pays: one fresh buffer in, one
/// encoded buffer out, no hidden extra copy this wrapper would add and a
/// real caller wouldn't.
pub fn encode(rgba: Vec<u8>, px_w: u32, px_h: u32, max_colors: u16) -> Result<Vec<u8>, String> {
    let image = SixelImage::try_from_rgba(rgba, px_w as usize, px_h as usize).map_err(|e| e.to_string())?;
    let text = image
        .with_background_mode(BackgroundMode::Transparent)
        .encode_with(&encode_options(max_colors))
        .map_err(|e| e.to_string())?;
    Ok(text.into_bytes())
}
