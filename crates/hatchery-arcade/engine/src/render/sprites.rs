//! The pixel-tier sprite catalog: one genuinely distinct shape per
//! [`TileId`] (not merely per broad category the way `backend_sixel.rs`'s
//! own `rasterize_tile` groups several tower/enemy kinds under one shared
//! shape) -- built from real URX draw commands (bezier paths, linear/
//! radial/sweep gradients, real rounded rects, a real `LineBatch`), not
//! the old hand-rolled analytic-AA circles/rings/diamonds this catalog
//! used to bake into a small standalone canvas.
//!
//! # Painting straight into the shared scene, no baking step
//!
//! The old `bake_sprite` returned a small standalone [`super::PixelCanvas`]
//! that `backend_pixel::compose_frame` then bilinearly blitted onto the
//! board canvas at a continuous pixel position -- a "bake, then blit"
//! two-step forced by the old rasteriser's own primitives only knowing how
//! to paint into a `PixelSink`. URX draw commands carry their own absolute
//! geometry (every path/rect/gradient center below is built in the SAME
//! coordinate space as the destination `Pixmap`), so [`paint_tile`] pushes
//! commands straight into the caller's shared [`Scene`] at the entity's own
//! `(cx, cy)` pixel-space centre -- one step, and genuinely sub-pixel (no
//! integer-size rounding a baked sprite's own width/height used to force
//! on a growing/shrinking effect's `scale`).

use uzor_urx_core::math::{Affine, BezPath, Brush, Color, ColorStop, ColorStops, Extend, Gradient, LinearGradientPosition, Point, RadialGradientPosition, Rect, Vec2};
use uzor_urx_core::scene::{Dash, DrawCommand, FillRule, LineBatchSegment, LineCap, LineJoin, Scene, Stroke};

use super::{Rgb, TileId};

// ---------------------------------------------------------------------------
// Colour/brush helpers
// ---------------------------------------------------------------------------

