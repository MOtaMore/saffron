//! Remappable keyboard controls. Gameplay systems ask `Keybinds` for an
//! [`Action`] instead of reading a hard-coded [`KeyCode`], so the "Controles"
//! screen (reachable from Configuración) can rebind them. Persisted to
//! `keybinds.json` next to the executable.
//!
//! Structural keys stay fixed: `Esc` (menús), `1`‥`0` / numpad (hotbar),
//! `Ctrl` (zoom / bajar al volar) and `Shift` as paso grande en la rebanada.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::pause::GameFlow;

pub struct KeybindsPlugin;

impl Plugin for KeybindsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Keybinds::load())
            .init_resource::<RebindScreen>()
            .add_systems(Startup, spawn_controls_screen)
            .add_systems(
                Update,
                (
                    open_controls_screen,
                    controls_screen_buttons,
                    capture_rebind,
                    controls_screen_sync,
                    controls_button_visuals,
                    close_on_play,
                ),
            );
    }
}

// === Actions ==========================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Action {
    Run,
    Jump,
    Inventory,
    Interact,
    Consume,
    CameraLeft,
    CameraRight,
    RoomMode,
    ViewToggle,
    ViewFull,
    ViewLower,
    ViewRaise,
    VisionCutout,
    QuickSave,
    FirstPerson,
}

impl Action {
    pub const ALL: [Action; 15] = [
        Action::Run,
        Action::Jump,
        Action::Inventory,
        Action::Interact,
        Action::Consume,
        Action::CameraLeft,
        Action::CameraRight,
        Action::RoomMode,
        Action::ViewToggle,
        Action::ViewFull,
        Action::ViewLower,
        Action::ViewRaise,
        Action::VisionCutout,
        Action::QuickSave,
        Action::FirstPerson,
    ];

    /// Label shown on the Controls screen.
    pub fn label(self) -> &'static str {
        match self {
            Action::Run => "Run (hold)",
            Action::Jump => "Jump",
            Action::Inventory => "Inventory",
            Action::Interact => "Stations / interact",
            Action::Consume => "Eat / drink",
            Action::CameraLeft => "Rotate camera <-",
            Action::CameraRight => "Rotate camera ->",
            Action::RoomMode => "Room build mode",
            Action::ViewToggle => "View slice (auto)",
            Action::ViewFull => "Full view",
            Action::ViewLower => "Lower view ceiling",
            Action::ViewRaise => "Raise view ceiling",
            Action::VisionCutout => "Vision cutout",
            Action::QuickSave => "Quick save",
            Action::FirstPerson => "First-person view",
        }
    }

    fn default_key(self) -> KeyCode {
        match self {
            Action::Run => KeyCode::ShiftLeft,
            Action::Jump => KeyCode::Space,
            Action::Inventory => KeyCode::KeyI,
            Action::Interact => KeyCode::KeyW,
            Action::Consume => KeyCode::KeyG,
            Action::CameraLeft => KeyCode::KeyQ,
            Action::CameraRight => KeyCode::KeyE,
            Action::RoomMode => KeyCode::KeyB,
            Action::ViewToggle => KeyCode::KeyL,
            Action::ViewFull => KeyCode::Backslash,
            Action::ViewLower => KeyCode::BracketLeft,
            Action::ViewRaise => KeyCode::BracketRight,
            Action::VisionCutout => KeyCode::KeyK,
            Action::QuickSave => KeyCode::F5,
            Action::FirstPerson => KeyCode::KeyV,
        }
    }
}

// === Keybinds resource ===============================================

#[derive(Resource)]
pub struct Keybinds(HashMap<Action, KeyCode>);

impl Keybinds {
    pub fn key(&self, a: Action) -> KeyCode {
        self.0.get(&a).copied().unwrap_or_else(|| a.default_key())
    }

