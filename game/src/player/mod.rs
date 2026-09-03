//! Player: point-&-click movement (League / Diablo style) with gravity,
//! swept-AABB voxel collision and automatic 1-block step-up.
//!
//! This module also groups everything the player drives: the eagle `camera`, the
//! `firstperson` view, block `interact`ion, remappable `keybinds`, `skins` and
//! the `station` key. All re-exported flat at the crate root by `main.rs`.

pub mod camera;
pub mod firstperson;
pub mod interact;
pub mod keybinds;
pub mod skins;
pub mod station;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::camera::MainCamera;
use crate::chunk::CHUNK_HEIGHT;
use crate::chunk_material::CutoutSettings;
use crate::container::OpenContainer;
use crate::fishing::FishingState;
use crate::interact::Ghost;
use crate::item::{Inventory, InventoryOpen, Item};
use crate::net::ChatLog;
use crate::pause::{GameFlow, Paused};
use crate::skins::SkinChoice;
use crate::station::StationChoices;
use crate::streaming::ChunkWorld;
use crate::worldgen::WorldGenHandle;

/// The player only moves when nothing is capturing input: not paused, in-game,
/// and no inventory / container / fishing / station / chat menu open.
#[allow(clippy::too_many_arguments)]
pub fn player_free(
    paused: Res<Paused>,
    flow: Res<State<GameFlow>>,
    inventory_open: Res<InventoryOpen>,
    container: Res<OpenContainer>,
    station_menu: Res<StationChoices>,
    fishing: Res<FishingState>,
    chat: Res<ChatLog>,
) -> bool {
    !paused.0
        && matches!(flow.get(), GameFlow::Playing)
        && !inventory_open.0
        && container.0.is_none()
        && station_menu.0.is_empty()
        && !fishing.busy()
        && !chat.capturing()
}

/// Where a loaded save / a joined server wants the player to appear (consumed by
/// `spawn_player`).
#[derive(Resource)]
pub struct PendingPlayerSpawn {
    pub pos: Vec3,
}

const GRAVITY: f32 = 26.0;
const TERMINAL: f32 = 55.0;
const WALK_SPEED: f32 = 6.5;
const RUN_SPEED: f32 = 10.5;
const JUMP_SPEED: f32 = 9.0;

const STEP_HEIGHT: f32 = 1.05;
const ARRIVE_RADIUS: f32 = 0.25;
const STUCK_GIVEUP: f32 = 0.8;

// Half-extents of the player's collision box.
const HALF_X: f32 = 0.3;
const HALF_Y: f32 = 0.9;
const HALF_Z: f32 = 0.3;

// --- Visual models ------------------------------------------------------
pub const PLAYER_MODEL: &str = "models/player/Player.glb";
/// The Blockbench model is ~0.69 units tall, origin at the feet.
pub const PLAYER_MODEL_SCALE: f32 = 2.6;
/// Local Y to drop the model so its feet sit on the collision-box floor.
pub const PLAYER_MODEL_DROP: f32 = -HALF_Y;
/// Items that show a glTF model in the hand (paths live in `Item::hand_model`).
const HELD_MODEL_ITEMS: [Item; 6] = [
    Item::FishingRod,
    Item::Knife,
    Item::Axe,
    Item::Pick,
    Item::Shovel,
    Item::Sickle,
];
/// glTF node the held item is parented to, so it follows the arm (and its
/// walk-cycle swing).
const HAND_BONE: &str = "right_arm";
/// Position of the hand (centre of the bottom face of the `right_arm` mesh) in
/// that node's local space — raw model units; the anchor cancels the model
/// scale itself.
const HAND_LOCAL: Vec3 = Vec3::new(0.0625, -0.125, 0.0);
/// Below this horizontal speed the walk cycle holds still.
const WALK_ANIM_MIN_SPEED: f32 = 0.5;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerSpawned>()
            .add_systems(Startup, setup_player_assets)
            .add_systems(
                Update,
                (spawn_player, update_held_item).run_if(in_state(GameFlow::Playing)),
            )
            .add_systems(
                Update,
                (
                    apply_player_skin,
                    attach_held_to_arm,
                    attach_player_anim,
                    drive_player_anim,
                ),
            )
            // The blob shadow follows the model, which is hidden in first person.
            .add_systems(
                Update,
                update_player_shadow.run_if(crate::firstperson::eagle_view),
            )
            .add_systems(
                Update,
                (issue_move_order, player_movement, update_move_marker)
                    .chain()
                    .run_if(player_free)
                    // The eagle-view point-&-click scheme; first-person has its
                    // own movement in `firstperson.rs`.
                    .run_if(crate::firstperson::eagle_view),
            );
    }
}

