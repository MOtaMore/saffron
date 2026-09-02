//! Infinite world streaming: generate, mesh, spawn and despawn chunk columns
//! around the player (or the camera before the player exists).
//!
//! Generation and meshing run on `AsyncComputeTaskPool`; results are polled
//! and applied on the main thread.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures_lite::future;

use crate::block::Block;
use crate::camera::MainCamera;
use crate::chunk_material::{ChunkMaterialHandle, GlassMaterialHandle, WaterMaterialHandle};
use crate::chunk::{
    CHUNK_HEIGHT, CHUNK_SIZE, ChunkCoord, ChunkData, chunk_of_pos, chunk_of_world, chunk_origin,
};
use crate::mesher::{MeshData, Neighbors, build_mesh};
use crate::pause::GameFlow;
use crate::player::Player;
use crate::view::ViewSlice;
use crate::worldgen::{WorldGen, WorldGenHandle, WorldSeed};

pub const VIEW_RADIUS: i32 = 6;
const KEEP_RADIUS: i32 = VIEW_RADIUS + 2;
const MAX_GEN_INFLIGHT: usize = 8;
const MAX_MESH_INFLIGHT: usize = 8;

pub struct StreamingPlugin;

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldSeed>()
            .init_resource::<ChunkWorld>()
            .init_resource::<GenTasks>()
            .init_resource::<MeshTasks>()
            .add_systems(OnEnter(GameFlow::Playing), setup_worldgen)
            .add_systems(
                Update,
                (
                    queue_generation,
                    apply_generation,
                    queue_meshing,
                    apply_meshing,
                    unload_chunks,
                )
                    .chain()
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

pub fn setup_worldgen(
    mut commands: Commands,
    seed: Res<WorldSeed>,
    library: Option<Res<crate::structure::StructureLibrary>>,
) {
    let lib = library
        .map(|l| l.0.clone())
        .unwrap_or_else(|| Arc::new(crate::structure::Library::default()));
    commands.insert_resource(WorldGenHandle(Arc::new(WorldGen::new(seed.0, lib))));
}

pub struct ChunkSlot {
    pub data: Arc<ChunkData>,
    pub entity: Option<Entity>,
    pub mesh: Option<Handle<Mesh>>,
    pub water_entity: Option<Entity>,
    pub water_mesh: Option<Handle<Mesh>>,
    pub glass_entity: Option<Entity>,
    pub glass_mesh: Option<Handle<Mesh>>,
    /// The view-slice cutoff this chunk's current mesh was built with. `None`
    /// means it has never been meshed; a value different from the live cutoff
    /// means it is stale and needs rebuilding.
    pub meshed_at: Option<i32>,
    /// Bumped on every block edit; lets `apply_meshing` discard a mesh that
    /// finished computing against now-outdated data.
    pub revision: u32,
}

impl ChunkSlot {
    pub fn is_meshed(&self) -> bool {
        self.meshed_at.is_some()
    }
}

#[derive(Resource, Default)]
pub struct ChunkWorld {
    pub chunks: HashMap<ChunkCoord, ChunkSlot>,
    /// Positions of blocks that render as a glTF model (workbench/chest/furnace),
    /// maintained by `set_block` so `props.rs` can reconcile cheaply.
    pub prop_blocks: HashMap<IVec3, Block>,
    /// Every player edit (place/break), applied on top of fresh chunks so it
    /// survives unload/reload. This is what the save file persists.
    pub edits: HashMap<IVec3, Block>,
}

impl ChunkWorld {
    /// Block at world coordinates. Unloaded columns read as solid stone so the
    /// player can never fall through terrain that has not streamed in yet.
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Block {
        if y < 0 {
            return Block::Bedrock;
        }
        match self.chunks.get(&chunk_of_world(x, z)) {
            Some(slot) => slot
                .data
                .get(x.rem_euclid(CHUNK_SIZE), y, z.rem_euclid(CHUNK_SIZE)),
            None => Block::Stone,
        }
    }

    /// Like `block_at`, but unloaded columns read as air. Used for raycasting,
    /// where a not-yet-streamed chunk must not count as a hit.
    pub fn sample_loaded(&self, x: i32, y: i32, z: i32) -> Block {
        if !(0..CHUNK_HEIGHT).contains(&y) {
            return Block::Air;
        }
        match self.chunks.get(&chunk_of_world(x, z)) {
            Some(slot) => slot
                .data
                .get(x.rem_euclid(CHUNK_SIZE), y, z.rem_euclid(CHUNK_SIZE)),
            None => Block::Air,
        }
    }

    /// Block at world coords, or `None` if that column is not loaded.
    pub fn get_loaded(&self, x: i32, y: i32, z: i32) -> Option<Block> {
        if !(0..CHUNK_HEIGHT).contains(&y) {
            return None;
        }
        self.chunks
            .get(&chunk_of_world(x, z))
            .map(|slot| slot.data.get(x.rem_euclid(CHUNK_SIZE), y, z.rem_euclid(CHUNK_SIZE)))
    }

    /// Overwrites a block and flags the affected chunk (and any bordering
    /// chunk) for re-meshing. Returns whether anything actually changed.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, block: Block) -> bool {
        if !(1..CHUNK_HEIGHT).contains(&y) {
            return false; // never touch the bedrock floor or the ceiling
        }
        let cc = chunk_of_world(x, z);
        let (lx, lz) = (x.rem_euclid(CHUNK_SIZE), z.rem_euclid(CHUNK_SIZE));

        {
            let Some(slot) = self.chunks.get_mut(&cc) else {
                return false;
            };
            if slot.data.get(lx, y, lz) == block {
                return false;
            }
            Arc::make_mut(&mut slot.data).set(lx, y, lz, block);
            slot.meshed_at = None;
            slot.revision = slot.revision.wrapping_add(1);
        }

        let pos = IVec3::new(x, y, z);
        self.edits.insert(pos, block);
        if block.renders_as_model() {
            self.prop_blocks.insert(pos, block);
        } else {
            self.prop_blocks.remove(&pos);
        }

        let mut borders = Vec::new();
        if lx == 0 {
            borders.push(cc + IVec2::new(-1, 0));
        }
        if lx == CHUNK_SIZE - 1 {
            borders.push(cc + IVec2::new(1, 0));
        }
        if lz == 0 {
            borders.push(cc + IVec2::new(0, -1));
        }
        if lz == CHUNK_SIZE - 1 {
            borders.push(cc + IVec2::new(0, 1));
        }
        for c in borders {
            if let Some(n) = self.chunks.get_mut(&c) {
                n.meshed_at = None;
                n.revision = n.revision.wrapping_add(1);
            }
        }
        true
    }
}

