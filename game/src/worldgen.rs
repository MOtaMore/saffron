//! Procedural terrain generation.
//!
//! Purely functional: `WorldGen::generate` takes a chunk coordinate and returns
//! fully-populated `ChunkData`, so it can run on a background task pool.
//!
//! Features: rolling base terrain, rare **mountain regions** with sharp snowy
//! ridgelines, lowland **rivers**, 3D **cave** tunnels + caverns, and vertical
//! **ravines** ("grietas") that break the surface.

use std::sync::Arc;

use bevy::prelude::*;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use crate::block::Block;
use crate::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, ChunkCoord, ChunkData};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Biome {
    Plains,
    Forest,
    Desert,
    Snow,
}

impl Biome {
    fn surface(self, height: i32, sea: i32) -> Block {
        if self != Biome::Snow && height <= sea + 1 {
            return Block::Sand; // beaches
        }
        match self {
            Biome::Desert => Block::Sand,
            Biome::Snow => Block::Snow,
            Biome::Plains | Biome::Forest => Block::Grass,
        }
    }

    fn filler(self) -> Block {
        match self {
            Biome::Desert => Block::Sand,
            _ => Block::Dirt,
        }
    }

    fn has_trees(self) -> bool {
        matches!(self, Biome::Plains | Biome::Forest)
    }

    fn tree_chance(self) -> f32 {
        match self {
            Biome::Forest => 0.06,
            Biome::Plains => 0.012,
            _ => 0.0,
        }
    }
}

/// The world seed. Set by the menu / a loaded save before entering `Playing`.
#[derive(Resource)]
pub struct WorldSeed(pub u32);

impl Default for WorldSeed {
    fn default() -> Self {
        Self(0x5EED_1234)
    }
}

/// Shared, cheap-to-clone handle to the world generator (built on `OnEnter`
/// `GameFlow::Playing` from `WorldSeed`).
#[derive(Resource, Clone)]
pub struct WorldGenHandle(pub Arc<WorldGen>);

pub struct WorldGen {
    height: Fbm<Perlin>,
    mtn_mask: Fbm<Perlin>,
    mountain: Fbm<Perlin>,
    biome: Fbm<Perlin>,
    river: Fbm<Perlin>,
    ravine: Fbm<Perlin>,
    ravine_mask: Perlin,
    snow: Perlin,
    rock: Perlin,
    gravel: Perlin,
    cave_a: Perlin,
    cave_b: Perlin,
    cavern: Perlin,
    pub sea_level: i32,
}

/// One resolved terrain column.
struct Column {
    /// Index of the topmost solid block (heightmap, before caves/ravines).
    height: i32,
    /// 0 = no river here, →1 towards a river's centreline.
    river_t: f32,
}

impl WorldGen {
    pub fn new(seed: u32) -> Self {
        let height = Fbm::<Perlin>::new(seed)
            .set_octaves(5)
            .set_frequency(0.0042)
            .set_persistence(0.5)
            .set_lacunarity(2.05);
        let mtn_mask = Fbm::<Perlin>::new(seed.wrapping_add(71))
            .set_octaves(2)
            .set_frequency(0.0011);
        let mountain = Fbm::<Perlin>::new(seed.wrapping_add(151))
            .set_octaves(4)
            .set_frequency(0.0060)
            .set_persistence(0.55);
        let biome = Fbm::<Perlin>::new(seed.wrapping_add(101))
            .set_octaves(3)
            .set_frequency(0.0016);
        let river = Fbm::<Perlin>::new(seed.wrapping_add(211))
            .set_octaves(2)
            .set_frequency(0.00095);
        let ravine = Fbm::<Perlin>::new(seed.wrapping_add(307))
            .set_octaves(2)
            .set_frequency(0.0040);
        Self {
            height,
            mtn_mask,
            mountain,
            biome,
            river,
            ravine,
            ravine_mask: Perlin::new(seed.wrapping_add(313)),
            snow: Perlin::new(seed.wrapping_add(404)),
            rock: Perlin::new(seed.wrapping_add(451)),
            gravel: Perlin::new(seed.wrapping_add(505)),
            cave_a: Perlin::new(seed.wrapping_add(601)),
            cave_b: Perlin::new(seed.wrapping_add(619)),
            cavern: Perlin::new(seed.wrapping_add(637)),
            sea_level: 48,
        }
    }

