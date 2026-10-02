# hatchery-arcade

Arcade (mini-game engine + Pet Bastion) is **part of hatchery**, not a
`gate4agent` sibling and not part of g4a. g4a stays pure node + C2 +
provider tools.

## Status (2026-10-02)

Sources are vendored in-tree (owner push `aef255e`). Cargo package ids are
`hatchery-arcade-*` (renamed from the former local sibling's
`gate4agent-arcade-*`). Rust imports use `hatchery_arcade_*`.

Members are folded into the **root** hatchery workspace (no nested
`[workspace]` here). Check with `cargo check --workspace` from the repo
root, or `-p hatchery-arcade-engine` etc.

```
hatchery-arcade/
  engine/                    # hatchery-arcade-engine
  games/pet-bastion/         # hatchery-arcade-pet-bastion
  games/pet-bastion-render/  # hatchery-arcade-pet-bastion-render
  sweep/ preview/ bench/
```

## Consumers

- `crates/hatchery-tui` — Pet Bastion overlay (`src/pet_arcade.rs`); path
  deps point here (`../hatchery-arcade/...`).
