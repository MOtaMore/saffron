//! Turns chunk voxel data into a render mesh via face culling.
//!
//! Runs on a background task pool, so it only touches plain data
//! (`ChunkData` behind `Arc`) and produces a `MeshData` POD that the
//! main thread converts into a `bevy::Mesh`.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use crate::block::{ATLAS_COLS, Block};
use crate::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, ChunkData};

/// Neighbouring chunks, in order: `-X, +X, -Z, +Z`.
pub type Neighbors = [Option<Arc<ChunkData>>; 4];

#[derive(Default)]
pub struct Buffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// Atlas UV rect `(u0, u1, v0, v1)` for a tile column, inset half a texel.
fn tile_rect(tile: u32) -> (f32, f32, f32, f32) {
    let cols = ATLAS_COLS as f32;
    let inset = 0.5 / (cols * 16.0);
    let u0 = tile as f32 / cols + inset;
    let u1 = (tile as f32 + 1.0) / cols - inset;
    (u0, u1, inset, 1.0 - inset)
}

impl Buffers {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    fn push_face(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        face: &Face,
        color: [f32; 4],
        rect: (f32, f32, f32, f32),
    ) {
        let (u0, u1, v0, v1) = rect;
        let start = self.positions.len() as u32;
        for (corner, sel) in face.corners.iter().zip(face.uv) {
            self.positions.push([
                x as f32 + corner[0],
                y as f32 + corner[1],
                z as f32 + corner[2],
            ]);
            self.normals.push(face.normal);
            self.colors.push(color);
            self.uvs
                .push([u0 + (u1 - u0) * sel[0], v0 + (v1 - v0) * sel[1]]);
        }
        self.indices
            .extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    }

    pub fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

/// Minecraft-style crossed planes: two vertical quads through the centre at ±45°,
/// each `width` across and `height` tall, base at y = 0. Meant for plants and
/// crops with a masked, double-sided material. UVs run 0..1 per quad.
pub fn crossed_quads(width: f32, height: f32) -> Mesh {
    let hw = width * 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(8);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(8);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(8);
    let mut indices: Vec<u32> = Vec::with_capacity(12);

    for (q, dir) in [
        Vec3::new(1.0, 0.0, 1.0).normalize(),
        Vec3::new(1.0, 0.0, -1.0).normalize(),
    ]
    .into_iter()
    .enumerate()
    {
        let n = Vec3::new(dir.z, 0.0, -dir.x); // horizontal normal
        let (a, b) = (-dir * hw, dir * hw);
        positions.extend_from_slice(&[
            [a.x, 0.0, a.z],
            [b.x, 0.0, b.z],
            [b.x, height, b.z],
            [a.x, height, a.z],
        ]);
        for _ in 0..4 {
            normals.push([n.x, n.y, n.z]);
        }
        uvs.extend_from_slice(&[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        let base = q as u32 * 4;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Opaque terrain and the translucent blocks (water, glass) are meshed into
/// separate buffers so the translucent ones can be drawn in a blended pass.
/// Water is flat-tinted; glass carries atlas UVs like solid terrain.
#[derive(Default)]
pub struct MeshData {
    pub solid: Buffers,
    pub water: Buffers,
    pub glass: Buffers,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.solid.is_empty() && self.water.is_empty() && self.glass.is_empty()
    }
}

// Unit-cube corners.
const A: [f32; 3] = [0.0, 0.0, 0.0];
const B: [f32; 3] = [1.0, 0.0, 0.0];
const C: [f32; 3] = [1.0, 0.0, 1.0];
const D: [f32; 3] = [0.0, 0.0, 1.0];
const E: [f32; 3] = [0.0, 1.0, 0.0];
const F: [f32; 3] = [1.0, 1.0, 0.0];
const G: [f32; 3] = [1.0, 1.0, 1.0];
const H: [f32; 3] = [0.0, 1.0, 1.0];

struct Face {
    offset: [i32; 3],
    normal: [f32; 3],
    corners: [[f32; 3]; 4],
    /// Per-corner atlas selectors: `[u_side, v_side]` where 0 → u0/v0, 1 → u1/v1.
    /// Chosen so the texture stands upright on every face.
    uv: [[f32; 2]; 4],
    shade: f32,
}

const FACES: [Face; 6] = [
    Face { offset: [0, 1, 0], normal: [0.0, 1.0, 0.0], corners: [E, H, G, F],
           uv: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]], shade: 1.00 },
    Face { offset: [0, -1, 0], normal: [0.0, -1.0, 0.0], corners: [A, B, C, D],
           uv: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], shade: 0.45 },
    Face { offset: [1, 0, 0], normal: [1.0, 0.0, 0.0], corners: [B, F, G, C],
           uv: [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], shade: 0.68 },
    Face { offset: [-1, 0, 0], normal: [-1.0, 0.0, 0.0], corners: [A, D, H, E],
           uv: [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]], shade: 0.68 },
    Face { offset: [0, 0, 1], normal: [0.0, 0.0, 1.0], corners: [D, C, G, H],
           uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], shade: 0.85 },
    Face { offset: [0, 0, -1], normal: [0.0, 0.0, -1.0], corners: [A, E, F, B],
           uv: [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]], shade: 0.85 },
];

