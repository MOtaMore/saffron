//! Ambient wildlife: cows, chickens, sheep and pigs that spawn in herds of 3
//! per chunk and wander around, following the ground.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::camera::MainCamera;
use crate::chunk::{CHUNK_SIZE, ChunkCoord};
use crate::item::{Inventory, Item, ToolKind};
use crate::player::{Player, player_free};
use crate::scatter::Pickup;
use crate::streaming::ChunkWorld;
use crate::worldgen::WorldGenHandle;

const HERD_CHANCE_PCT: u32 = 14;
const HERD_SIZE: usize = 3;
const WANDER_RADIUS: f32 = 7.0;
/// Reach and cursor tolerance for hitting an animal.
const HIT_REACH: f32 = 5.5;
const HIT_PIXELS: f32 = 55.0;
/// How close the player must be, holding the animal's favourite food, to lure it.
const LURE_RADIUS: f32 = 6.0;

/// Feeding an adult puts it in "love mode" for this long; two loving animals of
/// the same kind that meet spawn a baby, then can't breed again for a while.
const LOVE_TIME: f32 = 18.0;
const BREED_COOLDOWN: f32 = 75.0;
const BREED_DIST: f32 = 2.2;
/// Seconds a newborn stays small; feeding a baby its favourite food speeds it up.
const BABY_GROW_TIME: f32 = 75.0;
const BABY_SCALE: f32 = 0.5;
/// Don't let a single home chunk's herd grow past this.
const MAX_PER_HOME: usize = 12;

pub struct AnimalPlugin;

impl Plugin for AnimalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Herded>()
            .add_systems(Startup, setup_animal_assets)
            .add_systems(
                Update,
                (spawn_herds, animal_ai, breed_animals, despawn_far_animals)
                    .run_if(in_state(crate::pause::GameFlow::Playing)),
            )
            .add_systems(Update, attack_animals.run_if(player_free));
    }
}

#[derive(Resource, Default)]
struct Herded(HashSet<ChunkCoord>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnimalKind {
    Cow,
    Chicken,
    Sheep,
    Pig,
}

impl AnimalKind {
    const ALL: [AnimalKind; 4] = [
        AnimalKind::Cow,
        AnimalKind::Chicken,
        AnimalKind::Sheep,
        AnimalKind::Pig,
    ];

    fn index(self) -> usize {
        match self {
            AnimalKind::Cow => 0,
            AnimalKind::Chicken => 1,
            AnimalKind::Sheep => 2,
            AnimalKind::Pig => 3,
        }
    }

    fn body(self) -> Vec3 {
        match self {
            AnimalKind::Cow => Vec3::new(1.1, 0.9, 0.6),
            AnimalKind::Chicken => Vec3::new(0.35, 0.4, 0.3),
            AnimalKind::Sheep => Vec3::new(0.9, 0.85, 0.55),
            AnimalKind::Pig => Vec3::new(0.95, 0.7, 0.55),
        }
    }

    fn color(self) -> Color {
        match self {
            AnimalKind::Cow => Color::srgb(0.36, 0.25, 0.19),
            AnimalKind::Chicken => Color::srgb(0.93, 0.92, 0.86),
            AnimalKind::Sheep => Color::srgb(0.88, 0.87, 0.83),
            AnimalKind::Pig => Color::srgb(0.86, 0.55, 0.58),
        }
    }

    fn speed(self) -> f32 {
        match self {
            AnimalKind::Chicken => 2.2,
            _ => 1.5,
        }
    }

    fn max_health(self) -> f32 {
        match self {
            AnimalKind::Chicken => 4.0,
            AnimalKind::Sheep => 8.0,
            AnimalKind::Pig => 10.0,
            AnimalKind::Cow => 14.0,
        }
    }

    /// What the corpse leaves on the ground.
    fn drops(self) -> &'static [(Item, u32)] {
        match self {
            AnimalKind::Pig => &[(Item::Meat, 2), (Item::Fat, 1)],
            AnimalKind::Cow => &[(Item::RedMeat, 3), (Item::Leather, 2)],
            AnimalKind::Sheep => &[(Item::Mutton, 2), (Item::Wool, 2)],
            AnimalKind::Chicken => &[(Item::WhiteMeat, 1), (Item::Feather, 2)],
        }
    }

    /// The crop this animal is drawn to and can be fed to heal it.
    fn favorite_food(self) -> Item {
        match self {
            AnimalKind::Chicken => Item::Seeds,
            AnimalKind::Cow | AnimalKind::Sheep => Item::Wheat,
            AnimalKind::Pig => Item::Potato,
        }
    }
}

