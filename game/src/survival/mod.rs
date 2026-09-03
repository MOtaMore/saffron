//! Vida, hambre, sed — y, para el giro post-apocalíptico, **radiación** e
//! **intoxicación**. El hambre sólo baja al *hacer* algo (andar/correr o el
//! molino); la sed baja siempre. Radiación e intoxicación las sube el agua
//! contaminada (beberla cruda, o nadar en ella) y bajan solas despacio —
//! mucho más rápido con Vodka (tox) o Anti-Rad (rad). Con la salud a 0 renaces
//! en tu punto de aparición con las estadísticas llenas (inventario intacto).
//!
//! Agrupa también `farming`, `fishing` y el feedback de `radiation`. Todo se
//! re-exporta plano en la raíz del crate desde `main.rs`.

pub mod farming;
pub mod fishing;
pub mod radiation;

use bevy::prelude::*;

use crate::block::Block;
use crate::item::{Inventory, Item, WearKind};
use crate::pause::not_paused;
use crate::player::{Player, PlayerBody};
use crate::streaming::ChunkWorld;

const HUNGER_RATE: f32 = 0.275; // por segundo, sólo al moverse o moler
/// Ajustado para que la sed sola (sin correr) baje 100 → 0 en 12:30.
const THIRST_RATE: f32 = 100.0 / 750.0;
const RUN_MULT: f32 = 1.8;
const MOVE_THRESHOLD: f32 = 0.15;
const STARVE_DMG: f32 = 1.1;
const REGEN: f32 = 1.6;
const REGEN_THRESHOLD: f32 = 60.0;
const DRINK_AMOUNT: f32 = 28.0;
const DEATH_FLASH: f32 = 1.6;

// --- Radiación / intoxicación ----------------------------------------
/// Decaimiento natural (por segundo). La radiación se va muy despacio.
const RAD_DECAY: f32 = 0.35;
const TOX_DECAY: f32 = 1.1;
/// Por encima de esto empieza a hacer daño (escala con el nivel).
const RAD_SICK: f32 = 45.0;
const TOX_SICK: f32 = 40.0;
const RAD_DMG: f32 = 2.6;
const TOX_DMG: f32 = 3.2;
/// Cura rápida con item.
const VODKA_TOX_CURE: f32 = 45.0;
const ANTIRAD_CURE: f32 = 50.0;
/// Dosis al beber un balde crudo / hervido, o directamente de la orilla.
const BUCKET_RAD_DOSE: f32 = 32.0;
const BUCKET_TOX_DOSE: f32 = 36.0;
const BOILED_RESIDUAL: f32 = 7.0;
const SHORE_RAD_DOSE: f32 = 16.0;
const SHORE_TOX_DOSE: f32 = 20.0;
/// Ganancia por segundo mientras estás dentro de agua contaminada.
const RAD_SWIM: f32 = 6.0;
const TOX_SWIM: f32 = 7.0;

// --- Extremidades (paper doll, cosmético) ---------------------------
/// Peso relativo de cada parte respecto a la salud global: la cabeza y el torso
/// "aguantan" mejor, las extremidades peor → el monigote diverge un poco.
/// Orden: cabeza, torso, brazo izq., brazo der., pierna izq., pierna der.
const LIMB_WEIGHT: [f32; 6] = [1.10, 1.05, 0.90, 0.90, 0.95, 0.95];
const LIMB_LERP: f32 = 2.5;
/// Salud que devuelve un botiquín.
const MEDKIT_HEAL: f32 = 55.0;

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
            .add_systems(Update, (update_survival_ui, update_paper_doll));
    }
}

#[derive(Resource)]
pub struct Stats {
    pub health: f32,
    pub hunger: f32,
    pub thirst: f32,
    pub radiation: f32,
    pub toxicity: f32,
    /// Per-limb health (cosmetic breakdown of `health`) for the paper doll.
    /// Order: head, torso, arm-L, arm-R, leg-L, leg-R.
    pub limbs: [f32; 6],
    /// Cuenta atrás tras renacer para parpadear el mensaje de muerte.
    pub death_flash: f32,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            health: 100.0,
            hunger: 100.0,
            thirst: 100.0,
            radiation: 0.0,
            toxicity: 0.0,
            limbs: [100.0; 6],
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
    world: Res<ChunkWorld>,
    mut stats: ResMut<Stats>,
    player: Query<(&Transform, &PlayerBody), With<Player>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    stats.death_flash = (stats.death_flash - dt).max(0.0);

    let (pos, speed) = player
        .single()
        .map(|(tf, b)| (tf.translation, Vec2::new(b.velocity.x, b.velocity.z).length()))
        .unwrap_or((Vec3::ZERO, 0.0));
    let running = speed > 8.0;
    let mult = if running { RUN_MULT } else { 1.0 };