fn color(c: Rgb, alpha: f32) -> Color {
    Color::from_rgba8(c.0, c.1, c.2, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub(super) fn solid(c: Rgb, alpha: f32) -> Brush {
    Brush::Solid(color(c, alpha))
}

pub(super) fn stop(offset: f32, c: Rgb, alpha: f32) -> ColorStop {
    ColorStop { offset, color: color(c, alpha).into() }
}

pub(super) fn linear(stops: Vec<ColorStop>, x1: f64, y1: f64, x2: f64, y2: f64) -> Brush {
    Brush::Gradient(Gradient {
        kind: LinearGradientPosition { start: Point::new(x1, y1), end: Point::new(x2, y2) }.into(),
        stops: ColorStops::from(stops.as_slice()),
        extend: Extend::Pad,
        ..Gradient::default()
    })
}

pub(super) fn radial(stops: Vec<ColorStop>, cx: f64, cy: f64, r: f64) -> Brush {
    Brush::Gradient(Gradient {
        kind: RadialGradientPosition { start_center: Point::new(cx, cy), start_radius: 0.0, end_center: Point::new(cx, cy), end_radius: (r.max(0.01)) as f32 }.into(),
        stops: ColorStops::from(stops.as_slice()),
        extend: Extend::Pad,
        ..Gradient::default()
    })
}

fn lighten(c: Rgb) -> Rgb {
    let mix = |v: u8| (((v as u16) + 255) / 2) as u8;
    Rgb(mix(c.0), mix(c.1), mix(c.2))
}

pub(super) fn darken(c: Rgb, permille: u32) -> Rgb {
    let scale = |v: u8| (((v as u32) * permille) / 1000).min(255) as u8;
    Rgb(scale(c.0), scale(c.1), scale(c.2))
}

/// Mixes `c` toward white by `permille` (0..=1000) parts per thousand --
/// a gentler, tunable cousin of [`lighten`] (which is hardcoded to a 50%
/// mix), used where a colour needs only a subtle lift (ground-tile tonal
/// variety, a water-pool rim highlight) rather than `lighten`'s own much
/// brighter "sheen" effect.
pub(super) fn tint(c: Rgb, permille: u32) -> Rgb {
    let permille = permille.min(1000);
    let mix = |v: u8| ((((v as u32) * (1000 - permille)) + 255 * permille) / 1000).min(255) as u8;
    Rgb(mix(c.0), mix(c.1), mix(c.2))
}

/// A boss body's own brightness, as a [`darken`] permille, from the
/// remaining HP in tenths every boss `TileArt::variant` already carries
/// (`gate4agent-arcade-pet-bastion-render`'s own `boss_hp_tenths`, whose
/// doc comment states outright that this variant exists so a procedural
/// boss shape can "react to how close the fight is").
///
/// Nothing in this module used to read that variant at all, so a boss at
/// five percent health was drawn pixel-identical to one at full and the
/// board carried no tell for the single most important fact of a boss
/// wave -- the HUD's own `HP n/m` line sits behind a click on one body,
/// not on the board. Draining the body's own light, rather than stacking
/// a gauge on top of it, is what "the shape reacts" means here, and it is
/// the only form that stays coherent for a boss made of SEVERAL bodies
/// (`interpolated_dynamic_sprites` emits one tile-sized sprite per body,
/// all carrying this same variant): every body dims together, instead of
/// one health gauge being repeated across the boss.
///
/// `1000` (i.e. [`darken`] leaves the colour untouched) at full health,
/// floored at [`BOSS_DIM_FLOOR_PERMILLE`] at death.
///
/// The floor is not a taste call. Night Maw's own body colour is
/// `Rgb(140, 40, 180)` (`gate4agent-arcade-pet-bastion-render`'s own
/// `boss_color`), a purple whose luminance is already low, and the night
/// garden's own ground is `Rgb(20, 40, 30)` (`background.rs`'s own
/// `env_cell` base). Dimming that purple much past this floor drops the
/// boss BELOW the luminance of the ground it is standing on -- it stops
/// being a silhouette and becomes a hole. The tell may cost brightness;
/// it may never cost visibility, and
/// [`boss_stays_brighter_than_the_ground_it_dies_on`] pins that against
/// both bosses' own real colours rather than against a placeholder.
/// The rest of the tell is carried by each boss's own inner light --
/// Bellkeeper's toll-glow and clapper, Night Maw's core ember -- which
/// shrink and fade across the FULL range, so what a dying boss loses is
/// mostly its light, not its outline.
fn boss_vitality_permille(variant: u8) -> u32 {
    BOSS_DIM_FLOOR_PERMILLE + (1000 - BOSS_DIM_FLOOR_PERMILLE) / 10 * variant.min(10) as u32
}

/// A boss's own remaining inner light, `0.0` at death to `1.0` at full
/// health, from the permille [`boss_vitality_permille`] produced. Kept
/// separate from that permille because the two do different jobs: the
/// permille dims a BODY and is floored so the silhouette survives (see
/// its own doc comment), while this ramp drives light that may fade all
/// the way out -- a glow and a core ember that reach zero cost the boss
/// nothing it needs to stay readable.
fn boss_light(vitality_permille: u32) -> f64 {
    (vitality_permille.saturating_sub(BOSS_DIM_FLOOR_PERMILLE) as f64) / ((1000 - BOSS_DIM_FLOOR_PERMILLE) as f64)
}

/// How much of its own full size a boss body is drawn at, `1.0` at full
/// health down to [`BOSS_MIN_BODY_SCALE`] at death.
///
/// This, not brightness, is what actually carries the health tell at the
/// size these sprites live at. One board cell is 20x19 device pixels
/// (`backend_pixel`'s own `PX_PER_CELL_W`/`PX_PER_CELL_H`), and a
/// brightness ramp floored for legibility (see
/// [`boss_vitality_permille`]) spans too little of that to be seen at a
/// glance -- measured by rendering both bosses across the full variant
/// range and looking at the result at real size, where `10` and `1` were
/// nearly indistinguishable on brightness alone. Coverage reads
/// immediately where a few percent of luminance does not, and it costs
/// no contrast at all: a shrinking body keeps every one of its own
/// colours.
///
/// The silhouette itself is never traded away -- every shape scales
/// whole, so a dying boss is a smaller Bellkeeper or a smaller Night
/// Maw, never a different or an ambiguous shape. This is presentation
/// only: the sim's own body positions and the board tiles they occupy do
/// not move (`interpolated_dynamic_sprites` places one tile-sized sprite
/// per boss body regardless).
fn boss_body_scale(variant: u8) -> f64 {
    BOSS_MIN_BODY_SCALE + (1.0 - BOSS_MIN_BODY_SCALE) * (variant.min(10) as f64) / 10.0
}

/// See [`boss_body_scale`] -- small enough that the shrink is obvious
/// across a fight, large enough that a nearly-dead boss still reads as
/// the biggest thing on the board, which is what it still is.
const BOSS_MIN_BODY_SCALE: f64 = 0.68;

/// See [`boss_vitality_permille`] -- the darkest a boss body may be drawn.
const BOSS_DIM_FLOOR_PERMILLE: u32 = 700;

// ---------------------------------------------------------------------------
// Path builders -- real cubic/quadratic Bezier curves, not polygon
// approximations plotted pixel by pixel.
// ---------------------------------------------------------------------------

/// The standard cubic-Bezier circle-approximation constant (four cubic
/// segments, tangent-matched at each quadrant) -- the same decomposition
/// every vector 2D toolkit uses for a circle/ellipse.
const CIRCLE_K: f64 = 0.5522847498307936;

pub(super) fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> BezPath {
    let kx = rx * CIRCLE_K;
    let ky = ry * CIRCLE_K;
    let mut path = BezPath::new();
    path.move_to((cx + rx, cy));
    path.curve_to((cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry));
    path.curve_to((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy));
    path.curve_to((cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry));
    path.curve_to((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy));
    path.close_path();
    path
}

pub(super) fn circle_path(cx: f64, cy: f64, r: f64) -> BezPath {
    ellipse_path(cx, cy, r, r)
}

fn diamond_path(cx: f64, cy: f64, rx: f64, ry: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((cx, cy - ry));
    path.line_to((cx + rx, cy));
    path.line_to((cx, cy + ry));
    path.line_to((cx - rx, cy));
    path.close_path();
    path
}

/// A tall, pointed spindle -- two cubic curves bulging out to `half_w` at
/// the vertical centre, tapering to a point at top and bottom. Reads as a
/// "needle/spike" silhouette, not the old flat diamond every other pointed
/// tile also used.
fn spindle_path(cx: f64, cy: f64, half_w: f64, half_h: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((cx, cy - half_h));
    path.curve_to((cx + half_w, cy - half_h * 0.35), (cx + half_w, cy + half_h * 0.35), (cx, cy + half_h));
    path.curve_to((cx - half_w, cy + half_h * 0.35), (cx - half_w, cy - half_h * 0.35), (cx, cy - half_h));
    path.close_path();
    path
}

/// A bell silhouette: a domed top curving out into a flared skirt -- built
/// from four cubic curves plus the flat bottom edge, the genuine curved
/// shape a "Bell" tower's own name promises instead of the old two
/// unrelated circles.
fn bell_path(cx: f64, cy: f64, half: f64) -> BezPath {
    let top = cy - half * 0.85;
    let dome_r = half * 0.4;
    let skirt_y = cy + half * 0.75;
    let skirt_w = half * 0.62;
    let mut path = BezPath::new();
    path.move_to((cx, top));
    path.curve_to((cx + dome_r, top), (cx + dome_r, cy - half * 0.05), (cx + dome_r * 0.85, cy));
    path.curve_to((cx + skirt_w * 0.75, cy + half * 0.3), (cx + skirt_w, skirt_y - half * 0.15), (cx + skirt_w, skirt_y));
    path.line_to((cx - skirt_w, skirt_y));
    path.curve_to((cx - skirt_w, skirt_y - half * 0.15), (cx - skirt_w * 0.75, cy + half * 0.3), (cx - dome_r * 0.85, cy));
    path.curve_to((cx - dome_r, cy - half * 0.05), (cx - dome_r, top), (cx, top));
    path.close_path();
    path
}

/// A turtle-shell dome: a half-ellipse arch over a flat baseline -- an
/// Shellback's own armoured-hump silhouette, distinct in proportion and
/// curvature from [`bell_path`]'s own taller dome-plus-skirt.
fn shell_path(cx: f64, cy: f64, half: f64) -> BezPath {
    let rx = half * 0.42;
    let ry = half * 0.34;
    let base_y = cy + ry * 0.5;
    let mut path = BezPath::new();
    path.move_to((cx - rx, base_y));
    path.curve_to((cx - rx, cy - ry), (cx + rx, cy - ry), (cx + rx, base_y));
    path.close_path();
    path
}

/// A jagged void silhouette: alternating spike tips at `outer` radius,
/// each pair joined by a real quadratic-Bezier scallop bowed inward
/// through an `inner`-radius control point -- an organic set of teeth, not
/// a plain straight-edged star polygon.
fn maw_path(cx: f64, cy: f64, half: f64) -> BezPath {
    // Fewer, deeper teeth than an early tuning of this shape carried
    // (`5` spikes at a `0.62`/`0.16` outer/inner split, not `7` at
    // `0.56`/`0.24`): at this tile's real ~9.5px `half`, seven shallow
    // teeth (amplitude under 3px) anti-aliased away into a plain blurred
    // ring -- the whole "jagged void" this path's own name promises
    // needs teeth big enough to survive that blur, which means fewer and
    // deeper, not more and finer (the same "design for the size you
    // actually have" reasoning this pass's own brief states directly).
    let spikes = 5usize;
    let outer = half * 0.62;
    let inner = half * 0.16;
    let mut path = BezPath::new();
    for i in 0..spikes {
        let a_out = i as f64 / spikes as f64 * std::f64::consts::TAU;
        let a_in = (i as f64 + 0.5) / spikes as f64 * std::f64::consts::TAU;
        let a_next = (i as f64 + 1.0) / spikes as f64 * std::f64::consts::TAU;
        let (ox, oy) = (cx + a_out.cos() * outer, cy + a_out.sin() * outer);
        if i == 0 {
            path.move_to((ox, oy));
        } else {
            path.line_to((ox, oy));
        }
        let (ix, iy) = (cx + a_in.cos() * inner, cy + a_in.sin() * inner);
        let (nx, ny) = (cx + a_next.cos() * outer, cy + a_next.sin() * outer);
        path.quad_to((ix, iy), (nx, ny));
    }
    path.close_path();
    path
}

/// Three overlapping lobes arranged 120 degrees apart around a shared
/// centre, combined into one [`BezPath`] under [`FillRule::NonZero`] (all
/// three [`ellipse_path`] contours wind the same direction, so the
/// overlap reads as one fused blob, not three separately-outlined
/// circles) -- a Splitter's own "about to divide into three" tell (the
/// night-garden plan's own "Spawns three Mites on death" role for this
/// enemy), replacing the old plain crossed-lines "X" that carried no hint
/// of what this enemy actually does on death. See the `EnemySplitter` arm
/// of [`paint_tile`] for the one caller.
fn trefoil_path(cx: f64, cy: f64, orbit: f64, lobe_r: f64) -> BezPath {
    let mut path = BezPath::new();
    for i in 0..3 {
        let a = std::f64::consts::FRAC_PI_2 + i as f64 / 3.0 * std::f64::consts::TAU;
        path.extend(ellipse_path(cx + a.cos() * orbit, cy + a.sin() * orbit, lobe_r, lobe_r));
    }
    path
}

/// A hooded, cloak-tapered silhouette -- a narrow rounded hood over a
/// wide, flared robe, built with the same dome-plus-skirt cubic-curve
/// recipe [`bell_path`] uses for the Bell tower, but narrower at the
/// crown and longer/softer through the flare so the two families read as
/// cousins (fitting: a Husher is this board's own "hush spirit", and its
/// "Suppresses nearby tower fire rate" role is deliberately bell-adjacent
/// fiction) without ever being mistaken for the actual Bell tower body.
/// See the `EnemyHusher` arm of [`paint_tile`] for the suppression-aura
/// rings painted around this body.
fn hood_path(cx: f64, cy: f64, half: f64) -> BezPath {
    let top = cy - half * 0.5;
    let hood_r = half * 0.22;
    let hem_y = cy + half * 0.46;
    let hem_w = half * 0.4;
    let mut path = BezPath::new();
    path.move_to((cx, top));
    path.curve_to((cx + hood_r, top), (cx + hood_r, cy - half * 0.1), (cx + hood_r * 0.7, cy + half * 0.05));
    path.curve_to((cx + hem_w * 0.8, cy + half * 0.22), (cx + hem_w, hem_y - half * 0.12), (cx + hem_w, hem_y));
    path.line_to((cx - hem_w, hem_y));
    path.curve_to((cx - hem_w, hem_y - half * 0.12), (cx - hem_w * 0.8, cy + half * 0.22), (cx - hood_r * 0.7, cy + half * 0.05));
    path.curve_to((cx - hood_r, cy - half * 0.1), (cx - hood_r, top), (cx, top));
    path.close_path();
    path
}

/// A shallow, twig-woven basin -- the mirror image of [`shell_path`]'s own
/// dome (concave up instead of convex up): one cubic curve dipping from a
/// flat rim down to a rounded floor and back, closed straight across the
/// rim -- the actual nest [`TileId::TowerEmberNest`]'s own name promises
/// under its embers, replacing the old three glow circles that floated
/// with nothing built underneath them. `rx`/`ry` (not a single `half`
/// scale, unlike this catalog's other single-scale path builders): the
/// `TowerEmberNest` arm of [`paint_tile`] needs this basin sized to
/// visibly outgrow its own three ember glow circles on every side (a
/// basin narrower than the embers it holds reads as embers floating past
/// their own container's edge, the opposite of "sitting inside it"), and
/// that footprint's own width and depth do not grow at the same rate.
fn nest_path(cx: f64, cy: f64, rx: f64, ry: f64) -> BezPath {
    let rim_y = cy - ry * 0.4;
    let mut path = BezPath::new();
    path.move_to((cx - rx, rim_y));
    path.curve_to((cx - rx, cy + ry), (cx + rx, cy + ry), (cx + rx, rim_y));
    path.close_path();
    path
}

// ---------------------------------------------------------------------------
// Push helpers -- append one draw command (or a handful) to `scene`.
// ---------------------------------------------------------------------------

pub(super) fn push_fill(scene: &mut Scene, path: BezPath, rule: FillRule, brush: Brush) {
    if path.elements().is_empty() {
        return;
    }
    scene.push(DrawCommand::FillPath { path, rule, brush, transform: Affine::IDENTITY });
}

pub(super) fn push_circle(scene: &mut Scene, cx: f64, cy: f64, r: f64, c: Rgb, alpha: f32) {
    if r <= 0.0 || alpha <= 0.0 {
        return;
    }
    push_fill(scene, circle_path(cx, cy, r), FillRule::NonZero, solid(c, alpha));
}

fn push_ring(scene: &mut Scene, cx: f64, cy: f64, outer: f64, inner: f64, c: Rgb, alpha: f32) {
    push_ring_brush(scene, cx, cy, outer, inner, solid(c, alpha));
}

fn push_ring_brush(scene: &mut Scene, cx: f64, cy: f64, outer: f64, inner: f64, brush: Brush) {
    if outer <= 0.0 || inner >= outer {
        return;
    }
    let mut path = circle_path(cx, cy, outer);
    path.extend(circle_path(cx, cy, inner));
    push_fill(scene, path, FillRule::EvenOdd, brush);
}

fn push_diamond(scene: &mut Scene, cx: f64, cy: f64, rx: f64, ry: f64, c: Rgb, alpha: f32) {
    push_diamond_brush(scene, cx, cy, rx, ry, solid(c, alpha));
}

fn push_diamond_brush(scene: &mut Scene, cx: f64, cy: f64, rx: f64, ry: f64, brush: Brush) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    push_fill(scene, diamond_path(cx, cy, rx, ry), FillRule::NonZero, brush);
}

/// The `Brush`-taking core [`push_stroke_path`] delegates to -- lets a
/// caller (`crate::render::background`'s own continuous route ribbon) push
/// a GRADIENT-brushed stroke (e.g. one route segment fading from one
/// tile's own colour to its neighbour's) through the identical stroke
/// geometry (round join, round cap) every solid-coloured stroke in this
/// catalog already uses, without duplicating that `Stroke` construction.
pub(super) fn push_stroke_brush(scene: &mut Scene, path: BezPath, width: f64, brush: Brush, dash: Option<Dash>) {
    if width <= 0.0 || path.elements().is_empty() {
        return;
    }
    let stroke = Stroke { width: width as f32, join: LineJoin::Round, cap: LineCap::Round, dash, ..Stroke::default() };
    scene.push(DrawCommand::StrokePath { path, stroke, brush, transform: Affine::IDENTITY });
}

pub(super) fn push_stroke_path(scene: &mut Scene, path: BezPath, width: f64, c: Rgb, alpha: f32, dash: Option<Dash>) {
    if alpha <= 0.0 {
        return;
    }
    push_stroke_brush(scene, path, width, solid(c, alpha), dash);
}

fn push_diamond_outline(scene: &mut Scene, cx: f64, cy: f64, rx: f64, ry: f64, width: f64, c: Rgb, alpha: f32) {
    push_stroke_path(scene, diamond_path(cx, cy, rx, ry), width, c, alpha, None);
}

fn push_dashed_circle(scene: &mut Scene, cx: f64, cy: f64, r: f64, c: Rgb, alpha: f32) {
    push_stroke_path(scene, circle_path(cx, cy, r), 1.0, c, alpha, Some(Dash { pattern: vec![3.0, 2.2], phase: 0.0 }));
}

pub(super) fn push_rect_brush(scene: &mut Scene, x0: f64, y0: f64, x1: f64, y1: f64, radius: f32, brush: Brush) {
    let rect = Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
    let radii = (radius > 0.0).then_some([radius; 4]);
    scene.push(DrawCommand::FillRect { rect, radii, brush, transform: Affine::IDENTITY });
}

pub(super) fn push_rect(scene: &mut Scene, x0: f64, y0: f64, x1: f64, y1: f64, radius: f32, c: Rgb, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    push_rect_brush(scene, x0, y0, x1, y1, radius, solid(c, alpha));
}

fn push_lines(scene: &mut Scene, segments: Vec<LineBatchSegment>, width: f64, c: Rgb, alpha: f32) {
    if segments.is_empty() || width <= 0.0 || alpha <= 0.0 {
        return;
    }
    let stroke = Stroke { width: width as f32, join: LineJoin::Round, cap: LineCap::Round, ..Stroke::default() };
    scene.push(DrawCommand::LineBatch { segments, stroke, brush: solid(c, alpha), transform: Affine::IDENTITY });
}

/// A ring of `count` short leg strokes, evenly spaced by angle, each
/// running from `inner` (a body's own edge) out to `outer` -- the one
/// cheap way this catalog gives a small bug silhouette actual limbs
/// without a per-kind hand-authored leg path. `phase` (radians) offsets
/// where the first leg sits, so two different bug kinds sharing this
/// helper at different `count`/`phase` values (the `EnemyMite`/
/// `EnemySkitter` arms of [`paint_tile`]) never align their legs at the
/// identical angles and read as the same creature at a glance -- a real
/// top-down silhouette this small has no room for a directional "facing"
/// (`paint_tile` is never told which way an entity is travelling, see its
/// own doc comment), so every leg ring here is rotationally symmetric by
/// construction rather than aimed at a heading this call cannot know.
fn push_radiating_legs(scene: &mut Scene, cx: f64, cy: f64, inner: f64, outer: f64, count: usize, phase: f64, width: f64, c: Rgb, alpha: f32) {
    let mut segments = Vec::with_capacity(count);
    for i in 0..count {
        let a = phase + i as f64 / count as f64 * std::f64::consts::TAU;
        segments.push(LineBatchSegment { from: Vec2::new(cx + a.cos() * inner, cy + a.sin() * inner), to: Vec2::new(cx + a.cos() * outer, cy + a.sin() * outer) });
    }
    push_lines(scene, segments, width, c, alpha);
}

/// A soft rounded-rect wash -- `frac` of `(w, h)`, centred at `(cx, cy)` --
/// the terrain/tower background tint every other primitive in this module
/// paints first (a night-garden tile ground, not a hard-edged square).
fn wash(scene: &mut Scene, cx: f64, cy: f64, w: f64, h: f64, frac: f64, c: Rgb, alpha: f32) {
    let hw = w * frac / 2.0;
    let hh = h * frac / 2.0;
    push_rect(scene, cx - hw, cy - hh, cx + hw, cy + hh, (hw.min(hh) * 0.28) as f32, c, alpha);
}

/// The soft square wash every enemy sprite gets when carrying a status tint
/// (`Some(SLOW_BG)`/`Some(STUN_BG)` from `gate4agent-arcade-pet-bastion-
/// render`'s own colour catalog) -- factored out since six enemy arms all
/// need the identical call, and it must stay identical across all six for
/// "slowed"/"stunned" to read as one consistent visual language regardless
/// of which enemy kind is showing it.
fn status_wash(scene: &mut Scene, cx: f64, cy: f64, w: f64, h: f64, bg: Option<Rgb>, alpha: f32) {
    if let Some(bg) = bg {
        wash(scene, cx, cy, w, h, 0.8, bg, alpha);
    }
}

pub(super) const ROAD_NORTH: u8 = 1;
pub(super) const ROAD_EAST: u8 = 2;
pub(super) const ROAD_SOUTH: u8 = 4;
pub(super) const ROAD_WEST: u8 = 8;

/// A road/path tile shaped by its own connectivity `mask` (bits
/// [`ROAD_NORTH`]/[`ROAD_EAST`]/[`ROAD_SOUTH`]/[`ROAD_WEST`], set by
/// `gate4agent-arcade-pet-bastion-render`'s own `paint_terrain` from real
/// route-neighbour adjacency, carried into this pixel tier via `TileArt::
/// variant`) -- flush against every side that connects to another route
/// tile, inset against every side that does not, so a whole route reads
/// as one continuous ribbon with a real, visible edge against the
/// surrounding [`TileId::Ground`], not a chain of separately-inset
/// squares. A light-to-dark vertical wash gives the ribbon a shallow
/// cross-section (depth), the "перепад тона" this pass's own brief asked
/// for on top of a flat fill.
fn push_road(scene: &mut Scene, cx: f64, cy: f64, w: f64, h: f64, mask: u8, fg: Rgb, alpha: f32) {
    let margin_x = w * 0.19;
    let margin_y = h * 0.19;
    let x0 = cx - w / 2.0 + if mask & ROAD_WEST != 0 { 0.0 } else { margin_x };
    let x1 = cx + w / 2.0 - if mask & ROAD_EAST != 0 { 0.0 } else { margin_x };
    let y0 = cy - h / 2.0 + if mask & ROAD_NORTH != 0 { 0.0 } else { margin_y };
    let y1 = cy + h / 2.0 - if mask & ROAD_SOUTH != 0 { 0.0 } else { margin_y };
    let corner = (margin_x.min(margin_y) * 0.35) as f32;
    let brush = linear(vec![stop(0.0, tint(fg, 220), alpha), stop(0.55, fg, alpha), stop(1.0, darken(fg, 620), alpha)], cx, y0, cx, y1);
    push_rect_brush(scene, x0, y0, x1, y1, corner, brush);
}

// ---------------------------------------------------------------------------
// The catalog itself.
// ---------------------------------------------------------------------------

/// Paints one entity's own shape straight into `scene`, at pixel-space
/// centre `(cx, cy)` and bounding box `(w, h)` -- the pixel-tier analogue
/// of `backend_sixel.rs`'s own `rasterize_tile`, at genuinely per-KIND
/// (Needle vs Bell vs Prism, Mite vs Skitter vs Shellback, ...) shape
/// resolution rather than per-CATEGORY. `alpha` (0.0-1.0) scales every
/// colour this call pushes, so a fading effect never needs a separate
/// blend pass. See this module's own doc comment for why there is no
/// baking/caching step: every call rebuilds its own commands fresh,
/// the same "no stale-cache invalidation surface" reasoning the old
/// `bake_sprite` already documented.
///
/// `dragging` matches every other backend's own `RenderBackend::project`
/// parameter of the same name -- consulted ONLY by the `BuildPad` arm (a
/// free build slot is a quiet flat ring most of the time, and only gets
/// its own glow while the owner is actually dragging a tower to place --
/// this pass's own "свободные слоты подсвечивать только когда игрок тянет
/// башню" requirement). A real Pet Bastion board carries roughly as many
/// `BuildPad` tiles as terrain tiles (`board::Board::near_route_cells`),
/// so this is also a real cost fix, not just a visual one: the radial-
/// gradient glow this arm used to paint unconditionally on every single
/// pad, every frame, is now paid only while a drag is actually in
/// progress. Every other `TileId` arm ignores this parameter entirely.
pub(crate) fn paint_tile(scene: &mut Scene, tile: TileId, variant: u8, fg: Rgb, bg: Option<Rgb>, cx: f64, cy: f64, w: f64, h: f64, alpha: f32, dragging: bool) {
    let half = w.min(h) / 2.0;

    match tile {
        TileId::Ground => {
            // Deterministic tone variety (`variant` hashed from this tile's
            // own coordinates by `paint_terrain`'s own `ground_variant`) --
            // one flat rect fill (cheap: no bezier tessellation) tinted off
            // the SAME base hue every tile shares, plus one small
            // off-centre accent patch so a single tile does not read as one
            // perfectly flat block up close. Two rect fills, not a
            // per-pixel noise texture -- the "фактура... не плоская
            // заливка" this pass's own brief asked for, at a cost bounded
            // and flat regardless of the actual board's own tile count.
            let toned = match variant % 4 {
                0 => darken(fg, 850),
                1 => fg,
                2 => darken(fg, 930),
                _ => tint(fg, 90),
            };
            wash(scene, cx, cy, w, h, 1.0, toned, alpha);
            let accent = if variant % 2 == 0 { darken(fg, 760) } else { tint(fg, 180) };
            let ax = cx + if variant & 2 != 0 { w * 0.16 } else { -w * 0.16 };
            let ay = cy + if variant & 1 != 0 { h * 0.14 } else { -h * 0.14 };
            push_rect(scene, ax - w * 0.13, ay - h * 0.10, ax + w * 0.13, ay + h * 0.10, (h * 0.08) as f32, accent, alpha * 0.45);
        }
        TileId::Rock => {
            let ox = cx + (match variant % 3 { 0 => -0.16, 1 => 0.02, _ => 0.17 }) * w;
            let oy = cy + (match (variant / 3) % 3 { 0 => -0.10, 1 => 0.06, _ => 0.16 }) * h;
            let rx = half * (0.24 + 0.03 * (variant % 2) as f64);
            let ry = rx * 0.66;
            push_fill(scene, ellipse_path(ox + rx * 0.2, oy + ry * 0.35, rx, ry * 0.7), FillRule::NonZero, solid(darken(fg, 250), 0.5 * alpha));
            let body = linear(vec![stop(0.0, tint(fg, 350), alpha), stop(1.0, darken(fg, 500), alpha)], ox, oy - ry, ox, oy + ry);
            push_fill(scene, ellipse_path(ox, oy, rx, ry), FillRule::NonZero, body);
        }
        TileId::Plant => {
            let ox = cx + (match variant % 3 { 0 => -0.15, 1 => 0.04, _ => 0.17 }) * w;
            let oy = cy + (match (variant / 3) % 2 { 0 => 0.12, _ => -0.04 }) * h;
            let blade_h = half * 0.55;
            let mut segments = Vec::with_capacity(3);
            for dx_frac in [-0.22f64, 0.0, 0.22] {
                segments.push(LineBatchSegment { from: Vec2::new(ox, oy), to: Vec2::new(ox + dx_frac * half, oy - blade_h) });
            }
            push_lines(scene, segments, 1.3, fg, alpha);
        }
        TileId::WaterPool => {
            let r = half * 0.64;
            let deep = radial(vec![stop(0.0, darken(fg, 500), alpha), stop(0.7, fg, alpha), stop(1.0, tint(fg, 250), 0.0)], cx, cy, r);
            push_fill(scene, circle_path(cx, cy, r), FillRule::NonZero, deep);
            push_ring(scene, cx, cy, r * 0.97, r * 0.87, tint(fg, 400), 0.6 * alpha);
        }
        TileId::Firefly => {
            let ox = cx + (match variant % 3 { 0 => -0.22, 1 => 0.16, _ => -0.04 }) * w;
            let oy = cy + (match (variant / 3) % 3 { 0 => -0.2, 1 => 0.1, _ => 0.24 }) * h;
            let glow = radial(vec![stop(0.0, Rgb(255, 255, 220), alpha), stop(0.4, fg, 0.8 * alpha), stop(1.0, fg, 0.0)], ox, oy, half * 0.24);
            push_fill(scene, circle_path(ox, oy, half * 0.24), FillRule::NonZero, glow);
            push_circle(scene, ox, oy, half * 0.05, Rgb(255, 255, 255), alpha);
        }
        TileId::Path => {
            push_road(scene, cx, cy, w, h, variant, fg, alpha);
        }
        TileId::Choke => {
            push_road(scene, cx, cy, w, h, variant, fg, alpha);
            push_circle(scene, cx, cy, half * 0.22, Rgb(fg.0 / 2, fg.1 / 2, fg.2 / 2), alpha);
        }
        TileId::BuildPad => {
            let outer = half * 0.6;
            let inner = half * 0.38;
            if dragging {
                // A flat, solid translucent disc, not a per-tile radial
                // gradient: a real board carries roughly as many `BuildPad`
                // tiles as terrain tiles, and while dragging EVERY one of
                // them repaints this glow, every frame -- a fresh
                // `Gradient` (and the LUT its own rasteriser has to build
                // from it) per tile measurably missed the frame budget at
                // that multiplicity (`bench`'s own `urx_render` binary,
                // `dragging=true` case). A flat fill reads as the same
                // "this slot is highlighted" cue at a fraction of the cost.
                push_circle(scene, cx, cy, outer, fg, 0.32 * alpha);
            }
            // A single stroked circle, not `push_ring`'s own two nested
            // filled circle paths combined via `EvenOdd` -- same real
            // board density concern as the glow above (this arm runs on
            // roughly as many tiles as terrain carries at all while
            // dragging), and one stroke-expanded path measurably costs
            // less than filling and EvenOdd-combining two separate ones
            // (`bench`'s own `urx_render` binary).
            push_stroke_path(scene, circle_path(cx, cy, (outer + inner) / 2.0), outer - inner, fg, alpha, None);
        }
        TileId::Heartseed => {
            let core = half * 0.42;
            let glow = radial(vec![stop(0.0, fg, alpha), stop(0.55, fg, 0.65 * alpha), stop(1.0, fg, 0.0)], cx, cy, core * 1.6);
            push_fill(scene, circle_path(cx, cy, core * 1.6), FillRule::NonZero, glow);
            push_ring(scene, cx, cy, half * 0.66, half * 0.58, fg, alpha);
        }
        TileId::PetAnchor => {
            push_diamond_outline(scene, cx, cy, half * 0.48, half * 0.48, 1.1, fg, alpha);
        }
        TileId::TowerNeedle => {
            if let Some(bg) = bg {
                wash(scene, cx, cy, w, h, 0.86, bg, alpha);
            }
            let level = variant.min(2) as f64;
            // A dagger -- blade, crossguard, hilt -- not one bare spike:
            // the old single `spindle_path` topped out at `half_w = 0.14
            // * half`, under 3px wide at this tile's real `20x19` size,
            // and read as a tally mark once actually rendered (the
            // cheapest, most-built tower in the game, so the shape the
            // board is most covered in). A first fix tried flanking the
            // blade with two smaller `spindle_path` thorns, but at a
            // width that survives this tile's own real scale (over 3px,
            // the same floor this pass's earlier enemy-leg work already
            // found) those thorns overlapped the main blade's own body
            // almost entirely and never showed as separate features --
            // checked by rendering and pixel-sampling the result, not by
            // eye alone. A crossguard is the fix that cannot fail that
            // way: a rect wider than the blade on both sides is visible
            // by construction, at any width, the same reasoning
            // [`nest_path`]'s own basin already leaned on for
            // `TowerEmberNest`.
            let blade_cy = cy - half * 0.15;
            let blade_hw = half * (0.15 + 0.025 * level);
            let blade_hh = half * (0.6 + 0.07 * level);
            let blade = spindle_path(cx, blade_cy, blade_hw, blade_hh);
            let sheen = linear(vec![stop(0.0, lighten(fg), alpha), stop(1.0, fg, alpha)], cx, blade_cy - blade_hh, cx, blade_cy + blade_hh);
            push_fill(scene, blade, FillRule::NonZero, sheen);
            let guard_y = cy + half * (0.16 + 0.02 * level);
            let guard_hw = half * (0.34 + 0.05 * level);
            // `0.17`, not the `0.07` this started as. `half` is about
            // 9.5 device pixels at this tile's real `20x19` size
            // (`backend_pixel`'s own `PX_PER_CELL_W`/`PX_PER_CELL_H`), so
            // `0.07` made the crossguard 0.66px tall -- BELOW one pixel.
            // It survived at level 0 only by happening to straddle a
            // pixel row, and vanished completely at levels 1 and 2, where
            // the whole sprite measured 2px wide with no crossguard row
            // at all: the bare tally mark this arm exists to stop being.
            // Measured, not eyeballed -- the per-row ink widths were
            // counted at every level. A feature that must read here needs
            // to be at least a pixel and a half, the same floor this
            // module's own enemy-leg and lantern-post work already
            // landed on.
            let guard_hh = half * 0.17;
            push_rect(scene, cx - guard_hw, guard_y - guard_hh, cx + guard_hw, guard_y + guard_hh, (guard_hh * 0.8) as f32, darken(fg, 650), alpha);
            let hilt_hw = half * (0.11 + 0.015 * level);
            push_rect(scene, cx - hilt_hw, guard_y + guard_hh, cx + hilt_hw, cy + half * 0.48, (hilt_hw * 0.6) as f32, darken(fg, 480), alpha);
        }
        TileId::TowerBell => {
            if let Some(bg) = bg {
                wash(scene, cx, cy, w, h, 0.86, bg, alpha);
            }
            let level = variant.min(2) as f64;
            let body = bell_path(cx, cy, half * (0.62 + 0.08 * level));
            push_fill(scene, body, FillRule::NonZero, solid(fg, alpha));
            let mouth_outer = half * (0.34 + 0.06 * level);
            let mouth_cy = cy + half * 0.32;
            push_ring(scene, cx, mouth_cy, mouth_outer, half * (0.24 + 0.05 * level), darken(fg, 700), alpha);
            // The clapper -- a small dark dot hanging inside the mouth
            // ring. Every level before this had exactly ONE feature that
            // grows with `level` (the body/ring pair, both scaled by the
            // SAME `level` term), which reads as "the same bell, a bit
            // bigger" rather than a genuine escalation once actually
            // rendered next to itself at real size (checked by rendering
            // base/L2/L3 side by side on the real night-garden ground --
            // see this pass's own verification). A clapper is a real part
            // of a bell no earlier arm here ever drew.
            push_circle(scene, cx, mouth_cy, half * (0.08 + 0.02 * level), darken(fg, 450), alpha);
            // A second, wider "toll" ring appears ONLY at L2/L3 -- a
            // feature that is simply ABSENT at Base rather than merely
            // smaller, so a levelled Bell reads as louder/further-
            // reaching, not just larger. Present/absent is what actually
            // survives this tile's real ~9.5px `half`: two rings a few
            // tenths of a pixel apart in RADIUS ALONE (the old escalation
            // this arm relied on) blur together under anti-aliasing at
            // this size, but a ring that simply is not there at Base is
            // unmistakable.
            if level >= 1.0 {
                let echo_alpha = (0.3 + 0.25 * (level - 1.0)) as f32 * alpha;
                push_stroke_path(scene, circle_path(cx, mouth_cy, mouth_outer * 1.35), 1.0, tint(fg, 350), echo_alpha, None);
            }
        }
        TileId::TowerPrism => {
            if let Some(bg) = bg {
                wash(scene, cx, cy, w, h, 0.86, bg, alpha);
            }
            let level = variant.min(2) as f64;
            // Deliberately a much bigger fraction of `half` than this
            // tower's own OLD (pre-URX) diamond -- at this tile's real
            // on-screen size (native `20x19` px, never upscaled; see
            // `backend_pixel.rs`'s own `PX_PER_CELL_W` doc comment) a
            // diamond sized off the old, smaller fractions renders as a
            // soft few-pixel smear, not a readable faceted gem.
            let rx = half * (0.62 + 0.1 * level);
            let ry = half * (0.8 + 0.12 * level);
            // The facet outline itself thickens with level (`1.8` was a
            // flat constant regardless of `level` before this pass) -- so
            // the CUT, not only the STONE, escalates: `rx`/`ry` already
            // grow the gem's own size, but a same-width outline on a
            // bigger gem reads as a thinner-looking edge relative to the
            // body, the opposite of what a heavier L3 facet should look
            // like next to Base.
            push_diamond_outline(scene, cx, cy, rx, ry, 1.5 + 0.45 * level, fg, alpha);
            let glass = linear(
                vec![stop(0.0, Rgb(255, 255, 255), 0.55 * alpha), stop(0.5, fg, 0.35 * alpha), stop(1.0, Rgb(255, 255, 255), 0.15 * alpha)],
                cx - rx * 0.6,
                cy - ry * 0.6,
                cx + rx * 0.6,
                cy + ry * 0.6,
            );
            push_diamond_brush(scene, cx, cy, rx * 0.62, ry * 0.62, glass);
        }
        TileId::TowerEmberNest => {
            if let Some(bg) = bg {
                wash(scene, cx, cy, w, h, 0.86, bg, alpha);
            }
            let level = variant.min(2) as f64;
            // The basin body a "nest" needs before its embers sit inside it
            // (`nest_path`'s own doc comment) -- a charred twig tone
            // (`darken(fg, ...)`, since this arm only ever receives its own
            // ember-orange `fg`, never a separate wood colour) rather than
            // an unrelated new colour constant, so the tower still reads
            // off ONE hue like every other kind in this catalog.
            let basin = nest_path(cx, cy, half * (0.58 + 0.05 * level), half * (0.56 + 0.05 * level));
            push_fill(scene, basin.clone(), FillRule::NonZero, solid(darken(fg, 380), alpha));
            // A rim highlight along the basin's own edge -- charred-twig
            // brown (`darken(fg, 380)`) sits close enough in LUMA to the
            // night-garden ground for the basin's own silhouette to nearly
            // vanish once actually rendered at this tile's real size
            // (checked by rendering it: without this stroke the basin read
            // as a vague dark smudge behind the embers, not a nest with a
            // real edge -- see this pass's own verification). One lighter
            // stroke along the SAME [`nest_path`] the fill already uses
            // (the same "smaller/adjacent path, same builder" rim trick
            // [`TileId::EnemyShellback`]'s own dome highlight already
            // relies on) gives the basin a real, visible boundary without
            // a second, unrelated shape.
            push_stroke_path(scene, basin, 0.9, tint(darken(fg, 380), 320), 0.6 * alpha, None);
            let r = half * (0.16 + 0.03 * level);
            let mut embers = vec![(0.0, -0.2, 1.0), (-0.22, 0.16, 0.85), (0.22, 0.16, 0.85)];
            // A fourth, smaller ember joins the cluster ONLY at L3 --
            // level escalation here used to be "the same three embers, a
            // little bigger" (`r`'s own `0.03 * level` term), which reads
            // as barely distinguishable from L2 next to it at real size.
            // A genuinely new ember (present/absent, the same "add a
            // feature, don't just rescale one" fix `TowerBell`'s own
            // clapper/echo-ring above already applies) is what actually
            // reads as "this nest is stronger," not merely "a bit bigger."
            if variant >= 2 {
                embers.push((0.0, 0.34, 0.55));
            }
            for (dx, dy, scale) in embers {
                let ex = cx + dx * half;
                let ey = cy + dy * half;
                let er = r * scale;
                let glow = radial(vec![stop(0.0, Rgb(255, 220, 150), alpha), stop(0.55, fg, alpha), stop(1.0, darken(fg, 500), 0.0)], ex, ey, er * 1.4);
                push_fill(scene, circle_path(ex, ey, er * 1.4), FillRule::NonZero, glow);
            }
        }
        TileId::TowerMoonwell => {
            if let Some(bg) = bg {
                wash(scene, cx, cy, w, h, 0.86, bg, alpha);
            }
            let level = variant.min(2) as f64;
            let outer = half * (0.44 + 0.08 * level);
            let inner = half * (0.34 + 0.06 * level);
            push_ring(scene, cx, cy, outer, inner, fg, alpha);
            // Deep water, not a spinning sweep -- an EARLIER version of
            // this arm filled the inner disc with a full-turn
            // `SweepGradientPosition` ("the Moonwell's own rotating field
            // glow"), which at this tile's real ~7px inner radius
            // rasterises across too few pixels per colour band to read as
            // anything but noise (checked by rendering it: it looked like
            // a flat smear, not a swirl). The SAME "deep water" radial
            // recipe [`TileId::WaterPool`] already proves legible at this
            // exact tile size -- dark centre fading through the base hue
            // to a pale rim -- reused here so a Moonwell's own basin reads
            // as an actual lit pool, which is what its own name promises.
            let water_r = half * (0.36 + 0.05 * level);
            let deep = radial(vec![stop(0.0, darken(fg, 450), alpha), stop(0.6, fg, alpha), stop(1.0, tint(fg, 300), 0.5 * alpha)], cx, cy, water_r);
            push_fill(scene, circle_path(cx, cy, water_r), FillRule::NonZero, deep);
            // A moon-glint highlight, off-centre -- the one feature in
            // this arm that visibly ESCALATES with level (bigger, brighter)
            // rather than merely scaling alongside everything else, the
            // same "add a feature that grows disproportionately" fix
            // `TowerBell`'s own clapper/echo-ring and `TowerEmberNest`'s
            // own fourth ember above already apply. Also the one part of
            // this basin that reads as genuinely LIT (moonlight caught on
            // still water), rather than a flat-coloured disc.
            let glint_r = half * (0.07 + 0.035 * level);
            push_circle(scene, cx - water_r * 0.28, cy - water_r * 0.32, glint_r, Rgb(255, 255, 255), (0.55 + 0.15 * level) as f32 * alpha);
            // A stone rim lip just outside the water ring -- one extra
            // stroke, grounding what was otherwise a ring floating over
            // bare terrain with nothing reading as the basin's own edge.
            push_stroke_path(scene, circle_path(cx, cy, outer * 1.1), 1.0, darken(fg, 550), 0.55 * alpha, None);
        }
        TileId::TowerRelay => {
            if let Some(bg) = bg {
                wash(scene, cx, cy, w, h, 0.86, bg, alpha);
            }
            let level = variant.min(2) as f64;
            // A caged beacon on a stone plinth, not the thin garden-
            // lantern post this arm tried first: that post was `half *
            // 0.07` wide (under 1.5px at this tile's real `20x19` size)
            // and vanished completely once actually rendered at real
            // size against the night-garden ground, leaving only the
            // lantern's own glow -- a soft dot with no structure, the
            // same "reads only zoomed in" failure this pass's own brief
            // calls out by name. A wide plinth block anchors the shape at
            // the SAME visual weight every other tower body in this
            // catalog now carries, a diamond cage outline (this
            // catalog's own faceted-gem vocabulary, [`push_diamond_
            // outline`], reused so a Relay still reads as kin to Prism's
            // own gem rather than inventing a fourth silhouette family)
            // gives the beacon real edges instead of only a blurred
            // glow, and a short solid neck ties cage to plinth so the
            // whole thing reads as one built object, not two unrelated
            // shapes floating near each other. This tower ATTACKS
            // NOTHING (`No attack; extends Circuit by one tower` per the
            // night-garden plan's own Towers table) -- a beacon a Circuit
            // signal relays THROUGH is still the honest fiction, just one
            // that survives being rendered at this tile's real size.
            let plinth_w = half * (0.34 + 0.05 * level);
            let plinth_h = half * (0.16 + 0.02 * level);
            let plinth_y = cy + half * 0.34;
            push_rect(scene, cx - plinth_w, plinth_y - plinth_h, cx + plinth_w, plinth_y + plinth_h, (plinth_h * 0.7) as f32, darken(fg, 480), alpha);
            let cage_cy = cy - half * (0.14 + 0.04 * level);
            let cage_r = half * (0.32 + 0.05 * level);
            push_rect(scene, cx - half * 0.05, cage_cy + cage_r * 0.5, cx + half * 0.05, plinth_y - plinth_h, (half * 0.04) as f32, darken(fg, 500), alpha);
            let glow = radial(vec![stop(0.0, Rgb(255, 255, 220), alpha), stop(0.6, fg, 0.85 * alpha), stop(1.0, fg, 0.0)], cx, cage_cy, cage_r);
            push_fill(scene, circle_path(cx, cage_cy, cage_r), FillRule::NonZero, glow);
            push_diamond_outline(scene, cx, cage_cy, cage_r, cage_r, 1.4, darken(fg, 620), alpha);
            push_circle(scene, cx, cage_cy, cage_r * 0.3, Rgb(255, 255, 255), alpha);
        }
        TileId::EnemyMite => {
            status_wash(scene, cx, cy, w, h, bg, alpha);
            // A round tick-like body -- bumped from the old `half * 0.22`
            // (a 2px dot at this tile's real `20x19` size, see this
            // module's own top-level doc comment: never upscaled) up to a
            // fraction that actually FILLS the cell it is given, per this
            // pass's own "biggest readability win" note. Six short legs
            // (a mite's own many-legged silhouette, kept few for a
            // "cheap swarm body" read -- this is wave 1's tutorial enemy,
            // the plan's own lowest-HP/lowest-threat kind) drawn UNDER the
            // body so they peek out from its own edge rather than
            // free-floating.
            let r = half * 0.34;
            push_radiating_legs(scene, cx, cy, r * 0.85, r * 1.55, 6, 0.3, 0.8, darken(fg, 650), alpha);
            let glow = radial(vec![stop(0.0, lighten(fg), alpha), stop(1.0, fg, alpha)], cx - r * 0.3, cy - r * 0.3, r * 1.3);
            push_fill(scene, circle_path(cx, cy, r), FillRule::NonZero, glow);
        }
        TileId::EnemySkitter => {
            status_wash(scene, cx, cy, w, h, bg, alpha);
            // Eight long, thin, splayed legs -- more numerous and longer
            // than [`TileId::EnemyMite`]'s own six short ones, the
            // "Punishes slow first hits" / highest-speed-among-early-
            // enemies read this kind's own plan entry calls for (a fast,
            // twitchy many-legged scuttler, not a slow tick). `phase =
            // 0.0` (vs. Mite's `0.3`) keeps the two leg rings from ever
            // aligning even at the identical `count`/size this pass could
            // have reused. A small diamond core (vs. Mite's round body)
            // keeps the two silhouettes apart even with the legs ignored.
            let r = half * 0.26;
            push_radiating_legs(scene, cx, cy, r * 0.9, r * 2.1, 8, 0.0, 0.7, fg, alpha);
            push_diamond(scene, cx, cy, r, r, fg, alpha);
            push_circle(scene, cx, cy, r * 0.32, darken(fg, 550), alpha);
        }
        TileId::EnemyShellback => {
            status_wash(scene, cx, cy, w, h, bg, alpha);
            // Two small feet peeking from under the shell's own flat base
            // edge (`shell_path`'s own `base_y`), painted BEFORE the shell
            // body so the shell's own fill naturally covers whatever part
            // of each foot circle sits above that edge -- a creature
            // actually standing under its own armour, not a shell floating
            // with nothing under it.
            push_circle(scene, cx - half * 0.3, cy + half * 0.24, half * 0.06, darken(fg, 650), alpha);
            push_circle(scene, cx + half * 0.3, cy + half * 0.24, half * 0.06, darken(fg, 650), alpha);
            push_fill(scene, shell_path(cx, cy, half), FillRule::NonZero, solid(fg, alpha));
            // A thin inset rim highlight -- the same "smaller radius, same
            // path builder" trick [`push_ring`] already uses for a ring,
            // applied here to [`shell_path`] itself, for the domed-shell
            // sheen a flat single fill cannot show on its own.
            push_stroke_path(scene, shell_path(cx, cy, half * 0.92), 0.7, lighten(fg), 0.45 * alpha, None);
            push_circle(scene, cx, cy + half * 0.14, half * 0.1, darken(fg, 700), alpha);
        }
        TileId::EnemySplitter => {
            status_wash(scene, cx, cy, w, h, bg, alpha);
            // A trefoil -- three fused lobes, `trefoil_path`'s own "about
            // to divide into three" tell for a kind whose entire role is
            // "Spawns three Mites on death", replacing the old plain
            // crossed-lines "X" that carried no hint of what this enemy
            // actually does. A small darker core dot holds the three
            // lobes together visually (the shared body that has not yet
            // actually split).
            push_fill(scene, trefoil_path(cx, cy, half * 0.22, half * 0.27), FillRule::NonZero, solid(fg, alpha));
            push_circle(scene, cx, cy, half * 0.12, darken(fg, 550), alpha);
        }
        TileId::EnemyHusher => {
            status_wash(scene, cx, cy, w, h, bg, alpha);
            // A hooded cloak body (`hood_path`'s own doc comment) instead
            // of the old ring-plus-dashed-circle with nothing solid inside
            // it -- the suppression aura (the same two shapes as before)
            // now wraps AROUND a real silhouette instead of standing in
            // for one. A hollow dark notch where a face would be keeps the
            // "hush spirit" faceless/mysterious, matching its own
            // "Suppresses nearby tower fire rate" fiction (a Bell-adjacent
            // ROLE, but never drawn as the actual Bell tower body).
            push_fill(scene, hood_path(cx, cy, half), FillRule::NonZero, solid(fg, alpha));
            push_fill(scene, ellipse_path(cx, cy - half * 0.2, half * 0.1, half * 0.13), FillRule::NonZero, solid(darken(fg, 400), 0.8 * alpha));
            push_ring(scene, cx, cy, half * 0.62, half * 0.52, fg, 0.55 * alpha);
            push_dashed_circle(scene, cx, cy, half * 0.72, fg, 0.7 * alpha);
        }
        TileId::EnemyMirror => {
            status_wash(scene, cx, cy, w, h, bg, alpha);
            // A hard-edged two-facet gem, not a gradient-shaded one: a
            // thin ANTI-ALIASED outline/gradient rim (this arm's own
            // first attempt) blurs into its own fill at this tile's real
            // ~9px body radius, and Mirror's own base hue (`Rgb(210, 210,
            // 255)`, `games/pet-bastion-render/src/lib.rs::enemy_color`)
            // sits close to white already, so there is barely any tonal
            // room left for a soft rim to read against. A hard light-top/
            // dark-bottom split -- two solid triangles sharing one
            // straight seam, no gradient softening the join -- reads as a
            // genuine faceted cut regardless of how pale the base hue is,
            // since the seam is a hard colour-to-colour boundary, not an
            // alpha falloff a renderer this small can blur away. Plus the
            // original diagonal sheen streak and a bright core pip
            // standing in for the light this kind's own "Temporarily
            // resists the last damage family" reflective fiction implies
            // it is always catching.
            let r = half * 0.42;
            let mut top_facet = BezPath::new();
            top_facet.move_to((cx, cy - r));
            top_facet.line_to((cx + r, cy));
            top_facet.line_to((cx - r, cy));
            top_facet.close_path();
            push_fill(scene, top_facet, FillRule::NonZero, solid(lighten(fg), alpha));
            let mut bottom_facet = BezPath::new();
            bottom_facet.move_to((cx, cy + r));
            bottom_facet.line_to((cx + r, cy));
            bottom_facet.line_to((cx - r, cy));
            bottom_facet.close_path();
            push_fill(scene, bottom_facet, FillRule::NonZero, solid(darken(fg, 550), alpha));
            push_diamond_outline(scene, cx, cy, r, r, 1.0, darken(fg, 300), alpha);
            let sheen = linear(
                vec![stop(0.0, Rgb(255, 255, 255), 0.0), stop(0.5, Rgb(255, 255, 255), 0.7 * alpha), stop(1.0, Rgb(255, 255, 255), 0.0)],
                cx - r * 0.7,
                cy - r * 0.7,
                cx + r * 0.7,
                cy + r * 0.7,
            );
            push_diamond_brush(scene, cx, cy, r * 0.7, r * 0.7, sheen);
            push_circle(scene, cx, cy, r * 0.14, Rgb(255, 255, 255), alpha);
        }
        TileId::BossBellkeeper => {
            // A boss's own dark backdrop, as a radial falloff with no
            // edge at all -- never the hard cell-sized `push_rect` this
            // arm used to draw. A full-cell rectangle with a zero corner
            // radius IS the board's own grid, drawn on the single most
            // eye-catching object in the scene, and this renderer's whole
            // ground/route construction exists to keep that grid off the
            // board (`background.rs`'s own module doc: blooms on their
            // own spacing, roads as round caps and strokes, nothing
            // aligned to a cell edge). A rounded rect is not enough
            // either: at 20x19 device pixels per cell
            // (`backend_pixel`'s own `PX_PER_CELL_W`/`PX_PER_CELL_H`)
            // [`wash`]'s own corner radius comes to about two pixels,
            // which still reads as a tile-shaped block -- checked by
            // rendering it at real size. Only a falloff has no edge to
            // align.
            if let Some(bg) = bg {
                push_ambient_glow(scene, cx, cy, w.max(h) * 0.62, bg, alpha);
            }
            // A genuine bell -- the actual [`bell_path`] silhouette the
            // Bell tower's own body already uses, at boss scale, rather
            // than the old plain glowing orb-plus-ring that gave this
            // boss no shape a name like "Bellkeeper" promised. `0.9` (vs.
            // a base tower's own max `0.78` at L3) reads as one real,
            // oversized bell rather than a levelled tower repeated -- the
            // boss quality bar this pass's own brief points at
            // ([`TileId::BossNightMaw`]'s own `maw_path(cx, cy, half)`
            // call already draws at this same full scale). An ambient
            // toll-glow behind it (same construction as
            // [`push_ambient_glow`], inlined since it needs `alpha`
            // scaling this arm already carries) reads as the sound this
            // boss's own bell periodically tolls to silence towers.
            // The bell's own light drains with the fight (see
            // [`boss_vitality_permille`]): a full-health Bellkeeper tolls
            // brightly, a nearly-dead one is a dull, barely-glowing
            // shape.
            let lit = boss_vitality_permille(variant);
            // Every radius below comes off `hb`, not `half`, so the whole
            // bell -- body, mouth ring and clapper together -- shrinks as
            // one shape (see [`boss_body_scale`]).
            let hb = half * boss_body_scale(variant);
            let glow_alpha = (0.12 + 0.43 * boss_light(lit) as f32) * alpha;
            let glow = radial(vec![stop(0.0, lighten(fg), glow_alpha), stop(1.0, fg, 0.0)], cx, cy - hb * 0.1, hb * 0.85);
            push_fill(scene, circle_path(cx, cy - hb * 0.1, hb * 0.85), FillRule::NonZero, glow);
            let body = bell_path(cx, cy, hb * 0.9);
            let shade = linear(
                vec![stop(0.0, darken(lighten(fg), lit), alpha), stop(1.0, darken(fg, 550 * lit / 1000), alpha)],
                cx,
                cy - hb * 0.7,
                cx,
                cy + hb * 0.6,
            );
            push_fill(scene, body, FillRule::NonZero, shade);
            push_ring(scene, cx, cy + hb * 0.3, hb * 0.32, hb * 0.22, darken(fg, 400 * lit / 1000), alpha);
            push_circle(scene, cx, cy + hb * 0.02, hb * 0.1, darken(fg, 280 * lit / 1000), alpha);
        }
        TileId::BossNightMaw => {
            // A boss's own dark backdrop, as a radial falloff with no
            // edge at all -- never the hard cell-sized `push_rect` this
            // arm used to draw. A full-cell rectangle with a zero corner
            // radius IS the board's own grid, drawn on the single most
            // eye-catching object in the scene, and this renderer's whole
            // ground/route construction exists to keep that grid off the
            // board (`background.rs`'s own module doc: blooms on their
            // own spacing, roads as round caps and strokes, nothing
            // aligned to a cell edge). A rounded rect is not enough
            // either: at 20x19 device pixels per cell
            // (`backend_pixel`'s own `PX_PER_CELL_W`/`PX_PER_CELL_H`)
            // [`wash`]'s own corner radius comes to about two pixels,
            // which still reads as a tile-shaped block -- checked by
            // rendering it at real size. Only a falloff has no edge to
            // align.
            if let Some(bg) = bg {
                push_ambient_glow(scene, cx, cy, w.max(h) * 0.62, bg, alpha);
            }
            // The Maw's own eye closes as it dies (see
            // [`boss_vitality_permille`]) -- the void dims and the core
            // ember shrinks, while `maw_path`'s own toothed outline stays
            // exactly as it is: dimming may never cost this boss the
            // silhouette that identifies it.
            let lit = boss_vitality_permille(variant);
            // `hb` for the same reason as Bellkeeper's own: the toothed
            // outline, the void behind it and the core ember shrink
            // together, so the silhouette that names this boss survives
            // the shrink intact.
            let hb = half * boss_body_scale(variant);
            let void = radial(
                vec![stop(0.0, darken(fg, lit), alpha), stop(0.6, darken(fg, 500 * lit / 1000), alpha), stop(1.0, Rgb(5, 5, 10), alpha)],
                cx,
                cy,
                hb * 0.6,
            );
            push_fill(scene, maw_path(cx, cy, hb), FillRule::NonZero, void);
            let core_r = hb * (0.05 + 0.11 * boss_light(lit));
            push_circle(scene, cx, cy, core_r, darken(fg, 250 * lit / 1000), alpha);
        }
        TileId::Pet => {
            if let Some(bg) = bg {
                wash(scene, cx, cy, w, h, 0.86, bg, alpha);
            }
            match variant {
                // Moth: two rounded wing lobes fanned up and out from a
                // dark body bar, not the old shared-root teardrop pair
                // (a small `wing_path` helper this arm used to call,
                // since removed as dead code once nothing else called
                // it). Both teardrops shared their single narrow point
                // at `(cx, cy)` and only bulged outward along one flat
                // horizontal band, so once actually rendered at this
                // tile's real `20x19` size the two lobes read as one
                // fused horizontal bar with a slightly tapered end, not
                // two wings -- checked by rendering it on the real
                // night-garden ground, the same way this arm's own
                // default case below was caught. Two full
                // [`ellipse_path`] lobes (round, not shared-point
                // teardrops) plus a dark body bar painted OVER their own
                // shared middle is what actually keeps them read as two
                // wings framing a body: the bar's own darker tone cuts a
                // visible seam through the overlap a same-colour join
                // could not show.
                1 => {
                    // `dx_mult = 1.15` against `wing_r = 0.28 * half`
                    // leaves each wing's own INNER edge short of `cx` by
                    // `0.15 * wing_r` -- a real gap between the two
                    // lobes, not the overlap a first attempt at this
                    // shape used (both wings reaching PAST centre so
                    // only a same-hue seam separated them, which a
                    // render at this tile's real size blurred away
                    // entirely -- checked by rendering it, not by eye
                    // alone). The body bar below is sized to bridge that
                    // exact gap and extend visibly beyond both wings top
                    // and bottom, so what shows is unambiguously three
                    // parts -- wing, body, wing -- rather than a shape
                    // that only reads as separate parts in the source
                    // code, never on screen.
                    let wing_r = half * 0.28;
                    for dx in [-1.0f64, 1.0] {
                        push_fill(scene, ellipse_path(cx + dx * wing_r * 1.15, cy, wing_r, wing_r * 0.8), FillRule::NonZero, solid(fg, alpha));
                    }
                    push_rect(scene, cx - half * 0.08, cy - half * 0.34, cx + half * 0.08, cy + half * 0.36, (half * 0.06) as f32, darken(fg, 500), alpha);
                    push_circle(scene, cx, cy - half * 0.28, half * 0.1, lighten(fg), alpha);
                }
                // Crab: a wide oval carapace with two claws that stick
                // out CLEARLY beyond the body's own silhouette, plus
                // four short legs underneath. The old design's own end
                // circles (`half * 0.16` radius) sat barely proud of the
                // body bar's own half-height (`half * 0.12`) and almost
                // flush with its own rounded corners, so once rendered at
                // real size the whole shape read as one elongated bar
                // with barely-there bulges, not claws -- the same
                // "feature too small to survive this tile's own real
                // scale" failure this pass's own enemy-leg work already
                // found and fixed with wider, further-out features (see
                // [`push_radiating_legs`]'s own doc comment); the fix
                // here is the same in kind: bigger claws, set further
                // out past the body's own edge, not flush against it.
                2 => {
                    let body_rx = half * 0.36;
                    let body_ry = half * 0.24;
                    push_radiating_legs(scene, cx, cy, body_ry * 0.7, body_ry * 1.7, 4, 0.9, 0.8, darken(fg, 650), alpha);
                    push_fill(scene, ellipse_path(cx, cy, body_rx, body_ry), FillRule::NonZero, solid(fg, alpha));
                    for dx in [-1.0f64, 1.0] {
                        let claw_x = cx + dx * (body_rx + half * 0.18);
                        let claw_y = cy - half * 0.14;
                        push_circle(scene, claw_x, claw_y, half * 0.22, fg, alpha);
                        push_circle(scene, claw_x + dx * half * 0.1, claw_y, half * 0.09, darken(fg, 500), alpha);
                    }
                }
                // Wisp: a soft radial glow plus a bright core.
                3 => {
                    let glow = radial(vec![stop(0.0, fg, alpha), stop(1.0, fg, 0.0)], cx, cy, half * 0.5);
                    push_fill(scene, circle_path(cx, cy, half * 0.5), FillRule::NonZero, glow);
                    push_circle(scene, cx, cy, half * 0.22, fg, alpha);
                }
                // Default (no evolution chosen yet): a round-eared
                // creature -- body, two ears, a tail -- not the old plus
                // sign (two overlapping bars, `half * 0.12` half-width
                // vertical against `half * 0.42` half-width horizontal).
                // Rendered on the real night-garden ground at real size,
                // that cross read as one dominant horizontal bar with its
                // own vertical arm nearly lost to anti-aliasing at 2-3px
                // wide, not a legible creature -- and this is the pet's
                // OWN default appearance for every run up to wave 4's
                // evolution choice (the plan's own run-loop table), so it
                // has to read as a creature on its own, not lean on a
                // symbol. Three round features (ears, tail) instead of
                // straight bars keep every stroke wide enough to survive
                // this tile's real ~9.5px `half` the same way the enemy
                // legs/lobes above already do.
                _ => {
                    let body_r = half * 0.32;
                    push_circle(scene, cx - body_r * 0.95, cy + body_r * 0.95, body_r * 0.34, fg, alpha);
                    push_fill(scene, ellipse_path(cx, cy + half * 0.05, body_r, body_r * 0.88), FillRule::NonZero, solid(fg, alpha));
                    push_circle(scene, cx - body_r * 0.62, cy - body_r * 0.68, body_r * 0.36, fg, alpha);
                    push_circle(scene, cx + body_r * 0.62, cy - body_r * 0.68, body_r * 0.36, fg, alpha);
                    push_circle(scene, cx, cy + body_r * 0.1, body_r * 0.16, darken(fg, 500), alpha);
                }
            }
        }
        TileId::CircuitLink => {
            push_ring(scene, cx, cy, half * 0.62, half * 0.5, fg, alpha);
        }
        TileId::Projectile => {
            let glow = radial(vec![stop(0.0, Rgb(255, 255, 255), alpha), stop(0.5, fg, alpha), stop(1.0, fg, 0.0)], cx, cy, half * 0.32);
            push_fill(scene, circle_path(cx, cy, half * 0.32), FillRule::NonZero, glow);
            push_circle(scene, cx, cy, half * 0.1, Rgb(255, 255, 255), alpha);
        }
        TileId::ImpactFlash => {
            // Growth over the effect's own lifetime is carried entirely by
            // `DynamicSprite::scale` (the caller grows `(w, h)` itself, see
            // `crate::render::backend_pixel::build_scene`), not by
            // `variant` here -- one growth mechanism, not two
            // independently-tuned ones.
            push_ring(scene, cx, cy, half * 0.55, half * 0.3, fg, alpha);
            push_circle(scene, cx, cy, half * 0.16, Rgb(255, 255, 255), alpha);
            let mut segments = Vec::with_capacity(6);
            for i in 0..6 {
                let a = i as f64 / 6.0 * std::f64::consts::TAU;
                let (inner, outer) = (half * 0.6, half * 0.85);
                segments.push(LineBatchSegment { from: Vec2::new(cx + a.cos() * inner, cy + a.sin() * inner), to: Vec2::new(cx + a.cos() * outer, cy + a.sin() * outer) });
            }
            push_lines(scene, segments, 1.0, Rgb(255, 255, 255), 0.8 * alpha);
        }
        TileId::DeathBurst => {
            let fade = radial(vec![stop(0.0, fg, 0.0), stop(0.7, fg, 0.55 * alpha), stop(1.0, fg, 0.0)], cx, cy, half * 0.68);
            push_ring_brush(scene, cx, cy, half * 0.68, half * 0.46, fade);
        }
        TileId::SplashRing => {
            let wave = radial(vec![stop(0.0, fg, 0.0), stop(0.85, fg, 0.5 * alpha), stop(1.0, fg, 0.0)], cx, cy, half * 0.75);
            push_ring_brush(scene, cx, cy, half * 0.75, half * 0.55, wave);
        }
        TileId::LinkPulse => {
            // Distinct from `ImpactFlash`'s own 6-ray spark and
            // `SplashRing`'s own plain gradient wave: a filled core plus a
            // longer, denser 8-ray burst -- the "отличимый Link Burst"
            // this pass's own brief asked for.
            push_ring(scene, cx, cy, half * 0.55, half * 0.4, fg, alpha);
            push_circle(scene, cx, cy, half * 0.16, fg, alpha);
            let mut segments = Vec::with_capacity(8);
            for i in 0..8 {
                let a = i as f64 / 8.0 * std::f64::consts::TAU;
                let (inner, outer) = (half * 0.4, half * 0.72);
                segments.push(LineBatchSegment { from: Vec2::new(cx + a.cos() * inner, cy + a.sin() * inner), to: Vec2::new(cx + a.cos() * outer, cy + a.sin() * outer) });
            }
            push_lines(scene, segments, 0.9, fg, 0.85 * alpha);
        }
    }

}

/// A soft ambient light pool on the ground under a light-emitting object
/// (a placed tower) -- one fade-to-transparent radial gradient, the exact
/// same "no hard edge, ever" construction [`radial`] already gives every
/// other soft shape in this catalog. This pass's own "подсветка у
/// источников света" depth requirement for towers; [`TileId::Heartseed`]'s
/// own glow already exists as its own `paint_tile` arm and needs no
/// separate helper. See `crate::render::backend_pixel`'s own per-frame
/// overlay pass for the one caller.
pub(super) fn push_ambient_glow(scene: &mut Scene, cx: f64, cy: f64, radius: f64, c: Rgb, alpha: f32) {
    if radius <= 0.0 || alpha <= 0.0 {
        return;
    }
    let glow = radial(vec![stop(0.0, c, alpha), stop(0.6, c, alpha * 0.4), stop(1.0, c, 0.0)], cx, cy, radius);
    push_fill(scene, circle_path(cx, cy, radius), FillRule::NonZero, glow);
}

/// A soft dark contact-shadow ellipse -- this pass's own "мягкие тени под
/// объектами" depth requirement, painted UNDER a body (a tower, an enemy,
/// the pet, the boss) before that body's own sprite. Deliberately a soft
/// fade (never a hard-edged ellipse fill), the same "no hard edge anywhere
/// in this depth layer" reasoning [`push_ambient_glow`] already documents.
pub(super) fn push_soft_shadow(scene: &mut Scene, cx: f64, cy: f64, rx: f64, ry: f64, alpha: f32) {
    if rx <= 0.0 || ry <= 0.0 || alpha <= 0.0 {
        return;
    }
    let dark = Rgb(0, 0, 0);
    let shadow = radial(vec![stop(0.0, dark, alpha), stop(0.7, dark, alpha * 0.5), stop(1.0, dark, 0.0)], cx, cy, rx.max(ry));
    push_fill(scene, ellipse_path(cx, cy, rx, ry), FillRule::NonZero, shadow);
}

/// Darkens the `w x h` canvas toward its own four edges (corners darken
/// twice, from the two overlapping bands that meet there -- the honest
/// "corners read darkest" vignette shape) -- night-garden depth
/// ("виньетирование") that no per-tile primitive above can express, since
/// it reads relative to the CANVAS as a whole, not any one tile.
///
/// Four edge-band `FillRect`s with a LINEAR gradient each, not one
/// full-canvas radial gradient: `bench`'s own `urx_render` binary measured
/// a full-canvas radial vignette at ~3ms of this scene's own ~7ms p50 draw
/// cost -- CPU rasterisation cost here is dominated by the PIXEL COUNT a
/// gradient actually covers, and a full canvas is the single most
/// expensive area this backend ever fills. Four bands sized to
/// [`VIGNETTE_BAND_FRACTION`] of the shorter side cover under half that
/// pixel count while still reading as a real vignette (the visible,
/// perceptually-dominant part of any vignette IS its own edges/corners;
/// the wide, barely-darkened middle a full-canvas radial gradient also
/// pays to compute contributes almost nothing to how it actually looks).
/// Pushed exactly once, last, by [`super::backend_pixel::build_scene`],
/// strictly on top of every tile/sprite/stroke this module's own
/// [`paint_tile`] already painted.
const VIGNETTE_BAND_FRACTION: f64 = 0.15;
const VIGNETTE_ALPHA: f32 = 0.5;

pub(crate) fn paint_vignette(scene: &mut Scene, w: f64, h: f64) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let band = (w.min(h) * VIGNETTE_BAND_FRACTION).max(1.0);
    let dark = Rgb(0, 0, 0);

    let top = linear(vec![stop(0.0, dark, VIGNETTE_ALPHA), stop(1.0, dark, 0.0)], 0.0, 0.0, 0.0, band);
    push_rect_brush(scene, 0.0, 0.0, w, band, 0.0, top);

    let bottom = linear(vec![stop(0.0, dark, 0.0), stop(1.0, dark, VIGNETTE_ALPHA)], 0.0, h - band, 0.0, h);
    push_rect_brush(scene, 0.0, h - band, w, h, 0.0, bottom);

    let left = linear(vec![stop(0.0, dark, VIGNETTE_ALPHA), stop(1.0, dark, 0.0)], 0.0, 0.0, band, 0.0);
    push_rect_brush(scene, 0.0, 0.0, band, h, 0.0, left);

    let right = linear(vec![stop(0.0, dark, 0.0), stop(1.0, dark, VIGNETTE_ALPHA)], w - band, 0.0, w, 0.0);
    push_rect_brush(scene, w - band, 0.0, w, h, 0.0, right);
}

