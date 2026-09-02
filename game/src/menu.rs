//! Front-end: the four-button title screen and its sub-screens.
//!
//! * **Jugar** – list of worlds in `saves/`, pick one or create a new one.
//! * **Multijugador** – list of servers from `servers.json`, add by IP and join.
//! * **Configuración** – Skin / Controles / Gráficos.
//! * **Salir** – quit.
//!
//! Skin and Controles screens live in their own modules; this file only drops
//! the buttons that open them. Graphics settings live here (`graphics.json`).

use std::path::PathBuf;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::chunk_material::CutoutSettings;
use crate::keybinds::OpenControlsButton;
use crate::net::{JoinServer, normalize_addr};
use crate::pause::GameFlow;
use crate::save::{SAVES_DIR, StartWorld};
use crate::skins::OpenSkinsButton;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuNav>()
            .init_resource::<WorldList>()
            .insert_resource(ServerList::load())
            .insert_resource(GraphicsSettings::load())
            .init_resource::<TextEntry>()
            .add_systems(Startup, spawn_menu)
            .add_systems(
                Update,
                (
                    root_buttons,
                    nav_buttons,
                    world_buttons,
                    server_buttons,
                    graphics_buttons,
                    text_entry_capture,
                    menu_escape,
                )
                    .run_if(in_state(GameFlow::Menu)),
            )
            .add_systems(
                Update,
                (
                    rebuild_world_list,
                    rebuild_server_list,
                    sync_menu_visibility,
                    sync_text_entry,
                    sync_graphics_labels,
                    menu_button_visuals,
                    apply_graphics,
                ),
            );
    }
}

// === Navigation ======================================================

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum MenuNav {
    #[default]
    Root,
    Worlds,
    Servers,
    Settings,
    Graphics,
}

#[derive(Component)]
struct ScreenOf(MenuNav);

#[derive(Component, Clone, Copy)]
enum RootButton {
    Play,
    Multiplayer,
    Editor,
    Settings,
    Quit,
}

/// Generic "go to this screen" button (also used for Back -> Root).
#[derive(Component, Clone, Copy)]
struct GoTo(MenuNav);

// === World list ======================================================

#[derive(Resource, Default)]
struct WorldList {
    worlds: Vec<WorldItem>,
    dirty: bool,
}

#[derive(Clone)]
struct WorldItem {
    name: String,
    path: PathBuf,
}

fn scan_worlds() -> Vec<WorldItem> {
    let mut items: Vec<(WorldItem, std::time::SystemTime)> = std::fs::read_dir(SAVES_DIR)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((WorldItem { name, path }, modified))
        })
        .collect();
    items.sort_by(|a, b| b.1.cmp(&a.1));
    items.into_iter().map(|(w, _)| w).collect()
}

fn sanitize_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-'))
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "World".to_string()
    } else {
        cleaned.to_string()
    }
}

fn unique_world_path(name: &str) -> PathBuf {
    let base = PathBuf::from(SAVES_DIR);
    let mut candidate = base.join(format!("{name}.json"));
    let mut n = 2;
    while candidate.exists() {
        candidate = base.join(format!("{name} ({n}).json"));
        n += 1;
    }
    candidate
}

// === Server list =====================================================

#[derive(Resource, Default)]
struct ServerList {
    servers: Vec<ServerItem>,
    #[allow(dead_code)]
    loaded: bool,
    dirty: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct ServerItem {
    name: String,
    addr: String,
}

impl ServerList {
    fn path() -> PathBuf {
        PathBuf::from("servers.json")
    }
    fn load() -> Self {
        let servers = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<ServerItem>>(&t).ok())
            .unwrap_or_default();
        ServerList {
            servers,
            loaded: true,
            dirty: true,
        }
    }
    fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(&self.servers) {
            let _ = std::fs::write(Self::path(), text);
        }
    }
}

// === Graphics settings ===============================================

