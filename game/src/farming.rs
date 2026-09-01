//! Farming: till soil with the flint sickle and plant seeds on it to grow
//! wheat. (Grinding wheat → flour is the hand-mill panel in `container.rs`.)
//! Farmland dries back into plain dirt if it isn't chained (through other
//! farmland, up to 3 hops) to a water block — like Minecraft's hydration, but
//! here it actually reverts over time instead of just slowing crop growth.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::block::Block;
use crate::camera::MainCamera;
use crate::interact::{CellHit, raycast_cell};
use crate::item::{Inventory, Item};
use crate::pause::not_paused;
use crate::player::{Player, player_free};
use crate::streaming::ChunkWorld;

const REACH: f32 = 5.5;
const RAY_MAX: f32 = 4000.0;

/// How often farmland hydration is recomputed / dry timers advance.
const FARM_TICK: f32 = 4.0;
/// How long unwatered farmland survives before reverting to dirt.
const DRY_THRESHOLD: f32 = 45.0;
/// Chain length (through other farmland tiles) a water block can hydrate over.
const HYDRATE_CHAIN: i32 = 3;

/// Seconds of unhydrated growth for a wheat crop to fully mature.
const CROP_GROWTH_TIME: f32 = 180.0;
const CROP_GROWTH_RATE: f32 = 1.0 / CROP_GROWTH_TIME;
const CROP_HYDRATED_BOOST: f32 = 1.5;

pub struct FarmingPlugin;

impl Plugin for FarmingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CropStore>()
            .init_resource::<HydratedFarmland>()
            .init_resource::<FarmTimer>()
            .init_resource::<DryTimers>()
            .add_systems(Update, (till_soil, plant_seeds).run_if(player_free))
            .add_systems(Update, (tick_crops, tick_farmland).run_if(not_paused));
    }
}

/// Growth (0..1) of every planted `Block::WheatCrop`, keyed by world position.
/// Read by `props.rs` to animate the stalk.
#[derive(Resource, Default)]
pub struct CropStore(pub HashMap<IVec3, f32>);

/// Farmland tiles considered watered as of the last hydration pass.
#[derive(Resource, Default)]
struct HydratedFarmland(HashSet<IVec3>);

#[derive(Resource, Default)]
struct FarmTimer(f32);

#[derive(Resource, Default)]
struct DryTimers(HashMap<IVec3, f32>);

/// Raycasts from the cursor into the voxel world, same as building/mining do.
fn cursor_hit(
    windows: &Query<&Window>,
    camera_q: &Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    world: &ChunkWorld,
) -> Option<CellHit> {
    let window = windows.single().ok()?;
    let (camera, cam_tf) = camera_q.single().ok()?;
    let cursor = window.cursor_position()?;
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    raycast_cell(world, ray.origin, *ray.direction, RAY_MAX, None)
}

fn in_reach(player: &Transform, cell: IVec3) -> bool {
    player.translation.distance(cell.as_vec3() + Vec3::splat(0.5)) <= REACH
}

/// The flint sickle only tills — left-click a Dirt/Grass block with it selected.
fn till_soil(
    mouse: Res<ButtonInput<MouseButton>>,
    inventory: Res<Inventory>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    player_q: Query<&Transform, With<Player>>,
    mut world: ResMut<ChunkWorld>,
) {
    if !mouse.just_pressed(MouseButton::Left) || inventory.selected_item() != Some(Item::Sickle) {
        return;
    }
    let Some(hit) = cursor_hit(&windows, &camera_q, &world) else {
        return;
    };
    let Ok(player) = player_q.single() else {
        return;
    };
    if !in_reach(player, hit.cell) {
        return;
    }
    if matches!(
        world.get_loaded(hit.cell.x, hit.cell.y, hit.cell.z),
        Some(Block::Dirt) | Some(Block::Grass)
    ) {
        world.set_block(hit.cell.x, hit.cell.y, hit.cell.z, Block::Farmland);
    }
}

/// Seeds selected + left-click on farmland with air above it → plant a crop.
fn plant_seeds(
    mouse: Res<ButtonInput<MouseButton>>,
    mut inventory: ResMut<Inventory>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    player_q: Query<&Transform, With<Player>>,
    mut world: ResMut<ChunkWorld>,
    mut crops: ResMut<CropStore>,
) {
    if !mouse.just_pressed(MouseButton::Left) || inventory.selected_item() != Some(Item::Seeds) {
        return;
    }
    let Some(hit) = cursor_hit(&windows, &camera_q, &world) else {
        return;
    };
    let Ok(player) = player_q.single() else {
        return;
    };
    if !in_reach(player, hit.cell) {
        return;
    }
    if world.get_loaded(hit.cell.x, hit.cell.y, hit.cell.z) != Some(Block::Farmland) {
        return;
    }
    let above = hit.cell + IVec3::Y;
    if world.get_loaded(above.x, above.y, above.z) != Some(Block::Air) {
        return;
    }
    if inventory.take(Item::Seeds, 1) == 1
        && world.set_block(above.x, above.y, above.z, Block::WheatCrop)
    {
        crops.0.insert(above, 0.0);
    }
}