#[cfg(test)]
mod tests {
    use super::*;
    use uzor_urx_cpu::{CpuBackend, Pixmap};

    /// Paints one tile in isolation into a fresh 40x40 pixmap centred at
    /// (20, 20) and returns the rendered straight bytes -- large enough to
    /// hold every shape in this catalog (the boss test below uses a bigger
    /// bounding box, still comfortably inside 40x40) without any edge
    /// clipping skewing the "did this paint something" / "do two kinds
    /// differ" assertions below.
    fn render(tile: TileId, variant: u8, w: f64, h: f64, fg: Rgb, bg: Option<Rgb>) -> Vec<u8> {
        render_dragging(tile, variant, w, h, fg, bg, false)
    }

    fn render_dragging(tile: TileId, variant: u8, w: f64, h: f64, fg: Rgb, bg: Option<Rgb>, dragging: bool) -> Vec<u8> {
        let mut scene = Scene::new();
        paint_tile(&mut scene, tile, variant, fg, bg, 20.0, 20.0, w, h, 1.0, dragging);
        let mut pixmap = Pixmap::new(40, 40);
        CpuBackend::new().render(&scene, &mut pixmap).expect("a hand-built scene from this module's own catalog must never unbalance a clip push/pop");
        pixmap.pixels().to_vec()
    }

