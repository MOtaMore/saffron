//! Rudimentary fishing: with the rod selected, left-click on water to cast the
//! bobber. After a short wait the fish bites (the bobber dips); left-click again
//! to reel it in and pocket the fish. Clicking before the bite just reels in an
//! empty line. No minigame. Rod is crafted only at a bench.

use std::collections::HashSet;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::block::Block;
use crate::camera::MainCamera;
use crate::chunk::{CHUNK_SIZE, ChunkCoord};
use crate::container::OpenContainer;
use crate::interact::raycast_cell;
use crate::item::{Inventory, InventoryOpen, Item};
use crate::pause::{GameFlow, not_paused};
use crate::player::Player;
use crate::station::StationChoices;
use crate::streaming::ChunkWorld;
use crate::worldgen::WorldGenHandle;

const REACH: f32 = 80.0;
const RAY_MAX: f32 = 4000.0;
const HOTBAR_GUARD_PX: f32 = 76.0;

/// Seconds between casting and the bite.
const BITE_MIN: f32 = 1.8;
const BITE_MAX: f32 = 5.0;
/// How far the bobber sinks when the fish takes the hook.
const BOBBER_DIP: f32 = 0.28;

pub struct FishingPlugin;

impl Plugin for FishingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FishingState>()
            .init_resource::<FishStocked>()
            .add_systems(Startup, (setup_fishing_assets, spawn_fishing_ui))
            .add_systems(Update, (cast, tick).chain().run_if(not_paused))
            .add_systems(
                Update,
                (update_fishing_ui, spawn_fish, swim_fish, despawn_fish)
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

#[derive(Resource, Default)]
pub struct FishingState(Phase);

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    /// Bobber is out; counting down to the bite.
    Waiting {
        timer: f32,
    },
    /// Fish is on the hook, waiting for the player to reel in.
    Hooked,
}

impl FishingState {
    pub fn busy(&self) -> bool {
        !matches!(self.0, Phase::Idle)
    }
}

#[derive(Resource)]
struct FishAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    fish_mesh: Handle<Mesh>,
    fish_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct Bobber;

#[derive(Component)]
struct SwimFish {
    home: Vec3,
    target: Vec3,
    wait: f32,
    chunk: ChunkCoord,
}

#[derive(Resource, Default)]
struct FishStocked(HashSet<ChunkCoord>);

fn setup_fishing_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(FishAssets {
        mesh: meshes.add(Sphere::new(0.12)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.35, 0.25),
            unlit: true,
            ..default()
        }),
        fish_mesh: meshes.add(Cuboid::new(0.4, 0.16, 0.16)),
        fish_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.68, 0.72),
            perceptual_roughness: 0.4,
            ..default()
        }),
    });
}

/// All fishing input: cast, reel-in-empty, or land the fish, depending on phase.
#[allow(clippy::too_many_arguments)]
/// Bundled "is a menu eating input?" resources, to keep `cast` under Bevy's
/// 16-parameter system limit.
#[derive(SystemParam)]
struct CastGuards<'w> {
    inv_open: Res<'w, InventoryOpen>,
    container_open: Res<'w, OpenContainer>,
    station_menu: Res<'w, StationChoices>,
    cam_mode: Res<'w, crate::firstperson::CameraMode>,
}

