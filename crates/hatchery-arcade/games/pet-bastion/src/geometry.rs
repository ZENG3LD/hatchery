//! Fixed-point geometry shared by the board, towers, enemies and pet.
//!
//! No floats anywhere: tile positions are small integers, continuous
//! positions along a route are integer fixed-point (scaled by
//! [`FIXED_SCALE`](crate::constants::FIXED_SCALE)), and "distance" for
//! range/nearest checks is always a squared value -- comparisons between
//! squared distances are monotonic with real distance, so no square root is
//! ever needed.

use crate::constants::FIXED_SCALE;

/// A logical board tile coordinate.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
}

impl Tile {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Converts to a fixed-point position at this tile's centre.
    pub fn to_fixed(self) -> FixedPos {
        FixedPos {
            x: self.x as i64 * FIXED_SCALE,
            y: self.y as i64 * FIXED_SCALE,
        }
    }
}

/// A continuous fixed-point board position, scaled by `FIXED_SCALE` units
/// per tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FixedPos {
    pub x: i64,
    pub y: i64,
}

impl FixedPos {
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }

    /// Squared distance to another position, in `FIXED_SCALE^2` units.
    pub fn dist2(self, other: FixedPos) -> i64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }
}

/// Converts a whole-tile distance (e.g. a tower's range, given in tiles in
/// the design docs) into fixed-point units for comparison against
/// [`FixedPos::dist2`].
pub const fn tiles_to_fixed(whole: i64, tenths: i64) -> i64 {
    // whole.tenths, e.g. tiles_to_fixed(3, 5) == 3.5 tiles.
    whole * FIXED_SCALE + tenths * (FIXED_SCALE / 10)
}

/// Squared distance from `p` to the nearest point on the axis-aligned
/// segment `a`-`b` (one of `a.x == b.x` or `a.y == b.y` must hold -- every
/// route leg in this crate is axis-aligned by construction, see
/// `board::Route::build`'s own `debug_assert`). No projection division is
/// needed for an axis-aligned segment: clamping each axis independently to
/// the segment's own bounding box gives the exact nearest point directly.
pub fn dist2_to_axis_aligned_segment(p: FixedPos, a: FixedPos, b: FixedPos) -> i64 {
    let (min_x, max_x) = if a.x <= b.x { (a.x, b.x) } else { (b.x, a.x) };
    let (min_y, max_y) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
    let nearest = FixedPos::new(p.x.clamp(min_x, max_x), p.y.clamp(min_y, max_y));
    p.dist2(nearest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dist2_to_axis_aligned_segment_is_zero_on_the_segment_itself() {
        let a = FixedPos::new(0, 0);
        let b = FixedPos::new(200_000, 0);
        assert_eq!(dist2_to_axis_aligned_segment(FixedPos::new(100_000, 0), a, b), 0);
        assert_eq!(dist2_to_axis_aligned_segment(a, a, b), 0);
        assert_eq!(dist2_to_axis_aligned_segment(b, a, b), 0);
    }

    #[test]
    fn dist2_to_axis_aligned_segment_clamps_perpendicular_offset() {
        // Segment y=3 tiles, x in [0,20] tiles; a point 2 tiles directly
        // above x=10 is exactly 2 tiles (perpendicular) from the segment.
        let a = Tile::new(0, 3).to_fixed();
        let b = Tile::new(20, 3).to_fixed();
        let p = Tile::new(10, 1).to_fixed();
        let expected = tiles_to_fixed(2, 0) * tiles_to_fixed(2, 0);
        assert_eq!(dist2_to_axis_aligned_segment(p, a, b), expected);
    }

    #[test]
    fn dist2_to_axis_aligned_segment_clamps_past_the_endpoint() {
        // A point past the segment's own x range clamps to the nearest
        // endpoint, not to an unbounded extension of the line.
        let a = Tile::new(0, 3).to_fixed();
        let b = Tile::new(20, 3).to_fixed();
        let p = Tile::new(25, 3).to_fixed();
        let expected = tiles_to_fixed(5, 0) * tiles_to_fixed(5, 0);
        assert_eq!(dist2_to_axis_aligned_segment(p, a, b), expected);
    }
}