    fn has_any_visible_pixel(rgba: &[u8]) -> bool {
        rgba.chunks_exact(4).any(|px| px[3] > 0)
    }

    #[test]
    fn every_tile_id_paints_a_non_empty_sprite() {
        let all = [
            TileId::Ground,
            TileId::Rock,
            TileId::Plant,
            TileId::WaterPool,
            TileId::Firefly,
            TileId::Path,
            TileId::Choke,
            TileId::BuildPad,
            TileId::Heartseed,
            TileId::PetAnchor,
            TileId::TowerNeedle,
            TileId::TowerBell,
            TileId::TowerPrism,
            TileId::TowerEmberNest,
            TileId::TowerMoonwell,
            TileId::TowerRelay,
            TileId::EnemyMite,
            TileId::EnemySkitter,
            TileId::EnemyShellback,
            TileId::EnemySplitter,
            TileId::EnemyHusher,
            TileId::EnemyMirror,
            TileId::BossBellkeeper,
            TileId::BossNightMaw,
            TileId::Pet,
            TileId::CircuitLink,
            TileId::Projectile,
            TileId::ImpactFlash,
            TileId::DeathBurst,
            TileId::SplashRing,
            TileId::LinkPulse,
        ];
        for tile in all {
            let pixels = render(tile, 0, 10.0, 19.0, Rgb(200, 200, 200), Some(Rgb(20, 20, 20)));
            assert!(has_any_visible_pixel(&pixels), "{tile:?} painted a fully transparent sprite");
        }
    }