fn cast(
    mouse: Res<ButtonInput<MouseButton>>,
    guards: CastGuards,
    assets: Res<FishAssets>,
    mut state: ResMut<FishingState>,
    mut inventory: ResMut<Inventory>,
    mut commands: Commands,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    player_q: Query<&Transform, With<Player>>,
    bobbers: Query<Entity, With<Bobber>>,
    bobber_pos: Query<&Transform, With<Bobber>>,
    swimmers: Query<(Entity, &Transform), With<SwimFish>>,
    world: Res<ChunkWorld>,
    mut rng: Local<u32>,
) {
    if guards.inv_open.0 || guards.container_open.0.is_some() || !guards.station_menu.0.is_empty() {
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if inventory.selected_item() != Some(Item::FishingRod) {
        return;
    }

    match state.0 {
        // Already fishing: this click reels in.
        Phase::Hooked => {
            let n = if frand(&mut rng) < 0.25 { 2 } else { 1 };
            inventory.add(Item::Fish, n);
            // Despawn the nearest visible fish to the bobber, for flavour.
            if let Some(bob) = bobber_pos.iter().next() {
                if let Some((fish, _)) = swimmers
                    .iter()
                    .map(|(e, t)| (e, t.translation.distance(bob.translation)))
                    .filter(|(_, d)| *d < 6.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                {
                    commands.entity(fish).despawn();
                }
            }
            end_fishing(&mut state, &mut commands, &bobbers);
            return;
        }
        Phase::Waiting { .. } => {
            end_fishing(&mut state, &mut commands, &bobbers); // reeled in nothing
            return;
        }
        Phase::Idle => {}
    }

    // Idle: try to cast onto a water cell under the cursor.
    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), camera_q.single()) else {
        return;
    };
    let Some(cursor) = crate::firstperson::aim_point(window, &guards.cam_mode) else {
        return;
    };
    if cursor.y > window.height() - HOTBAR_GUARD_PX {
        return;
    }
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else {
        return;
    };
    let Some(hit) = raycast_cell(&world, ray.origin, *ray.direction, RAY_MAX, None) else {
        return;
    };
    if world.get_loaded(hit.cell.x, hit.cell.y, hit.cell.z) != Some(Block::Water) {
        return;
    }
    let spot = hit.cell.as_vec3() + Vec3::new(0.5, 1.0, 0.5);
    if let Some(pt) = player_q.iter().next() {
        if spot.distance(pt.translation) > REACH {
            return;
        }
    }

    state.0 = Phase::Waiting {
        timer: BITE_MIN + rand_seeded(hit.cell.x ^ hit.cell.z) * (BITE_MAX - BITE_MIN),
    };
    commands.spawn((
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
        Transform::from_translation(spot),
        Bobber,
    ));
}

/// Counts down the wait; when it elapses the fish bites and the bobber dips.
fn tick(
    time: Res<Time>,
    mut state: ResMut<FishingState>,
    mut bobbers: Query<&mut Transform, With<Bobber>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    if let Phase::Waiting { timer } = &mut state.0 {
        *timer -= dt;
        if *timer <= 0.0 {
            state.0 = Phase::Hooked;
            for mut tf in &mut bobbers {
                tf.translation.y -= BOBBER_DIP;
            }
        }
    }
}

pub fn end_fishing(
    state: &mut FishingState,
    commands: &mut Commands,
    bobbers: &Query<Entity, With<Bobber>>,
) {
    state.0 = Phase::Idle;
    for entity in bobbers.iter() {
        commands.entity(entity).despawn();
    }
}

// --- Swimming fish -----------------------------------------------------

const FISH_WANDER: f32 = 3.5;
const FISH_SPEED: f32 = 1.3;

fn spawn_fish(
    mut commands: Commands,
    world: Res<ChunkWorld>,
    world_gen: Res<WorldGenHandle>,
    assets: Res<FishAssets>,
    mut stocked: ResMut<FishStocked>,
) {
    let sea = world_gen.0.sea_level;

    for (coord, slot) in world.chunks.iter() {
        if slot.meshed_at.is_none() || stocked.0.contains(coord) {
            continue;
        }
        stocked.0.insert(*coord);

        for i in 0..4u32 {
            let lx = (fish_hash(coord.x, coord.y, i * 2) % CHUNK_SIZE as u32) as i32;
            let lz = (fish_hash(coord.x, coord.y, i * 2 + 1) % CHUNK_SIZE as u32) as i32;
            // Needs water a couple of blocks deep here.
            if slot.data.get(lx, sea, lz) != Block::Water
                || slot.data.get(lx, sea - 2, lz) != Block::Water
            {
                continue;
            }
            let y = sea as f32 - 0.8 - (fish_hash(coord.x, coord.y, i + 50) % 3) as f32 * 0.5;
            let pos = Vec3::new(
                (coord.x * CHUNK_SIZE + lx) as f32 + 0.5,
                y,
                (coord.y * CHUNK_SIZE + lz) as f32 + 0.5,
            );
            commands.spawn((
                Mesh3d(assets.fish_mesh.clone()),
                MeshMaterial3d(assets.fish_material.clone()),
                Transform::from_translation(pos),
                SwimFish {
                    home: pos,
                    target: pos,
                    wait: 0.0,
                    chunk: *coord,
                },
            ));
        }
    }
}

