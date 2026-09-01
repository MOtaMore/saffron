//! Breaking blocks (timed, with a progress bar and flying debris), felling
//! trees, placing blocks, and a rectangular room builder (walls + floor).
//!
//! Left mouse button is the "use" button; right mouse stays for movement.
//! - block in hand  -> click places it
//! - empty hand     -> hold to mine the targeted block
//! - room mode (`B`)-> two clicks define a rectangle

use std::collections::HashSet;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::block::Block;
use crate::camera::MainCamera;
use crate::chunk_material::CutoutSettings;
use crate::container::{ChestStores, FurnaceStores, OpenContainer};
use crate::fishing::FishingState;
use crate::item::{Inventory, InventoryOpen, Item, can_harvest, ideal_tool};
use crate::pause::not_paused;
use crate::station::StationChoices;
use crate::player::Player;
use crate::streaming::ChunkWorld;

const RAY_MAX: f32 = 4000.0;
const INTERACT_REACH: f32 = 80.0;
const HOTBAR_GUARD_PX: f32 = 76.0;

const ROOM_HEIGHT: i32 = 4;
const ROOM_MAX_SIDE: i32 = 40;
const TREE_FLOOD_LIMIT: usize = 1024;

/// Debris bursts spread across a full mine.
const MINE_STAGES: u8 = 6;

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuildState>()
            .init_resource::<MiningState>()
            .init_resource::<TargetInfo>()
            .add_systems(Startup, (setup_mining_assets, setup_mining_ui))
            .add_systems(
                Update,
                (
                    interact_click,
                    mining,
                    update_particles,
                    draw_highlight,
                    mining_bar_ui,
                )
                    .chain()
                    .run_if(not_paused),
            );
    }
}

#[derive(Resource, Default)]
pub struct BuildState {
    pub room_mode: bool,
    pub room_corner: Option<IVec3>,
}

#[derive(Resource, Default)]
pub struct MiningState(Option<Mining>);

struct Mining {
    cell: IVec3,
    block: Block,
    progress: f32,
    stage: u8,
}

impl MiningState {
    pub fn progress(&self) -> Option<f32> {
        self.0.as_ref().map(|m| m.progress.clamp(0.0, 1.0))
    }
}

/// What the cursor is currently pointing at, for the HUD.
#[derive(Resource, Default)]
pub struct TargetInfo {
    pub block: Option<Block>,
    pub harvestable: bool,
}

pub struct CellHit {
    pub cell: IVec3,
    /// Face the ray crossed, pointing back towards the camera.
    pub normal: IVec3,
}

fn mine_time(block: Block) -> f32 {
    match block {
        Block::Leaves | Block::WheatCrop => 0.15,
        Block::Grass | Block::Dirt | Block::Sand | Block::Snow | Block::Farmland => 0.4,
        Block::Wood => 0.9,
        Block::Stone => 1.2,
        _ => 0.5,
    }
}

// --- Click: place block / room corners -------------------------------------

#[allow(clippy::too_many_arguments)]
fn interact_click(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    inv_open: Res<InventoryOpen>,
    container_open: Res<OpenContainer>,
    fishing: Res<FishingState>,
    station_menu: Res<StationChoices>,
    cutout: Res<CutoutSettings>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    player_q: Query<&Transform, With<Player>>,
    mut world: ResMut<ChunkWorld>,
    mut inventory: ResMut<Inventory>,
    mut build: ResMut<BuildState>,
) {
    let ui_busy = inv_open.0
        || container_open.0.is_some()
        || fishing.busy()
        || !station_menu.0.is_empty();
    if !ui_busy && keys.just_pressed(KeyCode::KeyB) {
        build.room_mode = !build.room_mode;
        build.room_corner = None;
    }
    if ui_busy || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // Left-click on a chest/furnace opens it (handled in `container.rs`) — don't
    // also place a block there unless something is actually in hand.
    let placing = matches!(inventory.selected_item(), Some(Item::Block(_)));
    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), camera_q.single()) else {
        return;
    };
    let player_pos = player_q.iter().next().map(|t| t.translation);
    let ghost = ghost_region(&cutout, player_pos, cam_tf);
    let Some(hit) = pick(window, camera, cam_tf, &world, player_pos, ghost) else {
        return;
    };

    if build.room_mode {
        room_click(&mut world, &mut inventory, &mut build, &hit, player_pos);
        return;
    }

    // Pointing at an interactable block with nothing to place -> leave it for
    // `container.rs` to open.
    let pointed = world.get_loaded(hit.cell.x, hit.cell.y, hit.cell.z);
    if !placing && matches!(pointed, Some(Block::Chest) | Some(Block::Furnace)) {
        return;
    }

    if let Some(Item::Block(block)) = inventory.selected_item() {
        let target = hit.cell + hit.normal;
        if can_place(&world, target, player_pos)
            && world.set_block(target.x, target.y, target.z, block)
        {
            inventory.take(Item::Block(block), 1);
        }
    }
}

