//! Ambient-radiation feedback: a **film-grain overlay** and a **Geiger-counter
//! crackle** that ramp up as the player nears (or wades into) irradiated water.
//! This is purely presentational — the actual dose to the body lives in
//! `survival.rs` (`Stats::radiation`).

use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::audio::{PlaybackSettings, Pitch, Volume};
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::widget::NodeImageMode;

use crate::block::Block;
use crate::pause::GameFlow;
use crate::player::Player;
use crate::streaming::ChunkWorld;
use crate::survival::Stats;

/// How far around the player (X/Z) we count irradiated blocks.
const SCAN_R: i32 = 8;
const SCAN_DY: i32 = 3;
/// Irradiated blocks in range for the effect to hit full strength.
const SAT: f32 = 45.0;
const SAMPLE_DT: f32 = 0.18;
/// Exponential smoothing rate for the field.
const SMOOTH: f32 = 3.5;
/// Max opacity of the grain overlay.
const GRAIN_MAX_A: f32 = 0.34;
/// Geiger click spacing, from calm to a screaming source.
const GEIGER_SLOW: f32 = 0.85;
const GEIGER_FAST: f32 = 0.045;
const NOISE_SIZE: usize = 128;
const NOISE_FRAMES: usize = 4;

pub struct RadiationPlugin;

impl Plugin for RadiationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RadField>()
            .add_systems(Startup, setup_radiation)
            .add_systems(
                Update,
                (sample_rad_field, smooth_rad_field, update_grain, geiger_tick)
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

/// Ambient irradiation intensity at the player, 0..1 (smoothed).
#[derive(Resource, Default)]
pub struct RadField {
    pub ambient: f32,
    target: f32,
    timer: f32,
}

#[derive(Resource)]
struct RadAssets {
    noise: [Handle<Image>; NOISE_FRAMES],
    click: [Handle<Pitch>; 3],
}

#[derive(Component)]
struct GrainOverlay;

fn xorshift(s: &mut u32) -> u32 {
    if *s == 0 {
        *s = 0x9E37_79B9;
    }
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}

fn setup_radiation(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut pitches: ResMut<Assets<Pitch>>,
) {
    // A few frames of monochrome static.
    let mut seed = 0x1234_5678u32;
    let noise = std::array::from_fn(|_| {
        let mut data = vec![0u8; NOISE_SIZE * NOISE_SIZE * 4];
        for px in data.chunks_exact_mut(4) {
            let v = (xorshift(&mut seed) >> 24) as u8;
            px[0] = v;
            px[1] = v;
            px[2] = v;
            px[3] = 255;
        }
        let mut img = Image::new(
            Extent3d {
                width: NOISE_SIZE as u32,
                height: NOISE_SIZE as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = ImageSampler::nearest();
        images.add(img)
    });

    // Short sine "ticks" at a few pitches for a less mechanical rhythm.
    let click = [1500.0, 1780.0, 2050.0]
        .map(|f| pitches.add(Pitch::new(f, Duration::from_millis(8))));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        ImageNode {
            image: noise[0].clone(),
            color: Color::NONE,
            image_mode: NodeImageMode::Tiled {
                tile_x: true,
                tile_y: true,
                stretch_value: 1.0,
            },
            ..default()
        },
        // Behind the HUD (which sits at the implicit z 0) but over the 3D world.
        GlobalZIndex(-10),
        Pickable::IGNORE,
        Visibility::Hidden,
        GrainOverlay,
    ));

    commands.insert_resource(RadAssets { noise, click });
}

fn sample_rad_field(
    time: Res<Time>,
    world: Res<ChunkWorld>,
    mut field: ResMut<RadField>,
    player: Query<&Transform, With<Player>>,
) {
    field.timer -= time.delta_secs();
    if field.timer > 0.0 {
        return;
    }
    field.timer = SAMPLE_DT;

    let Ok(tf) = player.single() else {
        field.target = 0.0;
        return;
    };
    let base = tf.translation.floor().as_ivec3();

    let mut count = 0u32;
    let mut inside = false;
    for dy in -SCAN_DY..=SCAN_DY {
        for dz in -SCAN_R..=SCAN_R {
            for dx in -SCAN_R..=SCAN_R {
                if world.get_loaded(base.x + dx, base.y + dy, base.z + dz)
                    == Some(Block::RadWater)
                {
                    count += 1;
                    if dx == 0 && dz == 0 && (-1..=1).contains(&dy) {
                        inside = true;
                    }
                }
            }
        }
    }
    let mut t = (count as f32 / SAT).min(1.0);
    if inside {
        t = t.max(0.9);
    }
    field.target = t;
}

fn smooth_rad_field(time: Res<Time>, mut field: ResMut<RadField>) {
    let k = 1.0 - (-SMOOTH * time.delta_secs()).exp();
    field.ambient += (field.target - field.ambient) * k;
    if field.ambient < 0.001 {
        field.ambient = 0.0;
    }
}

fn update_grain(
    field: Res<RadField>,
    assets: Res<RadAssets>,
    mut frame: Local<usize>,
    mut overlay: Query<(&mut ImageNode, &mut Visibility), With<GrainOverlay>>,
) {
    let Ok((mut image, mut vis)) = overlay.single_mut() else {
        return;
    };
    let a = field.ambient;
    if a <= 0.02 {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Visible;
    *frame = (*frame + 1) % NOISE_FRAMES;
    image.image = assets.noise[*frame].clone();
    // Sickly green tint, opacity scaled by intensity (eased so it bites late).
    image.color = Color::srgba(0.72, 1.0, 0.66, a * a * GRAIN_MAX_A);
}

fn geiger_tick(
    time: Res<Time>,
    field: Res<RadField>,
    stats: Res<Stats>,
    assets: Res<RadAssets>,
    mut commands: Commands,
    mut next: Local<f32>,
    mut rng: Local<u32>,
) {
    // Nearby source, or just a hot body — whichever screams louder.
    let intensity = field.ambient.max(stats.radiation / 100.0 * 0.55);
    if intensity <= 0.02 {
        *next = 0.0;
        return;
    }
    *next -= time.delta_secs();
    if *next > 0.0 {
        return;
    }
    let base = GEIGER_SLOW + (GEIGER_FAST - GEIGER_SLOW) * intensity;
    let jitter = 0.45 + (xorshift(&mut rng) % 1000) as f32 / 1000.0; // 0.45..1.45
    *next = (base * jitter).max(GEIGER_FAST);

    let pick = (xorshift(&mut rng) % 3) as usize;
    commands.spawn((
        AudioPlayer::<Pitch>(assets.click[pick].clone()),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.12 + intensity * 0.5)),
    ));
}