fn swim_fish(
    time: Res<Time>,
    world: Res<ChunkWorld>,
    mut fish: Query<(&mut Transform, &mut SwimFish)>,
    mut rng: Local<u32>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (mut transform, mut fish) in &mut fish {
        if fish.wait > 0.0 {
            fish.wait -= dt;
            continue;
        }
        let to = fish.target - transform.translation;
        if to.length() < 0.4 {
            fish.target = fish.home
                + Vec3::new(
                    frand(&mut rng) * 2.0 - 1.0,
                    (frand(&mut rng) - 0.5) * 0.6,
                    frand(&mut rng) * 2.0 - 1.0,
                ) * FISH_WANDER;
            fish.wait = 0.4 + frand(&mut rng) * 1.6;
            continue;
        }
        let dir = to.normalize_or_zero();
        let next = transform.translation + dir * FISH_SPEED * dt;
        let cell = next.floor().as_ivec3();
        if world.get_loaded(cell.x, cell.y, cell.z) == Some(Block::Water) {
            transform.translation = next;
            if dir.length_squared() > 1e-4 {
                let yaw = dir.x.atan2(dir.z);
                let t = 1.0 - (-8.0 * dt).exp();
                transform.rotation = transform.rotation.slerp(Quat::from_rotation_y(yaw), t);
            }
        } else {
            fish.target = fish.home; // bumped a wall — head home
        }
    }
}

fn despawn_fish(
    mut commands: Commands,
    world: Res<ChunkWorld>,
    mut stocked: ResMut<FishStocked>,
    fish: Query<(Entity, &SwimFish)>,
) {
    stocked.0.retain(|c| world.chunks.contains_key(c));
    for (entity, fish) in &fish {
        if !world.chunks.contains_key(&fish.chunk) {
            commands.entity(entity).despawn();
        }
    }
}

fn fish_hash(x: i32, z: i32, i: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x1656_67b1)
        ^ (z as u32).wrapping_mul(0x2545_1d31)
        ^ i.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h
}

// --- UI ------------------------------------------------------------------

#[derive(Component)]
struct FishingRoot;
#[derive(Component)]
struct FishingHint;

fn spawn_fishing_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(96.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            FishingRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ))
            .with_children(|pill| {
                pill.spawn((
                    Text::new(""),
                    TextFont::from_font_size(15.0),
                    TextColor(Color::WHITE),
                    FishingHint,
                ));
            });
        });
}

fn update_fishing_ui(
    state: Res<FishingState>,
    mut root: Query<&mut Visibility, With<FishingRoot>>,
    mut hint: Query<&mut Text, With<FishingHint>>,
) {
    let (label, show) = match state.0 {
        Phase::Idle => ("", false),
        Phase::Waiting { .. } => ("Cast out... wait for a bite", true),
        Phase::Hooked => ("Bite! LEFT CLICK to reel in", true),
    };
    if let Ok(mut vis) = root.single_mut() {
        *vis = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if show {
        if let Ok(mut text) = hint.single_mut() {
            text.0 = label.into();
        }
    }
}

fn frand(state: &mut u32) -> f32 {
    if *state == 0 {
        *state = 0x9e37_79b9;
    }
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 24) as f32
}

fn rand_seeded(seed: i32) -> f32 {
    let mut s = (seed as u32) ^ 0x2545_f491;
    frand(&mut s)
}