#[derive(Component)]
pub struct Player;

#[derive(Component, Default)]
pub struct PlayerBody {
    pub velocity: Vec3,
    pub grounded: bool,
    pub fly: bool,
}

/// Current click-to-move destination in world space (only XZ is steered to).
#[derive(Component, Default)]
pub struct MoveOrder {
    pub target: Option<Vec3>,
    stuck_timer: f32,
}

#[derive(Component)]
pub struct MoveMarker;

/// Transform anchor in the player's hand; parents the held visuals.
#[derive(Component)]
struct HeldItem;
/// Flat cube shown for a generic selected item (textured or tinted).
#[derive(Component)]
struct HeldCube;
/// A per-item glTF model in the hand, shown only when that item is selected.
#[derive(Component)]
struct HeldModel(Item);
/// The `Player.glb` scene root (child of the `Player` entity).
#[derive(Component)]
pub struct PlayerModel;
/// Soft blob shadow that tracks the player on the ground.
#[derive(Component)]
pub struct PlayerShadow;
/// The scene entity Bevy attached an `AnimationPlayer` to.
#[derive(Component)]
struct PlayerAnimTarget;

#[derive(Resource)]
struct PlayerAnimAssets {
    graph: Handle<AnimationGraph>,
    walk: AnimationNodeIndex,
}

#[derive(Resource, Default)]
struct PlayerSpawned(bool);

fn setup_player_assets(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let clip = server.load(GltfAssetLabel::Animation(0).from_asset(PLAYER_MODEL));
    let (graph, walk) = AnimationGraph::from_clip(clip);
    commands.insert_resource(PlayerAnimAssets {
        graph: graphs.add(graph),
        walk,
    });
}