    #[test]
    fn distinct_tower_kinds_paint_distinct_pixels() {
        let needle = render(TileId::TowerNeedle, 0, 10.0, 19.0, Rgb(200, 200, 200), None);
        let bell = render(TileId::TowerBell, 0, 10.0, 19.0, Rgb(200, 200, 200), None);
        let prism = render(TileId::TowerPrism, 0, 10.0, 19.0, Rgb(200, 200, 200), None);
        assert_ne!(needle, bell, "Needle and Bell must not paint byte-identical sprites");
        assert_ne!(bell, prism, "Bell and Prism must not paint byte-identical sprites");
        assert_ne!(needle, prism, "Needle and Prism must not paint byte-identical sprites");
    }

    #[test]
    fn distinct_enemy_kinds_paint_distinct_pixels() {
        let mite = render(TileId::EnemyMite, 0, 10.0, 19.0, Rgb(170, 220, 120), None);
        let splitter = render(TileId::EnemySplitter, 0, 10.0, 19.0, Rgb(170, 220, 120), None);
        let mirror = render(TileId::EnemyMirror, 0, 10.0, 19.0, Rgb(170, 220, 120), None);
        assert_ne!(mite, splitter);
        assert_ne!(splitter, mirror);
        assert_ne!(mite, mirror);
    }