// --- Hold: mine the targeted block over time -------------------------------

/// Read-only "is another mode capturing input?" resources, bundled to keep
/// `mining`'s parameter count under Bevy's 16-tuple limit.
#[derive(SystemParam)]
struct Guards<'w> {
    build: Res<'w, BuildState>,
    inv_open: Res<'w, InventoryOpen>,
    container_open: Res<'w, OpenContainer>,
    fishing: Res<'w, FishingState>,
    station_menu: Res<'w, StationChoices>,
    cutout: Res<'w, CutoutSettings>,
}

fn ghost_region(
    cutout: &CutoutSettings,
    player_pos: Option<Vec3>,
    cam_tf: &GlobalTransform,
) -> Option<Ghost> {
    Ghost::from(cutout, player_pos?, cam_tf)
}

#[allow(clippy::too_many_arguments)]
fn mining(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    player_q: Query<&Transform, With<Player>>,
    guards: Guards,
    mut world: ResMut<ChunkWorld>,
    mut inventory: ResMut<Inventory>,
    mut chests: ResMut<ChestStores>,
    mut furnaces: ResMut<FurnaceStores>,
    mut state: ResMut<MiningState>,
    mut commands: Commands,
    assets: Res<MiningAssets>,
    mut rng: Local<u32>,
) {
    let selected = inventory.selected_item();
    let holding_block = matches!(selected, Some(Item::Block(_)));
    let tool = selected.and_then(Item::tool);

    let can_mine = mouse.pressed(MouseButton::Left)
        && !guards.build.room_mode
        && !guards.inv_open.0
        && guards.container_open.0.is_none()
        && !guards.fishing.busy()
        && guards.station_menu.0.is_empty()
        && !holding_block;
    if !can_mine {
        state.0 = None;
        return;
    }

    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), camera_q.single()) else {
        state.0 = None;
        return;
    };
    let player_pos = player_q.iter().next().map(|t| t.translation);
    let ghost = ghost_region(&guards.cutout, player_pos, cam_tf);
    let Some(hit) = pick(window, camera, cam_tf, &world, player_pos, ghost) else {
        state.0 = None;
        return;
    };
    let Some(target) = world.get_loaded(hit.cell.x, hit.cell.y, hit.cell.z) else {
        state.0 = None;
        return;
    };
    // Wrong / missing tool: can't even start.
    if !target.is_breakable() || !can_harvest(target, tool) {
        state.0 = None;
        return;
    }
    // Chests / furnaces are opened, not mined — hold Shift to actually break one.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if matches!(target, Block::Chest | Block::Furnace | Block::HandMill) && !shift {
        state.0 = None;
        return;
    }

    // Start over if the cursor moved to a different block.
    if !matches!(&state.0, Some(m) if m.cell == hit.cell) {
        state.0 = Some(Mining {
            cell: hit.cell,
            block: target,
            progress: 0.0,
            stage: 0,
        });
        return;
    }

    let fast = tool.is_some() && tool == ideal_tool(target);
    let duration = mine_time(target) * if fast { 0.55 } else { 1.0 };

    let m = state.0.as_mut().unwrap();
    m.progress += time.delta_secs() / duration;

    let reached = ((m.progress * MINE_STAGES as f32) as u8).min(MINE_STAGES);
    if reached > m.stage {
        m.stage = reached;
        spawn_debris(&mut commands, &assets, &mut rng, m.cell, m.block, 2);
    }

    if m.progress >= 1.0 {
        let (cell, block) = (m.cell, m.block);
        state.0 = None;
        spawn_debris(&mut commands, &assets, &mut rng, cell, block, 6);
        if matches!(block, Block::Chest | Block::Furnace) {
            crate::container::on_broken(cell, &mut chests, &mut furnaces, &mut inventory);
        }
        if block == Block::Wood {
            fell_tree(&mut world, &mut inventory, cell);
        } else if world.set_block(cell.x, cell.y, cell.z, Block::Air) {
            grant_drop(&mut inventory, block, &mut rng);
        }
    }
}