fn spawn_player(
    mut commands: Commands,
    mut spawned: ResMut<PlayerSpawned>,
    pending: Option<Res<PendingPlayerSpawn>>,
    world: Res<ChunkWorld>,
    world_gen: Option<Res<WorldGenHandle>>,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if spawned.0 {
        return;
    }

    let start = if let Some(p) = pending.as_deref() {
        // Loaded save / joined a server: appear at the stored position.
        p.pos
    } else {
        // New game: wait for the spawn column, then drop onto the surface.
        let ready = world
            .chunks
            .get(&IVec2::ZERO)
            .map(|s| s.is_meshed())
            .unwrap_or(false);
        let Some(world_gen) = world_gen.as_deref() else {
            return;
        };
        if !ready {
            return;
        }
        // Never drop the player into the sea — find the nearest dry land.
        let (sx, sz) = world_gen.0.find_land(0, 0);
        let h = world_gen.0.surface_height(sx, sz);
        Vec3::new(sx as f32 + 0.5, h as f32 + 3.0, sz as f32 + 0.5)
    };

    commands
        .spawn((
            Player,
            PlayerBody::default(),
            MoveOrder::default(),
            Transform::from_translation(start),
            Visibility::default(),
        ))
        .with_children(|c| {
            // Blocky character model. Its feet sit at the origin and it faces
            // -Z, so drop it to the box floor and spin it to face travel (+Z).
            c.spawn((
                PlayerModel,
                WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(PLAYER_MODEL))),
                Transform::from_xyz(0.0, -HALF_Y, 0.0)
                    .with_scale(Vec3::splat(PLAYER_MODEL_SCALE))
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
            ));

            // Hand anchor. Spawned here so it exists immediately; `attach_held_
            // _to_arm` re-parents it onto the model's `right_arm` node (and only
            // then makes it visible) so it rides the walk-cycle swing. Its scale
            // cancels the model scale so the children keep their authored size.
            c.spawn((
                HeldItem,
                Transform::from_scale(Vec3::splat(1.0 / PLAYER_MODEL_SCALE)),
                Visibility::Hidden,
            ))
            .with_children(|hand| {
                hand.spawn((
                    HeldCube,
                    Mesh3d(meshes.add(Cuboid::new(0.28, 0.28, 0.28))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::WHITE,
                        perceptual_roughness: 0.75,
                        ..default()
                    })),
                    // Lift a block up into the fist rather than around the wrist.
                    Transform::from_xyz(0.0, 0.14, 0.0),
                    Visibility::Hidden,
                ));
                for item in HELD_MODEL_ITEMS {
                    let Some(path) = item.hand_model() else {
                        continue;
                    };
                    hand.spawn((
                        HeldModel(item),
                        WorldAssetRoot(
                            server.load(GltfAssetLabel::Scene(0).from_asset(path)),
                        ),
                        hand_model_transform(item),
                        Visibility::Hidden,
                    ));
                }
            });
        });

    commands.spawn((
        MoveMarker,
        Mesh3d(meshes.add(Cylinder::new(0.42, 0.06))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.85, 0.25, 0.85),
            emissive: LinearRgba::rgb(0.7, 0.5, 0.05),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0.0, -1000.0, 0.0),
        Visibility::Hidden,
    ));

    // Fake contact shadow — `update_player_shadow` snaps it to the ground.
    commands.spawn((
        PlayerShadow,
        Mesh3d(meshes.add(Circle::new(0.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.0, 0.4),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_xyz(0.0, -1000.0, 0.0)
            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        NotShadowCaster,
        Visibility::Hidden,
    ));

    spawned.0 = true;
    if pending.is_some() {
        commands.remove_resource::<PendingPlayerSpawn>();
    }
    info!("Player spawned at {start:?}");
}

/// Right mouse button (held) sets / updates the move target by raycasting the
/// cursor into the voxel world.
fn issue_move_order(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    world: Res<ChunkWorld>,
    cutout: Res<CutoutSettings>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut order_q: Query<(&Transform, &mut MoveOrder), With<Player>>,
) {
    if !mouse.pressed(MouseButton::Right) {
        return;
    }
    let (Ok((player_tf, mut order)), Ok(window), Ok((camera, cam_transform))) =
        (order_q.single_mut(), windows.single(), camera_q.single())
    else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cam_transform, cursor) else {
        return;
    };

    // Let the ray pass through blocks the camera cutout is hiding, so clicking
    // near the player doesn't snap the move target onto a translucent ceiling.
    let ghost = Ghost::from(&cutout, player_tf.translation, cam_transform);
    if let Some(hit) = raycast_voxels(&world, ray.origin, *ray.direction, 2000.0, ghost) {
        order.target = Some(hit);
        order.stuck_timer = 0.0;
    }
}

/// One frame of desired player motion, from whatever control scheme is active
/// (point-&-click steering, or first-person WASD).
pub struct MoveInput {
    /// Horizontal move direction, normalised (or zero).
    pub wish: Vec3,
    pub run: bool,
    /// A jump was pressed this frame (caller already checked `grounded`).
    pub jump: bool,
    /// Rotate the body to face `wish` (yes for the eagle view, no in first
    /// person — there the camera does the aiming).
    pub face_wish: bool,
}