    #[test]
    fn the_two_boss_kinds_paint_distinct_silhouettes() {
        let bellkeeper = render(TileId::BossBellkeeper, 5, 20.0, 38.0, Rgb(255, 80, 80), Some(Rgb(80, 10, 10)));
        let night_maw = render(TileId::BossNightMaw, 5, 20.0, 38.0, Rgb(255, 80, 80), Some(Rgb(80, 10, 10)));
        assert_ne!(bellkeeper, night_maw);
    }

    #[test]
    fn pet_evolution_variants_paint_distinct_shapes() {
        let base = render(TileId::Pet, 0, 10.0, 19.0, Rgb(203, 166, 247), None);
        let moth = render(TileId::Pet, 1, 10.0, 19.0, Rgb(203, 166, 247), None);
        let crab = render(TileId::Pet, 2, 10.0, 19.0, Rgb(203, 166, 247), None);
        let wisp = render(TileId::Pet, 3, 10.0, 19.0, Rgb(203, 166, 247), None);
        assert_ne!(base, moth);
        assert_ne!(moth, crab);
        assert_ne!(crab, wisp);
    }

    #[test]
    fn a_status_background_actually_changes_an_enemy_sprite() {
        let plain = render(TileId::EnemyMite, 0, 10.0, 19.0, Rgb(170, 220, 120), None);
        let slowed = render(TileId::EnemyMite, 1, 10.0, 19.0, Rgb(170, 220, 120), Some(Rgb(40, 90, 140)));
        assert_ne!(plain, slowed);
    }