/// Puts the break result into the inventory. Gravel has a chance to give flint.
fn grant_drop(inventory: &mut Inventory, block: Block, rng: &mut u32) {
    if block == Block::Gravel {
        if rf(rng) < 0.35 {
            inventory.add(Item::Flint, 1);
        } else {
            inventory.add(Item::Block(Block::Gravel), 1);
        }
        return;
    }
    if let Some(drop) = block.drop_item() {
        inventory.add(Item::Block(drop), 1);
    }
}

// --- Room builder ---------------------------------------------------------

fn room_click(
    world: &mut ChunkWorld,
    inventory: &mut Inventory,
    build: &mut BuildState,
    hit: &CellHit,
    player: Option<Vec3>,
) {
    let cell = hit.cell + hit.normal;
    match build.room_corner {
        None => build.room_corner = Some(cell),
        Some(a) => {
            build.room_corner = None;
            let Some(block) = inventory.selected_item().and_then(Item::as_block) else {
                return;
            };
            build_room(world, inventory, a, cell, block, player);
        }
    }
}

fn build_room(
    world: &mut ChunkWorld,
    inventory: &mut Inventory,
    a: IVec3,
    b: IVec3,
    block: Block,
    player: Option<Vec3>,
) {
    let base = a.y;
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (z0, z1) = (a.z.min(b.z), a.z.max(b.z));
    if x1 - x0 > ROOM_MAX_SIDE || z1 - z0 > ROOM_MAX_SIDE {
        return;
    }

    let mut budget = inventory.count(Item::Block(block));
    let mut used = 0u32;

    for x in x0..=x1 {
        for z in z0..=z1 {
            let perimeter = x == x0 || x == x1 || z == z0 || z == z1;
            for y in base..base + ROOM_HEIGHT {
                let is_floor = y == base;
                if !is_floor && !perimeter {
                    continue;
                }
                if budget == 0 {
                    inventory.take(Item::Block(block), used);
                    return;
                }
                let cell = IVec3::new(x, y, z);
                if can_build_over(world, cell, player) && world.set_block(x, y, z, block) {
                    budget -= 1;
                    used += 1;
                }
            }
        }
    }
    inventory.take(Item::Block(block), used);
}

fn is_block(world: &ChunkWorld, x: i32, y: i32, z: i32, block: Block) -> bool {
    world.get_loaded(x, y, z) == Some(block)
}

/// Chops a natural tree: the single vertical trunk column through `start`, plus
/// its own nearby canopy. Player-placed wood (long runs, no leaves, tall pillars)
/// just breaks one block, and neighbouring trees keep their trunks.
fn fell_tree(world: &mut ChunkWorld, inventory: &mut Inventory, start: IVec3) {
    // Extent of the vertical trunk run through `start` (bounded).
    let mut bottom = start.y;
    for _ in 0..24 {
        if is_block(world, start.x, bottom - 1, start.z, Block::Wood) {
            bottom -= 1;
        } else {
            break;
        }
    }
    let mut top = start.y;
    for _ in 0..24 {
        if is_block(world, start.x, top + 1, start.z, Block::Wood) {
            top += 1;
        } else {
            break;
        }
    }

    // A real tree = a short trunk topped with leaves.
    let canopy_near_top = (-1..=3).any(|dy| {
        (-2..=2).any(|dx| {
            (-2..=2).any(|dz| is_block(world, start.x + dx, top + dy, start.z + dz, Block::Leaves))
        })
    });
    if top - bottom > 8 || !canopy_near_top {
        if world.set_block(start.x, start.y, start.z, Block::Air) {
            inventory.add(Item::Block(Block::Wood), 1);
        }
        return;
    }

    // Trunk column.
    let mut logs = 0u32;
    for y in bottom..=top {
        if world.set_block(start.x, y, start.z, Block::Air) {
            logs += 1;
        }
    }

    // This tree's canopy: flood through leaves only (never wood, so we can't
    // hop onto a neighbour's trunk), kept within a small box around the trunk.
    let mut stack: Vec<IVec3> = Vec::new();
    let mut seen: HashSet<IVec3> = HashSet::new();
    for dy in -1..=3 {
        for dx in -2..=2 {
            for dz in -2..=2 {
                let p = IVec3::new(start.x + dx, top + dy, start.z + dz);
                if is_block(world, p.x, p.y, p.z, Block::Leaves) && seen.insert(p) {
                    stack.push(p);
                }
            }
        }
    }
    let mut visited = 0;
    while let Some(c) = stack.pop() {
        visited += 1;
        if visited > TREE_FLOOD_LIMIT {
            break;
        }
        if !is_block(world, c.x, c.y, c.z, Block::Leaves) {
            continue;
        }
        if (c.x - start.x).abs() > 3 || (c.z - start.z).abs() > 3 || c.y < bottom || c.y > top + 4 {
            continue;
        }
        world.set_block(c.x, c.y, c.z, Block::Air);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let n = c + IVec3::new(dx, dy, dz);
                    if is_block(world, n.x, n.y, n.z, Block::Leaves) && seen.insert(n) {
                        stack.push(n);
                    }
                }
            }
        }
    }

    if logs > 0 {
        inventory.add(Item::Block(Block::Wood), logs); // leaves give nothing
    }
}

