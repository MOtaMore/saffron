//! `W` = "use a station". Scans nearby workbenches / chests / furnaces; opens
//! the one found, or shows a picker when there are several. Replaces the old
//! proximity-only workbench.

use bevy::prelude::*;

use crate::block::Block;
use crate::container::{OpenContainer, open_at};
use crate::item::{AtWorkbench, Inventory, InventoryOpen};
use crate::pause::not_paused;
use crate::player::Player;
use crate::streaming::ChunkWorld;

const SCAN_RADIUS: i32 = 4;
const MENU_SLOTS: usize = 6;

pub struct StationPlugin;

impl Plugin for StationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StationChoices>()
            .add_systems(Startup, spawn_station_menu)
            .add_systems(
                Update,
                (station_key, station_menu_click).run_if(not_paused),
            )
            .add_systems(Update, (sync_station_menu, station_button_visuals));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StationKind {
    Workbench,
    Chest,
    Furnace,
}

impl StationKind {
    fn label(self) -> &'static str {
        match self {
            StationKind::Workbench => "Banco de trabajo",
            StationKind::Chest => "Cofre",
            StationKind::Furnace => "Horno",
        }
    }

    fn of(block: Block) -> Option<StationKind> {
        match block {
            Block::Workbench => Some(StationKind::Workbench),
            Block::Chest => Some(StationKind::Chest),
            Block::Furnace => Some(StationKind::Furnace),
            _ => None,
        }
    }
}

/// Non-empty while the "which station?" picker is on screen.
#[derive(Resource, Default)]
pub struct StationChoices(pub Vec<(StationKind, IVec3)>);

fn open_station(
    kind: StationKind,
    pos: IVec3,
    world: &ChunkWorld,
    inv_open: &mut InventoryOpen,
    at_workbench: &mut AtWorkbench,
    container: &mut OpenContainer,
    inventory: &mut Inventory,
) {
    inventory.return_carried();
    match kind {
        StationKind::Workbench => {
            inv_open.0 = true;
            at_workbench.0 = true;
        }
        StationKind::Chest | StationKind::Furnace => {
            if let Some(descriptor) = open_at(world, pos) {
                container.0 = Some(descriptor);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn station_key(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::keybinds::Keybinds>,
    world: Res<ChunkWorld>,
    player_q: Query<&Transform, With<Player>>,
    mut choices: ResMut<StationChoices>,
    mut inv_open: ResMut<InventoryOpen>,
    mut at_workbench: ResMut<AtWorkbench>,
    mut container: ResMut<OpenContainer>,
    mut inventory: ResMut<Inventory>,
) {
    if !binds.just_pressed(&keys, crate::keybinds::Action::Interact) {
        return;
    }

    // `W` toggles: if anything is already open, close it.
    if !choices.0.is_empty() {
        choices.0.clear();
        return;
    }
    if inv_open.0 || container.0.is_some() {
        inv_open.0 = false;
        container.0 = None;
        at_workbench.0 = false;
        inventory.stow_all();
        inventory.return_carried();
        return;
    }

    let Some(pt) = player_q.iter().next() else {
        return;
    };
    let base = pt.translation.floor().as_ivec3();

    let mut found: Vec<(StationKind, IVec3)> = Vec::new();
    for dx in -SCAN_RADIUS..=SCAN_RADIUS {
        for dy in -SCAN_RADIUS..=SCAN_RADIUS {
            for dz in -SCAN_RADIUS..=SCAN_RADIUS {
                let p = base + IVec3::new(dx, dy, dz);
                let Some(kind) = world.get_loaded(p.x, p.y, p.z).and_then(StationKind::of) else {
                    continue;
                };
                // Skip the far half of a double chest (axis-adjacent to a listed one).
                if kind == StationKind::Chest
                    && found.iter().any(|&(k, q)| {
                        let d = (q - p).abs();
                        k == StationKind::Chest && d.x + d.y + d.z == 1
                    })
                {
                    continue;
                }
                found.push((kind, p));
            }
        }
    }
    found.sort_by_key(|&(_, p)| (p - base).length_squared());
    found.truncate(MENU_SLOTS);

    match found.len() {
        0 => {}
        1 => open_station(
            found[0].0,
            found[0].1,
            &world,
            inv_open.as_mut(),
            at_workbench.as_mut(),
            container.as_mut(),
            inventory.as_mut(),
        ),
        _ => choices.0 = found,
    }
}

fn station_menu_click(
    world: Res<ChunkWorld>,
    mut choices: ResMut<StationChoices>,
    mut inv_open: ResMut<InventoryOpen>,
    mut at_workbench: ResMut<AtWorkbench>,
    mut container: ResMut<OpenContainer>,
    mut inventory: ResMut<Inventory>,
    buttons: Query<(&Interaction, &StationButton), Changed<Interaction>>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(&(kind, pos)) = choices.0.get(button.0) {
            open_station(
                kind,
                pos,
                &world,
                inv_open.as_mut(),
                at_workbench.as_mut(),
                container.as_mut(),
                inventory.as_mut(),
            );
            choices.0.clear();
        }
    }
}

// --- UI --------------------------------------------------------------

#[derive(Component)]
struct StationMenuRoot;
#[derive(Component)]
struct StationButton(usize);
#[derive(Component)]
struct StationButtonText(usize);

fn spawn_station_menu(mut commands: Commands) {
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
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            GlobalZIndex(60),
            Visibility::Hidden,
            StationMenuRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.10, 0.13, 0.98)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Que quieres usar?"),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::WHITE),
                ));
                for i in 0..MENU_SLOTS {
                    panel
                        .spawn((
                            Button,
                            Node {
                                width: Val::Px(280.0),
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.20, 0.22, 0.28)),
                            StationButton(i),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new(""),
                                TextFont::from_font_size(13.0),
                                TextColor(Color::WHITE),
                                StationButtonText(i),
                            ));
                        });
                }
            });
        });
}

fn sync_station_menu(
    choices: Res<StationChoices>,
    mut root: Query<&mut Visibility, With<StationMenuRoot>>,
    mut buttons: Query<(&StationButton, &mut Node)>,
    mut texts: Query<(&StationButtonText, &mut Text)>,
) {
    if let Ok(mut vis) = root.single_mut() {
        *vis = if choices.0.is_empty() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
    for (button, mut node) in &mut buttons {
        node.display = if button.0 < choices.0.len() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (label, mut text) in &mut texts {
        text.0 = match choices.0.get(label.0) {
            Some((kind, pos)) => format!("{}   ({}, {}, {})", kind.label(), pos.x, pos.y, pos.z),
            None => String::new(),
        };
    }
}

fn station_button_visuals(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<StationButton>),
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
