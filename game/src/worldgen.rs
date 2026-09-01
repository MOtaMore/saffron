//! Procedural terrain generation.
//!
//! Purely functional: `WorldGen::generate` takes a chunk coordinate and returns
//! fully-populated `ChunkData`, so it can run on a background task pool.

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
    biome: Fbm<Perlin>,
    gravel: Perlin,
    pub sea_level: i32,
}

impl WorldGen {
    pub fn new(seed: u32) -> Self {
        let height = Fbm::<Perlin>::new(seed)
            .set_octaves(5)
            .set_frequency(0.0042)
            .set_persistence(0.5)
            .set_lacunarity(2.05);
        let biome = Fbm::<Perlin>::new(seed.wrapping_add(101))
            .set_octaves(3)
            .set_frequency(0.0016);
        Self {
            height,
            biome,
            gravel: Perlin::new(seed.wrapping_add(505)),
            sea_level: 48,
        }
    }

    /// Terrain surface height (index of the topmost solid block) at a world column.
    pub fn surface_height(&self, x: i32, z: i32) -> i32 {
        let n = self.height.get([x as f64, z as f64]); // ~ -1..1
        let normalized = (n * 0.5 + 0.5).clamp(0.0, 1.0);
        let h = 34.0 + normalized.powf(1.15) * 66.0; // ~34..100
        (h.round() as i32).clamp(1, CHUNK_HEIGHT - 2)
    }

    pub fn biome_at(&self, x: i32, z: i32, height: i32) -> Biome {
        if height >= 90 {
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

    pub fn generate(&self, coord: ChunkCoord) -> ChunkData {
        let mut data = ChunkData::empty(coord);
        let ox = coord.x * CHUNK_SIZE;
        let oz = coord.y * CHUNK_SIZE;

        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let wx = ox + lx;
                let wz = oz + lz;
                let h = self.surface_height(wx, wz);
                let biome = self.biome_at(wx, wz, h);

                for y in 0..CHUNK_HEIGHT {
                    let mut block = if y == 0 {
                        Block::Bedrock
                    } else if y > h {
                        if y <= self.sea_level {
                            Block::Water
                        } else {
                            Block::Air
                        }
                    } else if y == h {
                        biome.surface(h, self.sea_level)
                    } else if y >= h - 3 {
                        biome.filler()
                    } else {
                        Block::Stone
                    };

                    // Gravel outcrops near the surface (and on some beaches):
                    // the bootstrap source of flint.
                    if matches!(block, Block::Stone | Block::Dirt | Block::Sand)
                        && y > h - 10
                        && y <= h
                    {
                        let g = self.gravel.get([
                            wx as f64 * 0.085,
                            y as f64 * 0.085,
                            wz as f64 * 0.085,
                        ]);
                        if g > 0.42 {
                            block = Block::Gravel;
                        }
                    }

                    if block != Block::Air {
                        data.set(lx, y, lz, block);
                    }
                }

                // Trees are kept fully inside the chunk to avoid cross-chunk writes.
                if biome.has_trees()
                    && (3..CHUNK_SIZE - 3).contains(&lx)
                    && (3..CHUNK_SIZE - 3).contains(&lz)
                    && h > self.sea_level
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