    #[test]
    fn link_pulse_and_splash_ring_are_visually_distinct_effects() {
        let link = render(TileId::LinkPulse, 0, 10.0, 19.0, Rgb(203, 166, 247), None);
        let splash = render(TileId::SplashRing, 0, 10.0, 19.0, Rgb(203, 166, 247), None);
        assert_ne!(link, splash, "Link Burst must read as visually distinct from a generic splash wave");
    }

    #[test]
    fn build_pad_only_glows_while_dragging() {
        let quiet = render_dragging(TileId::BuildPad, 0, 10.0, 19.0, Rgb(110, 110, 130), None, false);
        let glowing = render_dragging(TileId::BuildPad, 0, 10.0, 19.0, Rgb(110, 110, 130), None, true);
        assert_ne!(quiet, glowing, "dragging must add a visible glow a non-dragging frame never pays for");
        assert!(has_any_visible_pixel(&quiet), "a non-dragging build pad must still paint its own flat ring");
    }

    /// Both bosses carry their own remaining HP in `TileArt::variant`
    /// (tenths, 0..=10) and both must actually SHOW it -- see
    /// [`boss_vitality_permille`]'s own doc comment for why this was
    /// silently dropped before, and for why the tell is the body's own
    /// light rather than a gauge drawn over it.
    #[test]
    fn a_boss_body_visibly_reacts_to_its_own_remaining_health() {
        // Ink, not equality: `assert_ne!` would pass on a one-pixel
        // difference nobody could see. Coverage is what actually carries
        // this tell at 20x19px (see [`boss_body_scale`]), so the test
        // measures coverage, and measures it as a MONOTONE fall across
        // the whole variant range rather than at two hand-picked points.
        fn ink(rgba: &[u8]) -> usize {
            rgba.chunks_exact(4).filter(|px| px[3] > 0).count()
        }
        for (tile, fg) in [(TileId::BossBellkeeper, Rgb(255, 80, 80)), (TileId::BossNightMaw, Rgb(140, 40, 180))] {
            let coverage: Vec<usize> = (0..=10u8).map(|v| ink(&render(tile, v, 20.0, 19.0, fg, None))).collect();
            for pair in coverage.windows(2) {
                assert!(pair[0] <= pair[1], "{tile:?} must never grow as its own health falls, saw {coverage:?}");
            }
            assert!(
                coverage[10] > coverage[0],
                "{tile:?} at full health must visibly outweigh itself at death's door, saw {coverage:?}"
            );
            assert!(coverage[0] > 0, "{tile:?} must stay visible while dying -- it is what the player is shooting at");
        }
    }

