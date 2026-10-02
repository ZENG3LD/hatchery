//! The concrete tile-identity catalog. Named per Pet Bastion: Night Garden
//! (`hatchery-arcade-pet-bastion-render`'s own `snapshot_to_surface`
//! adapter is the only place that constructs [`TileArt`](crate::render::TileArt)
//! values against this enum), since it is this arcade's first, and so far
//! only, hosted mini-game. A future second game growing its own tile
//! vocabulary is the point at which this enum earns a real per-game
//! namespace (or the `tools/bake_tiles.py`-generated catalog the render
//! module's own doc comment already anticipates) -- there is no second
//! game to design that split against yet, so this pass does not invent
//! one.
//!
//! No backend in this pass reads a BAKED ASSET through this type (there is
//! still no `tools/bake_tiles.py`, no `include_bytes!` catalog) --
//! [`crate::render::backend_sixel::SixelBackend`] is the one backend that
//! reads the ENUM VARIANT ITSELF (not a baked bitmap) to pick a procedural
//! shape per tile kind; `GlyphBackend`/`HalfBlockBackend` never read `art`
//! at all, only `SurfaceCell::glyph`/`fg`/`bg` (see each backend's own doc
//! comment).

/// One board-tile's identity, independent of its current visual state
/// (colour/level/link/slow -- those live in [`crate::render::TileArt`]'s
/// own `variant` byte, not as further `TileId` variants, so this enum
/// stays a plain kind catalog).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TileId {
    // -- Terrain -----------------------------------------------------
    /// The night-garden ground itself -- painted onto EVERY board tile
    /// first, before any other terrain/decor/unit pass, so a normal frame
    /// never has a genuinely empty (fully transparent) cell: see
    /// `hatchery-arcade-pet-bastion-render`'s own `paint_terrain` doc
    /// comment for why a dense scene needs a real base layer under
    /// everything, not just under the tiles that happen to carry gameplay
    /// meaning. `TileArt::variant` (0..=3) selects one of a handful of
    /// deterministic tone/shade variants (hashed from the tile's own
    /// coordinates), so neighbouring tiles read as a textured surface with
    /// tonal variation instead of one flat fill repeated 392 times.
    Ground,
    /// A small decorative rock -- part of the deterministic, per-tile-seed
    /// static decor scatter (`paint_terrain`'s own doc comment); painted
    /// over [`TileId::Ground`], itself overridden by any real terrain/unit
    /// tile that later lands on the same cell (a route, an anchor, a
    /// tower, ...).
    Rock,
    /// A small decorative plant/fern tuft -- same static-decor scatter and
    /// override rules as [`TileId::Rock`].
    Plant,
    /// A still garden pool -- placed only in the small ring of background
    /// tiles immediately around [`TileId::Heartseed`], the "сток" (drain)
    /// the plan's own board description calls for: every route visually
    /// empties into this pool before reaching the Heartseed itself, not
    /// just abstractly "ending" at a bare marker tile.
    WaterPool,
    /// A single static firefly glow -- same static-decor scatter and
    /// override rules as [`TileId::Rock`]/[`TileId::Plant`]; deliberately
    /// static (a fixed position per tile, chosen once from the tile's own
    /// coordinates), not an animated/blinking sprite, per this pass's own
    /// "детерминированно... чтобы он не мигал между кадрами" requirement.
    Firefly,
    /// One tile of either route's own unique legs, before the two routes
    /// merge (`board::Board::new`'s own route waypoints).
    Path,
    /// One tile of the SHARED final leg both routes merge into before
    /// reaching [`TileId::Heartseed`] -- `board.rs`'s own "two routes
    /// merging into one final choke" (its own module doc, and its own
    /// `routes_merge_at_the_same_final_choke` test).
    Choke,
    /// A free build-zone tile, highlighted ONLY while the owner is
    /// actively dragging a tower from the palette -- see
    /// `hatchery-arcade-pet-bastion-render`'s own `paint_build_zone_highlight`
    /// doc comment for why this is never part of the ordinary per-tick
    /// `snapshot_to_surface` pass any more (a real board carries roughly
    /// as many buildable tiles as terrain tiles; painting every one of
    /// them, always, is exactly the "ковёр из одинаковых кружков" this
    /// pass's own brief asked to remove). Overridden by whichever `Tower*`
    /// variant below once a tower is actually placed on it.
    BuildPad,
    /// The single tile every route ends at (`board::HEARTSEED`).
    Heartseed,
    /// An unoccupied pet anchor (`board::ANCHORS`). Overridden by
    /// [`TileId::Pet`] whenever the pet is currently AT this anchor.
    PetAnchor,

    // -- Towers (level: `TileArt::variant` 0=Base, 1=L2, 2=L3) --------
    TowerNeedle,
    TowerBell,
    TowerPrism,
    TowerEmberNest,
    TowerMoonwell,
    TowerRelay,

    // -- Enemies (status: `TileArt::variant` 0=normal, 1=slowed, 2=stunned) --
    EnemyMite,
    EnemySkitter,
    EnemyShellback,
    EnemySplitter,
    EnemyHusher,
    EnemyMirror,

    // -- Boss (`TileArt::variant` = remaining HP in tenths, 0..=10) ---
    BossBellkeeper,
    BossNightMaw,

    // -- Pet (`TileArt::variant`: 0=none, 1=Moth, 2=Crab, 3=Wisp) -----
    Pet,

    // -- Effects -------------------------------------------------------
    /// The Living Circuit's own visual tell: a small halo of tiles around
    /// the pet's current anchor, painted whenever [`PetView::
    /// linked_towers`](../../../hatchery_arcade_pet_bastion/snapshot/struct.PetView.html)
    /// is non-empty. See `hatchery-arcade-pet-bastion-render`'s own
    /// adapter doc for why this halo, rather than a literal line traced
    /// from the anchor to every linked tower, is the honest choice given
    /// `Surface`'s one-glyph-per-tile contract (a traced line would have
    /// to either overwrite unrelated terrain tiles it happens to cross,
    /// or silently skip them, both of which read as a rendering DEFECT
    /// rather than a deliberate design choice).
    CircuitLink,

    // -- Combat effects (transient, driven by `SimEvent`, never persistent
    // sim state -- see `crate::render::pixel::DynamicSprite`/`DynamicStroke`
    // and `hatchery-arcade-pet-bastion-render`'s own `effects` module) --
    /// A flying shot's own bright leading point (paired with a
    /// [`crate::render::pixel::DynamicStroke`] trail from the firing
    /// tower).
    Projectile,
    /// A brief burst at a hit's own resolved position.
    ImpactFlash,
    /// An expanding, fading ring at a killed enemy's own last position.
    DeathBurst,
    /// An expanding, fading ring for an area-effect wave (Ember Nest splash,
    /// Moonwell's field) centred on its own origin.
    SplashRing,
    /// A double-ring pulse at a Link-Burst-triggering tower's own position.
    LinkPulse,
}

