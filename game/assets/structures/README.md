# Bundled structures

`.json` structures here are part of the game and **generate in the world**
(see `src/structure.rs` + `src/worldgen.rs`). Put a structure here — with
`"spawn": { "weight": <n>, ... }` where `weight > 0` — and it will appear
naturally during world generation, seed-deterministic.

- `weight` — relative frequency vs. other bundled structures (`0` = never).
- `min_y` / `max_y` — only where the ground surface is in this range.
- `max_slope` — skip if the surface varies more than this across the footprint.
- `sink` — blocks to bury the structure for foundations.
- `clear` — hollow the footprint before stamping (clean interiors).
- `fill_below` — dirt pillars from the underside down to the terrain.

Author them in the **Structure Editor** (menu → *Structure Editor*), set the
`w+ / w-` (weight) and `sink` controls, `Save file` (writes to
`game/structures/`), then move the finished `.json` here and commit it.

`game/structures/` (the editor's own output folder) is also scanned, so local
designs with `weight > 0` spawn too — but in **multiplayer** every machine must
have the same files, so commit shared structures here rather than leaving them in
`game/structures/`.

One structure per ~128×128-block *region*, and only ~13% of regions attempt one
(`worldgen::STRUCT_PER_MIL`) before the slope / Y-range checks — tune per
structure with `weight`, or the global rate in `worldgen.rs`.