#[derive(Resource, Default)]
struct GenTasks(HashMap<ChunkCoord, Task<ChunkData>>);

/// Each in-flight mesh task remembers the cutoff and chunk revision it started
/// from, so a result computed against stale data can be thrown away.
#[derive(Resource, Default)]
struct MeshTasks(HashMap<ChunkCoord, (i32, u32, Task<Option<MeshData>>)>);

#[derive(Component)]
#[allow(dead_code)] // used by future block-editing / re-mesh logic
pub struct ChunkTag(pub ChunkCoord);

fn chebyshev(a: ChunkCoord, b: ChunkCoord) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

fn focus_chunk(
    player: &Query<&Transform, With<Player>>,
    camera: &Query<&Transform, With<MainCamera>>,
) -> Option<ChunkCoord> {
    player
        .iter()
        .next()
        .or_else(|| camera.iter().next())
        .map(|t| chunk_of_pos(t.translation))
}

fn queue_generation(
    world: Res<ChunkWorld>,
    mut tasks: ResMut<GenTasks>,
    world_gen: Res<WorldGenHandle>,
    player: Query<&Transform, With<Player>>,
    camera: Query<&Transform, With<MainCamera>>,
) {
    let Some(center) = focus_chunk(&player, &camera) else {
        return;
    };

    let mut candidates: Vec<ChunkCoord> = Vec::new();
    for dz in -VIEW_RADIUS..=VIEW_RADIUS {
        for dx in -VIEW_RADIUS..=VIEW_RADIUS {
            let c = center + IVec2::new(dx, dz);
            if !world.chunks.contains_key(&c) && !tasks.0.contains_key(&c) {
                candidates.push(c);
            }
        }
    }
    candidates.sort_by_key(|c| chebyshev(*c, center));

    let pool = AsyncComputeTaskPool::get();
    for c in candidates {
        if tasks.0.len() >= MAX_GEN_INFLIGHT {
            break;
        }
        let world_gen = world_gen.0.clone();
        let task = pool.spawn(async move { world_gen.generate(c) });
        tasks.0.insert(c, task);
    }
}

