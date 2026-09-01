//! Chunk storage and coordinate helpers.
//!
//! The world is split into square *columns*: each chunk covers
//! `CHUNK_SIZE * CHUNK_SIZE` blocks on the XZ plane and the full
//! `CHUNK_HEIGHT` on Y. Columns are infinite on X and Z.

use bevy::prelude::*;

use crate::block::Block;

pub const CHUNK_SIZE: i32 = 32;
pub const CHUNK_HEIGHT: i32 = 128;
pub const CHUNK_AREA: usize = (CHUNK_SIZE * CHUNK_SIZE) as usize;
pub const CHUNK_VOLUME: usize = CHUNK_AREA * CHUNK_HEIGHT as usize;

/// Chunk column coordinate, in chunk units, on the XZ plane.
pub type ChunkCoord = IVec2;

#[inline]
pub fn local_index(x: i32, y: i32, z: i32) -> usize {
    debug_assert!((0..CHUNK_SIZE).contains(&x));
    debug_assert!((0..CHUNK_HEIGHT).contains(&y));
    debug_assert!((0..CHUNK_SIZE).contains(&z));
    (y as usize * CHUNK_AREA) + (z as usize * CHUNK_SIZE as usize) + x as usize
}

#[inline]
pub fn chunk_of_world(x: i32, z: i32) -> ChunkCoord {
    IVec2::new(x.div_euclid(CHUNK_SIZE), z.div_euclid(CHUNK_SIZE))
}

#[inline]
pub fn chunk_of_pos(p: Vec3) -> ChunkCoord {
    chunk_of_world(p.x.floor() as i32, p.z.floor() as i32)
}

/// World-space position of a chunk's local origin (its `(0, 0, 0)` corner).
#[inline]
pub fn chunk_origin(c: ChunkCoord) -> Vec3 {
    Vec3::new((c.x * CHUNK_SIZE) as f32, 0.0, (c.y * CHUNK_SIZE) as f32)
}

/// Block data for a single chunk column. `Clone` so the streamer can
/// copy-on-write it (`Arc::make_mut`) when the player edits a block.
#[derive(Clone)]
pub struct ChunkData {
    #[allow(dead_code)] // handy for debugging / future save system
    pub coord: ChunkCoord,
    blocks: Box<[Block]>,
}

impl ChunkData {
    pub fn empty(coord: ChunkCoord) -> Self {
        Self {
            coord,
            blocks: vec![Block::Air; CHUNK_VOLUME].into_boxed_slice(),
        }
    }

    /// Reads a block by local coordinates. Out-of-range Y is treated as air;
    /// X/Z are expected to be in `0..CHUNK_SIZE`.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Block {
        if !(0..CHUNK_HEIGHT).contains(&y) {
            return Block::Air;
        }
        self.blocks[local_index(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32, block: Block) {
        self.blocks[local_index(x, y, z)] = block;
    }
}