    #[inline]
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, a: Action) -> bool {
        keys.pressed(self.key(a))
    }

    #[inline]
    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, a: Action) -> bool {
        keys.just_pressed(self.key(a))
    }

    fn defaults() -> Self {
        Keybinds(Action::ALL.iter().map(|&a| (a, a.default_key())).collect())
    }

    fn path() -> PathBuf {
        PathBuf::from("keybinds.json")
    }

    fn load() -> Self {
        let mut binds = Self::defaults();
        if let Some(saved) = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str::<HashMap<Action, String>>(&t).ok())
        {
            for (action, name) in saved {
                if let Some(key) = key_from_name(&name) {
                    binds.0.insert(action, key);
                }
            }
        }
        binds
    }

    fn save(&self) {
        let map: HashMap<Action, String> = self
            .0
            .iter()
            .map(|(&a, &k)| (a, key_name(k).to_string()))
            .collect();
        if let Ok(text) = serde_json::to_string_pretty(&map) {
            if let Err(e) = std::fs::write(Self::path(), text) {
                warn!("could not save keybinds.json: {e}");
            }
        }
    }
}

// === Key <-> name table =============================================

/// The keys the rebind screen will accept. Names match the `KeyCode` variants.
const NAMED_KEYS: &[(&str, KeyCode)] = &[
    ("KeyA", KeyCode::KeyA), ("KeyB", KeyCode::KeyB), ("KeyC", KeyCode::KeyC),
    ("KeyD", KeyCode::KeyD), ("KeyE", KeyCode::KeyE), ("KeyF", KeyCode::KeyF),
    ("KeyG", KeyCode::KeyG), ("KeyH", KeyCode::KeyH), ("KeyI", KeyCode::KeyI),
    ("KeyJ", KeyCode::KeyJ), ("KeyK", KeyCode::KeyK), ("KeyL", KeyCode::KeyL),
    ("KeyM", KeyCode::KeyM), ("KeyN", KeyCode::KeyN), ("KeyO", KeyCode::KeyO),
    ("KeyP", KeyCode::KeyP), ("KeyQ", KeyCode::KeyQ), ("KeyR", KeyCode::KeyR),
    ("KeyS", KeyCode::KeyS), ("KeyT", KeyCode::KeyT), ("KeyU", KeyCode::KeyU),
    ("KeyV", KeyCode::KeyV), ("KeyW", KeyCode::KeyW), ("KeyX", KeyCode::KeyX),
    ("KeyY", KeyCode::KeyY), ("KeyZ", KeyCode::KeyZ),
    ("Digit0", KeyCode::Digit0), ("Digit1", KeyCode::Digit1),
    ("Digit2", KeyCode::Digit2), ("Digit3", KeyCode::Digit3),
    ("Digit4", KeyCode::Digit4), ("Digit5", KeyCode::Digit5),
    ("Digit6", KeyCode::Digit6), ("Digit7", KeyCode::Digit7),
    ("Digit8", KeyCode::Digit8), ("Digit9", KeyCode::Digit9),
    ("F1", KeyCode::F1), ("F2", KeyCode::F2), ("F3", KeyCode::F3),
    ("F4", KeyCode::F4), ("F5", KeyCode::F5), ("F6", KeyCode::F6),
    ("F7", KeyCode::F7), ("F8", KeyCode::F8), ("F9", KeyCode::F9),
    ("F10", KeyCode::F10), ("F11", KeyCode::F11), ("F12", KeyCode::F12),
    ("Space", KeyCode::Space),
    ("ShiftLeft", KeyCode::ShiftLeft), ("ShiftRight", KeyCode::ShiftRight),
    ("ControlLeft", KeyCode::ControlLeft), ("ControlRight", KeyCode::ControlRight),
    ("AltLeft", KeyCode::AltLeft), ("AltRight", KeyCode::AltRight),
    ("Tab", KeyCode::Tab), ("Enter", KeyCode::Enter),
    ("Backspace", KeyCode::Backspace),
    ("ArrowUp", KeyCode::ArrowUp), ("ArrowDown", KeyCode::ArrowDown),
    ("ArrowLeft", KeyCode::ArrowLeft), ("ArrowRight", KeyCode::ArrowRight),
    ("BracketLeft", KeyCode::BracketLeft), ("BracketRight", KeyCode::BracketRight),
    ("Backslash", KeyCode::Backslash), ("Semicolon", KeyCode::Semicolon),
    ("Quote", KeyCode::Quote), ("Comma", KeyCode::Comma),
    ("Period", KeyCode::Period), ("Slash", KeyCode::Slash),
    ("Minus", KeyCode::Minus), ("Equal", KeyCode::Equal),
    ("Backquote", KeyCode::Backquote),
];

