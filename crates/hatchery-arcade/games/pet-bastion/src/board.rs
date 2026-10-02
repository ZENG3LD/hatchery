//! The 28x14 logical board: two spawn entrances, two routes merging into one
//! final choke, free-placement building ANYWHERE legal, four pet anchors,
//! one Heartseed.
//!
//! Routes are axis-aligned waypoint chains (each consecutive pair shares an
//! x or a y) so every segment length is an exact integer number of tiles --
//! no diagonal segments, no square root anywhere in movement rules.
//!
//! # Free placement, not fixed pads, not a route-proximity radius either
//!
//! There used to be ten fixed build pads (`PadId`, `PADS`), then a build
//! radius around each route (`BUILD_RADIUS_FP`, since removed). The owner's
//! own ask ("я хочу ставить куда хочу") dropped the radius too: the player
//! now builds on ANY board cell that is not itself a route/choke tile, a
//! pet anchor, or already occupied by another tower -- see
//! [`BuildIneligibleReason`] for the full eligibility rule and
//! [`Board::build_zone_cells`]/[`Board::static_build_reason`] for the two
//! ways to query it (bulk enumeration for a UI/policy, single-tile lookup
//! for `Simulation::place_tower`'s own validation). The Heartseed itself
//! needs no separate case: it is the last waypoint of BOTH routes (`Board::
//! new`'s own waypoint lists), so it is already a route tile by
//! construction -- see this module's own
//! `tests::the_heartseed_tile_is_never_a_build_candidate`. `Command::Place`
//! addresses a [`Tile`] directly, not a pad id.

use crate::constants::{BOARD_HEIGHT, BOARD_WIDTH, FIXED_SCALE};
use crate::geometry::{dist2_to_axis_aligned_segment, FixedPos, Tile};

/// Stable identifier for a pet anchor (0..4).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct AnchorId(pub u8);

/// Stable identifier for a route (0 or 1).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct RouteId(pub u8);

pub const ANCHOR_COUNT: usize = 4;
pub const ROUTE_COUNT: usize = 2;

pub const HEARTSEED: Tile = Tile::new(27, 6);

/// Why a board tile cannot host a new tower right now. `None` (not this
/// enum -- see [`Board::static_build_reason`]/[`Board::build_zone_cells`])
/// means the tile is buildable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BuildIneligibleReason {
    /// Outside the 28x14 board entirely. Free placement (this module's own
    /// doc) means every IN-bounds tile is at least a candidate -- there is
    /// no narrower "zone" left to fall outside of short of the board's own
    /// edge.
    OutOfBounds,
    /// The tile is itself part of a route (enemies walk it) -- this also
    /// covers the Heartseed, the last waypoint of both routes.
    Route,
    /// The tile is a pet anchor.
    Anchor,
    /// The tile already holds a placed tower. `Board` itself never reports
    /// this variant -- it does not track live tower positions; only
    /// `Simulation` (via `snapshot`'s own `build_cells`) resolves it, by
    /// layering tower occupancy on top of `Board::static_build_reason`.
    Occupied,
}

pub const ANCHORS: [Tile; ANCHOR_COUNT] = [
    Tile::new(10, 2),
    Tile::new(10, 11),
    Tile::new(21, 8),
    Tile::new(24, 4),
];

/// One axis-aligned leg of a route: `from` and `to` share exactly one axis.
#[derive(Clone, Copy, Debug)]
pub struct RouteSegment {
    pub from: Tile,
    pub to: Tile,
    /// Exact segment length in fixed-point units (Manhattan == Euclidean
    /// for an axis-aligned leg).
    pub length_fp: i64,
    /// Cumulative length of all segments strictly before this one.
    pub cumulative_before_fp: i64,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub id: RouteId,
    pub segments: Vec<RouteSegment>,
}

