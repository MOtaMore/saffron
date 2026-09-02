//! Vida, hambre y sed. Hunger only drains while the player is *doing*
//! something — walking/running, or working the hand mill (`farming.rs`) —
//! never while idle. Thirst drains continuously (faster while running). At
//! zero, either eats into health; with both above 60% health regenerates.
//! `G` eats the selected food, or drinks when standing next to water. Reaching
//! zero health respawns you at your spawn point with full stats (inventory kept).

use bevy::prelude::*;

use crate::block::Block;
use crate::item::{Inventory, Item};
use crate::pause::not_paused;
use crate::player::{Player, PlayerBody};
use crate::streaming::ChunkWorld;

// Decay is deliberately gentle: half of the first pass, which the user found
// too punishing (dead in under one in-game day).
const HUNGER_RATE: f32 = 0.275; // per second, only while moving or milling
/// Tuned so thirst alone (not running) drains 100 -> 0 in exactly 12:30.
const THIRST_RATE: f32 = 100.0 / 750.0;
const RUN_MULT: f32 = 1.8;
/// Above this horizontal speed the player counts as "doing something".
const MOVE_THRESHOLD: f32 = 0.15;
const STARVE_DMG: f32 = 1.1;
const REGEN: f32 = 1.6;
const REGEN_THRESHOLD: f32 = 60.0;
const DRINK_AMOUNT: f32 = 28.0;
const DEATH_FLASH: f32 = 1.6;

pub struct SurvivalPlugin;

impl Plugin for SurvivalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stats>()
            .init_resource::<Spawnpoint>()
            .add_systems(Startup, spawn_survival_ui)
            .add_systems(
                Update,
                (capture_spawn, tick_stats, consume, handle_death)
                    .chain()
                    .run_if(not_paused),
            )
            .add_systems(Update, update_survival_ui);
    }
}

#[derive(Resource)]
pub struct Stats {
    pub health: f32,
    pub hunger: f32,
    pub thirst: f32,
    /// Ticks down after a respawn to flash the death message.
    pub death_flash: f32,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            health: 100.0,
            hunger: 100.0,
            thirst: 100.0,
            death_flash: 0.0,
        }
    }
}

impl Stats {
    pub fn reset(&mut self) {
        *self = Stats::default();
    }
}

#[derive(Resource, Default)]
struct Spawnpoint(Option<Vec3>);

fn capture_spawn(mut spawn: ResMut<Spawnpoint>, player: Query<&Transform, With<Player>>) {
    if spawn.0.is_none() {
        if let Ok(tf) = player.single() {
            spawn.0 = Some(tf.translation);
        }
    }
}

fn tick_stats(
    time: Res<Time>,
    mut stats: ResMut<Stats>,
    player: Query<&PlayerBody, With<Player>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    stats.death_flash = (stats.death_flash - dt).max(0.0);

    let speed = player
        .single()
        .map(|b| Vec2::new(b.velocity.x, b.velocity.z).length())
        .unwrap_or(0.0);
    let running = speed > 8.0;
    let mult = if running { RUN_MULT } else { 1.0 };

    // Hunger only drains from *doing* something: walking/running here, and the
    // hand mill draining it directly while it's being worked (`farming.rs`).
    if speed > MOVE_THRESHOLD {
        stats.hunger = (stats.hunger - HUNGER_RATE * mult * dt).clamp(0.0, 100.0);
    }
    // Thirst keeps draining all the time, same mechanism as before.
    stats.thirst = (stats.thirst - THIRST_RATE * mult * dt).clamp(0.0, 100.0);

    if stats.hunger <= 0.0 || stats.thirst <= 0.0 {
        stats.health = (stats.health - STARVE_DMG * dt).max(0.0);
    } else if stats.hunger > REGEN_THRESHOLD
        && stats.thirst > REGEN_THRESHOLD
        && stats.health < 100.0
    {
        stats.health = (stats.health + REGEN * dt).min(100.0);
    }
}

/// True if there is a water block anywhere in the 3×3 column around the player,
/// from 2 blocks below their centre up to 1 above — i.e. standing on the shore,
/// wading, or swimming all count.
fn near_water(world: &ChunkWorld, pos: Vec3) -> bool {
    let base = pos.floor().as_ivec3();
    for dy in -2..=1 {
        for dx in -1..=1 {
            for dz in -1..=1 {
                if world.get_loaded(base.x + dx, base.y + dy, base.z + dz) == Some(Block::Water) {
                    return true;
                }
            }
        }
    }
    false
}