fn apply_generation(mut world: ResMut<ChunkWorld>, mut tasks: ResMut<GenTasks>) {
    let mut done: Vec<ChunkCoord> = Vec::new();
    let mut ready: Vec<(ChunkCoord, ChunkData)> = Vec::new();
    for (coord, task) in tasks.0.iter_mut() {
        if let Some(data) = future::block_on(future::poll_once(task)) {
            done.push(*coord);
            ready.push((*coord, data));
        }
    }

    for (coord, mut data) in ready {
        // Re-apply the player's edits on top of the freshly generated chunk.
        let mut new_props: Vec<(IVec3, Block)> = Vec::new();
        for (&pos, &block) in world.edits.iter() {
            if chunk_of_world(pos.x, pos.z) == coord && (0..CHUNK_HEIGHT).contains(&pos.y) {
                data.set(
                    pos.x.rem_euclid(CHUNK_SIZE),
                    pos.y,
                    pos.z.rem_euclid(CHUNK_SIZE),
                    block,
                );
                if block.renders_as_model() {
                    new_props.push((pos, block));
                }
            }
        }
        for (pos, block) in new_props {
            world.prop_blocks.insert(pos, block);
        }

        world.chunks.insert(
            coord,
            ChunkSlot {
                data: Arc::new(data),
                entity: None,
                mesh: None,
                water_entity: None,
                water_mesh: None,
                glass_entity: None,
                glass_mesh: None,
                meshed_at: None,
                revision: 0,
            },
        );
    }
    for c in done {
        tasks.0.remove(&c);
    }
}

fn queue_meshing(
    world: Res<ChunkWorld>,
    mut tasks: ResMut<MeshTasks>,
    slice: Res<ViewSlice>,
    player: Query<&Transform, With<Player>>,
    camera: Query<&Transform, With<MainCamera>>,
) {
    let Some(center) = focus_chunk(&player, &camera) else {
        return;
    };
    let pool = AsyncComputeTaskPool::get();
    let max_y = slice.effective().unwrap_or(CHUNK_HEIGHT).clamp(1, CHUNK_HEIGHT);

    // A chunk needs (re)meshing if it was never meshed or was meshed with a
    // different slice cutoff. Nearest chunks are rebuilt first.
    let mut candidates: Vec<ChunkCoord> = world
        .chunks
        .iter()
        .filter(|(c, s)| s.meshed_at != Some(max_y) && !tasks.0.contains_key(*c))
        .map(|(c, _)| *c)
        .collect();
    candidates.sort_by_key(|c| chebyshev(*c, center));

    for c in candidates {
        if tasks.0.len() >= MAX_MESH_INFLIGHT {
            break;
        }
        let offsets = [
            c + IVec2::new(-1, 0),
            c + IVec2::new(1, 0),
            c + IVec2::new(0, -1),
            c + IVec2::new(0, 1),
        ];
        // Wait until all four neighbours exist so chunk borders mesh seamlessly.
        if offsets.iter().any(|o| !world.chunks.contains_key(o)) {
            continue;
        }
        let revision = world.chunks[&c].revision;
        let center_data = world.chunks[&c].data.clone();
        let neighbors: Neighbors = [
            Some(world.chunks[&offsets[0]].data.clone()),
            Some(world.chunks[&offsets[1]].data.clone()),
            Some(world.chunks[&offsets[2]].data.clone()),
            Some(world.chunks[&offsets[3]].data.clone()),
        ];
        let task = pool.spawn(async move {
            let mesh = build_mesh(&center_data, &neighbors, max_y);
            if mesh.is_empty() { None } else { Some(mesh) }
        });
        tasks.0.insert(c, (max_y, revision, task));
    }
}