/// Gravity, swept collision, 1-block step-up and jump. Shared by the eagle-view
/// and first-person movement systems.
pub fn step_player(
    world: &ChunkWorld,
    transform: &mut Transform,
    body: &mut PlayerBody,
    input: MoveInput,
    dt: f32,
) {
    let mut pos = transform.translation;
    let speed = if input.run { RUN_SPEED } else { WALK_SPEED };

    if body.fly {
        body.velocity = input.wish * (speed * 1.8);
    } else {
        body.velocity.x = input.wish.x * speed;
        body.velocity.z = input.wish.z * speed;
        if input.jump {
            body.velocity.y = JUMP_SPEED;
            body.grounded = false;
        }
        body.velocity.y = (body.velocity.y - GRAVITY * dt).clamp(-TERMINAL, TERMINAL);
    }

    let delta = body.velocity * dt;

    // Horizontal move with automatic step-up.
    let horizontal = Vec3::new(delta.x, 0.0, delta.z);
    let want = pos + horizontal;
    if !collides(world, want) {
        pos = want;
    } else if !body.fly && body.grounded && !collides(world, want + Vec3::Y * STEP_HEIGHT) {
        pos = want + Vec3::Y * STEP_HEIGHT; // climb a one-block ledge
    } else {
        let mut p = pos;
        p.x += delta.x;
        if collides(world, p) {
            p.x -= delta.x;
            body.velocity.x = 0.0;
        }
        p.z += delta.z;
        if collides(world, p) {
            p.z -= delta.z;
            body.velocity.z = 0.0;
        }
        pos = p;
    }

    // Vertical move.
    body.grounded = false;
    pos.y += delta.y;
    if collides(world, pos) {
        pos.y -= delta.y;
        if delta.y < 0.0 {
            body.grounded = true;
        }
        body.velocity.y = 0.0;
    }
    if pos.y < 1.0 {
        pos.y = 1.0;
        body.velocity.y = body.velocity.y.max(0.0);
        body.grounded = true;
    }

    if input.face_wish && input.wish.length_squared() > 1e-4 {
        let target_rot = Quat::from_rotation_y(input.wish.x.atan2(input.wish.z));
        let t = 1.0 - (-12.0 * dt).exp();
        transform.rotation = transform.rotation.slerp(target_rot, t);
    }

    transform.translation = pos;
}

fn player_movement(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::keybinds::Keybinds>,
    world: Res<ChunkWorld>,
    mut query: Query<(&mut Transform, &mut PlayerBody, &mut MoveOrder), With<Player>>,
) {
    use crate::keybinds::Action;
    let Ok((mut transform, mut body, mut order)) = query.single_mut() else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }

    let pos = transform.translation;

    // Steering toward the click target.
    let mut wish = Vec3::ZERO;
    if let Some(target) = order.target {
        let to = Vec3::new(target.x - pos.x, 0.0, target.z - pos.z);
        let dist = to.length();
        if dist < ARRIVE_RADIUS {
            order.target = None;
        } else {
            wish = to / dist;
        }
    }

    let run = binds.pressed(&keys, Action::Run);
    let jump = body.grounded && binds.just_pressed(&keys, Action::Jump);
    step_player(
        &world,
        &mut transform,
        &mut body,
        MoveInput { wish, run, jump, face_wish: true },
        dt,
    );

    // Give up on a target we can't make progress toward.
    if wish.length_squared() > 1e-4 {
        let horizontal_speed = Vec3::new(body.velocity.x, 0.0, body.velocity.z).length();
        if body.grounded && horizontal_speed < 0.4 {
            order.stuck_timer += dt;
            if order.stuck_timer > STUCK_GIVEUP {
                order.target = None;
                order.stuck_timer = 0.0;
            }
        } else {
            order.stuck_timer = 0.0;
        }
    }
}

fn update_move_marker(
    order_q: Query<&MoveOrder, With<Player>>,
    mut marker_q: Query<(&mut Transform, &mut Visibility), With<MoveMarker>>,
) {
    let Ok((mut transform, mut visibility)) = marker_q.single_mut() else {
        return;
    };
    match order_q.single().ok().and_then(|o| o.target) {
        Some(target) => {
            transform.translation = target + Vec3::Y * 0.03;
            *visibility = Visibility::Visible;
        }
        None => *visibility = Visibility::Hidden,
    }
}

