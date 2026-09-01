//! Player-skin catalogue plus a screen to preview and pick one. The chosen skin
//! name is persisted to `settings.json` next to the executable and consumed by
//! [`crate::player::apply_player_skin`]. Reachable from the main menu and the
//! pause menu.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

const SKIN_DIR: &str = "assets/textures/player_skin";
const DEFAULT_SKIN: &str = "motamore_skin";

pub struct SkinPlugin;

impl Plugin for SkinPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_choice())
            .insert_resource(scan_catalog())
            .init_resource::<SkinScreen>()
            .add_systems(Startup, spawn_skin_screen)
            .add_systems(
                Update,
                (skin_screen_buttons, skin_screen_sync, skin_button_visuals),
            );
    }
}

/// The skin the player wears. `apply_player_skin` watches this.
#[derive(Resource)]
pub struct SkinChoice(pub String);

impl SkinChoice {
    pub fn asset_path(&self) -> String {
        format!("textures/player_skin/{}.png", self.0)
    }
}

/// Every `*.png` found in `assets/textures/player_skin/`, sorted.
#[derive(Resource, Default)]
pub struct SkinCatalog(pub Vec<String>);

/// Whether the skin screen is up, and which catalogue entry it is previewing.
#[derive(Resource, Default)]
pub struct SkinScreen {
    pub open: bool,
    browsing: usize,
}

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    skin: String,
}

fn settings_path() -> PathBuf {
    PathBuf::from("settings.json")
}

fn load_choice() -> SkinChoice {
    let name = std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
        .map(|s| s.skin)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SKIN.to_string());
    SkinChoice(name)
}

fn save_choice(name: &str) {
    let text = serde_json::to_string_pretty(&Settings {
        skin: name.to_string(),
    })
    .unwrap_or_default();
    if let Err(e) = std::fs::write(settings_path(), text) {
        warn!("no se pudo guardar settings.json: {e}");
    }
}

fn scan_catalog() -> SkinCatalog {
    let mut names: Vec<String> = std::fs::read_dir(SKIN_DIR)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|e| e.to_str()) == Some("png"))
                .then(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))
                .flatten()
        })
        .collect();
    names.sort();
    if names.is_empty() {
        names.push(DEFAULT_SKIN.to_string());
    }
    SkinCatalog(names)
}

// --- UI --------------------------------------------------------------

#[derive(Component)]
struct SkinRoot;
#[derive(Component)]
struct SkinPreview;
#[derive(Component)]
struct SkinName;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum SkinBtn {
    Prev,
    Next,
    Apply,
    Close,
}

/// Public so the main menu / pause menu can drop an "open skins" button in.
#[derive(Component, Clone, Copy)]
pub struct OpenSkinsButton;

fn spawn_skin_screen(mut commands: Commands) {
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
                row_gap: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.07, 0.10)),
            GlobalZIndex(210),
            Visibility::Hidden,
            SkinRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("SKIN"),
                TextFont::from_font_size(34.0),
                TextColor(Color::srgb(0.9, 0.95, 1.0)),
            ));
            root.spawn((
                Node {
                    width: Val::Px(220.0),
                    height: Val::Px(220.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.12, 0.14, 0.18)),
                ImageNode {
                    color: Color::WHITE,
                    ..default()
                },
                SkinPreview,
            ));
            root.spawn((
                Text::new(""),
                TextFont::from_font_size(16.0),
                TextColor(Color::WHITE),
                SkinName,
            ));
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|row| {
                skin_button(row, "◀", SkinBtn::Prev, 60.0);
                skin_button(row, "▶", SkinBtn::Next, 60.0);
            });
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(10.0),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            })
            .with_children(|row| {
                skin_button(row, "Aplicar", SkinBtn::Apply, 150.0);
                skin_button(row, "Cerrar", SkinBtn::Close, 150.0);
            });
        });
}

fn skin_button(row: &mut ChildSpawnerCommands, label: &str, kind: SkinBtn, width: f32) {
    row.spawn((
        Button,
        Node {
            width: Val::Px(width),
            height: Val::Px(44.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
        kind,
    ))
    .with_children(|b| {
        b.spawn((
            Text::new(label),
            TextFont::from_font_size(16.0),
            TextColor(Color::WHITE),
        ));
    });
}

/// Opens the screen when an [`OpenSkinsButton`] is clicked, and drives the
/// prev/next/apply/close buttons.
fn skin_screen_buttons(
    mut screen: ResMut<SkinScreen>,
    catalog: Res<SkinCatalog>,
    mut choice: ResMut<SkinChoice>,
    openers: Query<&Interaction, (Changed<Interaction>, With<OpenSkinsButton>)>,
    buttons: Query<(&Interaction, &SkinBtn), Changed<Interaction>>,
) {
    for interaction in &openers {
        if *interaction == Interaction::Pressed {
            screen.open = true;
            screen.browsing = catalog
                .0
                .iter()
                .position(|n| *n == choice.0)
                .unwrap_or(0);
        }
    }

    let n = catalog.0.len().max(1);
    for (interaction, btn) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match btn {
            SkinBtn::Prev => screen.browsing = (screen.browsing + n - 1) % n,
            SkinBtn::Next => screen.browsing = (screen.browsing + 1) % n,
            SkinBtn::Apply => {
                if let Some(name) = catalog.0.get(screen.browsing) {
                    choice.0 = name.clone();
                    save_choice(name);
                }
                screen.open = false;
            }
            SkinBtn::Close => screen.open = false,
        }
    }
}

fn skin_screen_sync(
    screen: Res<SkinScreen>,
    catalog: Res<SkinCatalog>,
    server: Res<AssetServer>,
    mut root: Query<&mut Visibility, With<SkinRoot>>,
    mut preview: Query<&mut ImageNode, With<SkinPreview>>,
    mut name: Query<&mut Text, With<SkinName>>,
) {
    if let Ok(mut vis) = root.single_mut() {
        *vis = if screen.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !screen.open {
        return;
    }
    let Some(current) = catalog.0.get(screen.browsing) else {
        return;
    };
    if let Ok(mut image) = preview.single_mut() {
        image.image = server.load(format!("textures/player_skin/{current}.png"));
        image.color = Color::WHITE;
    }
    if let Ok(mut text) = name.single_mut() {
        text.0 = format!("{}  ({}/{})", current, screen.browsing + 1, catalog.0.len());
    }
}

fn skin_button_visuals(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<SkinBtn>),
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
