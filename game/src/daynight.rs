//! Day / night cycle. A single [`GameClock`] drives the sun's angle and colour,
//! the ambient light and the sky (`ClearColor`). One full day is
//! [`GameClock::day_len`] real seconds (default 20 min). `t` is persisted by the
//! save file.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::pause::not_paused;

pub struct DayNightPlugin;

impl Plugin for DayNightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameClock>().add_systems(
            Update,
            (advance_clock, apply_sky).chain().run_if(not_paused),
        );
    }
}

/// Marker on the directional light spawned in `main::setup_environment`.
#[derive(Component)]
pub struct Sun;

#[derive(Resource)]
pub struct GameClock {
    /// 0 = midnight, 0.25 = amanecer, 0.5 = mediodía, 0.75 = atardecer.
    pub t: f32,
    /// Real seconds for one full day.
    pub day_len: f32,
}

impl Default for GameClock {
    fn default() -> Self {
        Self {
            t: 0.30, // start just after dawn
            day_len: 1200.0,
        }
    }
}

impl GameClock {
    /// `HH:MM` for the HUD.
    pub fn clock_string(&self) -> String {
        let mins = (self.t.rem_euclid(1.0) * 24.0 * 60.0) as u32;
        format!("{:02}:{:02}", mins / 60, mins % 60)
    }
    pub fn is_night(&self) -> bool {
        !(0.23..0.77).contains(&self.t.rem_euclid(1.0))
    }
}

fn advance_clock(time: Res<Time>, mut clock: ResMut<GameClock>) {
    if clock.day_len > 0.0 {
        clock.t = (clock.t + time.delta_secs() / clock.day_len).rem_euclid(1.0);
    }
}

fn lerp3(a: Vec3, b: Vec3, s: f32) -> Vec3 {
    a + (b - a) * s.clamp(0.0, 1.0)
}

fn apply_sky(
    clock: Res<GameClock>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
) {
    // Sun elevation: -1 at midnight, 0 at dawn/dusk, +1 at noon.
    let elev = (clock.t * TAU - TAU * 0.25).sin();
    let day = elev.max(0.0); // 0 at night, 1 at noon
    // How "twilight" it is (near the horizon, either side).
    let twilight = (1.0 - (elev.abs() / 0.25).min(1.0)).max(0.0);

    if let Ok((mut tf, mut light)) = sun.single_mut() {
        // Direction the light travels (downwards while the sun is up).
        let travel = Vec3::new(0.35, -(elev.max(0.04)), 0.28).normalize();
        *tf = Transform::from_translation(Vec3::ZERO).looking_to(travel, Vec3::Y);

        light.illuminance = 400.0 + day * 12_000.0;
        let noon = Vec3::new(1.0, 0.98, 0.94);
        let dusk = Vec3::new(1.0, 0.55, 0.30);
        let night = Vec3::new(0.55, 0.62, 0.95); // faint moonlight tint
        let warm = lerp3(dusk, noon, day);
        let c = lerp3(night, warm, day.max(twilight * 0.6));
        light.color = Color::srgb(c.x, c.y, c.z);
    }

    // Ambient: dim and blue at night, bright and neutral by day.
    let amb = lerp3(
        Vec3::new(0.40, 0.48, 0.72),
        Vec3::new(0.75, 0.82, 1.0),
        day,
    );
    ambient.color = Color::srgb(amb.x, amb.y, amb.z);
    ambient.brightness = 70.0 + day * 330.0;

    // Sky.
    let night_sky = Vec3::new(0.03, 0.045, 0.11);
    let day_sky = Vec3::new(0.53, 0.72, 0.92);
    let dusk_sky = Vec3::new(0.62, 0.42, 0.34);
    let base = lerp3(night_sky, day_sky, day);
    let sky = lerp3(base, dusk_sky, twilight * 0.7 * (1.0 - day));
    clear.0 = Color::srgb(sky.x, sky.y, sky.z);
}
