//! Renders workbench / chest / furnace / hand-mill blocks as their glTF models
//! instead of a voxel cube. Chest lid animates open/closed; furnace swaps to its
//! lit texture while smelting. Torches are a small procedural prop (stick + flame
//! + `PointLight`); wheat crops are Minecraft-style crossed planes that grow.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::block::Block;
use crate::container::{ContainerKind, FurnaceStores, OpenContainer};
use crate::farming::CropStore;
use crate::mesher::crossed_quads;
use crate::streaming::ChunkWorld;

const PROP_SCALE: f32 = 1.0;
/// Vertical offset of the model relative to the block cell's bottom face.
const PROP_Y: f32 = 0.0;
const LID_SPEED: f32 = 7.0;
/// Lid rotation when fully open (radians about local X).
const LID_MAX_RAD: f32 = 1.9;

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PropIndex>()
            .add_systems(Startup, setup_prop_assets)
            .add_systems(Update, (sync_props, animate_props, furnace_glow));
    }
}

#[derive(Resource)]
struct PropAssets {
    furnace_unlit: Handle<StandardMaterial>,
    furnace_lit: Handle<StandardMaterial>,
    torch_stick_mesh: Handle<Mesh>,
    torch_flame_mesh: Handle<Mesh>,
    torch_stick_mat: Handle<StandardMaterial>,
    torch_flame_mat: Handle<StandardMaterial>,
    crop_mesh: Handle<Mesh>,
    crop_young_mat: Handle<StandardMaterial>,
    crop_mature_mat: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct PropIndex(HashMap<IVec3, Entity>);

#[derive(Component)]
struct Prop {
    pos: IVec3,
    kind: Block,
    /// Chest: the `chest_lid` hinge node, once the scene loads.
    lid: Option<Entity>,
    /// 0 = closed, 1 = open (chest lid interpolation).
    lid_t: f32,
    /// Furnace: cached mesh descendants + last applied lit state.
    meshes: Vec<Entity>,
    lit: Option<bool>,
}

/// The warm glow light we attach to a furnace model.
#[derive(Component)]
struct FurnaceGlow;

fn setup_prop_assets(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let furnace_unlit = materials.add(StandardMaterial {
        base_color_texture: Some(server.load("models/furnace_t1-texture.png")),
        perceptual_roughness: 0.9,
        ..default()
    });
    let furnace_lit = materials.add(StandardMaterial {
        base_color_texture: Some(server.load("models/furnace_t1_fire-texture.png")),
        emissive: LinearRgba::rgb(1.8, 0.8, 0.25),
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.insert_resource(PropAssets {
        furnace_unlit,
        furnace_lit,
        torch_stick_mesh: meshes.add(Cuboid::new(0.09, 0.42, 0.09)),
        torch_flame_mesh: meshes.add(Cuboid::new(0.16, 0.16, 0.16)),
        torch_stick_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.34, 0.23, 0.13),
            perceptual_roughness: 0.9,
            ..default()
        }),
        torch_flame_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.7, 0.28),
            emissive: LinearRgba::rgb(3.4, 1.9, 0.7),
            unlit: true,
            ..default()
        }),
        crop_mesh: meshes.add(crossed_quads(1.0, 1.05)),
        // One `wheat.png` for both stages; a green tint reads as "not ripe yet".
        crop_young_mat: materials.add(StandardMaterial {
            base_color_texture: Some(server.load("textures/blocks/wheat.png")),
            base_color: Color::srgb(0.68, 0.85, 0.5),
            perceptual_roughness: 0.9,
            double_sided: true,
            cull_mode: None,
            alpha_mode: AlphaMode::Mask(0.5),
            ..default()
        }),
        crop_mature_mat: materials.add(StandardMaterial {
            base_color_texture: Some(server.load("textures/blocks/wheat.png")),
            perceptual_roughness: 0.85,
            double_sided: true,
            cull_mode: None,
            alpha_mode: AlphaMode::Mask(0.5),
            ..default()
        }),
    });
}

