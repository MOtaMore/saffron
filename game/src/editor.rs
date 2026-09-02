//! Standalone **Structure Editor** (`GameFlow::Editor`): no player, no survival,
//! no hotbar. A free-fly camera and a content browser to build structures out of
//! any block / model prop, then export them to `structures/*.json`. The base
//! game / mods load those with `structure::{load_structure, stamp_structure}`.
//!
//! Camera: right-mouse = look, WASD = fly, Space / Ctrl = up / down,
//! Shift = faster, wheel = speed.
//!
//! **Build tool**: left-click places the brush block on the pointed face (or 8 m
//! ahead in empty space); the `Erase` brush or Alt+click removes.
//!
//! **Select tool**: left-click picks a block, Shift+click toggles one in/out of
//! the group, Ctrl+click selects everything. Arrow keys / PageUp-PageDown nudge
//! the selection by one cell; `Delete` removes it.
//!
//! `Textured` toggles between the real atlas texture and a flat-colour preview
//! (plain blocks only; model props always show their GLB).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::block::{ATLAS_COLS, Block};
use crate::block_atlas::BlockAtlasImage;
use crate::hud::HudText;
use crate::item::HotbarRoot;
use crate::pause::GameFlow;
use crate::props::model_path;
use crate::structure::{SpawnRule, Structure, from_cells, load_structure};
use crate::survival::SurvivalUi;

const STRUCT_DIR: &str = "structures";
const PANEL_W: f32 = 250.0;
const AIR_PLACE_DIST: f32 = 8.0;
const EDITOR_SKY: Color = Color::srgb(0.14, 0.15, 0.18);
const GAME_SKY: Color = Color::srgb(0.53, 0.72, 0.92);

/// Everything the browser can place (blocks + model props).
const PALETTE: &[Block] = &[
    Block::Grass,
    Block::Dirt,
    Block::Stone,
    Block::Sand,
    Block::Gravel,
    Block::Snow,
    Block::Wood,
    Block::WoodPlanks,
    Block::Leaves,
    Block::Glass,
    Block::Bedrock,
    Block::Water,
    Block::Farmland,
    Block::WheatCrop,
    Block::Workbench,
    Block::Chest,
    Block::Furnace,
    Block::HandMill,
    Block::Torch,
];

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .init_resource::<TexAssets>()
            .add_systems(Startup, (setup_editor_assets, spawn_panel))
            .add_systems(OnEnter(GameFlow::Editor), enter_editor)
            .add_systems(OnExit(GameFlow::Editor), exit_editor)
            .add_systems(
                Update,
                (
                    build_tex_assets,
                    editor_camera,
                    editor_click,
                    editor_move_selection,
                    editor_render,
                    editor_grid_gizmo,
                    editor_escape,
                    palette_buttons,
                    panel_buttons,
                    name_capture,
                    rebuild_file_list,
                    sync_panel_text,
                    panel_button_visuals,
                )
                    .run_if(in_state(GameFlow::Editor)),
            );
    }
}

// === State ===========================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Brush {
    Place(usize),
    Erase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    /// Place / erase with the brush.
    Build,
    /// Pick blocks and nudge them with the arrow / PageUp-Dn keys.
    Select,
}

const HINT: &str = "LMB build · RMB look · WASD fly · Space/Ctrl up-down · Esc → menu";

#[derive(Resource)]
struct EditorState {
    grid: HashMap<IVec3, Block>,
    tool: Tool,
    brush: Brush,
    /// Cells picked with the Select tool.
    selection: HashSet<IVec3>,
    /// Draw plain blocks with the real atlas texture instead of flat colour.
    textured: bool,
    /// Worldgen spawn rule saved with the structure.
    spawn: SpawnRule,
    name: String,
    renaming: bool,
    status: String,
    files: Vec<(String, PathBuf)>,
    files_dirty: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            grid: HashMap::new(),
            tool: Tool::Build,
            brush: Brush::Place(7), // WoodPlanks
            selection: HashSet::new(),
            textured: true,
            spawn: SpawnRule::default(),
            name: "my_structure".into(),
            renaming: false,
            status: HINT.into(),
            files: Vec::new(),
            files_dirty: true,
        }
    }
}

impl EditorState {
    fn brush_block(&self) -> Option<Block> {
        match self.brush {
            Brush::Place(i) => PALETTE.get(i).copied(),
            Brush::Erase => None,
        }
    }
}

#[derive(Component)]
struct EditorEntity;

