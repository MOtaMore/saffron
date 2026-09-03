//! Eagle-view orthographic camera that follows the player, with mouse-wheel
//! zoom and 90° yaw rotation on Q / E.

use bevy::camera::ScalingMode;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::player::Player;

/// Camera offset from its target, before yaw rotation. The ratio of height to
/// horizontal distance sets the (fixed) pitch of the eagle view.
const OFFSET: Vec3 = Vec3::new(78.0, 150.0, 78.0);
const FOLLOW_LERP: f32 = 7.0;
const YAW_LERP: f32 = 9.0;

const MIN_ZOOM: f32 = 18.0;
const MAX_ZOOM: f32 = 160.0;
const DEFAULT_ZOOM: f32 = 46.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera).add_systems(
            Update,
            (camera_input.run_if(crate::pause::not_paused), camera_follow)
                .chain()
                // Suspended while the first-person view drives the camera.
                .run_if(crate::firstperson::eagle_view),
        );
    }
}

#[derive(Component)]
pub struct MainCamera;

#[derive(Component, Default)]
pub struct CameraRig {
    pub yaw: f32,
    pub target_yaw: f32,
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: DEFAULT_ZOOM,
            },
            near: -1000.0,
            far: 4000.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(OFFSET).looking_at(Vec3::ZERO, Vec3::Y),
        // 4x MSAA drives the alpha-to-coverage cutout in the chunk shader.
        Msaa::Sample4,
        MainCamera,
        CameraRig::default(),
    ));
}

fn camera_input(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::keybinds::Keybinds>,
    mut wheel: MessageReader<MouseWheel>,
    mut rig: Query<&mut CameraRig>,
    mut projection: Query<&mut Projection, With<MainCamera>>,
) {
    use crate::keybinds::Action;
    if let Ok(mut rig) = rig.single_mut() {
        if binds.just_pressed(&keys, Action::CameraLeft) {
            rig.target_yaw += std::f32::consts::FRAC_PI_2;
        }
        if binds.just_pressed(&keys, Action::CameraRight) {
            rig.target_yaw -= std::f32::consts::FRAC_PI_2;
        }
    }

    // Zoom is Ctrl + wheel; plain wheel cycles the hotbar.
    if !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        wheel.clear();
        return;
    }

    let scroll: f32 = wheel.read().map(|e| e.y).sum();
    if scroll != 0.0 {
        if let Ok(mut projection) = projection.single_mut() {
            if let Projection::Orthographic(ortho) = projection.as_mut() {
                if let ScalingMode::FixedVertical { viewport_height } = &mut ortho.scaling_mode {
                    *viewport_height = (*viewport_height - scroll * 4.0).clamp(MIN_ZOOM, MAX_ZOOM);
                }
            }
        }
    }
}

fn camera_follow(
    time: Res<Time>,
    player: Query<&Transform, (With<Player>, Without<MainCamera>)>,
    mut camera: Query<(&mut Transform, &mut CameraRig), With<MainCamera>>,
) {
    let Ok((mut cam_transform, mut rig)) = camera.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    let yaw_t = 1.0 - (-YAW_LERP * dt).exp();
    rig.yaw = lerp_angle(rig.yaw, rig.target_yaw, yaw_t);

    let target = player.iter().next().map_or(Vec3::ZERO, |t| t.translation);
    let desired = target + Quat::from_rotation_y(rig.yaw) * OFFSET;

    let follow_t = 1.0 - (-FOLLOW_LERP * dt).exp();
    cam_transform.translation = cam_transform.translation.lerp(desired, follow_t);
    cam_transform.look_at(target, Vec3::Y);
}

fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    let mut diff = (to - from) % std::f32::consts::TAU;
    if diff > std::f32::consts::PI {
        diff -= std::f32::consts::TAU;
    } else if diff < -std::f32::consts::PI {
        diff += std::f32::consts::TAU;
    }
    from + diff * t
}