// --- Debris particles ---------------------------------------------------------

#[derive(Resource)]
struct MiningAssets {
    cube: Handle<Mesh>,
    /// Indexed by `block as usize` (enum order).
    particle_mat: [Handle<StandardMaterial>; 20],
}

#[derive(Component)]
struct Particle {
    velocity: Vec3,
    life: f32,
}

fn setup_mining_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let order = [
        Block::Air,
        Block::Grass,
        Block::Dirt,
        Block::Stone,
        Block::Sand,
        Block::Snow,
        Block::Water,
        Block::Wood,
        Block::Leaves,
        Block::Bedrock,
        Block::Gravel,
        Block::Workbench,
        Block::Chest,
        Block::Furnace,
        Block::Glass,
        Block::WoodPlanks,
        Block::Torch,
        Block::Farmland,
        Block::WheatCrop,
        Block::HandMill,
    ];
    let particle_mat = order.map(|b| {
        let c = b.color();
        materials.add(StandardMaterial {
            base_color: Color::srgb(c[0], c[1], c[2]),
            unlit: true,
            ..default()
        })
    });
    commands.insert_resource(MiningAssets { cube, particle_mat });
}

fn spawn_debris(
    commands: &mut Commands,
    assets: &MiningAssets,
    rng: &mut u32,
    cell: IVec3,
    block: Block,
    n: usize,
) {
    let base = cell.as_vec3() + Vec3::splat(0.5);
    for _ in 0..n {
        let jitter = Vec3::new(rf(rng) - 0.5, rf(rng) - 0.5, rf(rng) - 0.5) * 0.7;
        let vel = Vec3::new(rf(rng) * 2.0 - 1.0, rf(rng) * 1.4 + 1.6, rf(rng) * 2.0 - 1.0) * 3.0;
        commands.spawn((
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(assets.particle_mat[block as usize].clone()),
            Transform::from_translation(base + jitter).with_scale(Vec3::splat(0.18)),
            Particle {
                velocity: vel,
                life: 0.7,
            },
        ));
    }
}

fn update_particles(
    time: Res<Time>,
    mut commands: Commands,
    mut particles: Query<(Entity, &mut Transform, &mut Particle)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut particle) in &mut particles {
        particle.life -= dt;
        if particle.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        particle.velocity.y -= 20.0 * dt;
        transform.translation += particle.velocity * dt;
        transform.scale *= (1.0 - dt * 1.6).max(0.0);
    }
}

/// Tiny xorshift, seeded lazily.
fn rf(state: &mut u32) -> f32 {
    if *state == 0 {
        *state = 0x9E37_79B9;
    }
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

// --- Progress bar UI -------------------------------------------------------

#[derive(Component)]
struct MiningBar;

#[derive(Component)]
struct MiningBarFill;

fn setup_mining_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(120.0),
                height: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            Visibility::Hidden,
            MiningBar,
        ))
        .with_children(|bar| {
            bar.spawn((
                Node {
                    width: Val::Percent(0.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.45, 0.85, 0.35)),
                MiningBarFill,
            ));
        });
}