#[derive(Component)]
struct EditorCam {
    yaw: f32,
    pitch: f32,
    speed: f32,
}

#[derive(Component)]
struct EditorCell {
    pos: IVec3,
    block: Block,
    textured: bool,
}

#[derive(Resource)]
struct EditorAssets {
    cube: Handle<Mesh>,
    mats: HashMap<Block, Handle<StandardMaterial>>,
}

impl EditorAssets {
    fn mat(&self, b: Block) -> Handle<StandardMaterial> {
        self.mats
            .get(&b)
            .or_else(|| self.mats.get(&Block::Stone))
            .cloned()
            .unwrap_or_default()
    }
}

/// Atlas-textured cube meshes + shared material, built once the block atlas is
/// ready (`block_atlas::BlockAtlasImage`).
#[derive(Resource, Default)]
struct TexAssets {
    ready: bool,
    mat: Handle<StandardMaterial>,
    meshes: HashMap<Block, Handle<Mesh>>,
}

fn build_tex_assets(
    atlas: Option<Res<BlockAtlasImage>>,
    mut tex: ResMut<TexAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if tex.ready {
        return;
    }
    let Some(atlas) = atlas else {
        return;
    };
    tex.mat = materials.add(StandardMaterial {
        base_color_texture: Some(atlas.0.clone()),
        perceptual_roughness: 0.9,
        alpha_mode: AlphaMode::Mask(0.5),
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    for &b in PALETTE {
        if b.is_textured() {
            tex.meshes.insert(b, meshes.add(textured_cube(b)));
        }
    }
    tex.ready = true;
}

/// Atlas UV rect `(u0, u1, v0, v1)` for a tile column (mirrors `mesher::tile_rect`).
fn atlas_uv(tile: u32) -> (f32, f32, f32, f32) {
    let cols = ATLAS_COLS as f32;
    let inset = 0.5 / (cols * 16.0);
    (
        tile as f32 / cols + inset,
        (tile as f32 + 1.0) / cols - inset,
        inset,
        1.0 - inset,
    )
}

/// A `[0,1]³` cube with per-face UVs pointing at `block`'s atlas tiles.
fn textured_cube(block: Block) -> Mesh {
    // (face normal, 4 corners: bottom-left, bottom-right, top-right, top-left)
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 1.0, 0.0], [[0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]),
        ([0.0, -1.0, 0.0], [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
        ([1.0, 0.0, 0.0], [[1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0]]),
        ([-1.0, 0.0, 0.0], [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]]),
        ([0.0, 0.0, 1.0], [[1.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0]]),
        ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]),
    ];
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(24);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(24);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);
    for (n, quad) in faces {
        let (u0, u1, v0, v1) = atlas_uv(block.face_tile(n[1]));
        let base = positions.len() as u32;
        for &p in &quad {
            positions.push(p);
            normals.push(n);
        }
        uvs.push([u0, v1]);
        uvs.push([u1, v1]);
        uvs.push([u1, v0]);
        uvs.push([u0, v0]);
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

fn setup_editor_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut mats = HashMap::new();
    for &b in PALETTE {
        let c = b.color();
        mats.insert(
            b,
            materials.add(StandardMaterial {
                base_color: Color::srgb(c[0], c[1], c[2]),
                perceptual_roughness: 0.9,
                ..default()
            }),
        );
    }
    commands.insert_resource(EditorAssets { cube, mats });
}

// === Enter / exit ====================================================

/// Query filter for the game HUD roots + this editor's panel — toggled together
/// when entering / leaving the editor.
type UiRoots = Or<(With<HotbarRoot>, With<HudText>, With<SurvivalUi>, With<EditorPanel>)>;

fn set_ui(uis: &mut Query<(&mut Visibility, Has<EditorPanel>), UiRoots>, editor_open: bool) {
    for (mut v, is_panel) in uis.iter_mut() {
        *v = if is_panel == editor_open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[allow(clippy::type_complexity)]
fn enter_editor(
    mut commands: Commands,
    mut state: ResMut<EditorState>,
    mut clear: ResMut<ClearColor>,
    mut ambient: Option<ResMut<GlobalAmbientLight>>,
    mut main_cams: Query<&mut Camera, Without<EditorCam>>,
    mut uis: Query<(&mut Visibility, Has<EditorPanel>), UiRoots>,
) {
    state.grid.clear();
    state.selection.clear();
    state.tool = Tool::Build;
    state.spawn = SpawnRule::default();
    state.files_dirty = true;
    state.status = HINT.into();

    *clear = ClearColor(EDITOR_SKY);
    if let Some(ambient) = ambient.as_mut() {
        ambient.brightness = 550.0;
    }
    for mut cam in &mut main_cams {
        cam.is_active = false;
    }
    set_ui(&mut uis, true);

    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 1,
            ..default()
        },
        Transform::from_xyz(6.0, 6.0, 14.0).looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Y),
        EditorCam {
            yaw: 0.0,
            pitch: 0.0,
            speed: 14.0,
        },
        EditorEntity,
    ));
    // Sync the rig angles to the initial look direction.
    // (yaw 0 / pitch 0 is close enough; the first RMB drag settles it.)

    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.7, -0.9, 0.0)),
        EditorEntity,
    ));
}

