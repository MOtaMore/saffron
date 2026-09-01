//! Title screen + save/load. The save is a JSON file next to the executable's
//! working dir (`save.json`): world seed, the player's edit overlay, inventory,
//! chest/furnace contents and the player transform.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::Block;
use crate::container::{ChestStores, Furnace, FurnaceStores, Mill, MillStores};
use crate::item::{CRAFT_SLOTS, HOTBAR, Inventory, Stack};
use crate::net::NetMenuButton;
use crate::pause::GameFlow;
use crate::player::{PendingPlayerSpawn, Player, PlayerBody};
use crate::skins::OpenSkinsButton;
use crate::streaming::{ChunkWorld, setup_worldgen};
use crate::worldgen::WorldSeed;

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveRequest>()
            .init_resource::<PendingLoad>()
            .add_systems(Startup, spawn_main_menu)
            .add_systems(
                OnEnter(GameFlow::Playing),
                apply_pending_load.before(setup_worldgen),
            )
            .add_systems(
                Update,
                (
                    menu_visuals,
                    extra_menu_button_visuals,
                    menu_buttons,
                    save_hotkey,
                    handle_save_requests,
                ),
            );
    }
}

/// Ask `save.rs` to write the game (optionally quitting after).
#[derive(Message)]
pub struct SaveRequest {
    pub then_quit: bool,
}

#[derive(Resource, Default)]
struct PendingLoad(Option<SaveGame>);

fn save_path() -> PathBuf {
    PathBuf::from("save.json")
}

// --- Serialized shape --------------------------------------------------

fn full_stat() -> f32 {
    100.0
}
fn default_tod() -> f32 {
    0.30
}

#[derive(Serialize, Deserialize)]
struct SaveGame {
    seed: u32,
    player_pos: [f32; 3],
    player_fly: bool,
    selected: usize,
    inventory: Vec<Option<Stack>>,
    edits: Vec<([i32; 3], Block)>,
    #[serde(default = "default_tod")]
    time_of_day: f32,
    #[serde(default = "full_stat")]
    health: f32,
    #[serde(default = "full_stat")]
    hunger: f32,
    #[serde(default = "full_stat")]
    thirst: f32,
    chests: Vec<([i32; 3], Vec<Option<Stack>>)>,
    furnaces: Vec<([i32; 3], Furnace)>,
    #[serde(default)]
    mills: Vec<([i32; 3], Mill)>,
}

fn read_save() -> Option<SaveGame> {
    let text = std::fs::read_to_string(save_path()).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_save(data: &SaveGame) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(save_path(), text)
}

fn fresh_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let mut x = n ^ (n >> 33);
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x as u32
}

// --- Load ------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn apply_pending_load(
    mut commands: Commands,
    mut pending: ResMut<PendingLoad>,
    mut seed: ResMut<WorldSeed>,
    mut world: ResMut<ChunkWorld>,
    mut chests: ResMut<ChestStores>,
    mut furnaces: ResMut<FurnaceStores>,
    mut mills: ResMut<MillStores>,
    mut inventory: ResMut<Inventory>,
    mut stats: ResMut<crate::survival::Stats>,
    mut clock: ResMut<crate::daynight::GameClock>,
) {
    let Some(save) = pending.0.take() else {
        // New game: start from a clean slate.
        stats.reset();
        clock.t = default_tod();
        return;
    };

    seed.0 = save.seed;
    clock.t = save.time_of_day;
    stats.health = save.health;
    stats.hunger = save.hunger;
    stats.thirst = save.thirst;
    stats.death_flash = 0.0;

    inventory.slots = save.inventory;
    inventory.slots.resize(HOTBAR * 5, None);
    inventory.selected = save.selected.min(HOTBAR - 1);
    inventory.carried = None;
    inventory.craft = [None; CRAFT_SLOTS];

    world.edits = save
        .edits
        .into_iter()
        .map(|([x, y, z], b)| (IVec3::new(x, y, z), b))
        .collect();
    world.prop_blocks.clear();

    chests.0 = save
        .chests
        .into_iter()
        .map(|([x, y, z], s)| (IVec3::new(x, y, z), s))
        .collect();
    furnaces.0 = save
        .furnaces
        .into_iter()
        .map(|([x, y, z], f)| (IVec3::new(x, y, z), f))
        .collect();
    mills.0 = save
        .mills
        .into_iter()
        .map(|([x, y, z], m)| (IVec3::new(x, y, z), m))
        .collect();

    commands.insert_resource(PendingPlayerSpawn {
        pos: Vec3::from_array(save.player_pos),
    });
}

// --- Save ----------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn snapshot(
    seed: &WorldSeed,
    world: &ChunkWorld,
    chests: &ChestStores,
    furnaces: &FurnaceStores,
    mills: &MillStores,
    inventory: &Inventory,
    transform: &Transform,
    body: &PlayerBody,
    stats: &crate::survival::Stats,
    clock: &crate::daynight::GameClock,
) -> SaveGame {
    SaveGame {
        seed: seed.0,
        player_pos: transform.translation.to_array(),
        player_fly: body.fly,
        selected: inventory.selected,
        inventory: inventory.slots.clone(),
        time_of_day: clock.t,
        health: stats.health,
        hunger: stats.hunger,
        thirst: stats.thirst,
        edits: world
            .edits
            .iter()
            .map(|(p, b)| ([p.x, p.y, p.z], *b))
            .collect(),
        chests: chests
            .0
            .iter()
            .map(|(p, s)| ([p.x, p.y, p.z], s.clone()))
            .collect(),
        furnaces: furnaces
            .0
            .iter()
            .map(|(p, f)| ([p.x, p.y, p.z], f.clone()))
            .collect(),
        mills: mills
            .0
            .iter()
            .map(|(p, m)| ([p.x, p.y, p.z], m.clone()))
            .collect(),
    }
}

