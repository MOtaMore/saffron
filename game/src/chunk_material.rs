//! The chunk material: a `StandardMaterial` extended with a shader "cutout"
//! that turns occluding blocks translucent around the player, so the eagle-view
//! camera never loses sight of them.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::camera::MainCamera;
use crate::player::Player;

const SHADER_PATH: &str = "shaders/chunk_cutout.wgsl";

pub type ChunkMaterial = ExtendedMaterial<StandardMaterial, ChunkCutout>;

/// Handle to the single shared chunk material.
#[derive(Resource)]
pub struct ChunkMaterialHandle(pub Handle<ChunkMaterial>);

/// Translucent material for the separate per-chunk water mesh.
#[derive(Resource)]
pub struct WaterMaterialHandle(pub Handle<StandardMaterial>);

/// Blended variant of the chunk material (same atlas + cutout shader) for the
/// separate per-chunk glass mesh, so glass panes are see-through.
#[derive(Resource)]
pub struct GlassMaterialHandle(pub Handle<ChunkMaterial>);

#[derive(Resource)]
pub struct CutoutSettings {
    pub enabled: bool,
    /// Screen-plane radius of the see-through hole, in world units.
    pub radius: f32,
    pub feather: f32,
    /// Alpha (coverage) at the centre of the hole.
    pub min_alpha: f32,
}

impl Default for CutoutSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            radius: 3.4,
            feather: 1.4,
            min_alpha: 0.10,
        }
    }
}

#[derive(Clone, Copy, ShaderType, Debug, Default, Reflect)]
pub struct CutoutParams {
    player_pos: Vec3,
    radius: f32,
    view_dir: Vec3,
    min_alpha: f32,
    feather: f32,
    enabled: u32,
    _pad0: f32,
    _pad1: f32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct ChunkCutout {
    #[uniform(100)]
    params: CutoutParams,
}

impl MaterialExtension for ChunkCutout {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
    fn deferred_fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
}

pub struct ChunkMaterialPlugin;

impl Plugin for ChunkMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<ChunkMaterial>::default())
            .init_resource::<CutoutSettings>()
            .add_systems(Startup, setup_material)
            .add_systems(Update, (toggle_cutout, update_cutout).chain());
    }
}

fn setup_material(
    mut commands: Commands,
    mut materials: ResMut<Assets<ChunkMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
) {
    let handle = materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.92,
            metallic: 0.0,
            reflectance: 0.15,
            cull_mode: None,
            double_sided: true,
            // Screen-door transparency (needs MSAA): stays in the opaque pass,
            // no per-mesh sorting artefacts across chunks.
            alpha_mode: AlphaMode::AlphaToCoverage,
            ..default()
        },
        extension: ChunkCutout::default(),
    });
    commands.insert_resource(ChunkMaterialHandle(handle));

    let glass = materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.12,
            metallic: 0.0,
            reflectance: 0.4,
            cull_mode: None,
            double_sided: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: ChunkCutout::default(),
    });
    commands.insert_resource(GlassMaterialHandle(glass));

    let water = standard.add(StandardMaterial {
        // Colour comes from the mesh's vertex colours (Water's blue * face shade);
        // alpha (0.5) comes from the vertex colour too.
        base_color: Color::WHITE,
        perceptual_roughness: 0.1,
        metallic: 0.0,
        reflectance: 0.3,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    commands.insert_resource(WaterMaterialHandle(water));
}

fn toggle_cutout(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::keybinds::Keybinds>,
    mut settings: ResMut<CutoutSettings>,
) {
    if binds.just_pressed(&keys, crate::keybinds::Action::VisionCutout) {
        settings.enabled = !settings.enabled;
    }
}

fn update_cutout(
    handle: Option<Res<ChunkMaterialHandle>>,
    glass_handle: Option<Res<GlassMaterialHandle>>,
    mut materials: ResMut<Assets<ChunkMaterial>>,
    settings: Res<CutoutSettings>,
    player_q: Query<&Transform, With<Player>>,
    camera_q: Query<&Transform, With<MainCamera>>,
) {
    let mut params = CutoutParams::default();
    if let (Ok(player), Ok(camera)) = (player_q.single(), camera_q.single()) {
        params.player_pos = player.translation;
        params.view_dir = camera.forward().as_vec3().normalize_or_zero();
        params.radius = settings.radius;
        params.feather = settings.feather;
        params.min_alpha = settings.min_alpha;
        params.enabled = u32::from(settings.enabled);
    }

    let handles = [
        handle.map(|h| h.0.clone()),
        glass_handle.map(|h| h.0.clone()),
    ];
    for h in handles.into_iter().flatten() {
        if let Some(mut material) = materials.get_mut(&h) {
            material.extension.params = params;
        }
    }
}