#[derive(Component)]
struct Animal {
    kind: AnimalKind,
    home: ChunkCoord,
    target: Vec2,
    wait: f32,
    vy: f32,
    health: f32,
    /// >0 while in "love mode" (recently fed as an adult).
    love: f32,
    /// >0 while unable to breed again after a birth.
    breed_cd: f32,
    /// Seconds of baby-hood left; 0 = adult.
    grow: f32,
    /// Current visual scale (`BABY_SCALE`..1.0).
    scale: f32,
}

#[derive(Resource)]
struct AnimalAssets {
    body: [Handle<Mesh>; 4],
    head: [Handle<Mesh>; 4],
    material: [Handle<StandardMaterial>; 4],
    drop_mesh: Handle<Mesh>,
}

fn setup_animal_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let body = AnimalKind::ALL.map(|k| {
        let b = k.body();
        meshes.add(Cuboid::new(b.x, b.y, b.z))
    });
    let head = AnimalKind::ALL.map(|k| {
        let s = k.body().min_element() * 0.7;
        meshes.add(Cuboid::new(s, s, s))
    });
    let material = AnimalKind::ALL.map(|k| {
        materials.add(StandardMaterial {
            base_color: k.color(),
            perceptual_roughness: 0.9,
            ..default()
        })
    });
    commands.insert_resource(AnimalAssets {
        body,
        head,
        material,
        drop_mesh: meshes.add(Cuboid::new(0.22, 0.22, 0.22)),
    });
}

fn spawn_herds(
    mut commands: Commands,
    world: Res<ChunkWorld>,
    world_gen: Res<WorldGenHandle>,
    assets: Res<AnimalAssets>,
    mut herded: ResMut<Herded>,
) {
    let sea = world_gen.0.sea_level;

    for (coord, slot) in world.chunks.iter() {
        if slot.meshed_at.is_none() || herded.0.contains(coord) {
            continue;
        }
        herded.0.insert(*coord);

        if hash(coord.x, coord.y, 1) % 100 >= HERD_CHANCE_PCT {
            continue;
        }
        let kind = AnimalKind::ALL[(hash(coord.x, coord.y, 2) % 4) as usize];

        let cx = CHUNK_SIZE / 2 + (hash(coord.x, coord.y, 3) % 9) as i32 - 4;
        let cz = CHUNK_SIZE / 2 + (hash(coord.x, coord.y, 4) % 9) as i32 - 4;

        for m in 0..HERD_SIZE as u32 {
            let lx = (cx + (hash(coord.x, coord.y, 10 + m * 2) % 5) as i32 - 2).clamp(0, CHUNK_SIZE - 1);
            let lz =
                (cz + (hash(coord.x, coord.y, 11 + m * 2) % 5) as i32 - 2).clamp(0, CHUNK_SIZE - 1);
            let wx = coord.x * CHUNK_SIZE + lx;
            let wz = coord.y * CHUNK_SIZE + lz;
            let h = world_gen.0.surface_height(wx, wz);
            if h <= sea {
                continue;
            }

            let body = kind.body();
            let pos = Vec3::new(wx as f32 + 0.5, h as f32 + 1.0 + body.y * 0.5, wz as f32 + 0.5);
            spawn_animal(&mut commands, &assets, kind, *coord, pos, false);
        }
    }
}

/// Spawns one animal (body + head child). `baby` starts it small and growing.
fn spawn_animal(
    commands: &mut Commands,
    assets: &AnimalAssets,
    kind: AnimalKind,
    home: ChunkCoord,
    pos: Vec3,
    baby: bool,
) {
    let body = kind.body();
    let (grow, scale) = if baby {
        (BABY_GROW_TIME, BABY_SCALE)
    } else {
        (0.0, 1.0)
    };
    commands
        .spawn((
            Mesh3d(assets.body[kind.index()].clone()),
            MeshMaterial3d(assets.material[kind.index()].clone()),
            Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
            Animal {
                kind,
                home,
                target: pos.xz(),
                wait: 0.0,
                vy: 0.0,
                health: kind.max_health(),
                love: 0.0,
                breed_cd: 0.0,
                grow,
                scale,
            },
        ))
        .with_children(|a| {
            a.spawn((
                Mesh3d(assets.head[kind.index()].clone()),
                MeshMaterial3d(assets.material[kind.index()].clone()),
                Transform::from_xyz(body.x * 0.45, body.y * 0.2, 0.0),
            ));
        });
}