/// Per-item pose for its hand model. The rod pose is the one tuned earlier; the
/// flint tools grip the handle with the head pointing up-and-forward.
fn hand_model_transform(item: Item) -> Transform {
    use std::f32::consts::FRAC_PI_2;
    match item {
        Item::FishingRod => Transform::from_scale(Vec3::splat(1.6)).with_rotation(
            Quat::from_euler(EulerRot::XYZ, -0.7, 0.0, -0.25) * Quat::from_rotation_y(-FRAC_PI_2),
        ),
        Item::Knife => Transform::from_xyz(0.0, 0.10, 0.02)
            .with_scale(Vec3::splat(2.1))
            .with_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, 0.0, -0.15)),
        Item::Axe | Item::Pick => Transform::from_xyz(0.0, 0.12, 0.0)
            .with_scale(Vec3::splat(2.1))
            .with_rotation(Quat::from_euler(EulerRot::XYZ, -0.65, 0.0, -0.25)),
        Item::Shovel => Transform::from_xyz(0.0, 0.12, 0.0)
            .with_scale(Vec3::splat(2.1))
            .with_rotation(Quat::from_euler(EulerRot::XYZ, -0.6, 0.0, -0.2)),
        Item::Sickle => Transform::from_xyz(0.0, 0.11, 0.0)
            .with_scale(Vec3::splat(2.1))
            .with_rotation(Quat::from_euler(EulerRot::XYZ, -0.55, 0.0, -0.2)),
        _ => Transform::from_scale(Vec3::splat(2.0)),
    }
}

