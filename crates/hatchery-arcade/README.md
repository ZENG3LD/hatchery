# hatchery-arcade

Arcade (mini-game engine + Pet Bastion) is **part of hatchery**, not a
`gate4agent` sibling and not part of g4a. g4a stays pure node + C2 +
provider tools.

## Status (2026-10-02)

Sources are vendored in-tree (owner push `aef255e`). Cargo package ids are
`hatchery-arcade-*` (renamed from the former local sibling's
`gate4agent-arcade-*`). Rust imports use `hatchery_arcade_*`.

```
hatchery-arcade/
  engine/                    # hatchery-arcade-engine
  games/pet-bastion/         # hatchery-arcade-pet-bastion
  games/pet-bastion-render/  # hatchery-arcade-pet-bastion-render
  sweep/ preview/ bench/
```

This directory is a **nested Cargo workspace** (see `Cargo.toml`). The root
hatchery workspace keeps `crates/hatchery-arcade` and `crates/hatchery-tui`
in `exclude` so harness/observation stay independent; check arcade with
`cargo check` from this directory, and TUI from `crates/hatchery-tui`.

## Consumers

- `crates/hatchery-tui` — Pet Bastion overlay (`src/pet_arcade.rs`); path
  deps point here (`../hatchery-arcade/...`).