#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct GraphicsSettings {
    pub cutout: bool,
    pub cutout_radius: f32,
    pub brightness: f32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        GraphicsSettings {
            cutout: true,
            cutout_radius: 3.4,
            brightness: 380.0,
        }
    }
}

impl GraphicsSettings {
    fn path() -> PathBuf {
        PathBuf::from("graphics.json")
    }
    fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), text);
        }
    }
}

fn apply_graphics(
    gfx: Res<GraphicsSettings>,
    mut cutout: ResMut<CutoutSettings>,
    ambient: Option<ResMut<GlobalAmbientLight>>,
) {
    cutout.enabled = gfx.cutout;
    cutout.radius = gfx.cutout_radius;
    if let Some(mut ambient) = ambient {
        ambient.brightness = gfx.brightness;
    }
}

#[derive(Component, Clone, Copy)]
enum GraphicsButton {
    ToggleCutout,
    RadiusDown,
    RadiusUp,
    BrightnessDown,
    BrightnessUp,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum GraphicsLabel {
    Cutout,
    Radius,
    Brightness,
}

// === Text entry (new world / new server) =============================

#[derive(Resource, Default)]
struct TextEntry {
    target: Option<EntryTarget>,
    buf: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryTarget {
    NewWorld,
    NewServer,
}

#[derive(Component)]
struct EntryPill;
#[derive(Component)]
struct EntryText;
#[derive(Component, Clone, Copy)]
struct OpenEntry(EntryTarget);

// === Spawn ===========================================================

const LIST_MAX: usize = 8;

fn screen_root(nav: MenuNav) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: Val::Px(10.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.06, 0.09, 0.12)),
        GlobalZIndex(200),
        Visibility::Hidden,
        ScreenOf(nav),
    )
}

fn title(root: &mut ChildSpawnerCommands, text: &str, size: f32) {
    root.spawn((
        Text::new(text),
        TextFont::from_font_size(size),
        TextColor(Color::srgb(0.9, 0.95, 1.0)),
        Node {
            margin: UiRect::bottom(Val::Px(6.0)),
            ..default()
        },
    ));
}