    /// Base rolling height + mountain peak contribution (no rivers yet).
    fn raw_height(&self, x: f64, z: f64) -> f64 {
        let n = self.height.get([x, z]); // ~ -1..1
        let base = 30.0 + (n * 0.5 + 0.5).clamp(0.0, 1.0).powf(1.22) * 58.0; // ~30..88

        // Mountains: only the upper slice of the mask noise becomes a range, and
        // ridged `mountain` noise makes sharp crests instead of round domes.
        let mask = ((self.mtn_mask.get([x, z]) * 0.5 + 0.5) - 0.42).max(0.0) / 0.58;
        let ridge = 1.0 - self.mountain.get([x, z]).abs(); // 0..1, ~1 on a crest
        let peak = mask.powf(1.2) * ridge.powf(1.8) * 52.0;

        base + peak
    }

    fn column(&self, x: i32, z: i32) -> Column {
        let (fx, fz) = (x as f64, z as f64);
        let raw = self.raw_height(fx, fz);
        let sea = self.sea_level as f64;

        // Rivers: only in already-low land, so they always hold water.
        let mut river_t = 0.0_f64;
        let mut h = raw;
        if raw < sea + 12.0 {
            let w = 0.028 + 0.020 * (self.river.get([fx * 3.1, fz * 3.1]) * 0.5 + 0.5);
            let d = self.river.get([fx, fz]).abs();
            if d < w {
                river_t = smoothstep(1.0 - d / w);
                h = lerp(raw, sea - 3.0, river_t);
            }
        }

        Column {
            height: (h.round() as i32).clamp(2, CHUNK_HEIGHT - 6),
            river_t: river_t as f32,
        }
    }

    /// Terrain surface height (index of the topmost solid block) at a world column.
    pub fn surface_height(&self, x: i32, z: i32) -> i32 {
        self.column(x, z).height
    }

    /// Altitude (noisy) above which the surface turns to snow.
    pub fn snowline(&self, x: i32, z: i32) -> f64 {
        82.0 + (self.snow.get([x as f64 * 0.02, z as f64 * 0.02]) * 0.5 + 0.5) * 12.0
    }

    pub fn biome_at(&self, x: i32, z: i32, height: i32) -> Biome {
        if height as f64 >= self.snowline(x, z) {
            return Biome::Snow;
        }
        let t = (self.biome.get([x as f64, z as f64]) * 0.5 + 0.5) as f32;
        if t < 0.30 {
            Biome::Desert
        } else if t > 0.68 {
            Biome::Forest
        } else {
            Biome::Plains
        }
    }

    /// `true` if the voxel at `(x, y, z)` should be hollowed into a cave.
    fn cave_carve(&self, x: f64, y: f64, z: f64) -> bool {
        // Worm tunnels: near the shared zero-set of two 3D fields.
        let a = self.cave_a.get([x * 0.028, y * 0.046, z * 0.028]);
        let b = self.cave_b.get([x * 0.028, y * 0.046, z * 0.028]);
        if a * a + b * b < 0.020 {
            return true;
        }
        // Blobby caverns, progressively more common towards bedrock.
        let bias = ((42.0 - y).max(0.0)) * 0.004;
        self.cavern.get([x * 0.020, y * 0.030, z * 0.020]) > 0.58 - bias
    }

    /// If this column is inside a ravine, the Y of its floor.
    fn ravine_floor(&self, x: f64, z: f64, h: i32) -> Option<i32> {
        if self.ravine_mask.get([x * 0.0055, z * 0.0055]) < 0.55 {
            return None; // ravines only in scattered regions
        }
        let half = 0.014;
        let d = self.ravine.get([x, z]).abs();
        if d >= half {
            return None;
        }
        let t = 1.0 - d / half; // 0 at the lip, 1 at the crack's centre
        let depth = (16.0 + 24.0 * t) as i32;
        Some((h - depth).max(6))
    }