    /// No tower may be drawn as a bare vertical bar, at any level.
    ///
    /// `TowerNeedle` was exactly that, twice: first as a single
    /// `spindle_path` spike, and then again after gaining a crossguard
    /// whose height was set to `half * 0.07` -- 0.66 device pixels at
    /// this tile's real `20x19` size (`backend_pixel`'s own
    /// `PX_PER_CELL_W`/`PX_PER_CELL_H`). That survived at level 0 by
    /// straddling a pixel row and vanished entirely at levels 1 and 2,
    /// where the whole sprite measured two pixels wide. Both times the
    /// regression reached the owner, and both times it passed a review
    /// that looked at one level only -- so this asserts EVERY level of
    /// EVERY tower kind, by counting ink, not by rendering something and
    /// declaring it fine.
    #[test]
    fn no_tower_reads_as_a_bare_vertical_bar_at_any_level() {
        // The widest row a tower's own silhouette must reach. Five pixels
        // out of a twenty-pixel cell is the floor at which a shape stops
        // being a stroke and starts being a body -- comfortably under
        // what every kind here actually draws, so this catches a feature
        // that has DISAPPEARED rather than policing the art direction.
        const MIN_WIDEST_ROW_PX: usize = 5;
        let kinds = [
            (TileId::TowerNeedle, Rgb(180, 220, 255)),
            (TileId::TowerBell, Rgb(255, 220, 140)),
            (TileId::TowerPrism, Rgb(200, 160, 255)),
            (TileId::TowerEmberNest, Rgb(255, 150, 90)),
            (TileId::TowerMoonwell, Rgb(150, 220, 230)),
            (TileId::TowerRelay, Rgb(200, 200, 210)),
        ];
        for (tile, fg) in kinds {
            for level in 0u8..3 {
                let rgba = render(tile, level, 20.0, 19.0, fg, None);
                // `render` rasterises into a 40x40 pixmap with the tile
                // centred at (20, 20) and drawn at 20x19, so the cell
                // itself spans x 10..30, y 10..29.
                let widest = (10..29)
                    .map(|y| (10..30).filter(|x| rgba[(y * 40 + x) * 4 + 3] > 40).count())
                    .max()
                    .unwrap_or(0);
                assert!(
                    widest >= MIN_WIDEST_ROW_PX,
                    "{tile:?} at level {level} is {widest}px across at its widest -- that is a bar, not a tower"
                );
            }
        }
    }

    /// A boss used to lay a full-cell `push_rect` with a zero corner
    /// radius under itself, which is the board's own grid drawn on the
    /// most eye-catching object in the scene -- the exact artefact this
    /// renderer's ground and routes are built to avoid (`background.rs`'s
    /// own module doc). The tile's own corners are where that shows: a
    /// cell-sized rectangle paints them, a falloff cannot reach them.
    #[test]
    fn no_boss_paints_a_cell_aligned_hard_edge() {
        // `render`'s own pixmap is 40x40 with the tile centred at
        // (20, 20) and drawn at 20x19, so the cell's own corners sit at
        // (10, 10) and (29, 28).
        let corners = [(10usize, 10usize), (29, 10), (10, 28), (29, 28)];
        for (tile, fg, bg) in [
            (TileId::BossBellkeeper, Rgb(255, 80, 80), Rgb(70, 20, 20)),
            (TileId::BossNightMaw, Rgb(140, 40, 180), Rgb(40, 15, 55)),
        ] {
            let rgba = render(tile, 10, 20.0, 19.0, fg, Some(bg));
            for (x, y) in corners {
                let a = rgba[(y * 40 + x) * 4 + 3];
                assert_eq!(a, 0, "{tile:?} paints its own cell corner ({x}, {y}) with alpha {a} -- that is a tile-shaped edge, i.e. the grid");
            }
            assert!(has_any_visible_pixel(&rgba), "{tile:?} must still paint a body");
        }
    }

    /// The floor in [`boss_vitality_permille`] exists for exactly this:
    /// a boss dimmed past the luminance of the night-garden ground it
    /// walks on stops reading as a body and starts reading as a hole.
    /// Checked against both bosses' own REAL colours (`gate4agent-arcade-
    /// pet-bastion-render`'s own `boss_color`) and the real ground base
    /// (`background.rs`'s own `env_cell` seed), never a placeholder --
    /// Night Maw's purple is the one that actually fails a careless
    /// floor, and it is the one a test with an arbitrary bright `fg`
    /// would never have caught.
    #[test]
    fn boss_stays_brighter_than_the_ground_it_dies_on() {
        fn luma(Rgb(r, g, b): Rgb) -> f64 {
            0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64
        }
        let ground = luma(Rgb(20, 40, 30));
        for (name, fg) in [("Bellkeeper", Rgb(255, 80, 80)), ("NightMaw", Rgb(140, 40, 180))] {
            let dying = darken(fg, boss_vitality_permille(0));
            assert!(
                luma(dying) > ground * 1.3,
                "{name} at zero health dims to {dying:?} (luma {:.1}), which does not clear the night ground's own {ground:.1} by a readable margin",
                luma(dying)
            );
        }
    }

}