fn save_hotkey(keys: Res<ButtonInput<KeyCode>>, mut writer: MessageWriter<SaveRequest>) {
    if keys.just_pressed(KeyCode::F5) {
        writer.write(SaveRequest { then_quit: false });
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_save_requests(
    mut requests: MessageReader<SaveRequest>,
    mut exit: MessageWriter<AppExit>,
    seed: Res<WorldSeed>,
    world: Res<ChunkWorld>,
    chests: Res<ChestStores>,
    furnaces: Res<FurnaceStores>,
    mills: Res<MillStores>,
    mut inventory: ResMut<Inventory>,
    stats: Res<crate::survival::Stats>,
    clock: Res<crate::daynight::GameClock>,
    player_q: Query<(&Transform, &PlayerBody), With<Player>>,
) {
    let mut quit = false;
    let mut requested = false;
    for r in requests.read() {
        requested = true;
        quit |= r.then_quit;
    }
    if requested {
        inventory.stow_all();
        if let Ok((transform, body)) = player_q.single() {
            let data = snapshot(
                &seed, &world, &chests, &furnaces, &mills, &inventory, transform, body, &stats,
                &clock,
            );
            match write_save(&data) {
                Ok(()) => info!("Partida guardada"),
                Err(e) => error!("No se pudo guardar: {e}"),
            }
        }
    }
    if quit {
        exit.write(AppExit::Success);
    }
}

// --- Main menu UI -------------------------------------------------

#[derive(Component)]
struct MenuRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum MenuButton {
    New,
    Continue,
    Quit,
}

fn spawn_main_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(16.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.06, 0.09, 0.12)),
            GlobalZIndex(200),
            MenuRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("AVES"),
                TextFont::from_font_size(48.0),
                TextColor(Color::srgb(0.9, 0.95, 1.0)),
            ));
            root.spawn((
                Text::new("Supervivencia 2.5D"),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb(0.55, 0.65, 0.75)),
            ));
            for (label, kind) in [
                ("Nueva partida", MenuButton::New),
                ("Continuar", MenuButton::Continue),
            ] {
                menu_button(root, label, kind);
            }
            menu_button(root, "Hostear mundo", NetMenuButton::Host);
            menu_button(root, "Unirse a un amigo", NetMenuButton::Join);
            menu_button(root, "Skin", OpenSkinsButton);
            menu_button(root, "Salir", MenuButton::Quit);
        });
}

/// A menu-styled button carrying `marker`.
fn menu_button(root: &mut ChildSpawnerCommands, label: &str, marker: impl Bundle) {
    root.spawn((
        Button,
        Node {
            width: Val::Px(240.0),
            height: Val::Px(46.0),
            margin: UiRect::top(Val::Px(4.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
        marker,
    ))
    .with_children(|b| {
        b.spawn((
            Text::new(label),
            TextFont::from_font_size(17.0),
            TextColor(Color::WHITE),
        ));
    });
}

/// Hover/press colours for the non-`MenuButton` menu entries (host/join/skin).
fn extra_menu_button_visuals(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            Changed<Interaction>,
            Or<(With<NetMenuButton>, With<OpenSkinsButton>)>,
        ),
    >,
) {
    for (interaction, mut bg) in &mut buttons {
        *bg = BackgroundColor(match interaction {
            Interaction::Pressed => Color::srgb(0.30, 0.30, 0.36),
            Interaction::Hovered => Color::srgb(0.26, 0.40, 0.52),
            Interaction::None => Color::srgb(0.20, 0.22, 0.28),
        });
    }
}

fn menu_visuals(
    flow: Res<State<GameFlow>>,
    mut root: Query<&mut Visibility, With<MenuRoot>>,
    mut buttons: Query<(&MenuButton, &Interaction, &mut BackgroundColor)>,
) {
    if let Ok(mut vis) = root.single_mut() {
        *vis = if matches!(flow.get(), GameFlow::Menu) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let has_save = save_path().exists();
    for (button, interaction, mut bg) in &mut buttons {
        *bg = if *button == MenuButton::Continue && !has_save {
            BackgroundColor(Color::srgb(0.13, 0.13, 0.15))
        } else {
            BackgroundColor(match interaction {
                Interaction::Pressed => Color::srgb(0.30, 0.30, 0.36),
                Interaction::Hovered => Color::srgb(0.26, 0.40, 0.52),
                Interaction::None => Color::srgb(0.20, 0.22, 0.28),
            })
        };
    }
}

fn menu_buttons(
    mut next: ResMut<NextState<GameFlow>>,
    mut pending: ResMut<PendingLoad>,
    mut seed: ResMut<WorldSeed>,
    mut exit: MessageWriter<AppExit>,
    buttons: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            MenuButton::New => {
                pending.0 = None;
                seed.0 = fresh_seed();
                next.set(GameFlow::Playing);
            }
            MenuButton::Continue => {
                if let Some(save) = read_save() {
                    pending.0 = Some(save);
                    next.set(GameFlow::Playing);
                }
            }
            MenuButton::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}
