//! Ground scatter: loose sticks, small wild plants and wild potatoes that
//! generate on top of each chunk. Sticks and potatoes are picked up just by
//! walking over them; plants have to be cut with a flint knife (left-click
//! while holding one) to yield plant fibre (and sometimes seeds).

use std::collections::HashSet;

use bevy::prelude::*;

use crate::block::Block;
use crate::chunk::{CHUNK_SIZE, ChunkCoord};
use crate::item::{Inventory, Item};
use crate::player::{Player, player_free};
use crate::streaming::ChunkWorld;
use crate::worldgen::WorldGenHandle;

/// Ground-scatter attempts per chunk. Wild plants are the common outcome so
/// fields feel lush and seeds are easy to gather.
const PER_CHUNK: u32 = 34;
const PICKUP_RANGE: f32 = 1.5;
/// How close the player must be to a wild plant to cut it with the knife.
const CUT_RANGE: f32 = 2.4;

pub struct ScatterPlugin;

impl Plugin for ScatterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Scattered>()
            .add_systems(Startup, setup_scatter_assets)
            .add_systems(
                Update,
                (spawn_scatter, pickup_nearby, despawn_orphans)
                    .run_if(in_state(crate::pause::GameFlow::Playing)),
            )
            .add_systems(Update, cut_plants.run_if(player_free));
    }
}

#[derive(Resource, Default)]
struct Scattered(HashSet<ChunkCoord>);

#[derive(Resource)]
struct ScatterAssets {
    stick_mesh: Handle<Mesh>,
    stick_mat: Handle<StandardMaterial>,
    plant_mesh: Handle<Mesh>,
    plant_mat: Handle<StandardMaterial>,
    potato_mesh: Handle<Mesh>,
    potato_mat: Handle<StandardMaterial>,
}

/// A ground item that is vacuumed up when the player walks near it. Spawned by
/// `scatter.rs` (sticks) and `animal.rs` (mob drops).
#[derive(Component)]
pub struct Pickup {
    pub item: Item,
    pub amount: u32,
}

/// A wild plant tuft: only a flint knife turns it into plant fibre.
#[derive(Component)]
struct WildPlant;

enum ScatterKind {
    Stick,
    Plant,
    Potato,
}

#[derive(Component)]
struct ScatterOf(ChunkCoord);

fn plant_material(server: &AssetServer, tex: &str) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(server.load(tex.to_string())),
        perceptual_roughness: 0.85,
        double_sided: true,
        cull_mode: None,
        alpha_mode: AlphaMode::Mask(0.5),
        ..default()
    }
}

fn setup_scatter_assets(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ScatterAssets {
        stick_mesh: meshes.add(Cuboid::new(0.6, 0.06, 0.06)),
        stick_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.29, 0.15),
            perceptual_roughness: 0.9,
            ..default()
        }),
        plant_mesh: meshes.add(crate::mesher::crossed_quads(0.95, 0.85)),
        plant_mat: materials.add(plant_material(&server, "textures/blocks/tall_grass.png")),
        potato_mesh: meshes.add(crate::mesher::crossed_quads(0.75, 0.58)),
        potato_mat: materials.add(plant_material(&server, "textures/blocks/potatoes.png")),
    });
}