impl Route {
    fn build(id: u8, waypoints: &[Tile]) -> Route {
        let mut segments = Vec::with_capacity(waypoints.len().saturating_sub(1));
        let mut cumulative = 0i64;
        for pair in waypoints.windows(2) {
            let from = pair[0];
            let to = pair[1];
            debug_assert!(
                from.x == to.x || from.y == to.y,
                "route segments must be axis-aligned"
            );
            let dx = (to.x - from.x).unsigned_abs() as i64;
            let dy = (to.y - from.y).unsigned_abs() as i64;
            let length_fp = (dx + dy) * FIXED_SCALE;
            segments.push(RouteSegment {
                from,
                to,
                length_fp,
                cumulative_before_fp: cumulative,
            });
            cumulative += length_fp;
        }
        Route {
            id: RouteId(id),
            segments,
        }
    }

    pub fn total_length_fp(&self) -> i64 {
        self.segments
            .last()
            .map(|s| s.cumulative_before_fp + s.length_fp)
            .unwrap_or(0)
    }

    /// Position for a given segment index and offset (0..segment length)
    /// along that segment.
    pub fn position_at(&self, segment_index: usize, offset_fp: i64) -> FixedPos {
        let seg = match self.segments.get(segment_index) {
            Some(s) => s,
            None => {
                // Past the final segment: clamp to the route's end tile.
                let end = self
                    .segments
                    .last()
                    .map(|s| s.to)
                    .unwrap_or(Tile::new(0, 0));
                return end.to_fixed();
            }
        };
        let from = seg.from.to_fixed();
        let to = seg.to.to_fixed();
        let len = seg.length_fp.max(1);
        let t = offset_fp.clamp(0, seg.length_fp);
        FixedPos {
            x: from.x + (to.x - from.x) * t / len,
            y: from.y + (to.y - from.y) * t / len,
        }
    }

    /// Total distance travelled from the route's start to this point.
    pub fn progress_fp(&self, segment_index: usize, offset_fp: i64) -> i64 {
        match self.segments.get(segment_index) {
            Some(seg) => seg.cumulative_before_fp + offset_fp.clamp(0, seg.length_fp),
            None => self.total_length_fp(),
        }
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Converts an absolute route-progress value back into a
    /// `(segment_index, offset)` pair. Used for knockback and for placing a
    /// Night Maw split body at the mirrored progress point on the other
    /// route.
    pub fn locate(&self, progress_fp: i64) -> (usize, i64) {
        let mut idx = 0usize;
        let mut remaining = progress_fp.max(0);
        while let Some(seg) = self.segments.get(idx) {
            if remaining <= seg.length_fp || idx + 1 == self.segment_count() {
                return (idx, remaining.min(seg.length_fp));
            }
            remaining -= seg.length_fp;
            idx += 1;
        }
        (0, 0)
    }
}

/// Distance-squared from `tile` to the nearest point on any route segment
/// of `routes`, in fixed-point units. Zero exactly when `tile` is itself
/// part of a route.
fn distance2_to_route(routes: &[Route; ROUTE_COUNT], tile: Tile) -> i64 {
    let p = tile.to_fixed();
    routes
        .iter()
        .flat_map(|route| route.segments.iter())
        .map(|seg| dist2_to_axis_aligned_segment(p, seg.from.to_fixed(), seg.to.to_fixed()))
        .min()
        .unwrap_or(i64::MAX)
}

/// Classifies an in-bounds `tile` against the two static (never change
/// once a `Board` exists) build rules: route and anchor. `None` if both
/// pass -- a genuine build candidate, still subject to tower occupancy
/// (`Simulation`'s own concern, see `Board::open_build_tiles`). Bounds are
/// the caller's own responsibility (`Board::static_build_reason` checks
/// them; `compute_build_zone` only ever calls this on in-bounds tiles to
/// begin with).
fn classify_build(routes: &[Route; ROUTE_COUNT], tile: Tile) -> Option<BuildIneligibleReason> {
    if distance2_to_route(routes, tile) == 0 {
        return Some(BuildIneligibleReason::Route);
    }
    if ANCHORS.contains(&tile) {
        return Some(BuildIneligibleReason::Anchor);
    }
    None
}

/// Every board tile, paired with the static reason (route/anchor) it can
/// never be built on, or `None` if only tower occupancy is left to check.
/// Free placement (this module's own doc) means this is now the WHOLE
/// 28x14 board, not a narrower proximity zone -- computed once at
/// `Board::new()` time (routes/anchors never move once a `Board` exists),
/// since `Simulation::snapshot`'s own per-tick `build_cells` view and this
/// module's `Board::open_build_tiles` both walk this instead of
/// reclassifying every tile from scratch every tick.
fn compute_build_zone(routes: &[Route; ROUTE_COUNT]) -> Vec<(Tile, Option<BuildIneligibleReason>)> {
    let mut zone = Vec::with_capacity((BOARD_WIDTH * BOARD_HEIGHT) as usize);
    for y in 0..BOARD_HEIGHT {
        for x in 0..BOARD_WIDTH {
            let tile = Tile::new(x, y);
            zone.push((tile, classify_build(routes, tile)));
        }
    }
    zone
}

/// The full board layout. Constructed once per `Simulation`; entirely
/// immutable data (route mutation between waves, if ever added, would
/// replace which edges *future* spawns select, never move an enemy already
/// committed to a segment -- see the plan's own routing note).
pub struct Board {
    pub routes: [Route; ROUTE_COUNT],
    build_zone: Vec<(Tile, Option<BuildIneligibleReason>)>,
}

impl Board {
    pub fn new() -> Self {
        let route_a = Route::build(
            0,
            &[
                Tile::new(0, 3),
                Tile::new(20, 3),
                Tile::new(20, 6),
                HEARTSEED,
            ],
        );
        let route_b = Route::build(
            1,
            &[
                Tile::new(0, 10),
                Tile::new(20, 10),
                Tile::new(20, 6),
                HEARTSEED,
            ],
        );
        let routes = [route_a, route_b];
        let build_zone = compute_build_zone(&routes);
        Board { routes, build_zone }
    }