fn model_path(kind: Block) -> Option<&'static str> {
    match kind {
        Block::Workbench => Some("models/WorkstationT1.glb"),
        Block::Chest => Some("models/ChestT1.glb"),
        Block::Furnace => Some("models/FurnaceT1.glb"),
        Block::HandMill => Some("models/ManualMill.glb"),
        _ => None,
    }
}

fn sync_props(
    mut commands: Commands,
    server: Res<AssetServer>,
    assets: Res<PropAssets>,
    world: Res<ChunkWorld>,
    mut index: ResMut<PropIndex>,
) {
    for (&pos, &kind) in world.prop_blocks.iter() {
        if index.0.contains_key(&pos) {
            continue;
        }

        // Torch: a small procedural prop (no glTF), with a warm point light.
        if kind == Block::Torch {
            let entity = commands
                .spawn((
                    Transform::from_translation(pos.as_vec3() + Vec3::new(0.5, 0.0, 0.5)),
                    Visibility::default(),
                    Prop {
                        pos,
                        kind,
                        lid: None,
                        lid_t: 0.0,
                        meshes: Vec::new(),
                        lit: None,
                    },
                ))
                .with_children(|c| {
                    c.spawn((
                        Mesh3d(assets.torch_stick_mesh.clone()),
                        MeshMaterial3d(assets.torch_stick_mat.clone()),
                        Transform::from_xyz(0.0, 0.21, 0.0),
                    ));
                    c.spawn((
                        Mesh3d(assets.torch_flame_mesh.clone()),
                        MeshMaterial3d(assets.torch_flame_mat.clone()),
                        Transform::from_xyz(0.0, 0.48, 0.0),
                    ));
                    c.spawn((
                        PointLight {
                            color: Color::srgb(1.0, 0.72, 0.36),
                            intensity: 160_000.0,
                            range: 13.0,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        Transform::from_xyz(0.0, 0.55, 0.0),
                    ));
                })
                .id();
            index.0.insert(pos, entity);
            continue;
        }

        // Wheat crop: Minecraft-style crossed planes whose height/colour track
        // growth (`animate_props`, driven by `farming::CropStore`).
        if kind == Block::WheatCrop {
            let entity = commands
                .spawn((
                    Transform::from_translation(pos.as_vec3() + Vec3::new(0.5, 0.0, 0.5)),
                    Visibility::default(),
                    Prop {
                        pos,
                        kind,
                        lid: None,
                        lid_t: 0.0,
                        meshes: Vec::new(),
                        lit: None,
                    },
                ))
                .with_children(|c| {
                    c.spawn((
                        Mesh3d(assets.crop_mesh.clone()),
                        MeshMaterial3d(assets.crop_young_mat.clone()),
                        // Slight lift so the base doesn't z-fight the farmland top.
                        Transform::from_xyz(0.0, 0.02, 0.0),
                    ));
                })
                .id();
            index.0.insert(pos, entity);
            continue;
        }

        let Some(path) = model_path(kind) else {
            continue;
        };
        let entity = commands
            .spawn((
                WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))),
                Transform::from_translation(pos.as_vec3() + Vec3::new(0.5, PROP_Y, 0.5))
                    .with_scale(Vec3::splat(PROP_SCALE)),
                Prop {
                    pos,
                    kind,
                    lid: None,
                    lid_t: 0.0,
                    meshes: Vec::new(),
                    lit: None,
                },
            ))
            .id();

        if kind == Block::Furnace {
            commands.entity(entity).with_children(|c| {
                c.spawn((
                    PointLight {
                        color: Color::srgb(1.0, 0.55, 0.2),
                        intensity: 0.0,
                        range: 8.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.5, 0.35),
                    FurnaceGlow,
                ));
            });
        }

        index.0.insert(pos, entity);
    }

    let gone: Vec<IVec3> = index
        .0
        .keys()
        .copied()
        .filter(|p| !world.prop_blocks.contains_key(p))
        .collect();
    for pos in gone {
        if let Some(entity) = index.0.remove(&pos) {
            commands.entity(entity).despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn animate_props(
    time: Res<Time>,
    assets: Res<PropAssets>,
    container: Res<OpenContainer>,
    furnaces: Res<FurnaceStores>,
    crops: Res<CropStore>,
    children_q: Query<&Children>,
    name_q: Query<&Name>,
    mesh_q: Query<(), With<Mesh3d>>,
    mut commands: Commands,
    mut props: Query<(Entity, &mut Prop)>,
    mut transforms: Query<&mut Transform>,
) {
    let dt = time.delta_secs();

    for (root, mut prop) in &mut props {
        match prop.kind {
            Block::Chest => {
                if prop.lid.is_none() {
                    prop.lid = find_named(root, "chest_lid", &children_q, &name_q);
                }
                let open = matches!(
                    container.0.as_ref(),
                    Some(o) if o.kind == ContainerKind::Chest
                        && (o.a == prop.pos || o.b == Some(prop.pos))
                );
                let target = if open { 1.0 } else { 0.0 };
                prop.lid_t += (target - prop.lid_t) * (LID_SPEED * dt).min(1.0);
                if let Some(lid) = prop.lid {
                    if let Ok(mut t) = transforms.get_mut(lid) {
                        t.rotation = Quat::from_rotation_x(LID_MAX_RAD * prop.lid_t);
                    }
                }
            }
            Block::Furnace => {
                if prop.meshes.is_empty() {
                    prop.meshes = collect_meshes(root, &mesh_q, &children_q);
                }
                let lit = furnaces.0.get(&prop.pos).is_some_and(|f| f.burn > 0.0);
                if prop.lit != Some(lit) && !prop.meshes.is_empty() {
                    prop.lit = Some(lit);
                    let mat = if lit {
                        assets.furnace_lit.clone()
                    } else {
                        assets.furnace_unlit.clone()
                    };
                    for &m in &prop.meshes {
                        commands.entity(m).insert(MeshMaterial3d(mat.clone()));
                    }
                }
            }
            Block::WheatCrop => {
                if prop.meshes.is_empty() {
                    prop.meshes = collect_meshes(root, &mesh_q, &children_q);
                }
                let Some(&stalk) = prop.meshes.first() else {
                    continue;
                };
                let growth = crops.0.get(&prop.pos).copied().unwrap_or(0.0);
                if let Ok(mut t) = transforms.get_mut(stalk) {
                    // Crossed-quad mesh is base-anchored, so only scale height.
                    t.scale.y = 0.3 + 0.7 * growth;
                }
                let mature = growth >= 1.0;
                if prop.lit != Some(mature) {
                    prop.lit = Some(mature);
                    let mat = if mature {
                        assets.crop_mature_mat.clone()
                    } else {
                        assets.crop_young_mat.clone()
                    };
                    commands.entity(stalk).insert(MeshMaterial3d(mat));
                }
            }
            _ => {}
        }
    }
}

/// The warm point light on each furnace tracks its smelting state.
fn furnace_glow(
    furnaces: Res<FurnaceStores>,
    props: Query<&Prop>,
    mut lights: Query<(&ChildOf, &mut PointLight), With<FurnaceGlow>>,
) {
    for (child_of, mut light) in &mut lights {
        let Ok(prop) = props.get(child_of.parent()) else {
            continue;
        };
        let lit = furnaces.0.get(&prop.pos).is_some_and(|f| f.burn > 0.0);
        light.intensity = if lit { 400_000.0 } else { 0.0 };
    }
}

fn find_named(
    root: Entity,
    target: &str,
    children_q: &Query<&Children>,
    name_q: &Query<&Name>,
) -> Option<Entity> {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if name_q.get(entity).is_ok_and(|n| n.as_str() == target) {
            return Some(entity);
        }
        if let Ok(children) = children_q.get(entity) {
            stack.extend(children.iter());
        }
    }
    None
}

fn collect_meshes(
    root: Entity,
    mesh_q: &Query<(), With<Mesh3d>>,
    children_q: &Query<&Children>,
) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if mesh_q.contains(entity) {
            out.push(entity);
        }
        if let Ok(children) = children_q.get(entity) {
            stack.extend(children.iter());
        }
    }
    out
}