fn mining_bar_ui(
    state: Res<MiningState>,
    windows: Query<&Window>,
    mut bar: Query<(&mut Node, &mut Visibility), With<MiningBar>>,
    mut fill: Query<&mut Node, (With<MiningBarFill>, Without<MiningBar>)>,
) {
    let (Ok((mut node, mut visibility)), Ok(mut fill)) = (bar.single_mut(), fill.single_mut()) else {
        return;
    };
    match state.progress() {
        Some(progress) => {
            *visibility = Visibility::Visible;
            if let Ok(window) = windows.single() {
                if let Some(cursor) = window.cursor_position() {
                    node.left = Val::Px(cursor.x - 60.0);
                    node.top = Val::Px(cursor.y + 20.0);
                }
            }
            fill.width = Val::Percent(progress * 100.0);
        }
        None => *visibility = Visibility::Hidden,
    }
}

// --- Target highlight -----------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_highlight(
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    player_q: Query<&Transform, With<Player>>,
    world: Res<ChunkWorld>,
    inventory: Res<Inventory>,
    inv_open: Res<InventoryOpen>,
    cutout: Res<CutoutSettings>,
    build: Res<BuildState>,
    state: Res<MiningState>,
    mut target_info: ResMut<TargetInfo>,
    mut gizmos: Gizmos,
) {
    *target_info = TargetInfo::default();

    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), camera_q.single()) else {
        return;
    };
    if inv_open.0 {
        return;
    }
    let player_pos = player_q.iter().next().map(|t| t.translation);
    let ghost = ghost_region(&cutout, player_pos, cam_tf);
    let Some(hit) = pick(window, camera, cam_tf, &world, player_pos, ghost) else {
        return;
    };

    let selected = inventory.selected_item();
    let placing = matches!(selected, Some(Item::Block(_)));
    if !build.room_mode && !placing {
        if let Some(block) = world.get_loaded(hit.cell.x, hit.cell.y, hit.cell.z) {
            target_info.block = Some(block);
            target_info.harvestable = block.is_breakable()
                && can_harvest(block, selected.and_then(Item::tool));
        }
    }

    // Mining in progress on this block: shrink the outline as it dismantles.
    if let Some(m) = &state.0 {
        if m.cell == hit.cell {
            let scale = (1.0 - 0.8 * m.progress).max(0.06) * 1.02;
            let color = Color::srgb(1.0, 0.55 - 0.45 * m.progress, 0.18);
            gizmos.cube(
                Transform::from_translation(m.cell.as_vec3() + Vec3::splat(0.5))
                    .with_scale(Vec3::splat(scale)),
                color,
            );
            return;
        }
    }

    let (cell, color) = if build.room_mode {
        (hit.cell + hit.normal, Color::srgb(1.0, 0.8, 0.2))
    } else if placing {
        (hit.cell + hit.normal, Color::srgb(0.4, 0.9, 1.0))
    } else if target_info.harvestable {
        (hit.cell, Color::srgb(1.0, 0.35, 0.35))
    } else {
        (hit.cell, Color::srgb(0.45, 0.45, 0.45)) // can't harvest with this tool
    };
    gizmos.cube(
        Transform::from_translation(cell.as_vec3() + Vec3::splat(0.5)).with_scale(Vec3::splat(1.02)),
        color,
    );

    if build.room_mode {
        if let Some(a) = build.room_corner {
            let b = hit.cell + hit.normal;
            let min = a.min(b).as_vec3();
            let max = a.max(b).as_vec3() + Vec3::new(1.0, ROOM_HEIGHT as f32, 1.0);
            gizmos.cube(
                Transform::from_translation((min + max) * 0.5).with_scale(max - min),
                Color::srgb(1.0, 0.8, 0.2),
            );
        }
    }
}

// --- Shared picking ------------------------------------------------------

fn pick(
    window: &Window,
    camera: &Camera,
    cam_tf: &GlobalTransform,
    world: &ChunkWorld,
    player_pos: Option<Vec3>,
    ghost: Option<Ghost>,
) -> Option<CellHit> {
    let cursor = window.cursor_position()?;
    if cursor.y > window.height() - HOTBAR_GUARD_PX {
        return None;
    }
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    let hit = raycast_cell(world, ray.origin, *ray.direction, RAY_MAX, ghost)?;
    if let Some(p) = player_pos {
        if (hit.cell.as_vec3() + Vec3::splat(0.5)).distance(p) > INTERACT_REACH {
            return None;
        }
    }
    Some(hit)
}

fn can_place(world: &ChunkWorld, cell: IVec3, player: Option<Vec3>) -> bool {
    match world.get_loaded(cell.x, cell.y, cell.z) {
        Some(Block::Air) | Some(Block::Water) => {}
        _ => return false,
    }
    !player_overlaps(cell, player)
}