    pub fn route(&self, id: RouteId) -> &Route {
        &self.routes[id.0 as usize]
    }

    pub fn opposite_route(&self, id: RouteId) -> RouteId {
        RouteId(1 - id.0)
    }

    pub fn anchor_tile(anchor: AnchorId) -> Tile {
        ANCHORS[anchor.0 as usize]
    }

    pub fn bounds_contain(tile: Tile) -> bool {
        tile.x >= 0 && tile.x < BOARD_WIDTH && tile.y >= 0 && tile.y < BOARD_HEIGHT
    }

    /// Every board tile (the whole 28x14 grid -- free placement, this
    /// module's own doc), each paired with the static reason it cannot be
    /// built on (`None` for a genuine candidate, still subject to tower
    /// occupancy -- see this struct's own doc). The one, shared source
    /// both `Simulation::snapshot`'s `build_cells` view and this module's
    /// own `open_build_tiles` walk, rather than either reclassifying every
    /// tile from scratch.
    pub fn build_zone_cells(&self) -> &[(Tile, Option<BuildIneligibleReason>)] {
        &self.build_zone
    }

    /// Static (board-only) reason `tile` can never be built on, ignoring
    /// tower occupancy -- the one dynamic input only `Simulation` knows
    /// about (see its own `place_tower`, which layers a live-tower-position
    /// check on top of this). `None` means every static rule passes.
    pub fn static_build_reason(&self, tile: Tile) -> Option<BuildIneligibleReason> {
        if !Self::bounds_contain(tile) {
            return Some(BuildIneligibleReason::OutOfBounds);
        }
        classify_build(&self.routes, tile)
    }