#[inline]
fn should_render(here: Block, neighbor: Block) -> bool {
    if !here.is_solid() {
        false
    } else if here.is_opaque() {
        !neighbor.is_opaque()
    } else if here.is_waterlike() {
        // Water: only show the surface / edges against open air.
        neighbor == Block::Air
    } else {
        // Glass (and any other non-opaque solid): visible against anything that
        // isn't opaque, but hide the shared face between two of the same block.
        !neighbor.is_opaque() && neighbor != here
    }
}

/// `max_y` hides everything at or above that Y (the "view slice" feature).
/// Pass `CHUNK_HEIGHT` for a full chunk.
pub fn build_mesh(center: &ChunkData, neighbors: &Neighbors, max_y: i32) -> MeshData {
    let mut out = MeshData::default();
    let top = max_y.clamp(0, CHUNK_HEIGHT);

    let sample = |x: i32, y: i32, z: i32| -> Block {
        if y < 0 {
            return Block::Bedrock;
        }
        if y >= top {
            return Block::Air;
        }
        if (0..CHUNK_SIZE).contains(&x) && (0..CHUNK_SIZE).contains(&z) {
            return center.get(x, y, z);
        }
        let pick = |slot: &Option<Arc<ChunkData>>, lx: i32, lz: i32| {
            slot.as_ref().map_or(Block::Air, |c| c.get(lx, y, lz))
        };
        if x < 0 {
            pick(&neighbors[0], CHUNK_SIZE - 1, z)
        } else if x >= CHUNK_SIZE {
            pick(&neighbors[1], 0, z)
        } else if z < 0 {
            pick(&neighbors[2], x, CHUNK_SIZE - 1)
        } else {
            pick(&neighbors[3], x, 0)
        }
    };

    for y in 0..top {
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let here = center.get(x, y, z);
                if !here.is_solid() || here.renders_as_model() {
                    continue;
                }
                let base = here.color();
                let alpha = here.alpha();
                let is_water = here.is_waterlike();
                let is_glass = here == Block::Glass;
                let textured = here.is_textured();
                for face in &FACES {
                    let n = sample(x + face.offset[0], y + face.offset[1], z + face.offset[2]);
                    if !should_render(here, n) {
                        continue;
                    }
                    // Textured blocks: tint is just the face shade so the atlas
                    // pixels show at full colour. Untextured: block colour × shade.
                    let color = if textured {
                        [face.shade, face.shade, face.shade, alpha]
                    } else {
                        [
                            base[0] * face.shade,
                            base[1] * face.shade,
                            base[2] * face.shade,
                            alpha,
                        ]
                    };
                    let rect = tile_rect(here.face_tile(face.normal[1]));
                    let buffer = if is_water {
                        &mut out.water
                    } else if is_glass {
                        &mut out.glass
                    } else {
                        &mut out.solid
                    };
                    buffer.push_face(x, y, z, face, color, rect);
                }
            }
        }
    }

    out
}