fn key_name(key: KeyCode) -> &'static str {
    NAMED_KEYS
        .iter()
        .find(|(_, k)| *k == key)
        .map(|(n, _)| *n)
        .unwrap_or("?")
}

fn key_from_name(name: &str) -> Option<KeyCode> {
    NAMED_KEYS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, k)| *k)
}

/// Friendly one-glyph-ish label for the UI ("W", "Shift ←", "\", "F5").
fn key_display(key: KeyCode) -> String {
    let raw = key_name(key);
    if let Some(letter) = raw.strip_prefix("Key") {
        return letter.to_string();
    }
    if let Some(digit) = raw.strip_prefix("Digit") {
        return digit.to_string();
    }
    match raw {
        "ShiftLeft" => "Shift ←".into(),
        "ShiftRight" => "Shift →".into(),
        "ControlLeft" => "Ctrl ←".into(),
        "ControlRight" => "Ctrl →".into(),
        "AltLeft" => "Alt ←".into(),
        "AltRight" => "Alt →".into(),
        "BracketLeft" => "[".into(),
        "BracketRight" => "]".into(),
        "Backslash" => "\\".into(),
        "Semicolon" => ";".into(),
        "Quote" => "'".into(),
        "Comma" => ",".into(),
        "Period" => ".".into(),
        "Slash" => "/".into(),
        "Minus" => "-".into(),
        "Equal" => "=".into(),
        "Backquote" => "`".into(),
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        other => other.to_string(),
    }
}

// === Controls screen =================================================

#[derive(Resource, Default)]
pub struct RebindScreen {
    pub open: bool,
    /// The action currently waiting for a key press, if any.
    capturing: Option<Action>,
}

impl RebindScreen {
    pub fn is_capturing(&self) -> bool {
        self.capturing.is_some()
    }
}

/// Drop this on a button (in the Configuración screen) to open Controles.
#[derive(Component, Clone, Copy)]
pub struct OpenControlsButton;

#[derive(Component)]
struct ControlsRoot;

#[derive(Component)]
struct KeyCell(Action);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum ControlsBtn {
    Reset,
    Close,
}

fn spawn_controls_screen(mut commands: Commands) {
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
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.07, 0.10)),
            GlobalZIndex(220),
            Visibility::Hidden,
            ControlsRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("CONTROLS"),
                TextFont::from_font_size(30.0),
                TextColor(Color::srgb(0.9, 0.95, 1.0)),
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                },
            ));

            for action in Action::ALL {
                root.spawn(Node {
                    width: Val::Px(420.0),
                    height: Val::Px(34.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new(action.label()),
                        TextFont::from_font_size(15.0),
                        TextColor(Color::WHITE),
                    ));
                    row.spawn((
                        Button,
                        Node {
                            width: Val::Px(120.0),
                            height: Val::Px(28.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
                        KeyCell(action),
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new(""),
                            TextFont::from_font_size(14.0),
                            TextColor(Color::WHITE),
                        ));
                    });
                });
            }

            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(10.0),
                margin: UiRect::top(Val::Px(14.0)),
                ..default()
            })
            .with_children(|row| {
                controls_button(row, "Reset", ControlsBtn::Reset);
                controls_button(row, "Close", ControlsBtn::Close);
            });
        });
}

