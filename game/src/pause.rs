//! Escape pause menu: freezes gameplay (virtual time) and shows Resume / Quit.

use bevy::prelude::*;
use bevy::time::Virtual;

use crate::container::OpenContainer;
use crate::fishing::{Bobber, FishingState, end_fishing};
use crate::item::{Inventory, InventoryOpen};
use crate::net::ChatLog;
use crate::save::SaveRequest;
use crate::skins::OpenSkinsButton;
use crate::station::StationChoices;

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Paused>()
            .init_state::<GameFlow>()
            .add_systems(Startup, spawn_pause_menu)
            .add_systems(Update, (on_escape, pause_buttons, pause_button_visuals));
    }
}

/// Title screen vs. in-game.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameFlow {
    #[default]
    Menu,
    Playing,
}

#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// Run condition for gameplay systems: not paused *and* actually in-game.
pub fn not_paused(paused: Res<Paused>, flow: Res<State<GameFlow>>) -> bool {
    !paused.0 && matches!(flow.get(), GameFlow::Playing)
}

#[derive(Component)]
struct PauseRoot;

#[derive(Component, Clone, Copy)]
enum PauseButton {
    Resume,
    Quit,
}

fn spawn_pause_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            Visibility::Hidden,
            PauseRoot,
        ))
        .with_children(|root| {
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(14.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|col| {
                col.spawn((
                    Text::new("PAUSED"),
                    TextFont::from_font_size(30.0),
                    TextColor(Color::WHITE),
                ));
                pause_button(col, "Back to game", PauseButton::Resume);
                pause_button(col, "Skin", OpenSkinsButton);
                pause_button(col, "Save and quit", PauseButton::Quit);
            });
        });
}

fn pause_button(col: &mut ChildSpawnerCommands, label: &str, marker: impl Bundle) {
    col.spawn((
        Button,
        Node {
            width: Val::Px(230.0),
            height: Val::Px(46.0),
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
            TextFont::from_font_size(16.0),
            TextColor(Color::WHITE),
        ));
    });
}

/// Escape closes whatever is open (in priority order); only with nothing open
/// does it toggle the pause menu.
#[allow(clippy::too_many_arguments)]
fn on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    flow: Res<State<GameFlow>>,
    chat: Res<ChatLog>,
    mut paused: ResMut<Paused>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut inv_open: ResMut<InventoryOpen>,
    mut container: ResMut<OpenContainer>,
    mut station_menu: ResMut<StationChoices>,
    mut inventory: ResMut<Inventory>,
    mut fishing: ResMut<FishingState>,
    mut commands: Commands,
    bobbers: Query<Entity, With<Bobber>>,
    mut root: Query<&mut Visibility, With<PauseRoot>>,
) {
    if !keys.just_pressed(KeyCode::Escape)
        || matches!(flow.get(), GameFlow::Menu)
        || chat.capturing()
    {
        return;
    }
    if paused.0 {
        paused.0 = false;
        set_paused(false, &mut virtual_time, &mut root);
    } else if !station_menu.0.is_empty() {
        station_menu.0.clear();
    } else if container.0.is_some() {
        container.0 = None;
        inventory.return_carried();
    } else if inv_open.0 {
        inv_open.0 = false;
        inventory.stow_all();
    } else if fishing.busy() {
        end_fishing(&mut fishing, &mut commands, &bobbers);
    } else {
        paused.0 = true;
        set_paused(true, &mut virtual_time, &mut root);
    }
}

fn pause_buttons(
    mut paused: ResMut<Paused>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut save: MessageWriter<SaveRequest>,
    mut root: Query<&mut Visibility, With<PauseRoot>>,
    buttons: Query<(&Interaction, &PauseButton), Changed<Interaction>>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            PauseButton::Quit => {
                // save first, then quit (handled in `save.rs`)
                save.write(SaveRequest { then_quit: true });
            }
            PauseButton::Resume => {
                paused.0 = false;
                set_paused(false, &mut virtual_time, &mut root);
            }
        }
    }
}

fn set_paused(
    paused: bool,
    virtual_time: &mut Time<Virtual>,
    root: &mut Query<&mut Visibility, With<PauseRoot>>,
) {
    if paused {
        virtual_time.pause();
    } else {
        virtual_time.unpause();
    }
    if let Ok(mut visibility) = root.single_mut() {
        *visibility = if paused {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn pause_button_visuals(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            Changed<Interaction>,
            Or<(With<PauseButton>, With<OpenSkinsButton>)>,
        ),
    >,
) {
    for (interaction, mut bg) in &mut buttons {
        *bg = match interaction {
            Interaction::Pressed => BackgroundColor(Color::srgb(0.30, 0.30, 0.36)),
            Interaction::Hovered => BackgroundColor(Color::srgb(0.26, 0.40, 0.52)),
            Interaction::None => BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
        };
    }
}