fn apply_meshing(
    mut commands: Commands,
    mut world: ResMut<ChunkWorld>,
    mut tasks: ResMut<MeshTasks>,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ChunkMaterialHandle>,
    water_material: Res<WaterMaterialHandle>,
    glass_material: Res<GlassMaterialHandle>,
) {
    let mut done: Vec<ChunkCoord> = Vec::new();
    for (coord, (max_y, revision, task)) in tasks.0.iter_mut() {
        let Some(result) = future::block_on(future::poll_once(task)) else {
            continue;
        };
        done.push(*coord);

        let Some(slot) = world.chunks.get_mut(coord) else {
            continue;
        };
        if slot.revision != *revision {
            continue; // edited since this mesh started; queue_meshing will retry
        }
        slot.meshed_at = Some(*max_y);
        let origin = chunk_origin(*coord);

        // Rebuild both entities from scratch so no render components go stale.
        if let Some(old) = slot.mesh.take() {
            meshes.remove(&old);
        }
        if let Some(old) = slot.water_mesh.take() {
            meshes.remove(&old);
        }
        if let Some(old) = slot.glass_mesh.take() {
            meshes.remove(&old);
        }
        if let Some(entity) = slot.entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some(entity) = slot.water_entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some(entity) = slot.glass_entity.take() {
            commands.entity(entity).despawn();
        }

        if let Some(mesh_data) = result {
            if !mesh_data.solid.is_empty() {
                let handle = meshes.add(mesh_data.solid.into_mesh());
                let entity = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(material.0.clone()),
                        Transform::from_translation(origin),
                        ChunkTag(*coord),
                    ))
                    .id();
                slot.entity = Some(entity);
                slot.mesh = Some(handle);
            }
            if !mesh_data.water.is_empty() {
                let handle = meshes.add(mesh_data.water.into_mesh());
                let entity = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(water_material.0.clone()),
                        Transform::from_translation(origin),
                        ChunkTag(*coord),
                    ))
                    .id();
                slot.water_entity = Some(entity);
                slot.water_mesh = Some(handle);
            }
            if !mesh_data.glass.is_empty() {
                let handle = meshes.add(mesh_data.glass.into_mesh());
                let entity = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(glass_material.0.clone()),
                        Transform::from_translation(origin),
                        ChunkTag(*coord),
                    ))
                    .id();
                slot.glass_entity = Some(entity);
                slot.glass_mesh = Some(handle);
            }
        }
    }
    for c in done {
        tasks.0.remove(&c);
    }
}

fn unload_chunks(
    mut commands: Commands,
    mut world: ResMut<ChunkWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mesh_tasks: ResMut<MeshTasks>,
    mut gen_tasks: ResMut<GenTasks>,
    player: Query<&Transform, With<Player>>,
    camera: Query<&Transform, With<MainCamera>>,
) {
    let Some(center) = focus_chunk(&player, &camera) else {
        return;
    };
    let stale: Vec<ChunkCoord> = world
        .chunks
        .keys()
        .copied()
        .filter(|c| chebyshev(*c, center) > KEEP_RADIUS)
        .collect();
    for c in stale {
        if let Some(slot) = world.chunks.remove(&c) {
            for entity in [slot.entity, slot.water_entity, slot.glass_entity]
                .into_iter()
                .flatten()
            {
                commands.entity(entity).despawn();
            }
            for mesh in [slot.mesh, slot.water_mesh, slot.glass_mesh]
                .into_iter()
                .flatten()
            {
                meshes.remove(&mesh);
            }
        }
        mesh_tasks.0.remove(&c);
        gen_tasks.0.remove(&c);
    }

    // Drop prop entries whose chunk is gone (their model entities are cleaned
    // up by `props::sync_props`).
    if !world.prop_blocks.is_empty() {
        let loaded: Vec<ChunkCoord> = world.chunks.keys().copied().collect();
        world
            .prop_blocks
            .retain(|pos, _| loaded.contains(&chunk_of_world(pos.x, pos.z)));
    }
}
