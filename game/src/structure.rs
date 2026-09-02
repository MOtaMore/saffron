//! The shareable `.json` structure format, plus the **structure library** the
//! base game scans at startup so `worldgen` can spawn these builds naturally.
//! Authored in the standalone editor (`editor.rs`).
//!
//! ```jsonc
//! { "format": "saffron-structure", "version": 1, "name": "house",
//!   "size": [x, y, z], "author": "", "notes": "",
//!   "spawn": { "weight": 0.0, "min_y": 42, "max_y": 120, "max_slope": 4,
//!              "sink": 1, "clear": true, "fill_below": true },
//!   "blocks": [ [[0,0,0], "Stone"], [[0,1,0], "WoodPlanks"] ] }
//! ```
//! `blocks` coords are relative to the box's min corner (`0..size` per axis);
//! `Air` is omitted. `"BlockName"` is a `block::Block` variant name (`Wood` is
//! the log, `WoodPlanks` the plank block). `spawn.weight` = 0 → the structure is
//! never generated in the world (edit it, or set it in the editor).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::math::IVec3;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::Block;
use crate::streaming::ChunkWorld;

pub const FORMAT: &str = "saffron-structure";
pub const VERSION: u32 = 1;
/// A captured selection may not exceed this on any axis.
pub const MAX_DIM: i32 = 128;

/// Where the base game looks for structures (both scanned at startup).
pub const BUNDLED_DIR: &str = "assets/structures";
pub const USER_DIR: &str = "structures";

/// How a structure is allowed to appear during world generation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpawnRule {
    /// Relative frequency vs. other structures. **0 = never spawns.**
    #[serde(default)]
    pub weight: f32,
    /// Only where the ground surface sits within `[min_y, max_y]`.
    #[serde(default = "d_min_y")]
    pub min_y: i32,
    #[serde(default = "d_max_y")]
    pub max_y: i32,
    /// Skip if the surface varies more than this across the footprint (cliffs).
    #[serde(default = "d_slope")]
    pub max_slope: i32,
    /// Sink the structure this many blocks into the ground (foundations).
    #[serde(default = "d_sink")]
    pub sink: i32,
    /// Hollow the footprint volume before stamping (clean interiors).
    #[serde(default = "d_true")]
    pub clear: bool,
    /// Fill dirt pillars from the structure's underside down to the terrain.
    #[serde(default = "d_true")]
    pub fill_below: bool,
}

fn d_min_y() -> i32 {
    42
}
fn d_max_y() -> i32 {
    120
}
fn d_slope() -> i32 {
    4
}
fn d_sink() -> i32 {
    1
}
fn d_true() -> bool {
    true
}