/// Keeps the in-hand visuals in sync with the selected hotbar slot: a glTF model
/// for items that have one ([`Item::hand_model`]), otherwise a cube that is
/// textured for items with a sprite and flat-tinted for the rest, hidden when
/// the slot is empty.
fn update_held_item(
    inventory: Res<Inventory>,
    server: Res<AssetServer>,
    mut last: Local<Option<Option<Item>>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cube_q: Query<(&mut Visibility, &MeshMaterial3d<StandardMaterial>), With<HeldCube>>,
    mut model_q: Query<(&HeldModel, &mut Visibility), Without<HeldCube>>,
) {
    let selected = inventory.selected_item();
    if *last == Some(selected) {
        return;
    }
    let Ok((mut cube_vis, cube_mat)) = cube_q.single_mut() else {
        return; // player not spawned yet — retry next frame
    };

    let model_item = selected.filter(|it| it.hand_model().is_some());
    for (model, mut vis) in &mut model_q {
        *vis = if Some(model.0) == model_item {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    match (selected, model_item) {
        (Some(item), None) => {
            *cube_vis = Visibility::Visible;
            if let Some(mut mat) = materials.get_mut(&cube_mat.0) {
                match item.texture_path() {
                    Some(path) => {
                        mat.base_color = Color::WHITE;
                        mat.base_color_texture = Some(server.load(path));
                        mat.alpha_mode = AlphaMode::Mask(0.5);
                    }
                    None => {
                        mat.base_color = item.icon_color();
                        mat.base_color_texture = None;
                        mat.alpha_mode = AlphaMode::Opaque;
                    }
                }
            }
        }
        _ => *cube_vis = Visibility::Hidden,
    }
    *last = Some(selected);
}

/// Paints every mesh under `root` with `skin`. Returns how many it touched (0
/// until the scene's meshes have spawned). Shared by the local player and
/// `net`'s remote avatars.
pub fn repaint_skin(
    root: Entity,
    skin: &Handle<Image>,
    children_q: &Query<&Children>,
    mesh_q: &Query<&MeshMaterial3d<StandardMaterial>, With<Mesh3d>>,
    materials: &mut Assets<StandardMaterial>,
) -> usize {
    let mut stack = vec![root];
    let mut painted = 0;
    while let Some(entity) = stack.pop() {
        if let Ok(children) = children_q.get(entity) {
            stack.extend(children.iter());
        }
        if let Ok(handle) = mesh_q.get(entity) {
            if let Some(mut mat) = materials.get_mut(&handle.0) {
                mat.base_color = Color::WHITE;
                mat.base_color_texture = Some(skin.clone());
                // Basic shading: a plain matte surface lit by the scene.
                mat.unlit = false;
                mat.metallic = 0.0;
                mat.perceptual_roughness = 0.95;
            }
            painted += 1;
        }
    }
    painted
}

/// Keeps the local player's model wearing [`SkinChoice`]; re-applies on change.
fn apply_player_skin(
    mut applied: Local<Option<String>>,
    server: Res<AssetServer>,
    choice: Res<SkinChoice>,
    model_q: Query<Entity, With<PlayerModel>>,
    children_q: Query<&Children>,
    mesh_q: Query<&MeshMaterial3d<StandardMaterial>, With<Mesh3d>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if applied.as_deref() == Some(choice.0.as_str()) {
        return;
    }
    let Ok(root) = model_q.single() else {
        return;
    };
    let skin = server.load(choice.asset_path());
    if repaint_skin(root, &skin, &children_q, &mesh_q, &mut materials) > 0 {
        *applied = Some(choice.0.clone());
    }
}

/// Depth-first search for a named node inside a spawned scene.
fn find_named(
    root: Entity,
    name: &str,
    children_q: &Query<&Children>,
    name_q: &Query<&Name>,
) -> Option<Entity> {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if name_q.get(entity).is_ok_and(|n| n.as_str() == name) {
            return Some(entity);
        }
        if let Ok(children) = children_q.get(entity) {
            stack.extend(children.iter());
        }
    }
    None
}

/// Re-parent the hand anchor onto the model's `right_arm` node once the scene
/// exists, so the held item tracks the arm (walk-cycle swing included) with its
/// pivot at the base of the arm.
fn attach_held_to_arm(
    mut commands: Commands,
    mut done: Local<bool>,
    model_q: Query<Entity, With<PlayerModel>>,
    children_q: Query<&Children>,
    name_q: Query<&Name>,
    mut held_q: Query<(Entity, &mut Transform, &mut Visibility), With<HeldItem>>,
) {
    if *done {
        return;
    }
    let (Ok(model), Ok((held, mut transform, mut visibility))) =
        (model_q.single(), held_q.single_mut())
    else {
        return;
    };
    let Some(arm) = find_named(model, HAND_BONE, &children_q, &name_q) else {
        return; // scene still loading
    };

    commands.entity(arm).add_child(held);
    *transform = Transform::from_translation(HAND_LOCAL)
        .with_scale(Vec3::splat(1.0 / PLAYER_MODEL_SCALE));
    *visibility = Visibility::Inherited;
    *done = true;
}

/// Bevy adds an `AnimationPlayer` to any spawned scene that has clips — the
/// player model, but also animated props (the hand mill). Only wire the walk
/// graph to the one that lives under `PlayerModel`.
fn attach_player_anim(
    mut commands: Commands,
    anim: Res<PlayerAnimAssets>,
    model_q: Query<Entity, With<PlayerModel>>,
    parents: Query<&ChildOf>,
    mut players: Query<(Entity, &mut AnimationPlayer), Added<AnimationPlayer>>,
) {
    let Ok(model) = model_q.single() else {
        return;
    };
    for (entity, mut player) in &mut players {
        let under_player = std::iter::successors(Some(entity), |&e| parents.get(e).ok().map(|c| c.parent()))
            .any(|e| e == model);
        if !under_player {
            continue;
        }
        player.play(anim.walk).repeat();
        player.pause_all();
        commands
            .entity(entity)
            .insert((AnimationGraphHandle(anim.graph.clone()), PlayerAnimTarget));
    }
}

/// Play the walk cycle only while the player has horizontal speed; when it
/// stops, rewind to the first frame so the next step starts from the rest pose.
fn drive_player_anim(
    anim: Res<PlayerAnimAssets>,
    body_q: Query<&PlayerBody, With<Player>>,
    mut players: Query<&mut AnimationPlayer, With<PlayerAnimTarget>>,
) {
    let Ok(body) = body_q.single() else {
        return;
    };
    let moving =
        Vec3::new(body.velocity.x, 0.0, body.velocity.z).length() > WALK_ANIM_MIN_SPEED;
    for mut player in &mut players {
        let Some(walk) = player.animation_mut(anim.walk) else {
            continue;
        };
        if moving {
            walk.resume();
        } else {
            walk.pause();
            walk.seek_to(0.0);
        }
    }
}

/// The maximum drop over which the blob shadow still shows.
const SHADOW_FADE_HEIGHT: f32 = 6.0;

/// Snaps the fake contact shadow to the block surface under the player and fades
/// it with height (so it vanishes when jumping or flying high).
fn update_player_shadow(
    world: Res<ChunkWorld>,
    mut last_alpha: Local<f32>,
    player_q: Query<&Transform, (With<Player>, Without<PlayerShadow>)>,
    mut shadow_q: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>),
        With<PlayerShadow>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Ok(player), Ok((mut shadow, mut visibility, material))) =
        (player_q.single(), shadow_q.single_mut())
    else {
        return;
    };

    let feet = player.translation.y - HALF_Y;
    let (bx, bz) = (
        player.translation.x.floor() as i32,
        player.translation.z.floor() as i32,
    );
    let scan_top = (feet + 0.1).floor() as i32;

    let ground = (0..=scan_top.min(CHUNK_HEIGHT - 1))
        .rev()
        .find(|&y| world.block_at(bx, y, bz).is_collidable())
        .map(|y| y as f32 + 1.0);

    let Some(gy) = ground else {
        *visibility = Visibility::Hidden;
        return;
    };
    let height = (feet - gy).max(0.0);
    if height > SHADOW_FADE_HEIGHT {
        *visibility = Visibility::Hidden;
        return;
    }

    let t = height / SHADOW_FADE_HEIGHT;
    shadow.translation = Vec3::new(player.translation.x, gy + 0.03, player.translation.z);
    shadow.scale = Vec3::splat(1.0 - 0.35 * t);
    *visibility = Visibility::Visible;

    let alpha = 0.42 * (1.0 - t);
    if (alpha - *last_alpha).abs() > 0.02 {
        *last_alpha = alpha;
        if let Some(mut mat) = materials.get_mut(&material.0) {
            mat.base_color = Color::srgba(0.0, 0.0, 0.0, alpha);
        }
    }
}