#[allow(clippy::type_complexity)]
fn exit_editor(
    mut commands: Commands,
    mut clear: ResMut<ClearColor>,
    mut cursors: Query<&mut CursorOptions>,
    entities: Query<Entity, Or<(With<EditorEntity>, With<EditorCell>)>>,
    mut main_cams: Query<&mut Camera, Without<EditorCam>>,
    mut uis: Query<(&mut Visibility, Has<EditorPanel>), UiRoots>,
) {
    for e in &entities {
        commands.entity(e).despawn();
    }
    *clear = ClearColor(GAME_SKY);
    if let Ok(mut co) = cursors.single_mut() {
        co.grab_mode = CursorGrabMode::None;
        co.visible = true;
    }
    for mut cam in &mut main_cams {
        cam.is_active = true;
    }
    set_ui(&mut uis, false);
}

fn editor_escape(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameFlow>>) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(GameFlow::Menu);
    }
}

// === Camera ==========================================================

fn editor_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut cursors: Query<&mut CursorOptions>,
    mut cam: Query<(&mut Transform, &mut EditorCam)>,
) {
    let Ok((mut tf, mut ec)) = cam.single_mut() else {
        return;
    };
    let looking = mouse.pressed(MouseButton::Right);

    if let Ok(mut co) = cursors.single_mut() {
        let want = if looking {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        if co.grab_mode != want {
            co.grab_mode = want;
            co.visible = !looking;
        }
    }

    if looking {
        let d: Vec2 = motion.read().map(|m| m.delta).sum();
        ec.yaw -= d.x * 0.003;
        ec.pitch = (ec.pitch - d.y * 0.003).clamp(-1.54, 1.54);
        tf.rotation = Quat::from_euler(EulerRot::YXZ, ec.yaw, ec.pitch, 0.0);
    } else {
        motion.clear();
        // keep the stored angles matching whatever rotation we have
        let (y, p, _) = tf.rotation.to_euler(EulerRot::YXZ);
        ec.yaw = y;
        ec.pitch = p;
    }

    let scroll: f32 = wheel.read().map(|w| w.y).sum();
    if scroll != 0.0 {
        ec.speed = (ec.speed * (1.0 + scroll * 0.12)).clamp(2.0, 160.0);
    }

    let f = tf.forward().as_vec3();
    let r = tf.right().as_vec3();
    let mut dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        dir += f;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir -= f;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir += r;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir -= r;
    }
    if keys.pressed(KeyCode::Space) {
        dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::ControlLeft) {
        dir -= Vec3::Y;
    }
    if dir != Vec3::ZERO {
        let boost = if keys.pressed(KeyCode::ShiftLeft) { 3.5 } else { 1.0 };
        tf.translation += dir.normalize() * ec.speed * boost * time.delta_secs();
    }
}

// === Placement =======================================================