impl TileId {
    /// Whether this kind is a CONTINUOUSLY-positioned entity a pixel-tier
    /// renderer composites as a [`crate::render::pixel::DynamicSprite`] at
    /// its own sub-tile position, rather than a tile-aligned
    /// [`crate::render::Surface`] cell -- see `crate::render::pixel`'s own
    /// "Why a sibling buffer, not a `Surface` extension" doc section. A
    /// pixel-tier backend's own static layer (terrain, towers, build pads,
    /// the Circuit halo) skips every tile for which this returns `true`;
    /// everything else it paints straight off `Surface`, exactly as the
    /// glyph/half-block/per-tile-sixel tiers already do.
    pub fn is_dynamic_entity(self) -> bool {
        matches!(
            self,
            TileId::EnemyMite
                | TileId::EnemySkitter
                | TileId::EnemyShellback
                | TileId::EnemySplitter
                | TileId::EnemyHusher
                | TileId::EnemyMirror
                | TileId::BossBellkeeper
                | TileId::BossNightMaw
                | TileId::Pet
                | TileId::Projectile
                | TileId::ImpactFlash
                | TileId::DeathBurst
                | TileId::SplashRing
                | TileId::LinkPulse
        )
    }

    /// Whether this kind belongs to the board's own CONTINUOUS, cached
    /// background layer -- terrain that is a pure function of the board's
    /// own fixed layout and never changes for the lifetime of a run (see
    /// `crate::render::background`'s own module doc comment for the full
    /// seed/cache-invalidation contract this classification exists to
    /// support). A pixel-tier backend's own per-frame overlay pass
    /// (`crate::render::backend_pixel`'s own `paint_overlay_layer`) skips
    /// every tile for which this returns `true` -- that content was
    /// already painted once into the cached [`crate::render::background::
    /// BoardBackground`] every frame composites back in via a cheap
    /// `Pixmap` clone, never repainted tile by tile.
    ///
    /// Deliberately EXCLUSIVE of [`TileId::is_dynamic_entity`]'s own
    /// `true` set (a caller always checks that one FIRST and separately),
    /// and deliberately EXCLUDES everything whose STATE can change mid-run
    /// even though its own POSITION never moves: [`TileId::BuildPad`]
    /// toggles on and off with a drag gesture, every `TileId::Tower*` and
    /// [`TileId::CircuitLink`] can change level/link/appear/disappear
    /// across a run -- all three stay in the per-frame overlay pass
    /// instead, painted fresh from `Surface` every call exactly as before.
    pub fn is_board_environment(self) -> bool {
        matches!(
            self,
            TileId::Ground
                | TileId::Rock
                | TileId::Plant
                | TileId::WaterPool
                | TileId::Firefly
                | TileId::Path
                | TileId::Choke
                | TileId::Heartseed
                | TileId::PetAnchor
        )
    }