    if speed > MOVE_THRESHOLD {
        stats.hunger = (stats.hunger - HUNGER_RATE * mult * dt).clamp(0.0, 100.0);
    }
    stats.thirst = (stats.thirst - THIRST_RATE * mult * dt).clamp(0.0, 100.0);

    // Contamination: wading in it, then natural decay.
    match water_kind_at(&world, pos) {
        Some(Block::RadWater) => stats.radiation += RAD_SWIM * dt,
        Some(Block::ToxicWater) => stats.toxicity += TOX_SWIM * dt,
        _ => {}
    }
    stats.radiation = (stats.radiation - RAD_DECAY * dt).clamp(0.0, 100.0);
    stats.toxicity = (stats.toxicity - TOX_DECAY * dt).clamp(0.0, 100.0);

    // Health: starvation + sickness damage, or regen if everything is fine.
    let mut dmg = 0.0;
    if stats.hunger <= 0.0 || stats.thirst <= 0.0 {
        dmg += STARVE_DMG;
    }
    if stats.radiation > RAD_SICK {
        dmg += RAD_DMG * (stats.radiation / 100.0);
    }
    if stats.toxicity > TOX_SICK {
        dmg += TOX_DMG * (stats.toxicity / 100.0);
    }
    if dmg > 0.0 {
        stats.health = (stats.health - dmg * dt).max(0.0);
    } else if stats.hunger > REGEN_THRESHOLD
        && stats.thirst > REGEN_THRESHOLD
        && stats.health < 100.0
    {
        stats.health = (stats.health + REGEN * dt).min(100.0);
    }

    // Limbs ease toward a weighted share of the global health.
    let k = 1.0 - (-LIMB_LERP * dt).exp();
    for i in 0..6 {
        let target = (stats.health * LIMB_WEIGHT[i]).clamp(0.0, 100.0);
        stats.limbs[i] += (target - stats.limbs[i]) * k;
    }
}

/// The waterlike block the player's centre is inside, if any.
fn water_kind_at(world: &ChunkWorld, pos: Vec3) -> Option<Block> {
    let c = pos.floor().as_ivec3();
    world
        .get_loaded(c.x, c.y, c.z)
        .filter(|b| b.is_waterlike())
}

