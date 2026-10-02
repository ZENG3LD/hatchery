//! Shared pieces for the two `bench` binaries (`encode`, `live`): a
//! synthetic board rasterizer, the `icy_sixel` encode call both binaries
//! measure, percentile statistics, and (Windows-only, used by `live`) raw
//! console I/O for talking to the real terminal underneath. Pure
//! measurement plumbing -- draws nothing a player would ever see, answers
//! no gameplay question. See each binary's own module doc for what it
//! measures and why.

pub mod raster;
pub mod sixel;
pub mod stats;

#[cfg(windows)]
pub mod console;
