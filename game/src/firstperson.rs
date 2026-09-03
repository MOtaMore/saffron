//! Optional **first-person view** (`V`), FPS-style: mouse look, `WASD` relative
//! to where you're facing, `Space` jump, `Shift` run. Toggling swaps the eagle
//! camera's orthographic projection for a perspective one at the player's eyes,
//! grabs the cursor, hides the player model and shows a crosshair. The eagle
//! view's point-&-click movement and camera follow are suspended while it's on
//! (`eagle_view` run-condition); block picking aims at screen centre (see
//! `interact.rs`). Everything reverts on toggle-off.

use bevy::camera::ScalingMode;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::camera::MainCamera;
use crate::container::OpenContainer;
use crate::item::InventoryOpen;
use crate::keybinds::{Action, Keybinds};
use crate::net::ChatLog;
use crate::pause::{GameFlow, Paused};
use crate::player::{
    MoveInput, MoveMarker, Player, PlayerBody, PlayerModel, PlayerShadow, player_free, step_player,
};
use crate::streaming::ChunkWorld;

/// Mouse-look sensitivity (radians per pixel).
const SENS: f32 = 0.0025;
/// Eye height above the player's centre.
const EYE: f32 = 0.75;
/// Eagle-view default zoom, restored when leaving first person.
const EAGLE_ZOOM: f32 = 46.0;

pub struct FirstPersonPlugin;

impl Plugin for FirstPersonPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraMode>()
            .add_systems(Startup, spawn_crosshair)
            .add_systems(
                Update,
                toggle_first_person.run_if(in_state(GameFlow::Playing)),
            )
            .add_systems(
                Update,
                (first_person_move.run_if(player_free), fp_update)
                    .chain()
                    .run_if(first_person_view),
            );
    }
}

/// Which camera scheme is active. `eagle_view` / `first_person_view` are the
/// run-conditions other plugins gate on.
#[derive(Resource)]
pub struct CameraMode {
    pub first_person: bool,
    saved_zoom: f32,
}

impl Default for CameraMode {
    fn default() -> Self {
        Self { first_person: false, saved_zoom: EAGLE_ZOOM }
    }
}

pub fn eagle_view(m: Res<CameraMode>) -> bool {
    !m.first_person
}
pub fn first_person_view(m: Res<CameraMode>) -> bool {
    m.first_person
}

/// Where block picking should aim: the cursor in the eagle view, the screen
/// centre (crosshair) in first person.
pub fn aim_point(window: &Window, mode: &CameraMode) -> Option<Vec2> {
    if mode.first_person {
        Some(Vec2::new(window.width() * 0.5, window.height() * 0.5))
    } else {
        window.cursor_position()
    }
}

/// Yaw / pitch of the first-person camera (added to `MainCamera` on toggle-on).
#[derive(Component, Default)]
struct FpLook {
    yaw: f32,
    pitch: f32,
}

#[derive(Component)]
struct Crosshair;

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn toggle_first_person(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<Keybinds>,
    mut mode: ResMut<CameraMode>,
    mut commands: Commands,
    cam_q: Query<Entity, With<MainCamera>>,
    mut proj_q: Query<&mut Projection, With<MainCamera>>,
    mut cursors: Query<&mut CursorOptions>,
    mut hidden_in_fp: Query<
        &mut Visibility,
        (
            Or<(With<PlayerModel>, With<PlayerShadow>, With<MoveMarker>)>,
            Without<Crosshair>,
        ),
    >,
    mut crosshair: Query<&mut Visibility, With<Crosshair>>,
) {
    if !binds.just_pressed(&keys, Action::FirstPerson) {
        return;
    }
    let Ok(cam) = cam_q.single() else {
        return;
    };
    mode.first_person = !mode.first_person;
    let fp = mode.first_person;

    if let Ok(mut proj) = proj_q.single_mut() {
        if fp {
            if let Projection::Orthographic(o) = proj.as_ref() {
                if let ScalingMode::FixedVertical { viewport_height } = o.scaling_mode {
                    mode.saved_zoom = viewport_height;
                }
            }
            *proj = Projection::from(PerspectiveProjection {
                fov: 1.35,
                near: 0.05,
                ..default()
            });
        } else {
            *proj = Projection::from(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: mode.saved_zoom,
                },
                near: -1000.0,
                far: 4000.0,
                ..OrthographicProjection::default_3d()
            });
        }
    }
    if fp {
        commands.entity(cam).insert(FpLook::default());
    }
    if let Ok(mut co) = cursors.single_mut() {
        co.grab_mode = if fp {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        co.visible = !fp;
    }
    for mut v in &mut hidden_in_fp {
        *v = if fp { Visibility::Hidden } else { Visibility::Visible };
    }
    if let Ok(mut v) = crosshair.single_mut() {
        *v = if fp { Visibility::Visible } else { Visibility::Hidden };
    }
}