fn consume(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::keybinds::Keybinds>,
    world: Res<ChunkWorld>,
    mut inventory: ResMut<Inventory>,
    mut stats: ResMut<Stats>,
    player: Query<&Transform, With<Player>>,
) {
    if !binds.just_pressed(&keys, crate::keybinds::Action::Consume) {
        return;
    }
    let selected = inventory.selected_item();
    let by_water = player
        .single()
        .is_ok_and(|tf| near_water(&world, tf.translation));

    // Fill an empty bottle from a water source.
    if selected == Some(Item::Bottle) && by_water {
        if inventory.take(Item::Bottle, 1) == 1 {
            inventory.add(Item::WaterBottle, 1);
        }
        return;
    }

    // Eat / drink the selected consumable.
    if let Some(item) = selected {
        if let Some((hunger, thirst)) = item.food() {
            if inventory.take(item, 1) == 1 {
                stats.hunger = (stats.hunger + hunger).min(100.0);
                stats.thirst = (stats.thirst + thirst).min(100.0);
                if item == Item::WaterBottle {
                    inventory.add(Item::Bottle, 1); // keep the empty bottle
                }
                return;
            }
        }
    }

    // Otherwise, drink straight from a nearby source.
    if by_water {
        stats.thirst = (stats.thirst + DRINK_AMOUNT).min(100.0);
    }
}

fn handle_death(
    mut stats: ResMut<Stats>,
    spawn: Res<Spawnpoint>,
    mut player: Query<(&mut Transform, &mut PlayerBody), With<Player>>,
) {
    if stats.health > 0.0 {
        return;
    }
    stats.reset();
    stats.death_flash = DEATH_FLASH;
    if let (Ok((mut tf, mut body)), Some(home)) = (player.single_mut(), spawn.0) {
        tf.translation = home + Vec3::Y * 2.0;
        body.velocity = Vec3::ZERO;
    }
}

// --- UI --------------------------------------------------------------

#[derive(Component)]
enum Bar {
    Health,
    Hunger,
    Thirst,
}

#[derive(Component)]
struct BarFill(Bar);

#[derive(Component)]
struct DeathFlash;

/// Root of the health/hunger/thirst row — hidden in the creative editor.
#[derive(Component)]
pub struct SurvivalUi;

const BAR_W: f32 = 132.0;
const BAR_H: f32 = 11.0;
/// Width of the hotbar (10 × 44 px cells + 9 × 4 px gaps), so the stat bars sit
/// flush with its left and right ends.
const HOTBAR_W: f32 = 476.0;

fn bar(parent: &mut ChildSpawnerCommands, kind: Bar, color: Color) {
    parent
        .spawn((
            Node {
                width: Val::Px(BAR_W),
                height: Val::Px(BAR_H),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|track| {
            track.spawn((
                Node {
                    width: Val::Px(BAR_W),
                    height: Val::Px(BAR_H),
                    ..default()
                },
                BackgroundColor(color),
                BarFill(kind),
            ));
        });
}

fn spawn_survival_ui(mut commands: Commands) {
    // A row the width of the hotbar, centred just above it: Vida on the left end,
    // Hambre + Sed stacked on the right end.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(64.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            SurvivalUi,
        ))
        .with_children(|center| {
            center
                .spawn(Node {
                    width: Val::Px(HOTBAR_W),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::FlexEnd,
                    ..default()
                })
                .with_children(|row| {
                    bar(row, Bar::Health, Color::srgb(0.85, 0.25, 0.28));
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(3.0),
                        align_items: AlignItems::FlexEnd,
                        ..default()
                    })
                    .with_children(|col| {
                        bar(col, Bar::Hunger, Color::srgb(0.85, 0.55, 0.22));
                        bar(col, Bar::Thirst, Color::srgb(0.30, 0.62, 0.92));
                    });
                });
        });

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Percent(38.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new("YOU DIED"),
        TextFont::from_font_size(40.0),
        TextColor(Color::srgb(0.95, 0.25, 0.25)),
        Visibility::Hidden,
        DeathFlash,
    ));
}

fn update_survival_ui(
    stats: Res<Stats>,
    mut fills: Query<(&BarFill, &mut Node)>,
    mut flash: Query<&mut Visibility, With<DeathFlash>>,
) {
    for (fill, mut node) in &mut fills {
        let frac = match fill.0 {
            Bar::Health => stats.health,
            Bar::Hunger => stats.hunger,
            Bar::Thirst => stats.thirst,
        } / 100.0;
        node.width = Val::Px(BAR_W * frac.clamp(0.0, 1.0));
    }
    if let Ok(mut vis) = flash.single_mut() {
        *vis = if stats.death_flash > 0.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}