    pub fn generate(&self, coord: ChunkCoord) -> ChunkData {
        let mut data = ChunkData::empty(coord);
        let ox = coord.x * CHUNK_SIZE;
        let oz = coord.y * CHUNK_SIZE;
        let sea = self.sea_level;

        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let wx = ox + lx;
                let wz = oz + lz;
                let (fx, fz) = (wx as f64, wz as f64);

                let col = self.column(wx, wz);
                let h = col.height;
                let is_river = col.river_t > 0.0;
                let biome = self.biome_at(wx, wz, h);
                let snowline = self.snowline(wx, wz);
                let ravine_floor = self.ravine_floor(fx, fz, h);

                // Below this Y, caves may hollow the rock. Kept under the sea
                // floor near the coast so oceans don't leak through.
                let cave_ceiling = if h < sea + 2 {
                    (sea - 4).min(h - 3)
                } else {
                    h - 3
                };

                for y in 0..CHUNK_HEIGHT {
                    let mut block = if y == 0 {
                        Block::Bedrock
                    } else if y > h {
                        if y <= sea { Block::Water } else { Block::Air }
                    } else if y == h {
                        if is_river {
                            Block::Sand // river bed
                        } else {
                            let mut s = biome.surface(h, sea);
                            // Wind-scoured rock breaking through a snow cap.
                            if biome == Biome::Snow
                                && (h as f64) >= snowline + 6.0
                                && self.rock.get([fx * 0.09, fz * 0.09]) > 0.33
                            {
                                s = Block::Stone;
                            }
                            s
                        }
                    } else if y >= h - 3 {
                        // High ground is rocky right under the surface.
                        if h >= 84 { Block::Stone } else { biome.filler() }
                    } else {
                        Block::Stone
                    };

                    // Gravel outcrops near the surface: the bootstrap flint source.
                    if matches!(block, Block::Stone | Block::Dirt | Block::Sand)
                        && y > h - 10
                        && y <= h
                    {
                        let g = self.gravel.get([fx * 0.085, y as f64 * 0.085, fz * 0.085]);
                        if g > 0.42 {
                            block = Block::Gravel;
                        }
                    }

                    // --- Carve caves --------------------------------------
                    if y >= 4
                        && y < cave_ceiling
                        && matches!(
                            block,
                            Block::Stone | Block::Dirt | Block::Sand | Block::Gravel
                        )
                        && self.cave_carve(fx, y as f64, fz)
                    {
                        block = Block::Air;
                    }

                    // --- Carve ravines ----------------------------------
                    if let Some(floor) = ravine_floor {
                        if y > floor
                            && y <= h + 1
                            && !matches!(block, Block::Bedrock | Block::Water)
                        {
                            block = Block::Air;
                        }
                    }

                    if block != Block::Air {
                        data.set(lx, y, lz, block);
                    }
                }

                // Trees are kept fully inside the chunk to avoid cross-chunk writes.
                if biome.has_trees()
                    && !is_river
                    && (3..CHUNK_SIZE - 3).contains(&lx)
                    && (3..CHUNK_SIZE - 3).contains(&lz)
                    && h > sea
                    && data.get(lx, h, lz) == Block::Grass
                {
                    if hash2(wx, wz) < biome.tree_chance() {
                        let trunk = 4 + (hash2(wx ^ 7, wz ^ 13) * 3.0) as i32;
                        place_tree(&mut data, lx, h, lz, trunk);
                    }
                }
            }
        }

        data
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn place_tree(data: &mut ChunkData, x: i32, ground: i32, z: i32, trunk: i32) {
    let top = (ground + trunk).min(CHUNK_HEIGHT - 4);
    for y in (ground + 1)..=top {
        data.set(x, y, z, Block::Wood);
    }
    for dy in -1..=2 {
        let ly = top + dy;
        let radius = if dy <= 0 { 2 } else { 1 };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if dx * dx + dz * dz > radius * radius + 1 {
                    continue;
                }
                let (lx, lz) = (x + dx, z + dz);
                if (0..CHUNK_SIZE).contains(&lx)
                    && (0..CHUNK_SIZE).contains(&lz)
                    && (0..CHUNK_HEIGHT).contains(&ly)
                    && data.get(lx, ly, lz) == Block::Air
                {
                    data.set(lx, ly, lz, Block::Leaves);
                }
            }
        }
    }
}

/// Deterministic integer hash mapped to `[0, 1)`.
fn hash2(x: i32, z: i32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x1656_67b1)
        .wrapping_add((z as u32).wrapping_mul(0x2545_1d31));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h as f32) / (u32::MAX as f32)
}
