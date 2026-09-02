# Structure Editor & the `.json` structure format

The **Structure Editor** is a dedicated mode of the game (`src/editor.rs`,
`GameFlow::Editor`, reached from the menu button *Structure Editor*). It is not
the survival game: no player, no hunger/health, no hotbar — just a free-fly
camera and a content browser for authoring builds and exporting them as JSON
that the base game / mods can place.

## Controls

### Camera

| Input | Action |
|---|---|
| Right-mouse (hold) | Look around |
| WASD | Fly (relative to look) |
| Space / Ctrl | Up / down |
| Shift | Fly faster |
| Mouse wheel | Fly speed |
| Esc | Back to the menu |

### Build tool

| Input | Action |
|---|---|
| Left-click | Place the brush block on the pointed face (or 8 m ahead in empty space) |
| Alt + left-click, or the `Erase` brush | Delete the pointed block |

### Select tool

| Input | Action |
|---|---|
| Left-click | Pick one block |
| Shift + left-click | Toggle a block in/out of the group |
| Ctrl + left-click | Select everything |
| Arrow keys | Move the selection ±1 on X / Z |
| PageUp / PageDown | Move the selection ±1 on Y |
| Delete | Remove the selected blocks |

The left panel is the **content browser**: a swatch for every block plus every
model prop (Workbench, Chest, Furnace, Hand Mill, Torch) — click to pick the
brush. Buttons: `Build` / `Select` / `Textured` toggle, `Select all` /
`Clear sel` / `Delete`, `Erase brush`, `Save file`, `Copy/Paste JSON`,
`Clear all`, `Rename`, and a list of `structures/*.json` (click to load and keep
editing). A wireframe box shows the build's bounds; selected cells are outlined
yellow; a faint grid marks `y = 0`.

`Textured` switches plain blocks between the real block-atlas texture and a
flat-colour preview (the atlas UV mapping in the editor is approximate). Model
props always load their real GLB. Either way the exported `.json` is
texture-agnostic — the base game renders it correctly when stamped.

## File format

```jsonc
{
  "format": "saffron-structure",
  "version": 1,
  "name": "my_house",
  "size": [x, y, z],          // box extents
  "author": "",               // optional free text
  "notes": "",                // optional free text
  "blocks": [
    [[0, 0, 0], "Stone"],     // [ [x,y,z] relative to the min corner, "BlockName" ]
    [[1, 0, 0], "Stone"],
    [[0, 1, 0], "WoodPlanks"]
    // Air is omitted; coords run 0..size on each axis
  ]
}
```

`"BlockName"` is a `Block` enum variant name (`src/block.rs`): `Air, Grass, Dirt,
Stone, Sand, Snow, Water, Wood, Leaves, Bedrock, Gravel, Workbench, Chest,
Furnace, Glass, WoodPlanks, Torch, Farmland, WheatCrop, HandMill`. `Wood` is the
log; `WoodPlanks` the plank block. Editor exports skip `Air` and `Bedrock`.

Files live in `game/structures/` (git-ignored — per-machine designs). Commit
finished, official ones under `game/assets/structures/`.

## Worldgen spawning

`structure::StructureLibraryPlugin` scans both folders at startup into a
`StructureLibrary` shared into `WorldGen`. `worldgen::stamp_structures` (called
from `generate`) splits the world into **4×4-chunk regions**; each region rolls a
deterministic dice (seed + region coords, `STRUCT_PER_MIL ≈ 13 %`), picks a
structure weighted by `spawn.weight`, anchors it *inside* the region (footprint
never crosses a region boundary — so any structure ≤ 128 on a side is safe),
checks the `spawn` gates, then stamps its slice into each overlapping chunk.

Everything is seed-deterministic, so every client and every reload generates the
same layout with **no networking and no save state** — player edits layer on top
via `ChunkWorld.edits` exactly like terrain.

`spawn` fields (`#[serde(default)]`, so old files still parse):

| Field | Default | Meaning |
|---|---|---|
| `weight` | `0.0` | Relative frequency. **0 = never generates.** |
| `min_y` / `max_y` | `42` / `120` | Only where the ground surface is in this range |
| `max_slope` | `4` | Skip if the surface varies more than this across the footprint |
| `sink` | `1` | Blocks to bury the structure (foundations) |
| `clear` | `true` | Hollow the footprint volume before stamping |
| `fill_below` | `true` | Dirt pillars from the underside down to the terrain (patches dips) |

**Multiplayer:** every machine must have the same structure files (builds must
match anyway) — commit shared ones to `game/assets/structures/` rather than
leaving them in the editor's `game/structures/`.

To make a design spawn: open it in the editor, bump `w+`, `Save file`, then move
the `.json` to `game/assets/structures/`. Tune the global rate in
`worldgen::STRUCT_PER_MIL` / `REGION_CHUNKS`.

## Using a structure in the base game / a mod

`src/structure.rs` is a plain module (no ECS) exposing:

```rust
use crate::structure::{Structure, load_structure, stamp_structure, from_cells};

// e.g. from worldgen, a startup system, a command, a mod:
if let Some(s) = load_structure("assets/structures/well.json") {
    let placed = stamp_structure(&mut world, IVec3::new(x, y, z), &s);
    // `origin` is where the structure's min corner lands; returns blocks written
}
```

- `stamp_structure` writes through `ChunkWorld::set_block` (loaded chunks only,
  never the bedrock floor; replicates in multiplayer, persists on a server).
  For generation-time placement, walk `s.blocks` into `ChunkData` directly while
  the chunk is being built — the palette is just block-name strings.
- `Structure::{from_json, to_pretty_json, to_cells}` and `from_cells(&HashMap
  <IVec3, Block>, name)` for clipboard / programmatic use.
- `capture(&world, a, b, name)` bakes a box straight out of a live world.