/// Amanatides–Woo DDA against the sparse editor grid. Returns
/// `(hit_cell, face_normal)`.
#[allow(unused_assignments)]
fn raycast_grid(
    grid: &HashMap<IVec3, Block>,
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
) -> Option<(IVec3, IVec3)> {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    let mut cell = origin.floor().as_ivec3();
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
    let edge = |o: f32, d: f32| if d > 0.0 { o.floor() + 1.0 - o } else { o - o.floor() };
    let mut t_max = Vec3::new(
        if dir.x != 0.0 { edge(origin.x, dir.x) * inv.x } else { f32::INFINITY },
        if dir.y != 0.0 { edge(origin.y, dir.y) * inv.y } else { f32::INFINITY },
        if dir.z != 0.0 { edge(origin.z, dir.z) * inv.z } else { f32::INFINITY },
    );
    let mut normal = IVec3::Y;
    let mut t = 0.0f32;
    for _ in 0..4096 {
        if grid.contains_key(&cell) {
            return Some((cell, normal));
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

#[allow(clippy::too_many_arguments)]
fn editor_click(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    cam_q: Query<(&Camera, &GlobalTransform), With<EditorCam>>,
    panel_q: Query<
        &Interaction,
        Or<(With<PanelButton>, With<PaletteButton>, With<FileButton>)>,
    >,
    mut state: ResMut<EditorState>,
) {
    if !mouse.just_pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right) {
        return;
    }
    if panel_q.iter().any(|i| *i != Interaction::None) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor.x < PANEL_W + 10.0 {
        return;
    }
    let Ok((camera, cam_tf)) = cam_q.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else {
        return;
    };
    let (o, d) = (ray.origin, *ray.direction);
    let hit = raycast_grid(&state.grid, o, d, 240.0);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);

    // --- Select tool --------------------------------------------------
    if state.tool == Tool::Select {
        let Some((cell, _)) = hit else {
            state.selection.clear();
            state.status = "Selection cleared".into();
            return;
        };
        if ctrl {
            state.selection = state.grid.keys().copied().collect();
        } else if shift {
            if !state.selection.remove(&cell) {
                state.selection.insert(cell);
            }
        } else {
            state.selection.clear();
            state.selection.insert(cell);
        }
        state.status = format!(
            "{} selected (arrows / PageUp-Dn move · Del removes)",
            state.selection.len()
        );
        return;
    }

    // --- Build tool -------------------------------------------------
    if alt || state.brush == Brush::Erase {
        if let Some((cell, _)) = hit {
            state.grid.remove(&cell);
            state.selection.remove(&cell);
            state.status = "Erased".into();
        }
        return;
    }
    let Some(block) = state.brush_block() else {
        return;
    };
    let target = match hit {
        Some((cell, n)) => cell + n,
        None => (o + d * AIR_PLACE_DIST).floor().as_ivec3(),
    };
    if target.y.abs() > 512 {
        return;
    }
    state.grid.insert(target, block);
    state.status = format!("Placed {}  ({} blocks)", block.display_name(), state.grid.len());
}

/// Nudge the current selection with the arrow / PageUp-Dn keys; `Delete` removes
/// the selected blocks.
fn editor_move_selection(keys: Res<ButtonInput<KeyCode>>, mut state: ResMut<EditorState>) {
    if state.tool != Tool::Select || state.selection.is_empty() {
        return;
    }
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        let sel: Vec<IVec3> = state.selection.drain().collect();
        for p in sel {
            state.grid.remove(&p);
        }
        state.status = "Deleted selection".into();
        return;
    }
    let mut delta = IVec3::ZERO;
    if keys.just_pressed(KeyCode::ArrowLeft) {
        delta.x -= 1;
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        delta.x += 1;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        delta.z -= 1;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        delta.z += 1;
    }
    if keys.just_pressed(KeyCode::PageUp) {
        delta.y += 1;
    }
    if keys.just_pressed(KeyCode::PageDown) {
        delta.y -= 1;
    }
    if delta == IVec3::ZERO {
        return;
    }
    let sel: Vec<IVec3> = state.selection.iter().copied().collect();
    let moved: Vec<(IVec3, Block)> = sel
        .iter()
        .filter_map(|p| state.grid.get(p).map(|b| (*p + delta, *b)))
        .collect();
    for p in &sel {
        state.grid.remove(p);
    }
    for (p, b) in &moved {
        state.grid.insert(*p, *b);
    }
    state.selection = moved.iter().map(|(p, _)| *p).collect();
    state.status = format!("Moved {} blocks", state.selection.len());
}

// === Rendering =======================================================

#[allow(clippy::too_many_arguments)]
fn editor_render(
    mut commands: Commands,
    server: Res<AssetServer>,
    assets: Res<EditorAssets>,
    tex: Res<TexAssets>,
    state: Res<EditorState>,
    cells: Query<(Entity, &EditorCell)>,
) {
    let want_tex = state.textured && tex.ready;
    let mut present: HashSet<IVec3> = HashSet::new();
    for (e, cell) in &cells {
        if state.grid.get(&cell.pos) == Some(&cell.block) && cell.textured == want_tex {
            present.insert(cell.pos);
        } else {
            commands.entity(e).despawn();
        }
    }
    for (&pos, &block) in &state.grid {
        if !present.contains(&pos) {
            spawn_cell(&mut commands, &server, &assets, &tex, pos, block, want_tex);
        }
    }
}