/// Amanatides–Woo voxel DDA. Returns the world-space point where `dir` first
/// enters an opaque block. Unloaded columns and cells hidden by `ghost` (the
/// camera cutout) are treated as empty.
fn raycast_voxels(
    world: &ChunkWorld,
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
    ghost: Option<Ghost>,
) -> Option<Vec3> {
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

    let next_boundary = |o: f32, d: f32| -> f32 {
        if d > 0.0 {
            o.floor() + 1.0 - o
        } else {
            o - o.floor()
        }
    };
    let axis = |o: f32, d: f32| -> (f32, f32) {
        if d == 0.0 {
            (f32::INFINITY, f32::INFINITY)
        } else {
            (next_boundary(o, d) / d.abs(), 1.0 / d.abs())
        }
    };

    let (mut t_max_x, t_delta_x) = axis(origin.x, dir.x);
    let (mut t_max_y, t_delta_y) = axis(origin.y, dir.y);
    let (mut t_max_z, t_delta_z) = axis(origin.z, dir.z);

    let mut t = 0.0f32;
    for _ in 0..4096 {
        let blocked = world.sample_loaded(cell.x, cell.y, cell.z).is_opaque()
            && !ghost.is_some_and(|g| g.hides(cell));
        if blocked {
            return Some(origin + dir * t);
        }
        if t_max_x < t_max_y && t_max_x < t_max_z {
            cell.x += step.x;
            t = t_max_x;
            t_max_x += t_delta_x;
        } else if t_max_y < t_max_z {
            cell.y += step.y;
            t = t_max_y;
            t_max_y += t_delta_y;
        } else {
            cell.z += step.z;
            t = t_max_z;
            t_max_z += t_delta_z;
        }
        if t > max_dist {
            break;
        }
    }
    None
}

fn collides(world: &ChunkWorld, center: Vec3) -> bool {
    let min = center - Vec3::new(HALF_X, HALF_Y, HALF_Z);
    let max = center + Vec3::new(HALF_X, HALF_Y, HALF_Z);
    let (x0, x1) = (min.x.floor() as i32, max.x.floor() as i32);
    let (y0, y1) = (min.y.floor() as i32, max.y.floor() as i32);
    let (z0, z1) = (min.z.floor() as i32, max.z.floor() as i32);
    for x in x0..=x1 {
        for y in y0..=y1 {
            if !(0..CHUNK_HEIGHT).contains(&y) {
                continue;
            }
            for z in z0..=z1 {
                if world.block_at(x, y, z).is_collidable() {
                    return true;
                }
            }
        }
    }
    false
}