    /// Whether this kind is one of [`TileId::TowerNeedle`]'s own five
    /// siblings -- a placed, levelled tower. Used by `crate::render::
    /// backend_pixel`'s own per-frame overlay pass to decide which tiles
    /// get an extra soft ambient light pool on the ground underneath them
    /// (this pass's own "подсветка у источников света" depth requirement;
    /// [`TileId::Heartseed`]'s own glow is baked straight into the cached
    /// background instead, since it never moves or changes).
    pub fn is_tower(self) -> bool {
        matches!(self, TileId::TowerNeedle | TileId::TowerBell | TileId::TowerPrism | TileId::TowerEmberNest | TileId::TowerMoonwell | TileId::TowerRelay)
    }

    /// Whether a pixel-tier backend should drop a soft contact shadow on
    /// the ground under this kind's own dynamic sprite -- every grounded
    /// creature (enemy/boss/pet), never a transient combat EFFECT
    /// (`Projectile`/`ImpactFlash`/`DeathBurst`/`SplashRing`/`LinkPulse`)
    /// or the halo-only [`TileId::CircuitLink`], none of which read as a
    /// physical body standing on the ground. This pass's own "мягкие тени
    /// под объектами" depth requirement; see `crate::render::
    /// backend_pixel::build_scene`'s own dynamic-sprite loop for the one
    /// caller.
    pub fn casts_ground_shadow(self) -> bool {
        matches!(
            self,
            TileId::EnemyMite
                | TileId::EnemySkitter
                | TileId::EnemyShellback
                | TileId::EnemySplitter
                | TileId::EnemyHusher
                | TileId::EnemyMirror
                | TileId::BossBellkeeper
                | TileId::BossNightMaw
                | TileId::Pet
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_and_tower_kinds_are_never_dynamic() {
        for tile in [
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
            TileId::TowerRelay,
            TileId::CircuitLink,
        ] {
            assert!(!tile.is_dynamic_entity(), "{tile:?} must stay on the static layer");
        }
    }

    #[test]
    fn every_moving_entity_and_effect_kind_is_dynamic() {
        for tile in [
            TileId::EnemyMite,
            TileId::EnemySkitter,
            TileId::EnemyShellback,
            TileId::EnemySplitter,
            TileId::EnemyHusher,
            TileId::EnemyMirror,
            TileId::BossBellkeeper,
            TileId::BossNightMaw,
            TileId::Pet,
            TileId::Projectile,
            TileId::ImpactFlash,
            TileId::DeathBurst,
            TileId::SplashRing,
            TileId::LinkPulse,
        ] {
            assert!(tile.is_dynamic_entity(), "{tile:?} must be composited via the dynamic sprite path");
        }
    }

    #[test]
    fn environment_kinds_are_the_cached_background_never_dynamic_and_never_overlay_only() {
        for tile in [
            TileId::Ground,
            TileId::Rock,
            TileId::Plant,
            TileId::WaterPool,
            TileId::Firefly,
            TileId::Path,
            TileId::Choke,
            TileId::Heartseed,
            TileId::PetAnchor,
        ] {
            assert!(tile.is_board_environment(), "{tile:?} must be part of the cached background");
            assert!(!tile.is_dynamic_entity(), "{tile:?} is board environment, must never also be a dynamic entity");
        }
    }

    #[test]
    fn state_changing_board_content_is_never_board_environment() {
        for tile in [TileId::BuildPad, TileId::TowerNeedle, TileId::TowerBell, TileId::TowerPrism, TileId::TowerEmberNest, TileId::TowerMoonwell, TileId::TowerRelay, TileId::CircuitLink] {
            assert!(!tile.is_board_environment(), "{tile:?} can change mid-run, must stay in the per-frame overlay pass");
        }
    }

    #[test]
    fn every_dynamic_entity_is_exactly_one_of_environment_or_neither_never_both() {
        for tile in [
            TileId::EnemyMite,
            TileId::EnemySkitter,
            TileId::EnemyShellback,
            TileId::EnemySplitter,
            TileId::EnemyHusher,
            TileId::EnemyMirror,
            TileId::BossBellkeeper,
            TileId::BossNightMaw,
            TileId::Pet,
            TileId::Projectile,
            TileId::ImpactFlash,
            TileId::DeathBurst,
            TileId::SplashRing,
            TileId::LinkPulse,
        ] {
            assert!(!tile.is_board_environment(), "{tile:?} is dynamic, must never also be board environment");
        }
    }

    #[test]
    fn only_the_six_tower_kinds_report_is_tower() {
        for tile in [TileId::TowerNeedle, TileId::TowerBell, TileId::TowerPrism, TileId::TowerEmberNest, TileId::TowerMoonwell, TileId::TowerRelay] {
            assert!(tile.is_tower(), "{tile:?} must report is_tower");
        }
        for tile in [TileId::Ground, TileId::BuildPad, TileId::Heartseed, TileId::Pet, TileId::EnemyMite, TileId::CircuitLink] {
            assert!(!tile.is_tower(), "{tile:?} must not report is_tower");
        }
    }

    #[test]
    fn only_grounded_creatures_cast_a_ground_shadow() {
        for tile in [TileId::EnemyMite, TileId::EnemySkitter, TileId::EnemyShellback, TileId::EnemySplitter, TileId::EnemyHusher, TileId::EnemyMirror, TileId::BossBellkeeper, TileId::BossNightMaw, TileId::Pet] {
            assert!(tile.casts_ground_shadow(), "{tile:?} must cast a ground shadow");
        }
        for tile in [TileId::Projectile, TileId::ImpactFlash, TileId::DeathBurst, TileId::SplashRing, TileId::LinkPulse, TileId::CircuitLink, TileId::Ground, TileId::TowerNeedle] {
            assert!(!tile.casts_ground_shadow(), "{tile:?} must not cast a ground shadow");
        }
    }
}