fn spawn_cell(
    commands: &mut Commands,
    server: &AssetServer,
    assets: &EditorAssets,
    tex: &TexAssets,
    pos: IVec3,
    block: Block,
    want_tex: bool,
) {
    if let Some(path) = model_path(block) {
        commands
            .spawn((
                Transform::from_translation(pos.as_vec3() + Vec3::new(0.5, 0.0, 0.5)),
                Visibility::default(),
                EditorCell { pos, block, textured: want_tex },
            ))
            .with_children(|c| {
                c.spawn((
                    WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))),
                    Transform::default(),
                ));
            });
    } else if want_tex && tex.ready {
        if let Some(mesh) = tex.meshes.get(&block) {
            // `textured_cube` is already `[0,1]³` → no half-cell offset.
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(tex.mat.clone()),
                Transform::from_translation(pos.as_vec3()),
                EditorCell { pos, block, textured: want_tex },
            ));
            return;
        }
        commands.spawn((
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(assets.mat(block)),
            Transform::from_translation(pos.as_vec3() + Vec3::splat(0.5)),
            EditorCell { pos, block, textured: want_tex },
        ));
    } else {
        commands.spawn((
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(assets.mat(block)),
            Transform::from_translation(pos.as_vec3() + Vec3::splat(0.5)),
            EditorCell { pos, block, textured: want_tex },
        ));
    }
}

fn editor_grid_gizmo(state: Res<EditorState>, mut gizmos: Gizmos) {
    // Reference plane at y = 0.
    let n = 16i32;
    let faint = Color::srgba(0.5, 0.55, 0.6, 0.14);
    for i in -n..=n {
        let a = i as f32;
        let e = n as f32;
        gizmos.line(Vec3::new(a, 0.0, -e), Vec3::new(a, 0.0, e), faint);
        gizmos.line(Vec3::new(-e, 0.0, a), Vec3::new(e, 0.0, a), faint);
    }
    // Origin axes.
    gizmos.line(Vec3::ZERO, Vec3::X * 3.0, Color::srgb(0.9, 0.3, 0.3));
    gizmos.line(Vec3::ZERO, Vec3::Y * 3.0, Color::srgb(0.3, 0.9, 0.3));
    gizmos.line(Vec3::ZERO, Vec3::Z * 3.0, Color::srgb(0.3, 0.5, 0.9));

    // Bounding box of the current build.
    if !state.grid.is_empty() {
        let mn = state.grid.keys().fold(IVec3::MAX, |a, p| a.min(*p));
        let mx = state.grid.keys().fold(IVec3::MIN, |a, p| a.max(*p));
        let min = mn.as_vec3();
        let max = mx.as_vec3() + Vec3::ONE;
        gizmos.cube(
            Transform::from_translation((min + max) * 0.5).with_scale(max - min),
            Color::srgba(0.55, 0.4, 0.85, 0.35),
        );
    }

    // Selection: a wire cube per picked cell, or just its bounds if huge.
    if !state.selection.is_empty() {
        let sel = Color::srgb(1.0, 0.85, 0.15);
        if state.selection.len() <= 400 {
            for p in &state.selection {
                gizmos.cube(
                    Transform::from_translation(p.as_vec3() + Vec3::splat(0.5))
                        .with_scale(Vec3::splat(1.03)),
                    sel,
                );
            }
        } else {
            let mn = state.selection.iter().fold(IVec3::MAX, |a, p| a.min(*p));
            let mx = state.selection.iter().fold(IVec3::MIN, |a, p| a.max(*p));
            let min = mn.as_vec3();
            let max = mx.as_vec3() + Vec3::ONE;
            gizmos.cube(
                Transform::from_translation((min + max) * 0.5).with_scale(max - min),
                sel,
            );
        }
    }
}

// === Panel ===========================================================

#[derive(Component)]
struct EditorPanel;
#[derive(Component)]
struct FileListNode;
#[derive(Component)]
struct PaletteButton(usize);
#[derive(Component)]
struct FileButton(usize);

#[derive(Component, Clone, Copy)]
enum PanelButton {
    ToolBuild,
    ToolSelect,
    EraseBrush,
    SelectAll,
    ClearSel,
    DeleteSel,
    ToggleTextured,
    WeightDown,
    WeightUp,
    SinkDown,
    SinkUp,
    Rename,
    Save,
    CopyJson,
    PasteJson,
    ClearAll,
}

#[derive(Component, Clone, Copy)]
enum PanelText {
    Status,
    Tool,
    Brush,
    Count,
    SelInfo,
    Spawn,
    Name,
}