fn first_person_move(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<Keybinds>,
    world: Res<ChunkWorld>,
    cam_q: Query<&Transform, (With<MainCamera>, Without<Player>)>,
    mut player_q: Query<(&mut Transform, &mut PlayerBody), With<Player>>,
) {
    let (Ok(cam_tf), Ok((mut tf, mut body))) = (cam_q.single(), player_q.single_mut()) else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }
    let f = cam_tf.forward().as_vec3();
    let r = cam_tf.right().as_vec3();
    let flat_f = Vec3::new(f.x, 0.0, f.z).normalize_or_zero();
    let flat_r = Vec3::new(r.x, 0.0, r.z).normalize_or_zero();
    let mut wish = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        wish += flat_f;
    }
    if keys.pressed(KeyCode::KeyS) {
        wish -= flat_f;
    }
    if keys.pressed(KeyCode::KeyD) {
        wish += flat_r;
    }
    if keys.pressed(KeyCode::KeyA) {
        wish -= flat_r;
    }
    let wish = wish.normalize_or_zero();
    let run = binds.pressed(&keys, Action::Run);
    let jump = body.grounded && binds.just_pressed(&keys, Action::Jump);
    step_player(
        &world,
        &mut tf,
        &mut body,
        MoveInput { wish, run, jump, face_wish: false },
        dt,
    );
}

#[allow(clippy::too_many_arguments)]
fn fp_update(
    mut motion: MessageReader<MouseMotion>,
    inv: Res<InventoryOpen>,
    cont: Res<OpenContainer>,
    chat: Res<ChatLog>,
    paused: Res<Paused>,
    mut cursors: Query<&mut CursorOptions>,
    player_q: Query<&Transform, (With<Player>, Without<MainCamera>)>,
    mut cam_q: Query<(&mut Transform, &mut FpLook), With<MainCamera>>,
) {
    let Ok((mut tf, mut look)) = cam_q.single_mut() else {
        return;
    };
    let free = !inv.0 && cont.0.is_none() && !chat.capturing() && !paused.0;

    if free {
        let d: Vec2 = motion.read().map(|m| m.delta).sum();
        look.yaw -= d.x * SENS;
        look.pitch = (look.pitch - d.y * SENS).clamp(-1.54, 1.54);
    } else {
        motion.clear();
    }

    if let Ok(mut co) = cursors.single_mut() {
        let want = if free {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        if co.grab_mode != want {
            co.grab_mode = want;
            co.visible = !free;
        }
    }

    if let Ok(p) = player_q.single() {
        tf.translation = p.translation + Vec3::Y * EYE;
    }
    tf.rotation = Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0);
}

fn spawn_crosshair(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            Visibility::Hidden,
            Crosshair,
        ))
        .with_children(|c| {
            c.spawn((
                Text::new("+"),
                TextFont::from_font_size(22.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
                Pickable::IGNORE,
            ));
        });
}