impl Default for SpawnRule {
    fn default() -> Self {
        Self {
            weight: 0.0,
            min_y: d_min_y(),
            max_y: d_max_y(),
            max_slope: d_slope(),
            sink: d_sink(),
            clear: true,
            fill_below: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Structure {
    pub format: String,
    pub version: u32,
    pub name: String,
    /// `[x, y, z]` extents.
    pub size: [i32; 3],
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub spawn: SpawnRule,
    pub blocks: Vec<([i32; 3], Block)>,
}

impl Structure {
    pub fn to_pretty_json(&self) -> Option<String> {
        serde_json::to_string_pretty(self).ok()
    }

    pub fn from_json(text: &str) -> Option<Structure> {
        serde_json::from_str::<Structure>(text)
            .ok()
            .filter(|s| s.format == FORMAT)
    }
}

// === Library (scanned once at startup for worldgen) ==================

/// Structures with `spawn.weight > 0`, ready for weighted picking. Immutable and
/// cheap to `Arc`-share into `WorldGen`.
#[derive(Default)]
pub struct Library {
    pub entries: Vec<Structure>,
    /// Cumulative weights aligned with `entries`.
    cum: Vec<f32>,
    total: f32,
}

impl Library {
    pub(crate) fn build(all: impl IntoIterator<Item = Structure>) -> Self {
        let mut lib = Library::default();
        for s in all {
            let w = s.spawn.weight;
            if w.is_finite() && w > 0.0 {
                lib.total += w;
                lib.entries.push(s);
                lib.cum.push(lib.total);
            }
        }
        lib
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Pick a structure for `r` in `[0, 1)`.
    pub fn pick(&self, r: f32) -> Option<&Structure> {
        if self.entries.is_empty() {
            return None;
        }
        let target = r.rem_euclid(1.0) * self.total;
        let i = self
            .cum
            .partition_point(|&c| c < target)
            .min(self.entries.len() - 1);
        Some(&self.entries[i])
    }
}

/// Bevy resource wrapping the shared [`Library`].
#[derive(Resource, Clone)]
pub struct StructureLibrary(pub Arc<Library>);

/// Scan `assets/structures/*.json` + `structures/*.json`.
pub fn scan_library() -> Library {
    let mut found: Vec<Structure> = Vec::new();
    for dir in [BUNDLED_DIR, USER_DIR] {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p: PathBuf = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(s) = load_structure(&p) {
                found.push(s);
            }
        }
    }
    Library::build(found)
}

pub struct StructureLibraryPlugin;

impl Plugin for StructureLibraryPlugin {
    fn build(&self, app: &mut App) {
        let lib = scan_library();
        if !lib.is_empty() {
            info!(
                "structure library: {} spawnable structure(s)",
                lib.entries.len()
            );
        }
        app.insert_resource(StructureLibrary(Arc::new(lib)));
    }
}

/// Read a `structures/*.json` (or any path) into a [`Structure`].
pub fn load_structure(path: impl AsRef<Path>) -> Option<Structure> {
    Structure::from_json(&std::fs::read_to_string(path).ok()?)
}

/// Build a [`Structure`] from a sparse cell map, re-based so the min corner is
/// `[0,0,0]`. `Air` cells are dropped.
pub fn from_cells(cells: &HashMap<IVec3, Block>, name: &str) -> Result<Structure, String> {
    let solid: Vec<(IVec3, Block)> = cells
        .iter()
        .filter(|(_, b)| **b != Block::Air)
        .map(|(p, b)| (*p, *b))
        .collect();
    if solid.is_empty() {
        return Err("nothing to save".into());
    }
    let mn = solid.iter().fold(IVec3::MAX, |a, (p, _)| a.min(*p));
    let mx = solid.iter().fold(IVec3::MIN, |a, (p, _)| a.max(*p));
    let size = mx - mn + IVec3::ONE;
    if size.x > MAX_DIM || size.y > MAX_DIM || size.z > MAX_DIM {
        return Err(format!("structure too big (max {MAX_DIM} per axis)"));
    }
    let mut blocks: Vec<([i32; 3], Block)> = solid
        .into_iter()
        .map(|(p, b)| ((p - mn).to_array(), b))
        .collect();
    blocks.sort_by_key(|(c, _)| (c[1], c[2], c[0]));
    Ok(Structure {
        format: FORMAT.into(),
        version: VERSION,
        name: name.into(),
        size: [size.x, size.y, size.z],
        author: String::new(),
        notes: String::new(),
        spawn: SpawnRule::default(),
        blocks,
    })
}

impl Structure {
    /// The structure as a cell map keyed by relative coordinate.
    pub fn to_cells(&self) -> HashMap<IVec3, Block> {
        self.blocks
            .iter()
            .map(|&([x, y, z], b)| (IVec3::new(x, y, z), b))
            .collect()
    }
}

/// Copy the block box between world cells `a` and `b` (inclusive) into a
/// [`Structure`]. `Air` and `Bedrock` are skipped. (Dev helper — not used by the
/// editor, which works from a cell map, but handy for baking builds from a live
/// world.)
#[allow(dead_code)]
pub fn capture(world: &ChunkWorld, a: IVec3, b: IVec3, name: &str) -> Result<Structure, String> {
    let (mn, mx) = (a.min(b), a.max(b));
    let size = mx - mn + IVec3::ONE;
    if size.x > MAX_DIM || size.y > MAX_DIM || size.z > MAX_DIM {
        return Err(format!("selection too big (max {MAX_DIM} per axis)"));
    }
    let mut blocks = Vec::new();
    for x in mn.x..=mx.x {
        for y in mn.y..=mx.y {
            for z in mn.z..=mx.z {
                match world.get_loaded(x, y, z) {
                    Some(Block::Air) | Some(Block::Bedrock) | None => {}
                    Some(block) => blocks.push(([x - mn.x, y - mn.y, z - mn.z], block)),
                }
            }
        }
    }
    if blocks.is_empty() {
        return Err("selection has no blocks".into());
    }
    Ok(Structure {
        format: FORMAT.into(),
        version: VERSION,
        name: name.into(),
        size: [size.x, size.y, size.z],
        author: String::new(),
        notes: String::new(),
        spawn: SpawnRule::default(),
        blocks,
    })
}

/// Stamp a structure into the world with its min corner at `origin`. Returns the
/// number of blocks written. Goes through `ChunkWorld::set_block`, so it only
/// affects already-loaded chunks, never the bedrock floor, and replicates in
/// multiplayer / persists on a dedicated server. **This is the entry point for
/// placing editor structures in the base game / a mod** (call it from worldgen,
/// a command, a startup system, …).
#[allow(dead_code)]
pub fn stamp_structure(world: &mut ChunkWorld, origin: IVec3, s: &Structure) -> usize {
    let mut n = 0;
    for &([x, y, z], block) in &s.blocks {
        if block == Block::Air {
            continue;
        }
        let p = origin + IVec3::new(x, y, z);
        if world.set_block(p.x, p.y, p.z, block) {
            n += 1;
        }
    }
    n
}