fn spawn_panel(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.0),
                top: Val::Px(8.0),
                width: Val::Px(PANEL_W),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.92)),
            GlobalZIndex(60),
            Visibility::Hidden,
            EditorPanel,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("STRUCTURE EDITOR"),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb(0.75, 0.65, 0.95)),
            ));
            line(p, PanelText::Status, Color::srgb(0.8, 0.85, 0.9));

            line(p, PanelText::Tool, Color::srgb(0.85, 0.9, 1.0));
            btn_row(p, |r| {
                btn(r, "Build", PanelButton::ToolBuild, 62.0);
                btn(r, "Select", PanelButton::ToolSelect, 62.0);
                btn(r, "Textured", PanelButton::ToggleTextured, 84.0);
            });
            line(p, PanelText::SelInfo, Color::srgb(1.0, 0.9, 0.6));
            btn_row(p, |r| {
                btn(r, "Select all", PanelButton::SelectAll, 74.0);
                btn(r, "Clear sel", PanelButton::ClearSel, 68.0);
                btn(r, "Delete", PanelButton::DeleteSel, 56.0);
            });

            line(p, PanelText::Brush, Color::srgb(0.85, 0.9, 1.0));
            line(p, PanelText::Count, Color::srgb(0.6, 0.65, 0.72));

            p.spawn((
                Text::new("Content:"),
                TextFont::from_font_size(12.0),
                TextColor(Color::srgb(0.6, 0.65, 0.72)),
            ));
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(3.0),
                row_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|row| {
                for (i, &b) in PALETTE.iter().enumerate() {
                    let c = b.color();
                    row.spawn((
                        Button,
                        Node {
                            width: Val::Px(24.0),
                            height: Val::Px(24.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(c[0], c[1], c[2])),
                        PaletteButton(i),
                    ));
                }
            });

            btn_row(p, |r| {
                btn(r, "Erase brush", PanelButton::EraseBrush, 90.0);
                btn(r, "Clear all", PanelButton::ClearAll, 76.0);
            });

            line(p, PanelText::Spawn, Color::srgb(0.7, 0.85, 0.7));
            btn_row(p, |r| {
                btn(r, "w-", PanelButton::WeightDown, 32.0);
                btn(r, "w+", PanelButton::WeightUp, 32.0);
                btn(r, "sink-", PanelButton::SinkDown, 46.0);
                btn(r, "sink+", PanelButton::SinkUp, 46.0);
            });

            line(p, PanelText::Name, Color::srgb(0.9, 0.85, 0.7));
            btn_row(p, |r| {
                btn(r, "Rename", PanelButton::Rename, 66.0);
                btn(r, "Save file", PanelButton::Save, 74.0);
            });
            btn_row(p, |r| {
                btn(r, "Copy JSON", PanelButton::CopyJson, 84.0);
                btn(r, "Paste JSON", PanelButton::PasteJson, 90.0);
            });

            p.spawn((
                Text::new("Saved structures (click → load):"),
                TextFont::from_font_size(12.0),
                TextColor(Color::srgb(0.6, 0.65, 0.72)),
            ));
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                },
                FileListNode,
            ));
        });
}

fn btn_row(p: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(build);
}

fn btn(r: &mut ChildSpawnerCommands, text: &str, marker: PanelButton, w: f32) {
    r.spawn((
        Button,
        Node {
            width: Val::Px(w),
            height: Val::Px(24.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.18, 0.19, 0.24)),
        marker,
    ))
    .with_children(|b| {
        b.spawn((
            Text::new(text),
            TextFont::from_font_size(12.0),
            TextColor(Color::WHITE),
        ));
    });
}

fn line(p: &mut ChildSpawnerCommands, kind: PanelText, color: Color) {
    p.spawn((
        Text::new(""),
        TextFont::from_font_size(12.0),
        TextColor(color),
        kind,
    ));
}

fn palette_buttons(
    mut state: ResMut<EditorState>,
    buttons: Query<(&Interaction, &PaletteButton), Changed<Interaction>>,
) {
    for (interaction, PaletteButton(i)) in &buttons {
        if *interaction == Interaction::Pressed {
            state.brush = Brush::Place(*i);
            state.tool = Tool::Build;
        }
    }
}