fn controls_button(row: &mut ChildSpawnerCommands, label: &str, kind: ControlsBtn) {
    row.spawn((
        Button,
        Node {
            width: Val::Px(150.0),
            height: Val::Px(40.0),
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
            TextFont::from_font_size(15.0),
            TextColor(Color::WHITE),
        ));
    });
}

fn open_controls_screen(
    mut screen: ResMut<RebindScreen>,
    openers: Query<&Interaction, (Changed<Interaction>, With<OpenControlsButton>)>,
) {
    for interaction in &openers {
        if *interaction == Interaction::Pressed {
            screen.open = true;
            screen.capturing = None;
        }
    }
}

fn controls_screen_buttons(
    mut screen: ResMut<RebindScreen>,
    mut binds: ResMut<Keybinds>,
    cells: Query<(&Interaction, &KeyCell), Changed<Interaction>>,
    actions: Query<(&Interaction, &ControlsBtn), Changed<Interaction>>,
) {
    for (interaction, cell) in &cells {
        if *interaction == Interaction::Pressed {
            screen.capturing = Some(cell.0);
        }
    }
    for (interaction, btn) in &actions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match btn {
            ControlsBtn::Reset => {
                *binds = Keybinds::defaults();
                binds.save();
                screen.capturing = None;
            }
            ControlsBtn::Close => {
                screen.open = false;
                screen.capturing = None;
            }
        }
    }
}

fn capture_rebind(
    mut screen: ResMut<RebindScreen>,
    mut binds: ResMut<Keybinds>,
    mut events: MessageReader<KeyboardInput>,
) {
    let Some(action) = screen.capturing else {
        events.clear();
        return;
    };
    for ev in events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if ev.key_code == KeyCode::Escape {
            screen.capturing = None;
            break;
        }
        if key_name(ev.key_code) == "?" {
            continue; // not a bindable key
        }
        binds.0.insert(action, ev.key_code);
        binds.save();
        screen.capturing = None;
        break;
    }
}

fn controls_screen_sync(
    screen: Res<RebindScreen>,
    binds: Res<Keybinds>,
    mut root: Query<&mut Visibility, With<ControlsRoot>>,
    cells: Query<(&KeyCell, &Children)>,
    mut texts: Query<&mut Text>,
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
    for (cell, children) in &cells {
        let Some(&label) = children.first() else {
            continue;
        };
        let Ok(mut text) = texts.get_mut(label) else {
            continue;
        };
        text.0 = if screen.capturing == Some(cell.0) {
            "…".into()
        } else {
            key_display(binds.key(cell.0))
        };
    }
}

fn controls_button_visuals(
    screen: Res<RebindScreen>,
    mut cells: Query<
        (&Interaction, &KeyCell, &mut BackgroundColor),
        (Changed<Interaction>, Without<ControlsBtn>),
    >,
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<ControlsBtn>),
    >,
) {
    for (interaction, cell, mut bg) in &mut cells {
        *bg = BackgroundColor(if screen.capturing == Some(cell.0) {
            Color::srgb(0.42, 0.34, 0.16)
        } else {
            match interaction {
                Interaction::Pressed => Color::srgb(0.30, 0.30, 0.36),
                Interaction::Hovered => Color::srgb(0.26, 0.40, 0.52),
                Interaction::None => Color::srgb(0.20, 0.22, 0.28),
            }
        });
    }
    for (interaction, mut bg) in &mut buttons {
        *bg = BackgroundColor(match interaction {
            Interaction::Pressed => Color::srgb(0.30, 0.30, 0.36),
            Interaction::Hovered => Color::srgb(0.26, 0.40, 0.52),
            Interaction::None => Color::srgb(0.20, 0.22, 0.28),
        });
    }
}

/// Belt-and-braces: a `GameFlow` transition can't strand the screen open.
fn close_on_play(mut screen: ResMut<RebindScreen>, flow: Res<State<GameFlow>>) {
    if screen.open && matches!(flow.get(), GameFlow::Playing) {
        screen.open = false;
    }
}