fn can_build_over(world: &ChunkWorld, cell: IVec3, player: Option<Vec3>) -> bool {
    match world.get_loaded(cell.x, cell.y, cell.z) {
        None | Some(Block::Bedrock) => false,
        _ => !player_overlaps(cell, player),
    }
}

fn player_overlaps(cell: IVec3, player: Option<Vec3>) -> bool {
    let Some(p) = player else {
        return false;
    };
    let cmin = cell.as_vec3();
    let cmax = cmin + Vec3::ONE;
    let pmin = p - Vec3::new(0.3, 0.9, 0.3);
    let pmax = p + Vec3::new(0.3, 0.9, 0.3);
    pmin.x < cmax.x
        && pmax.x > cmin.x
        && pmin.y < cmax.y
        && pmax.y > cmin.y
        && pmin.z < cmax.z
        && pmax.z > cmin.z
}

/// The camera-cutout region: cells inside it are being drawn translucent so the
/// picking ray passes straight through them (`chunk_material` shader mirror).
#[derive(Clone, Copy)]
pub struct Ghost {
    pub player: Vec3,
    pub view_dir: Vec3,
    pub radius: f32,
}

impl Ghost {
    pub fn hides(&self, cell: IVec3) -> bool {
        let rel = cell.as_vec3() + Vec3::splat(0.5) - self.player;
        let along = rel.dot(self.view_dir);
        along < -0.25 && (rel - along * self.view_dir).length() < self.radius
    }

    /// Cutout region for `player` viewed along `cam_tf`'s forward, or `None` when
    /// the effect is disabled.
    pub fn from(cutout: &CutoutSettings, player: Vec3, cam_tf: &GlobalTransform) -> Option<Ghost> {
        cutout.enabled.then(|| Ghost {
            player,
            view_dir: cam_tf.forward().as_vec3(),
            radius: cutout.radius,
        })
    }
}

/// Amanatides–Woo voxel DDA returning the first solid cell and the face hit.
/// Unloaded columns are treated as empty; cells hidden by `ghost` are skipped.
#[allow(unused_assignments)]
pub fn raycast_cell(
    world: &ChunkWorld,
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
    ghost: Option<Ghost>,
) -> Option<CellHit> {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }

    let mut cell = IVec3::new(
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    );
    let step = IVec3::new(
        dir.x.signum() as i32,
        dir.y.signum() as i32,
        dir.z.signum() as i32,
    );
    let inv = Vec3::new(
        if dir.x != 0.0 { 1.0 / dir.x.abs() } else { f32::INFINITY },
        if dir.y != 0.0 { 1.0 / dir.y.abs() } else { f32::INFINITY },
        if dir.z != 0.0 { 1.0 / dir.z.abs() } else { f32::INFINITY },
    );
    let edge = |o: f32, d: f32| {
        if d > 0.0 {
            o.floor() + 1.0 - o
        } else {
            o - o.floor()
        }
    };
    let mut t_max = Vec3::new(
        if dir.x != 0.0 { edge(origin.x, dir.x) * inv.x } else { f32::INFINITY },
        if dir.y != 0.0 { edge(origin.y, dir.y) * inv.y } else { f32::INFINITY },
        if dir.z != 0.0 { edge(origin.z, dir.z) * inv.z } else { f32::INFINITY },
    );

    let mut normal = IVec3::Y;
    let mut t = 0.0f32;
    for _ in 0..8192 {
        let solid = world
            .get_loaded(cell.x, cell.y, cell.z)
            .is_some_and(Block::is_solid);
        if solid && !ghost.is_some_and(|g| g.hides(cell)) {
            return Some(CellHit { cell, normal });
        }
        if t_max.x <= t_max.y && t_max.x <= t_max.z {
            cell.x += step.x;
            normal = IVec3::new(-step.x, 0, 0);
            t = t_max.x;
            t_max.x += inv.x;
        } else if t_max.y <= t_max.z {
            cell.y += step.y;
            normal = IVec3::new(0, -step.y, 0);
            t = t_max.y;
            t_max.y += inv.y;
        } else {
            cell.z += step.z;
            normal = IVec3::new(0, 0, -step.z);
            t = t_max.z;
            t_max.z += inv.z;
        }
        if t > max_dist {
            break;
        }
    }
    None
}