fn panel_buttons(
    mut state: ResMut<EditorState>,
    buttons: Query<(&Interaction, &PanelButton), Changed<Interaction>>,
    file_buttons: Query<(&Interaction, &FileButton), Changed<Interaction>>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            PanelButton::ToolBuild => {
                state.tool = Tool::Build;
                state.status = HINT.into();
            }
            PanelButton::ToolSelect => {
                state.tool = Tool::Select;
                state.status =
                    "Select: LMB pick · Shift group · Ctrl all · arrows/PageUp-Dn move · Del".into();
            }
            PanelButton::EraseBrush => {
                state.brush = Brush::Erase;
                state.tool = Tool::Build;
            }
            PanelButton::SelectAll => {
                state.selection = state.grid.keys().copied().collect();
                state.tool = Tool::Select;
                state.status = format!("{} selected", state.selection.len());
            }
            PanelButton::ClearSel => {
                state.selection.clear();
                state.status = "Selection cleared".into();
            }
            PanelButton::DeleteSel => {
                let sel: Vec<IVec3> = state.selection.drain().collect();
                for p in sel {
                    state.grid.remove(&p);
                }
                state.status = "Deleted selection".into();
            }
            PanelButton::ToggleTextured => {
                state.textured = !state.textured;
                state.status = if state.textured {
                    "Textured preview ON".into()
                } else {
                    "Flat colour preview".into()
                };
            }
            PanelButton::WeightDown => {
                state.spawn.weight = (state.spawn.weight - 0.5).max(0.0);
            }
            PanelButton::WeightUp => {
                state.spawn.weight = (state.spawn.weight + 0.5).min(50.0);
            }
            PanelButton::SinkDown => {
                state.spawn.sink = (state.spawn.sink - 1).max(-4);
            }
            PanelButton::SinkUp => {
                state.spawn.sink = (state.spawn.sink + 1).min(16);
            }
            PanelButton::Rename => state.renaming = !state.renaming,
            PanelButton::ClearAll => {
                state.grid.clear();
                state.selection.clear();
                state.status = "Cleared".into();
            }
            PanelButton::Save => save_file(&mut state),
            PanelButton::CopyJson => copy_json(&mut state),
            PanelButton::PasteJson => paste_json(&mut state),
        }
    }
    for (interaction, FileButton(i)) in &file_buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some((_, path)) = state.files.get(*i).cloned() {
            match load_structure(&path) {
                Some(s) => {
                    state.status = format!(
                        "Loaded '{}' ({}×{}×{})",
                        s.name, s.size[0], s.size[1], s.size[2]
                    );
                    state.spawn = s.spawn.clone();
                    state.grid = s.to_cells();
                }
                None => state.status = "Could not read that file".into(),
            }
        }
    }
}

fn sanitize(raw: &str) -> String {
    let s: String = raw
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-'))
        .collect();
    let s = s.trim();
    if s.is_empty() { "structure".into() } else { s.to_string() }
}

fn unique_path(name: &str) -> PathBuf {
    let dir = PathBuf::from(STRUCT_DIR);
    let mut p = dir.join(format!("{name}.json"));
    let mut i = 2;
    while p.exists() {
        p = dir.join(format!("{name}_{i}.json"));
        i += 1;
    }
    p
}

fn save_file(state: &mut EditorState) {
    let name = sanitize(&state.name);
    match from_cells(&state.grid, &name).map(|mut s| {
        s.spawn = state.spawn.clone();
        s
    }) {
        Ok(s) => {
            let path = unique_path(&name);
            let _ = std::fs::create_dir_all(STRUCT_DIR);
            match s.to_pretty_json().and_then(|t| std::fs::write(&path, t).ok()) {
                Some(()) => {
                    state.status =
                        format!("Saved {} ({} blocks)", path.display(), s.blocks.len());
                    state.files_dirty = true;
                }
                None => state.status = "Write failed".into(),
            }
        }
        Err(e) => state.status = e,
    }
}

fn copy_json(state: &mut EditorState) {
    let name = sanitize(&state.name);
    match from_cells(&state.grid, &name)
        .map(|mut s| {
            s.spawn = state.spawn.clone();
            s
        })
        .and_then(|s| s.to_pretty_json().ok_or("serialize failed".into()))
    {
        Ok(text) => match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
            Ok(()) => state.status = format!("Copied '{name}' to clipboard"),
            Err(_) => state.status = "Clipboard unavailable".into(),
        },
        Err(e) => state.status = e,
    }
}