/// Grows every planted crop; harvesting is detected implicitly — the normal
/// mining system removes the block (it drops nothing on its own), and this
/// system notices the mismatch and hands out wheat (mature) or a seed back
/// (still growing).
fn tick_crops(
    time: Res<Time>,
    world: Res<ChunkWorld>,
    mut crops: ResMut<CropStore>,
    hydrated: Res<HydratedFarmland>,
    mut inventory: ResMut<Inventory>,
    mut rng: Local<u32>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let mut harvested: Vec<(IVec3, f32)> = Vec::new();
    for (&pos, growth) in crops.0.iter_mut() {
        if world.get_loaded(pos.x, pos.y, pos.z) != Some(Block::WheatCrop) {
            harvested.push((pos, *growth));
            continue;
        }
        let below = pos - IVec3::Y;
        let rate = if hydrated.0.contains(&below) {
            CROP_GROWTH_RATE * CROP_HYDRATED_BOOST
        } else {
            CROP_GROWTH_RATE
        };
        *growth = (*growth + rate * dt).min(1.0);
    }

    for (pos, growth) in harvested {
        crops.0.remove(&pos);
        if growth >= 1.0 {
            inventory.add(Item::Wheat, 1);
            if frand(&mut rng) < 0.5 {
                inventory.add(Item::Seeds, 1);
            }
        } else {
            inventory.add(Item::Seeds, 1); // cut it too early — at least get the seed back
        }
    }
}

/// Every `FARM_TICK` seconds: recompute which farmland is hydrated (chained
/// through farmland up to `HYDRATE_CHAIN` hops from a water block) and revert
/// anything that's stayed dry past `DRY_THRESHOLD`.
fn tick_farmland(
    time: Res<Time>,
    mut timer: ResMut<FarmTimer>,
    mut world: ResMut<ChunkWorld>,
    mut hydrated: ResMut<HydratedFarmland>,
    mut dry: ResMut<DryTimers>,
) {
    timer.0 += time.delta_secs();
    if timer.0 < FARM_TICK {
        return;
    }
    let elapsed = timer.0;
    timer.0 = 0.0;

    let farmland: HashSet<IVec3> = world
        .edits
        .iter()
        .filter(|(_, b)| **b == Block::Farmland)
        .map(|(p, _)| *p)
        .collect();

    let mut new_hydrated = HashSet::new();
    for &pos in &farmland {
        if is_hydrated(&world, pos, &farmland) {
            new_hydrated.insert(pos);
        }
    }

    let mut to_dirt = Vec::new();
    for &pos in &farmland {
        if new_hydrated.contains(&pos) {
            dry.0.remove(&pos);
        } else {
            let t = dry.0.entry(pos).or_insert(0.0);
            *t += elapsed;
            if *t > DRY_THRESHOLD {
                to_dirt.push(pos);
            }
        }
    }
    dry.0.retain(|p, _| farmland.contains(p));
    for pos in to_dirt {
        world.set_block(pos.x, pos.y, pos.z, Block::Dirt);
        dry.0.remove(&pos);
    }

    hydrated.0 = new_hydrated;
}

/// BFS through farmland-connected tiles (4-neighbours, same Y), up to
/// `HYDRATE_CHAIN` hops, checking every visited tile's 6 neighbours for water.
fn is_hydrated(world: &ChunkWorld, start: IVec3, farmland: &HashSet<IVec3>) -> bool {
    let mut visited = HashSet::new();
    let mut frontier = vec![(start, 0i32)];
    visited.insert(start);
    while let Some((pos, dist)) = frontier.pop() {
        for d in [IVec3::X, -IVec3::X, IVec3::Z, -IVec3::Z, IVec3::Y, -IVec3::Y] {
            let n = pos + d;
            if world.get_loaded(n.x, n.y, n.z) == Some(Block::Water) {
                return true;
            }
        }
        if dist >= HYDRATE_CHAIN {
            continue;
        }
        for d in [IVec3::X, -IVec3::X, IVec3::Z, -IVec3::Z] {
            let n = pos + d;
            if farmland.contains(&n) && visited.insert(n) {
                frontier.push((n, dist + 1));
            }
        }
    }
    false
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