/// The nastiest waterlike block in the 3×3 column around the player (shore /
/// wading / swimming all count), or `None`. Toxic beats irradiated beats plain.
fn water_kind_near(world: &ChunkWorld, pos: Vec3) -> Option<Block> {
    let base = pos.floor().as_ivec3();
    let mut found: Option<Block> = None;
    for dy in -2..=1 {
        for dx in -1..=1 {
            for dz in -1..=1 {
                let b = world.get_loaded(base.x + dx, base.y + dy, base.z + dz);
                match b {
                    Some(Block::ToxicWater) => return Some(Block::ToxicWater),
                    Some(Block::RadWater) => found = Some(Block::RadWater),
                    Some(Block::Water) if found.is_none() => found = Some(Block::Water),
                    _ => {}
                }
            }
        }
    }
    found
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
    let water = player
        .single()
        .ok()
        .and_then(|tf| water_kind_near(&world, tf.translation));

    // Medicine: takes priority over eating.
    if selected == Some(Item::Vodka) && inventory.take(Item::Vodka, 1) == 1 {
        stats.toxicity = (stats.toxicity - VODKA_TOX_CURE).max(0.0);
        stats.thirst = (stats.thirst - 6.0).max(0.0); // te deshidrata un poco
        return;
    }
    if selected == Some(Item::AntiRad) && inventory.take(Item::AntiRad, 1) == 1 {
        stats.radiation = (stats.radiation - ANTIRAD_CURE).max(0.0);
        return;
    }
    if selected == Some(Item::Medkit) && inventory.take(Item::Medkit, 1) == 1 {
        stats.health = (stats.health + MEDKIT_HEAL).min(100.0);
        for l in &mut stats.limbs {
            *l = (*l + MEDKIT_HEAL).min(100.0);
        }
        return;
    }
    if selected == Some(Item::Spoiled) && inventory.take(Item::Spoiled, 1) == 1 {
        stats.hunger = (stats.hunger + 3.0).min(100.0);
        stats.toxicity = (stats.toxicity + 25.0).min(100.0);
        return;
    }

    // Fill an empty bottle / bucket from the water you're standing in.
    if let Some(kind) = water {
        if selected == Some(Item::Bottle) && kind == Block::Water {
            if inventory.take(Item::Bottle, 1) == 1 {
                inventory.add(Item::WaterBottle, 1);
            }
            return;
        }
        if selected == Some(Item::Bucket) && inventory.take(Item::Bucket, 1) == 1 {
            inventory.add(
                match kind {
                    Block::RadWater => Item::BucketRadRaw,
                    Block::ToxicWater => Item::BucketToxicRaw,
                    _ => Item::BucketClean,
                },
                1,
            );
            return;
        }
    }

    // Eat / drink the selected consumable.
    if let Some(item) = selected {
        if let Some((hunger, thirst)) = item.food() {
            // Rancid food nourishes less and can turn your stomach.
            let wf = inventory
                .slots
                .get(inventory.selected)
                .copied()
                .flatten()
                .map_or(0.0, |s| s.wear_frac());
            if inventory.take(item, 1) == 1 {
                let quality = 1.0 - wf * 0.7;
                stats.hunger = (stats.hunger + hunger * quality).min(100.0);
                stats.thirst = (stats.thirst + thirst * quality).min(100.0);
                if wf > 0.75 && item.wear_kind() == WearKind::Rot {
                    stats.toxicity = (stats.toxicity + 12.0).min(100.0);
                }
                match item {
                    Item::WaterBottle => inventory.add(Item::Bottle, 1),
                    Item::BucketRadRaw => {
                        stats.radiation = (stats.radiation + BUCKET_RAD_DOSE).min(100.0);
                        inventory.add(Item::Bucket, 1);
                    }
                    Item::BucketToxicRaw => {
                        stats.toxicity = (stats.toxicity + BUCKET_TOX_DOSE).min(100.0);
                        inventory.add(Item::Bucket, 1);
                    }
                    Item::BucketHot => {
                        stats.toxicity = (stats.toxicity + BOILED_RESIDUAL).min(100.0);
                        inventory.add(Item::Bucket, 1);
                    }
                    Item::BucketClean => inventory.add(Item::Bucket, 1),
                    _ => {}
                }
                return;
            }
        }
    }

    // Otherwise, drink straight from the shore.
    match water {
        Some(Block::Water) => stats.thirst = (stats.thirst + DRINK_AMOUNT).min(100.0),
        Some(Block::RadWater) => {
            stats.thirst = (stats.thirst + DRINK_AMOUNT).min(100.0);
            stats.radiation = (stats.radiation + SHORE_RAD_DOSE).min(100.0);
        }
        Some(Block::ToxicWater) => {
            stats.thirst = (stats.thirst + DRINK_AMOUNT).min(100.0);
            stats.toxicity = (stats.toxicity + SHORE_TOX_DOSE).min(100.0);
        }
        _ => {}
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

#[derive(Component, Clone, Copy, PartialEq)]
enum Bar {
    Health,
    Hunger,
    Thirst,
    Radiation,
    Toxicity,
}

#[derive(Component)]
struct BarFill(Bar);
/// The dark backing of a bar — hidden for Radiation / Toxicity while ~0.
#[derive(Component)]
struct BarTrack(Bar);

#[derive(Component)]
struct DeathFlash;

/// Root of the stat row — hidden in the structure editor.
#[derive(Component)]
pub struct SurvivalUi;

const BAR_W: f32 = 132.0;
const BAR_H: f32 = 11.0;
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
            BarTrack(kind),
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
                        bar(col, Bar::Radiation, Color::srgb(0.55, 0.85, 0.35));
                        bar(col, Bar::Toxicity, Color::srgb(0.55, 0.75, 0.20));
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
    mut fills: Query<(&BarFill, &mut Node), Without<BarTrack>>,
    mut tracks: Query<(&BarTrack, &mut Visibility), Without<BarFill>>,
    mut flash: Query<&mut Visibility, (With<DeathFlash>, Without<BarTrack>)>,
) {
    let value = |b: Bar| match b {
        Bar::Health => stats.health,
        Bar::Hunger => stats.hunger,
        Bar::Thirst => stats.thirst,
        Bar::Radiation => stats.radiation,
        Bar::Toxicity => stats.toxicity,
    };
    for (fill, mut node) in &mut fills {
        node.width = Val::Px(BAR_W * (value(fill.0) / 100.0).clamp(0.0, 1.0));
    }
    for (track, mut vis) in &mut tracks {
        // Rad / Tox bars only show once the player is actually contaminated.
        let show = !matches!(track.0, Bar::Radiation | Bar::Toxicity) || value(track.0) > 0.5;
        *vis = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if let Ok(mut vis) = flash.single_mut() {
        *vis = if stats.death_flash > 0.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// Green → yellow → red by how healthy the limb is.
fn limb_color(v: f32) -> Color {
    let t = (v / 100.0).clamp(0.0, 1.0);
    if t < 0.5 {
        Color::srgb(0.85, 0.15 + t * 1.3, 0.15)
    } else {
        Color::srgb(0.85 - (t - 0.5) * 1.4, 0.80, 0.20)
    }
}

fn update_paper_doll(
    stats: Res<Stats>,
    mut nodes: Query<(&crate::item::LimbNode, &mut BackgroundColor)>,
    mut texts: Query<(&crate::item::LimbText, &mut Text)>,
) {
    for (node, mut bg) in &mut nodes {
        *bg = BackgroundColor(limb_color(stats.limbs[node.0]));
    }
    for (t, mut text) in &mut texts {
        text.0 = format!("{:.0}", stats.limbs[t.0]);
    }
}