fn paste_json(state: &mut EditorState) {
    match arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .ok()
        .and_then(|t| Structure::from_json(&t))
    {
        Some(s) => {
            state.status = format!("Loaded '{}' from clipboard", s.name);
            state.spawn = s.spawn.clone();
            state.grid = s.to_cells();
        }
        None => state.status = "No valid structure JSON on the clipboard".into(),
    }
}

fn name_capture(mut state: ResMut<EditorState>, mut keys: MessageReader<KeyboardInput>) {
    if !state.renaming {
        keys.clear();
        return;
    }
    for ev in keys.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match &ev.logical_key {
            Key::Enter | Key::Escape => state.renaming = false,
            Key::Backspace => {
                state.name.pop();
            }
            Key::Space => {
                if state.name.len() < 40 {
                    state.name.push(' ');
                }
            }
            Key::Character(s) => {
                for ch in s.chars() {
                    if !ch.is_control() && state.name.len() < 40 {
                        state.name.push(ch);
                    }
                }
            }
            _ => {}
        }
    }
}

fn rebuild_file_list(
    mut commands: Commands,
    mut state: ResMut<EditorState>,
    node: Query<Entity, With<FileListNode>>,
    old: Query<Entity, With<FileButton>>,
) {
    if !state.files_dirty {
        return;
    }
    state.files_dirty = false;
    state.files = {
        let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(STRUCT_DIR)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("json") {
                    return None;
                }
                let name = p.file_stem()?.to_str()?.to_string();
                Some((name, p))
            })
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };

    let Ok(node) = node.single() else {
        return;
    };
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.entity(node).with_children(|list| {
        if state.files.is_empty() {
            list.spawn((
                Text::new("(none yet)"),
                TextFont::from_font_size(12.0),
                TextColor(Color::srgb(0.5, 0.55, 0.6)),
                FileButton(usize::MAX),
            ));
        }
        for (i, (name, _)) in state.files.iter().take(8).enumerate() {
            list.spawn((
                Button,
                Node {
                    width: Val::Px(PANEL_W - 16.0),
                    height: Val::Px(22.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.16, 0.17, 0.22)),
                FileButton(i),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(name.clone()),
                    TextFont::from_font_size(12.0),
                    TextColor(Color::WHITE),
                ));
            });
        }
    });
}

fn sync_panel_text(state: Res<EditorState>, mut texts: Query<(&PanelText, &mut Text)>) {
    for (kind, mut text) in &mut texts {
        text.0 = match kind {
            PanelText::Status => state.status.clone(),
            PanelText::Tool => {
                let t = match state.tool {
                    Tool::Build => "Build",
                    Tool::Select => "Select",
                };
                let tex = if state.textured { "textured" } else { "flat" };
                format!("Tool: {t}   ·   preview: {tex}")
            }
            PanelText::Brush => match state.brush_block() {
                Some(b) => format!("Brush: {}", b.display_name()),
                None => "Brush: Erase".into(),
            },
            PanelText::Count => format!("{} blocks", state.grid.len()),
            PanelText::SelInfo => format!("Selection: {}", state.selection.len()),
            PanelText::Spawn => {
                if state.spawn.weight > 0.0 {
                    format!(
                        "Worldgen: weight {:.1} · sink {} · Y {}-{}",
                        state.spawn.weight, state.spawn.sink, state.spawn.min_y, state.spawn.max_y
                    )
                } else {
                    format!("Worldgen: off (weight 0) · sink {}", state.spawn.sink)
                }
            }
            PanelText::Name => {
                let c = if state.renaming { "_" } else { "" };
                format!("Name: {}{c}", state.name)
            }
        };
    }
}

fn panel_button_visuals(
    state: Res<EditorState>,
    mut buttons: Query<(&Interaction, &PanelButton, &mut BackgroundColor)>,
) {
    for (interaction, button, mut bg) in &mut buttons {
        let active = matches!(button, PanelButton::EraseBrush if state.brush == Brush::Erase)
            || matches!(button, PanelButton::ToolBuild if state.tool == Tool::Build)
            || matches!(button, PanelButton::ToolSelect if state.tool == Tool::Select)
            || matches!(button, PanelButton::ToggleTextured if state.textured)
            || matches!(button, PanelButton::Rename if state.renaming);
        *bg = BackgroundColor(if active {
            Color::srgb(0.32, 0.26, 0.5)
        } else {
            match interaction {
                Interaction::Pressed => Color::srgb(0.3, 0.3, 0.36),
                Interaction::Hovered => Color::srgb(0.24, 0.28, 0.4),
                Interaction::None => Color::srgb(0.18, 0.19, 0.24),
            }
        });
    }
}
