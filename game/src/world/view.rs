//! Camera vision layers: hide voxels at or above a Y cutoff so overhangs,
//! ravines, trees or buildings never hide the player from the eagle-view camera.
//!
//! The slice can follow the player automatically (toggle with `L`), sit at a
//! fixed manual height (`[` / `]`), or be off entirely (`\`). Purely visual —
//! world data and collision are untouched. `streaming` re-meshes loaded chunks
//! whenever the effective cutoff changes, nearest-to-player first.

use bevy::prelude::*;

use crate::chunk::CHUNK_HEIGHT;
use crate::pause::not_paused;
use crate::player::Player;

/// Blocks kept visible above the player's head in auto mode. 1 = hide anything
/// directly above the head (most aggressive; the player is never occluded).
const AUTO_HEADROOM: i32 = 1;
/// Key-repeat interval while `[` / `]` is held.
const STEP_REPEAT: f32 = 0.14;
/// In auto mode, don't push a new cutoff (which re-meshes chunks) more often
/// than this. Keeps a sustained climb from re-meshing every single frame.
const AUTO_REMESH_INTERVAL: f32 = 0.10;
const MIN_CUTOFF: i32 = 2;

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewSlice>().add_systems(
            Update,
            (slice_input.run_if(not_paused), auto_follow).chain(),
        );
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum SliceMode {
    Off,
    /// Cutoff tracks the player every frame.
    Auto,
    /// Cutoff stays where the player left it.
    Manual,
}

#[derive(Resource)]
pub struct ViewSlice {
    pub mode: SliceMode,
    /// Effective Y cutoff; voxels with `y >= cutoff` are hidden while slicing.
    pub cutoff: i32,
    /// Player nudge applied on top of the auto cutoff (via `[` / `]`).
    pub peek: i32,
}

impl Default for ViewSlice {
    fn default() -> Self {
        Self {
            mode: SliceMode::Off,
            cutoff: CHUNK_HEIGHT,
            peek: 0,
        }
    }
}

impl ViewSlice {
    /// The cutoff to mesh with, or `None` for a full (unsliced) world.
    pub fn effective(&self) -> Option<i32> {
        match self.mode {
            SliceMode::Off => None,
            _ => Some(self.cutoff.clamp(MIN_CUTOFF, CHUNK_HEIGHT)),
        }
    }
}

fn head_level(player: &Query<&Transform, With<Player>>) -> Option<i32> {
    player
        .iter()
        .next()
        .map(|t| (t.translation.y + 0.9).floor() as i32)
}

fn slice_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::keybinds::Keybinds>,
    player: Query<&Transform, With<Player>>,
    mut slice: ResMut<ViewSlice>,
    mut cooldown: Local<f32>,
) {
    use crate::keybinds::Action;
    *cooldown -= time.delta_secs();

    // Toggle automatic player-follow slicing.
    if binds.just_pressed(&keys, Action::ViewToggle) {
        slice.mode = if slice.mode == SliceMode::Auto {
            SliceMode::Off
        } else {
            slice.peek = 0;
            SliceMode::Auto
        };
    }

    // Full view.
    if binds.just_pressed(&keys, Action::ViewFull) {
        slice.mode = SliceMode::Off;
        slice.peek = 0;
    }

    // Lower / raise the ceiling. Shift = bigger steps.
    let step = if keys.pressed(KeyCode::ShiftLeft) { 5 } else { 1 };
    let mut delta = 0;
    if binds.pressed(&keys, Action::ViewLower) {
        delta -= step;
    }
    if binds.pressed(&keys, Action::ViewRaise) {
        delta += step;
    }
    if delta == 0 || *cooldown > 0.0 {
        return;
    }
    *cooldown = STEP_REPEAT;

    match slice.mode {
        // In auto mode the brackets just nudge the follow height.
        SliceMode::Auto => slice.peek += delta,
        // Otherwise they drive an absolute manual slice.
        SliceMode::Off | SliceMode::Manual => {
            if slice.mode == SliceMode::Off {
                slice.cutoff = head_level(&player).unwrap_or(64) + AUTO_HEADROOM;
                slice.mode = SliceMode::Manual;
            }
            slice.cutoff += delta;
            if slice.cutoff >= CHUNK_HEIGHT {
                slice.mode = SliceMode::Off;
                slice.peek = 0;
            } else {
                slice.cutoff = slice.cutoff.max(MIN_CUTOFF);
            }
        }
    }
}

fn auto_follow(
    time: Res<Time>,
    player: Query<&Transform, With<Player>>,
    mut slice: ResMut<ViewSlice>,
    mut cooldown: Local<f32>,
) {
    *cooldown -= time.delta_secs();
    if slice.mode != SliceMode::Auto {
        return;
    }
    let Some(head) = head_level(&player) else {
        return;
    };
    let desired = (head + AUTO_HEADROOM + slice.peek).clamp(MIN_CUTOFF, CHUNK_HEIGHT);
    if desired != slice.cutoff && *cooldown <= 0.0 {
        slice.cutoff = desired;
        *cooldown = AUTO_REMESH_INTERVAL;
    }
}