fn spawn_scatter(
    mut commands: Commands,
    world: Res<ChunkWorld>,
    world_gen: Res<WorldGenHandle>,
    assets: Res<ScatterAssets>,
    mut scattered: ResMut<Scattered>,
) {
    let sea = world_gen.0.sea_level;

    for (coord, slot) in world.chunks.iter() {
        if slot.meshed_at.is_none() || scattered.0.contains(coord) {
            continue;
        }
        scattered.0.insert(*coord);

        for i in 0..PER_CHUNK {
            let lx = (hash(coord.x, coord.y, i * 2) % CHUNK_SIZE as u32) as i32;
            let lz = (hash(coord.x, coord.y, i * 2 + 1) % CHUNK_SIZE as u32) as i32;
            let wx = coord.x * CHUNK_SIZE + lx;
            let wz = coord.y * CHUNK_SIZE + lz;
            let h = world_gen.0.surface_height(wx, wz);
            if h <= sea {
                continue;
            }
            if !matches!(
                slot.data.get(lx, h, lz),
                Block::Grass | Block::Dirt | Block::Sand
            ) {
                continue;
            }

            let ground = h as f32 + 1.0;
            let yaw = hash(coord.x, coord.y, i * 2 + 7) as f32 * 0.001;
            // 18% stick, 72% wild plant, 10% wild potato.
            let roll = hash(coord.x, coord.y, i * 5 + 3) % 50;
            let kind = if roll < 9 {
                ScatterKind::Stick
            } else if roll < 45 {
                ScatterKind::Plant
            } else {
                ScatterKind::Potato
            };
            let y = ground
                + match kind {
                    ScatterKind::Stick => 0.03,
                    // crossed-quad plant/potato meshes are base-anchored.
                    ScatterKind::Plant | ScatterKind::Potato => 0.0,
                };

            let mut e = commands.spawn((
                Transform::from_xyz(wx as f32 + 0.5, y, wz as f32 + 0.5)
                    .with_rotation(Quat::from_rotation_y(yaw)),
                ScatterOf(*coord),
            ));
            match kind {
                ScatterKind::Stick => {
                    e.insert((
                        Mesh3d(assets.stick_mesh.clone()),
                        MeshMaterial3d(assets.stick_mat.clone()),
                        Pickup {
                            item: Item::Stick,
                            amount: 1,
                        },
                    ));
                }
                ScatterKind::Plant => {
                    e.insert((
                        Mesh3d(assets.plant_mesh.clone()),
                        MeshMaterial3d(assets.plant_mat.clone()),
                        WildPlant,
                    ));
                }
                ScatterKind::Potato => {
                    e.insert((
                        Mesh3d(assets.potato_mesh.clone()),
                        MeshMaterial3d(assets.potato_mat.clone()),
                        Pickup {
                            item: Item::Potato,
                            amount: 1,
                        },
                    ));
                }
            }
        }
    }
}

fn pickup_nearby(
    mut commands: Commands,
    mut inventory: ResMut<Inventory>,
    player_q: Query<&Transform, With<Player>>,
    pickups: Query<(Entity, &Transform, &Pickup)>,
) {
    let Ok(player) = player_q.single() else {
        return;
    };
    for (entity, transform, pickup) in &pickups {
        if transform.translation.distance(player.translation) < PICKUP_RANGE {
            inventory.add(pickup.item, pickup.amount);
            commands.entity(entity).despawn();
        }
    }
}

/// Left-click with a flint knife in hand to cut the nearest wild plant within
/// reach into plant fibre (and sometimes a seed). This is the *only* way to
/// get fibre.
fn cut_plants(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    mut inventory: ResMut<Inventory>,
    player_q: Query<&Transform, With<Player>>,
    plants: Query<(Entity, &Transform), With<WildPlant>>,
    mut rng: Local<u32>,
) {
    if !mouse.just_pressed(MouseButton::Left)
        || inventory.selected_item() != Some(Item::Knife)
    {
        return;
    }
    let Ok(player) = player_q.single() else {
        return;
    };

    let Some((entity, _)) = plants
        .iter()
        .map(|(e, t)| (e, t.translation.distance(player.translation)))
        .filter(|(_, d)| *d <= CUT_RANGE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return;
    };

    inventory.add(Item::Fiber, 1);
    if frand(&mut rng) < 0.4 {
        inventory.add(Item::Seeds, 1);
    }
    commands.entity(entity).despawn();
}

fn frand(state: &mut u32) -> f32 {
    if *state == 0 {
        *state = 0x2545_f491;
    }
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 24) as f32
}

fn despawn_orphans(
    mut commands: Commands,
    world: Res<ChunkWorld>,
    mut scattered: ResMut<Scattered>,
    pickups: Query<(Entity, &ScatterOf)>,
) {
    scattered.0.retain(|c| world.chunks.contains_key(c));
    for (entity, owner) in &pickups {
        if !world.chunks.contains_key(&owner.0) {
            commands.entity(entity).despawn();
        }
    }
}

fn hash(x: i32, z: i32, i: u32) -> u32 {
    let mut h = (x as u32)
        .wrapping_mul(0x1656_67b1)
        ^ (z as u32).wrapping_mul(0x2545_1d31)
        ^ i.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h
}