fn button(root: &mut ChildSpawnerCommands, label: &str, marker: impl Bundle) {
    root.spawn((
        Button,
        Node {
            width: Val::Px(260.0),
            height: Val::Px(44.0),
            margin: UiRect::top(Val::Px(3.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
        MenuBtnTint,
        marker,
    ))
    .with_children(|b| {
        b.spawn((
            Text::new(label),
            TextFont::from_font_size(16.0),
            TextColor(Color::WHITE),
        ));
    });
}

/// Marker so one visuals system tints every menu button.
#[derive(Component)]
struct MenuBtnTint;

#[derive(Component)]
struct WorldListNode;
#[derive(Component)]
struct ServerListNode;

fn spawn_menu(mut commands: Commands) {
    // --- Root -------------------------------------------------------
    commands.spawn(screen_root(MenuNav::Root)).with_children(|root| {
        title(root, "SAFFRON", 48.0);
        root.spawn((
            Text::new("2.5D Survival"),
            TextFont::from_font_size(14.0),
            TextColor(Color::srgb(0.55, 0.65, 0.75)),
            Node {
                margin: UiRect::bottom(Val::Px(10.0)),
                ..default()
            },
        ));
        button(root, "Play", RootButton::Play);
        button(root, "Multiplayer", RootButton::Multiplayer);
        button(root, "Structure Editor", RootButton::Editor);
        button(root, "Settings", RootButton::Settings);
        button(root, "Quit", RootButton::Quit);
    });

    // --- Worlds ---------------------------------------------------
    commands
        .spawn(screen_root(MenuNav::Worlds))
        .with_children(|root| {
            title(root, "WORLDS", 32.0);
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    align_items: AlignItems::Center,
                    min_height: Val::Px(40.0),
                    ..default()
                },
                WorldListNode,
            ));
            button(root, "+ New world", OpenEntry(EntryTarget::NewWorld));
            button(root, "Back", GoTo(MenuNav::Root));
        });

    // --- Servers ------------------------------------------------
    commands
        .spawn(screen_root(MenuNav::Servers))
        .with_children(|root| {
            title(root, "MULTIPLAYER", 30.0);
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    align_items: AlignItems::Center,
                    min_height: Val::Px(40.0),
                    ..default()
                },
                ServerListNode,
            ));
            button(root, "+ Add server", OpenEntry(EntryTarget::NewServer));
            button(root, "Back", GoTo(MenuNav::Root));
        });

    // --- Settings ---------------------------------------------
    commands
        .spawn(screen_root(MenuNav::Settings))
        .with_children(|root| {
            title(root, "SETTINGS", 30.0);
            button(root, "Skin", OpenSkinsButton);
            button(root, "Controls", OpenControlsButton);
            button(root, "Graphics", GoTo(MenuNav::Graphics));
            button(root, "Back", GoTo(MenuNav::Root));
        });

    // --- Graphics -------------------------------------------
    commands
        .spawn(screen_root(MenuNav::Graphics))
        .with_children(|root| {
            title(root, "GRAPHICS", 30.0);
            gfx_row(root, GraphicsLabel::Cutout, "Vision cutout", &[
                ("", GraphicsButton::ToggleCutout),
            ]);
            gfx_row(root, GraphicsLabel::Radius, "Cutout radius", &[
                ("−", GraphicsButton::RadiusDown),
                ("＋", GraphicsButton::RadiusUp),
            ]);
            gfx_row(root, GraphicsLabel::Brightness, "Ambient brightness", &[
                ("−", GraphicsButton::BrightnessDown),
                ("＋", GraphicsButton::BrightnessUp),
            ]);
            button(root, "Back", GoTo(MenuNav::Settings));
        });

    // --- Shared text-entry pill (floats near bottom) -------------
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(90.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(205),
            Visibility::Hidden,
            EntryPill,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    min_width: Val::Px(320.0),
                    height: Val::Px(40.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(12.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.12, 0.14, 0.18)),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(""),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::WHITE),
                    EntryText,
                ));
            });
        });
}

fn gfx_row(
    root: &mut ChildSpawnerCommands,
    label: GraphicsLabel,
    name: &str,
    buttons: &[(&str, GraphicsButton)],
) {
    root.spawn(Node {
        width: Val::Px(360.0),
        height: Val::Px(34.0),
        flex_direction: FlexDirection::Row,
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(|row| {
        row.spawn((
            Text::new(name),
            TextFont::from_font_size(15.0),
            TextColor(Color::WHITE),
        ));
        row.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|controls| {
            controls.spawn((
                Text::new(""),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb(0.8, 0.85, 0.95)),
                label,
            ));
            for (glyph, kind) in buttons {
                controls
                    .spawn((
                        Button,
                        Node {
                            width: Val::Px(38.0),
                            height: Val::Px(28.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
                        MenuBtnTint,
                        *kind,
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new(*glyph),
                            TextFont::from_font_size(15.0),
                            TextColor(Color::WHITE),
                        ));
                    });
            }
        });
    });
}

// === Systems: navigation ============================================