    /// Every tile that is a genuine build candidate right now: not a
    /// route/anchor tile, and not in `occupied` (tile coordinates of every
    /// already-placed tower, e.g. `snapshot.towers.iter().map(|t| t.
    /// position)`). The enumeration `sweep`'s own build policies walk in
    /// place of the old fixed pad table.
    pub fn open_build_tiles(&self, occupied: &[(i32, i32)]) -> Vec<Tile> {
        self.build_zone
            .iter()
            .filter(|(tile, reason)| reason.is_none() && !occupied.contains(&(tile.x, tile.y)))
            .map(|(tile, _)| *tile)
            .collect()
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_anchors_within_bounds_and_distinct() {
        for anchor in ANCHORS {
            assert!(Board::bounds_contain(anchor));
        }
        for i in 0..ANCHORS.len() {
            for j in (i + 1)..ANCHORS.len() {
                assert_ne!(ANCHORS[i], ANCHORS[j]);
            }
        }
    }

    #[test]
    fn a_route_tile_is_never_a_build_candidate() {
        let board = Board::new();
        // (10, 3) sits exactly on route 0's own first (0,3)-(20,3) leg.
        assert_eq!(board.static_build_reason(Tile::new(10, 3)), Some(BuildIneligibleReason::Route));
        assert!(!board.build_zone_cells().iter().any(|(t, r)| *t == Tile::new(10, 3) && r.is_none()));
    }

    #[test]
    fn an_anchor_tile_is_never_a_build_candidate() {
        let board = Board::new();
        assert_eq!(board.static_build_reason(ANCHORS[0]), Some(BuildIneligibleReason::Anchor));
    }

    #[test]
    fn the_heartseed_tile_is_never_a_build_candidate() {
        // No separate case needed for it: it is the last waypoint of BOTH
        // routes (`Board::new`), so `classify_build`'s own route check
        // (distance-to-route == 0) already catches it -- this test is the
        // proof the module doc's own claim rests on.
        let board = Board::new();
        assert_eq!(board.static_build_reason(HEARTSEED), Some(BuildIneligibleReason::Route));
    }

    #[test]
    fn a_tile_far_from_every_route_is_now_a_valid_build_candidate() {
        // The owner's own ask ("я хочу ставить куда хочу"): dropping
        // `BUILD_RADIUS_FP` means a tile that used to be rejected purely on
        // route proximity is buildable now. The board is only 14 tiles
        // tall; (0, 13) sits far below every route waypoint on this map and
        // is not an anchor either.
        let board = Board::new();
        assert_eq!(board.static_build_reason(Tile::new(0, 13)), None);
    }

    #[test]
    fn out_of_bounds_tiles_are_never_a_build_candidate_even_when_geometrically_close() {
        let board = Board::new();
        // One tile left of the board, right next to route 0's own y=3 leg
        // -- must still be rejected on bounds, not accepted on proximity.
        assert_eq!(board.static_build_reason(Tile::new(-1, 3)), Some(BuildIneligibleReason::OutOfBounds));
    }

    #[test]
    fn open_build_tiles_excludes_occupied_tiles() {
        let board = Board::new();
        let candidates = board.open_build_tiles(&[]);
        assert!(!candidates.is_empty(), "free placement must produce at least one candidate on this map");
        let first = candidates[0];
        let occupied = [(first.x, first.y)];
        let after = board.open_build_tiles(&occupied);
        assert_eq!(after.len(), candidates.len() - 1);
        assert!(!after.contains(&first));
    }

    #[test]
    fn free_placement_makes_nearly_the_whole_board_buildable() {
        // The measurement this pass's own report cites: with the radius
        // gone, `open_build_tiles` covers almost the entire 28x14 board --
        // every tile except the route/choke tiles and the 4 anchors (no
        // towers placed yet, so occupancy excludes nothing here).
        let board = Board::new();
        let total = (BOARD_WIDTH * BOARD_HEIGHT) as usize;
        let candidates = board.open_build_tiles(&[]);
        let blocked = total - candidates.len();
        assert!(
            candidates.len() > total * 3 / 4,
            "free placement must open the large majority of the board (got {} of {} tiles, {} blocked)",
            candidates.len(),
            total,
            blocked
        );
    }

    #[test]
    fn routes_merge_at_the_same_final_choke() {
        let board = Board::new();
        let a_end = board.routes[0].segments.last().unwrap().to;
        let b_end = board.routes[1].segments.last().unwrap().to;
        assert_eq!(a_end, HEARTSEED);
        assert_eq!(b_end, HEARTSEED);
    }

    #[test]
    fn progress_is_monotonic_along_a_route() {
        let board = Board::new();
        let route = &board.routes[0];
        let mut last = -1i64;
        for seg_idx in 0..route.segment_count() {
            let len = route.segments[seg_idx].length_fp;
            for step in [0, len / 2, len] {
                let progress = route.progress_fp(seg_idx, step);
                assert!(progress >= last);
                last = progress;
            }
        }
    }
}
