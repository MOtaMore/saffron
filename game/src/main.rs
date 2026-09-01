//! Saffron — supervivencia 2.5D con mundo voxel infinito y vista de aguila.
//!
//! Hito 1: mundo procedural por chunks + exploracion.

mod animal;
mod block;
mod block_atlas;
mod camera;
mod chunk;
mod chunk_material;
mod container;
mod daynight;
mod discord;
mod farming;
mod fishing;
mod hud;
mod interact;
mod item;
mod keybinds;
mod menu;
mod mesher;
mod net;
mod pause;
mod player;
mod props;
mod save;
mod scatter;
mod skins;
mod station;
mod streaming;
mod survival;
mod view;
mod worldgen;

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WinitWindows;
use winit::window::Icon;

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--server") {
        return run_dedicated_server();
    }
    run_game(&args)
}

/// Windowed client (also used for Host / listen-server mode).
fn run_game(args: &[String]) -> AppExit {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Saffron — 2.5D Survival".into(),
                    ..default()
                }),
                ..default()
            })
            .set(ImagePlugin::default_nearest()),
    )
    .insert_resource(ClearColor(Color::srgb(0.53, 0.72, 0.92)))
    .add_plugins((
        chunk_material::ChunkMaterialPlugin,
        block_atlas::BlockAtlasPlugin,
        view::ViewPlugin,
        streaming::StreamingPlugin,
        camera::CameraPlugin,
        player::PlayerPlugin,
        item::InventoryPlugin,
        interact::InteractPlugin,
        skins::SkinPlugin,
        daynight::DayNightPlugin,
    ))
    .add_plugins((
        container::ContainerPlugin,
        station::StationPlugin,
        props::PropsPlugin,
        fishing::FishingPlugin,
        scatter::ScatterPlugin,
        animal::AnimalPlugin,
        pause::PausePlugin,
        save::SavePlugin,
        hud::HudPlugin,
        net::NetPlugin,
        survival::SurvivalPlugin,
        farming::FarmingPlugin,
    ))
    .add_plugins((
        keybinds::KeybindsPlugin,
        menu::MenuPlugin,
        discord::DiscordPlugin,
    ))
    .add_systems(Startup, setup_environment)
    .add_systems(Update, set_window_icon);

    // `--connect <host:port>` joins straight from the menu.
    if let Some(addr) = arg_value(args, "--connect") {
        app.insert_resource(net::AutoJoin(addr));
    }

    app.run()
}

/// Headless dedicated server ("modo servidor"). Config in `server.json`, world
/// persisted to `server_world.json`. Run with `game --server`.
fn run_dedicated_server() -> AppExit {
    let config = net::ServerConfig::load_or_create();
    App::new()
        .add_plugins(
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 30.0))),
        )
        .add_plugins(bevy::log::LogPlugin::default())
        .init_resource::<streaming::ChunkWorld>()
        .init_resource::<net::ChatLog>()
        .insert_resource(worldgen::WorldSeed(config.seed))
        .insert_resource(net::NetMode::Server)
        .insert_resource(config)
        .add_plugins(net::ServerCorePlugin)
        .run()
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn setup_environment(mut commands: Commands) {
    // The sun's light + ambient + sky colour are all driven by `daynight.rs`.
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.82, 1.0),
        brightness: 380.0,
        ..default()
    });
    commands.spawn((
        daynight::Sun,
        DirectionalLight {
            illuminance: 11_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default(),
    ));
}

/// Sets the taskbar / titlebar icon from the embedded `assets/icon.ico`. Runs
/// every frame until the OS window exists, then latches off.
fn set_window_icon(
    mut done: Local<bool>,
    winit_windows: Option<NonSend<WinitWindows>>,
    primary: Query<Entity, With<PrimaryWindow>>,
) {
    if *done {
        return;
    }
    let (Some(winit_windows), Ok(entity)) = (winit_windows, primary.single()) else {
        return;
    };
    let Some(window) = winit_windows.get_window(entity) else {
        return; // OS window not created yet
    };

    *done = true;
    let bytes = include_bytes!("../assets/icon.ico");
    let rgba = match image::load_from_memory(bytes) {
        Ok(img) => img.into_rgba8(),
        Err(e) => {
            warn!("window icon: {e}");
            return;
        }
    };
    let (w, h) = rgba.dimensions();
    match Icon::from_rgba(rgba.into_raw(), w, h) {
        Ok(icon) => window.set_window_icon(Some(icon)),
        Err(e) => warn!("window icon: {e}"),
    }
}
