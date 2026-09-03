//! Title screen + save/load. The save is a JSON file next to the executable's
//! working dir (`save.json`): world seed, the player's edit overlay, inventory,
//! chest/furnace contents and the player transform.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::Block;
use crate::container::{ChestStores, Furnace, FurnaceStores, Mill, MillStores};
use crate::item::{CRAFT_SLOTS, HOTBAR, Inventory, Stack};
use crate::pause::GameFlow;
use crate::player::{PendingPlayerSpawn, Player, PlayerBody};
use crate::streaming::{ChunkWorld, setup_worldgen};
use crate::worldgen::WorldSeed;

/// Folder (next to the executable) that holds one `<mundo>.json` per world.
pub const SAVES_DIR: &str = "saves";

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveRequest>()
            .add_message::<StartWorld>()
            .init_resource::<PendingLoad>()
            .init_resource::<CurrentWorld>()
            .add_systems(Startup, migrate_legacy_save)
            .add_systems(
                OnEnter(GameFlow::Playing),
                apply_pending_load.before(setup_worldgen),
            )
            .add_systems(
                Update,
                (
                    handle_start_world,
                    save_hotkey,
                    handle_save_requests,
                    tick_net_quit,
                ),
            );
    }
}

/// Which world file the running game loads from / saves to. Set by the menu
/// before entering [`GameFlow::Playing`].
#[derive(Resource)]
pub struct CurrentWorld(pub PathBuf);

impl Default for CurrentWorld {
    fn default() -> Self {
        CurrentWorld(PathBuf::from(SAVES_DIR).join("Mundo.json"))
    }
}

/// Fired by the "Jugar" screen: load `path` (or start it fresh if `create`).
#[derive(Message)]
pub struct StartWorld {
    pub path: PathBuf,
    pub create: bool,
}

fn migrate_legacy_save() {
    let legacy = PathBuf::from("save.json");
    let dir = PathBuf::from(SAVES_DIR);
    let empty = std::fs::read_dir(&dir)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true);
    if legacy.exists() && empty {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::rename(&legacy, dir.join("Mundo.json"));
    }
}

fn handle_start_world(
    mut events: MessageReader<StartWorld>,
    mut current: ResMut<CurrentWorld>,
    mut pending: ResMut<PendingLoad>,
    mut seed: ResMut<WorldSeed>,
    mut next: ResMut<NextState<GameFlow>>,
) {
    let Some(ev) = events.read().last() else {
        return;
    };
    current.0 = ev.path.clone();
    match (ev.create, read_world(&ev.path)) {
        (false, Some(save)) => {
            pending.0 = Some(save);
        }
        _ => {
            pending.0 = None;
            seed.0 = fresh_seed();
        }
    }
    next.set(GameFlow::Playing);
}

/// Ask `save.rs` to write the game (optionally quitting after).
#[derive(Message)]
pub struct SaveRequest {
    pub then_quit: bool,
}

#[derive(Resource, Default)]
pub(crate) struct PendingLoad(Option<SaveGame>);

// --- Serialized shape --------------------------------------------------

fn full_stat() -> f32 {
    100.0
}
fn default_tod() -> f32 {
    0.30
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SaveGame {
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

fn read_world(path: &Path) -> Option<SaveGame> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_world(path: &Path, data: &SaveGame) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
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
pub(crate) fn apply_pending_load(
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

fn save_hotkey(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::keybinds::Keybinds>,
    mut writer: MessageWriter<SaveRequest>,
) {
    if binds.just_pressed(&keys, crate::keybinds::Action::QuickSave) {
        writer.write(SaveRequest { then_quit: false });
    }
}

/// Delays `AppExit` in client mode so `net::client_save_on_request` — and, more
/// importantly, the background socket writer thread it hands the frame to — has
/// time to push the final `SaveState` to the server before the process ends.
/// A frame count was too short: with the pause menu open the loop is uncapped,
/// so 8 frames could elapse in under a millisecond, before the writer thread
/// was ever scheduled. Now it's a wall-clock deadline with a small blocking
/// sleep per tick so that thread actually gets CPU.
#[derive(Resource)]
struct NetQuitDelay(std::time::Instant);

fn tick_net_quit(delay: Option<Res<NetQuitDelay>>, mut exit: MessageWriter<AppExit>) {
    let Some(delay) = delay else {
        return;
    };
    std::thread::sleep(std::time::Duration::from_millis(20));
    if delay.0.elapsed() >= std::time::Duration::from_millis(300) {
        exit.write(AppExit::Success);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_save_requests(
    mut requests: MessageReader<SaveRequest>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    mode: Res<crate::net::NetMode>,
    seed: Res<WorldSeed>,
    world: Res<ChunkWorld>,
    chests: Res<ChestStores>,
    furnaces: Res<FurnaceStores>,
    mills: Res<MillStores>,
    mut inventory: ResMut<Inventory>,
    stats: Res<crate::survival::Stats>,
    clock: Res<crate::daynight::GameClock>,
    current: Res<CurrentWorld>,
    player_q: Query<(&Transform, &PlayerBody), With<Player>>,
) {
    let mut quit = false;
    let mut requested = false;
    for r in requests.read() {
        requested = true;
        quit |= r.then_quit;
    }
    if !requested {
        return;
    }
    inventory.stow_all();

    // On a client the server owns the save (`net::client_save_on_request` pushes
    // the state); don't write a local world file.
    if *mode == crate::net::NetMode::Client {
        if quit {
            commands.insert_resource(NetQuitDelay(std::time::Instant::now()));
        }
        return;
    }

    if let Ok((transform, body)) = player_q.single() {
        let data = snapshot(
            &seed, &world, &chests, &furnaces, &mills, &inventory, transform, body, &stats, &clock,
        );
        match write_world(&current.0, &data) {
            Ok(()) => info!("Game saved to {}", current.0.display()),
            Err(e) => error!("Could not save: {e}"),
        }
    }
    if quit {
        exit.write(AppExit::Success);
    }
}