fn animal_ai(
    time: Res<Time>,
    world: Res<ChunkWorld>,
    inventory: Res<Inventory>,
    player_q: Query<&Transform, (With<Player>, Without<Animal>)>,
    mut animals: Query<(&mut Transform, &mut Animal), Without<Player>>,
    mut rng: Local<u32>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    // A held food item lures every animal that likes it, within range.
    let lure = player_q.single().ok().map(|tf| (tf.translation, inventory.selected_item()));

    for (mut transform, mut animal) in &mut animals {
        // --- Breeding / growth timers ------------------------------
        if animal.love > 0.0 {
            animal.love -= dt;
        }
        if animal.breed_cd > 0.0 {
            animal.breed_cd -= dt;
        }
        if animal.grow > 0.0 {
            animal.grow -= dt;
            let t = 1.0 - (animal.grow / BABY_GROW_TIME).clamp(0.0, 1.0);
            animal.scale = BABY_SCALE + (1.0 - BABY_SCALE) * t;
        } else {
            animal.scale = 1.0;
        }
        transform.scale = Vec3::splat(animal.scale);

        if let Some((player_pos, Some(held))) = lure {
            if held == animal.kind.favorite_food()
                && transform.translation.xz().distance(player_pos.xz()) < LURE_RADIUS
            {
                animal.target = player_pos.xz();
                animal.wait = 0.0;
            }
        }

        // --- Wander --------------------------------------------------
        if animal.wait > 0.0 {
            animal.wait -= dt;
        } else {
            let to = animal.target - transform.translation.xz();
            if to.length() < 0.5 {
                let home = Vec2::new(
                    (animal.home.x * CHUNK_SIZE + CHUNK_SIZE / 2) as f32,
                    (animal.home.y * CHUNK_SIZE + CHUNK_SIZE / 2) as f32,
                );
                animal.target = home
                    + Vec2::new(frand(&mut rng) * 2.0 - 1.0, frand(&mut rng) * 2.0 - 1.0)
                        * WANDER_RADIUS;
                animal.wait = 0.6 + frand(&mut rng) * 1.8;
            } else {
                let dir = to.normalize_or_zero();
                let step = dir * animal.kind.speed() * dt;
                transform.translation.x += step.x;
                transform.translation.z += step.y;
                if dir.length_squared() > 1e-4 {
                    let yaw = dir.x.atan2(dir.y);
                    let t = 1.0 - (-10.0 * dt).exp();
                    transform.rotation = transform.rotation.slerp(Quat::from_rotation_y(yaw), t);
                }
            }
        }

        // --- Ground follow + gravity -------------------------------
        let (gx, gz) = (
            transform.translation.x.floor() as i32,
            transform.translation.z.floor() as i32,
        );
        let cy = transform.translation.y.round() as i32;
        let mut ground = 1.0f32;
        for y in ((cy - 5).max(1)..=(cy + 1)).rev() {
            if world.block_at(gx, y, gz).is_collidable() {
                ground = (y + 1) as f32;
                break;
            }
        }
        let half = animal.kind.body().y * 0.5 * animal.scale;
        animal.vy -= 22.0 * dt;
        let mut ny = transform.translation.y + animal.vy * dt;
        if ny - half <= ground {
            ny = ground + half;
            animal.vy = 0.0;
        }
        transform.translation.y = ny;
    }
}

/// Two adult, same-kind animals in love mode standing close together spawn a
/// baby, then go on cooldown. Capped per home chunk so herds can't explode.
fn breed_animals(
    mut commands: Commands,
    assets: Res<AnimalAssets>,
    mut animals: Query<(&Transform, &mut Animal), Without<Player>>,
) {
    let mut counts: HashMap<ChunkCoord, usize> = HashMap::new();
    for (_, a) in &animals {
        *counts.entry(a.home).or_default() += 1;
    }

    let mut births: Vec<(AnimalKind, ChunkCoord, Vec3)> = Vec::new();
    let mut pairs = animals.iter_combinations_mut();
    while let Some([(ta, mut a), (tb, mut b)]) = pairs.fetch_next() {
        if a.kind != b.kind
            || a.love <= 0.0
            || b.love <= 0.0
            || a.grow > 0.0
            || b.grow > 0.0
            || ta.translation.distance(tb.translation) > BREED_DIST
        {
            continue;
        }
        if counts.get(&a.home).copied().unwrap_or(0) >= MAX_PER_HOME {
            continue;
        }
        a.love = 0.0;
        b.love = 0.0;
        a.breed_cd = BREED_COOLDOWN;
        b.breed_cd = BREED_COOLDOWN;
        *counts.entry(a.home).or_default() += 1;
        births.push((a.kind, a.home, (ta.translation + tb.translation) * 0.5));
    }

    for (kind, home, pos) in births {
        spawn_animal(&mut commands, &assets, kind, home, pos, true);
    }
}