fn root_buttons(
    mut nav: ResMut<MenuNav>,
    mut exit: MessageWriter<AppExit>,
    mut worlds: ResMut<WorldList>,
    mut servers: ResMut<ServerList>,
    mut flow: ResMut<NextState<GameFlow>>,
    buttons: Query<(&Interaction, &RootButton), Changed<Interaction>>,
) {
    for (interaction, btn) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match btn {
            RootButton::Play => {
                worlds.dirty = true;
                *nav = MenuNav::Worlds;
            }
            RootButton::Multiplayer => {
                servers.dirty = true;
                *nav = MenuNav::Servers;
            }
            RootButton::Editor => flow.set(GameFlow::Editor),
            RootButton::Settings => *nav = MenuNav::Settings,
            RootButton::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

fn nav_buttons(
    mut nav: ResMut<MenuNav>,
    mut entry: ResMut<TextEntry>,
    buttons: Query<(&Interaction, &GoTo), Changed<Interaction>>,
) {
    for (interaction, go) in &buttons {
        if *interaction == Interaction::Pressed {
            entry.target = None;
            *nav = go.0;
        }
    }
}

fn menu_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut nav: ResMut<MenuNav>,
    mut entry: ResMut<TextEntry>,
    mut rebind: ResMut<crate::keybinds::RebindScreen>,
    mut skin: ResMut<crate::skins::SkinScreen>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    // Overlays first: close them, don't also navigate underneath.
    if rebind.open {
        if !rebind.is_capturing() {
            rebind.open = false;
        }
        return;
    }
    if skin.open {
        skin.open = false;
        return;
    }
    if entry.target.is_some() {
        entry.target = None;
        return;
    }
    *nav = match *nav {
        MenuNav::Graphics => MenuNav::Settings,
        _ => MenuNav::Root,
    };
}

fn sync_menu_visibility(
    nav: Res<MenuNav>,
    flow: Res<State<GameFlow>>,
    mut screens: Query<(&ScreenOf, &mut Visibility)>,
) {
    let in_menu = matches!(flow.get(), GameFlow::Menu);
    for (screen, mut vis) in &mut screens {
        *vis = if in_menu && screen.0 == *nav {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

// === Systems: world list ===========================================

fn rebuild_world_list(
    mut commands: Commands,
    mut list: ResMut<WorldList>,
    node: Query<Entity, With<WorldListNode>>,
    rows: Query<Entity, With<WorldRow>>,
) {
    if !list.dirty {
        return;
    }
    list.dirty = false;
    list.worlds = scan_worlds();

    let Ok(node) = node.single() else {
        return;
    };
    for row in &rows {
        commands.entity(row).despawn();
    }

    commands.entity(node).with_children(|list_node| {
        if list.worlds.is_empty() {
            list_node.spawn((
                Text::new("No worlds yet."),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb(0.6, 0.65, 0.72)),
                WorldRow,
            ));
        }
        for (i, world) in list.worlds.iter().take(LIST_MAX).enumerate() {
            list_node
                .spawn((
                    Node {
                        width: Val::Px(360.0),
                        height: Val::Px(34.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(6.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    WorldRow,
                ))
                .with_children(|row| {
                    row.spawn((
                        Button,
                        Node {
                            flex_grow: 1.0,
                            height: Val::Px(34.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
                        MenuBtnTint,
                        WorldRowButton { index: i },
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new(world.name.clone()),
                            TextFont::from_font_size(15.0),
                            TextColor(Color::WHITE),
                        ));
                    });
                    row.spawn((
                        Button,
                        Node {
                            width: Val::Px(34.0),
                            height: Val::Px(34.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.32, 0.16, 0.16)),
                        DeleteButton {
                            index: i,
                            armed: false,
                            server: false,
                        },
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new("✕"),
                            TextFont::from_font_size(14.0),
                            TextColor(Color::WHITE),
                        ));
                    });
                });
        }
    });
}

#[derive(Component)]
struct WorldRow;
#[derive(Component)]
struct WorldRowButton {
    index: usize,
}

fn world_buttons(
    mut list: ResMut<WorldList>,
    mut start: MessageWriter<StartWorld>,
    picks: Query<(&Interaction, &WorldRowButton), Changed<Interaction>>,
    mut deletes: Query<(&Interaction, &mut DeleteButton, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, pick) in &picks {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(world) = list.worlds.get(pick.index) {
            start.write(StartWorld {
                path: world.path.clone(),
                create: false,
            });
        }
    }
    for (interaction, mut del, mut bg) in &mut deletes {
        if *interaction != Interaction::Pressed || del.server {
            continue;
        }
        if !del.armed {
            del.armed = true;
            *bg = BackgroundColor(Color::srgb(0.75, 0.20, 0.20));
            continue;
        }
        if let Some(world) = list.worlds.get(del.index).cloned() {
            let _ = std::fs::remove_file(&world.path);
            list.dirty = true;
        }
    }
}

// === Systems: server list ==========================================

fn rebuild_server_list(
    mut commands: Commands,
    mut list: ResMut<ServerList>,
    node: Query<Entity, With<ServerListNode>>,
    rows: Query<Entity, With<ServerRow>>,
) {
    if !list.dirty {
        return;
    }
    list.dirty = false;

    let Ok(node) = node.single() else {
        return;
    };
    for row in &rows {
        commands.entity(row).despawn();
    }

    commands.entity(node).with_children(|list_node| {
        if list.servers.is_empty() {
            list_node.spawn((
                Text::new("No servers. Add one by its IP."),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb(0.6, 0.65, 0.72)),
                ServerRow,
            ));
        }
        for (i, server) in list.servers.iter().take(LIST_MAX).enumerate() {
            list_node
                .spawn((
                    Node {
                        width: Val::Px(360.0),
                        height: Val::Px(34.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(6.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    ServerRow,
                ))
                .with_children(|row| {
                    row.spawn((
                        Button,
                        Node {
                            flex_grow: 1.0,
                            height: Val::Px(34.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
                        MenuBtnTint,
                        ServerRowButton { index: i },
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new(format!("{}  —  {}", server.name, server.addr)),
                            TextFont::from_font_size(14.0),
                            TextColor(Color::WHITE),
                        ));
                    });
                    row.spawn((
                        Button,
                        Node {
                            width: Val::Px(34.0),
                            height: Val::Px(34.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.32, 0.16, 0.16)),
                        DeleteButton {
                            index: i,
                            armed: false,
                            server: true,
                        },
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new("✕"),
                            TextFont::from_font_size(14.0),
                            TextColor(Color::WHITE),
                        ));
                    });
                });
        }
    });
}

#[derive(Component)]
struct ServerRow;
#[derive(Component)]
struct ServerRowButton {
    index: usize,
}

#[derive(Component)]
struct DeleteButton {
    index: usize,
    armed: bool,
    server: bool,
}

fn server_buttons(
    mut list: ResMut<ServerList>,
    mut join: MessageWriter<JoinServer>,
    picks: Query<(&Interaction, &ServerRowButton), Changed<Interaction>>,
    mut deletes: Query<(&Interaction, &mut DeleteButton, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, pick) in &picks {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(server) = list.servers.get(pick.index) {
            join.write(JoinServer(server.addr.clone()));
        }
    }
    let mut changed = false;
    for (interaction, mut del, mut bg) in &mut deletes {
        if *interaction != Interaction::Pressed || !del.server {
            continue;
        }
        if !del.armed {
            del.armed = true;
            *bg = BackgroundColor(Color::srgb(0.75, 0.20, 0.20));
            continue;
        }
        if del.index < list.servers.len() {
            list.servers.remove(del.index);
            changed = true;
        }
    }
    if changed {
        list.save();
        list.dirty = true;
    }
}

// === Systems: graphics ============================================

fn graphics_buttons(
    mut gfx: ResMut<GraphicsSettings>,
    buttons: Query<(&Interaction, &GraphicsButton), Changed<Interaction>>,
) {
    for (interaction, btn) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match btn {
            GraphicsButton::ToggleCutout => gfx.cutout = !gfx.cutout,
            GraphicsButton::RadiusDown => {
                gfx.cutout_radius = (gfx.cutout_radius - 0.4).max(1.5)
            }
            GraphicsButton::RadiusUp => {
                gfx.cutout_radius = (gfx.cutout_radius + 0.4).min(7.0)
            }
            GraphicsButton::BrightnessDown => {
                gfx.brightness = (gfx.brightness - 40.0).max(120.0)
            }
            GraphicsButton::BrightnessUp => {
                gfx.brightness = (gfx.brightness + 40.0).min(700.0)
            }
        }
        gfx.save();
    }
}

fn sync_graphics_labels(
    gfx: Res<GraphicsSettings>,
    mut labels: Query<(&GraphicsLabel, &mut Text)>,
) {
    for (label, mut text) in &mut labels {
        text.0 = match label {
            GraphicsLabel::Cutout => {
                if gfx.cutout { "ON".into() } else { "OFF".into() }
            }
            GraphicsLabel::Radius => format!("{:.1}", gfx.cutout_radius),
            GraphicsLabel::Brightness => format!("{:.0}", gfx.brightness),
        };
    }
}

// === Systems: text entry =========================================

const ENTRY_MAX: usize = 64;

fn text_entry_capture(
    mut entry: ResMut<TextEntry>,
    mut servers: ResMut<ServerList>,
    mut start: MessageWriter<StartWorld>,
    keys: Res<ButtonInput<KeyCode>>,
    openers: Query<(&Interaction, &OpenEntry), Changed<Interaction>>,
    mut keebs: MessageReader<KeyboardInput>,
) {
    for (interaction, opener) in &openers {
        if *interaction == Interaction::Pressed {
            entry.target = Some(opener.0);
            entry.buf.clear();
        }
    }

    let Some(target) = entry.target else {
        keebs.clear();
        return;
    };

    let ctrl =
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    for ev in keebs.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        // Use the *logical* key so the layout decides the character — that is
        // what makes `:` (Shift+; on many layouts, its own key on others) work.
        match &ev.logical_key {
            Key::Escape => {
                entry.target = None;
                break;
            }
            Key::Backspace => {
                entry.buf.pop();
            }
            Key::Enter => {
                let raw = entry.buf.trim().to_string();
                entry.target = None;
                match target {
                    EntryTarget::NewWorld => {
                        let name = sanitize_name(&raw);
                        let path = unique_world_path(&name);
                        start.write(StartWorld { path, create: true });
                    }
                    EntryTarget::NewServer => {
                        if !raw.is_empty() {
                            let addr = normalize_addr(&raw);
                            servers.servers.push(ServerItem { name: raw, addr });
                            servers.save();
                            servers.dirty = true;
                        }
                    }
                }
                break;
            }
            Key::Space => push_char(&mut entry.buf, ' '),
            Key::Character(s) => {
                if ctrl {
                    if s.as_str().eq_ignore_ascii_case("v") {
                        if let Some(text) = read_clipboard() {
                            for ch in text.chars() {
                                push_char(&mut entry.buf, ch);
                            }
                        }
                    }
                    continue; // ignore other Ctrl+<key> combos
                }
                for ch in s.chars() {
                    push_char(&mut entry.buf, ch);
                }
            }
            _ => {}
        }
    }
}

fn push_char(buf: &mut String, ch: char) {
    if !ch.is_control() && buf.chars().count() < ENTRY_MAX {
        buf.push(ch);
    }
}

fn read_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn sync_text_entry(
    entry: Res<TextEntry>,
    mut pill: Query<&mut Visibility, With<EntryPill>>,
    mut text: Query<&mut Text, With<EntryText>>,
) {
    let active = entry.target.is_some();
    if let Ok(mut vis) = pill.single_mut() {
        *vis = if active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !active {
        return;
    }
    if let Ok(mut text) = text.single_mut() {
        let prompt = match entry.target {
            Some(EntryTarget::NewWorld) => "World name: ",
            Some(EntryTarget::NewServer) => "Address (IP:port): ",
            None => "",
        };
        text.0 = format!("{prompt}{}_", entry.buf);
    }
}

// === Visuals =====================================================

fn menu_button_visuals(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<MenuBtnTint>),
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
