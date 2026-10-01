# hatchery-arcade (sources missing)

Arcade (mini-game engine + Pet Bastion) is **part of hatchery**, not a
`gate4agent` sibling and not part of g4a. g4a stays pure node + C2 +
provider tools.

## Status (2026-10-01 / 2026-10-02)

The working tree that lived as a local sibling `../gate4agent-arcade`
(own git repo, never published to GitHub under `ZENG3LD`) is **not in
this checkout**. Hatchery history never vendored those crates — only
`hatchery-tui` path-deps pointed at the sibling.

This directory is the **canonical in-tree home**. Layout matches the
former sibling:

```
hatchery-arcade/
  engine/                 # package was gate4agent-arcade-engine
  games/pet-bastion/      # package was gate4agent-arcade-pet-bastion
  games/pet-bastion-render/
  sweep/ preview/ bench/
```

## What to restore

1. Copy/vend the missing arcade sources into the dirs above (or recover
   from the owner's local `nemo/gate4agent-arcade` checkout).
2. Keep or rename Cargo package ids (`gate4agent-arcade-*` →
   `hatchery-arcade-*`) in one pass with the TUI import paths
   (`gate4agent_arcade_*` → `hatchery_arcade_*`).
3. Add the packages to the root workspace `members` (or keep TUI nested
   workspace depending on them by path) and drop this README's "missing"
   banner.
4. `cargo check` inside `crates/hatchery-tui` (nested workspace).

Until then: root workspace **excludes** `crates/hatchery-tui` and does
**not** list arcade members — harness/observation still check without
arcade. TUI will not build.

## Consumers

- `crates/hatchery-tui` — Pet Bastion overlay (`src/pet_arcade.rs`); path
  deps point here (`../hatchery-arcade/...`), not `../../../gate4agent-arcade`.