fn despawn_far_animals(
    mut commands: Commands,
    world: Res<ChunkWorld>,
    mut herded: ResMut<Herded>,
    animals: Query<(Entity, &Animal)>,
) {
    herded.0.retain(|c| world.chunks.contains_key(c));
    for (entity, animal) in &animals {
        if !world.chunks.contains_key(&animal.home) {
            commands.entity(entity).despawn();
        }
    }
}

/// Left-click near an animal to hit it (knife/axe = 5, otherwise 2) — unless
/// you're holding its favourite food, in which case it's fed (healed to full)
/// instead of hurt. On death it drops its food + resources on the ground.
#[allow(clippy::too_many_arguments)]
fn attack_animals(
    mouse: Res<ButtonInput<MouseButton>>,
    mut inventory: ResMut<Inventory>,
    assets: Res<AnimalAssets>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    player_q: Query<&Transform, With<Player>>,
    mut animals: Query<(Entity, &mut Transform, &mut Animal), Without<Player>>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: Local<u32>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (Ok(window), Ok((camera, cam_tf)), Ok(player_tf)) =
        (windows.single(), camera_q.single(), player_q.single())
    else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    // Pick the animal closest to the cursor, within reach.
    let mut best: Option<(Entity, f32)> = None;
    for (entity, tf, _) in &animals {
        if tf.translation.distance(player_tf.translation) > HIT_REACH {
            continue;
        }
        let Ok(screen) = camera.world_to_viewport(cam_tf, tf.translation) else {
            continue;
        };
        let px = screen.distance(cursor);
        if px <= HIT_PIXELS && best.map_or(true, |(_, b)| px < b) {
            best = Some((entity, px));
        }
    }
    let Some((target, _)) = best else {
        return;
    };

    let Ok((_, mut tf, mut animal)) = animals.get_mut(target) else {
        return;
    };

    // Feed instead of hurt when holding the animal's favourite food: heals, and
    // either speeds a baby's growth or puts an adult into love mode.
    if inventory.selected_item() == Some(animal.kind.favorite_food()) {
        if inventory.take(animal.kind.favorite_food(), 1) == 1 {
            animal.health = animal.kind.max_health();
            animal.vy = 2.8; // happy hop
            if animal.grow > 0.0 {
                animal.grow = (animal.grow - 12.0).max(0.0);
            } else if animal.breed_cd <= 0.0 {
                animal.love = LOVE_TIME;
            }
        }
        return;
    }

    let damage = match inventory.selected_item().and_then(Item::tool) {
        Some(ToolKind::Knife) | Some(ToolKind::Axe) => 5.0,
        _ => 2.0,
    };
    animal.health -= damage;
    let knock = (tf.translation - player_tf.translation)
        .with_y(0.0)
        .normalize_or_zero();
    tf.translation += knock * 0.45;
    animal.vy = 3.5;

    if animal.health <= 0.0 {
        // Babies just disappear; only grown animals leave drops.
        let drops: &[(Item, u32)] = if animal.grow > 0.0 {
            &[]
        } else {
            animal.kind.drops()
        };
        let pos = tf.translation;
        commands.entity(target).despawn();
        for &(item, amount) in drops {
            let jitter = Vec3::new(frand(&mut rng) - 0.5, 0.0, frand(&mut rng) - 0.5) * 0.8;
            commands.spawn((
                Mesh3d(assets.drop_mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: item.icon_color(),
                    perceptual_roughness: 0.8,
                    ..default()
                })),
                Transform::from_translation(pos + Vec3::Y * 0.1 + jitter),
                Pickup { item, amount },
            ));
        }
    }
}

fn hash(x: i32, z: i32, i: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x1656_67b1)
        ^ (z as u32).wrapping_mul(0x2545_1d31)
        ^ i.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h
}

fn frand(state: &mut u32) -> f32 {
    if *state == 0 {
        *state = 0x2545_f491;
    }
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 24) as f32
}
