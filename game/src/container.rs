//! Chests (single, or a double when two touch), furnaces and the hand mill.
//! All are crafted only at a workbench. Left-click an installed one to open it;
//! Shift + hold left-click breaks it and returns its contents. The hand mill
//! has an input slot, a "hold to grind" button and an output slot.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::block::Block;
use crate::camera::MainCamera;
use crate::interact::raycast_cell;
use crate::item::{
    CELL_STRIDE, EMPTY_SLOT_COLOR, Inventory, InventoryOpen, Item, SLOTS, SlotCount, SlotKind, Stack,
    cell, paint_icon, stack_click, stack_count_text,
};
use crate::pause::not_paused;
use crate::player::Player;
use crate::station::StationChoices;
use crate::streaming::ChunkWorld;
use crate::survival::Stats;

pub const CHEST_SLOTS: usize = 27;
const MAX_CONTAINER_SLOTS: usize = 54;
const REACH: f32 = 6.0;
const RAY_MAX: f32 = 4000.0;
const HOTBAR_GUARD_PX: f32 = 76.0;
const SMELT_TIME: f32 = 5.0;
/// Seconds of held grinding per unit of flour, and the hunger it costs.
const GRIND_TIME: f32 = 2.5;
const MILL_HUNGER_RATE: f32 = 1.0;

pub struct ContainerPlugin;

impl Plugin for ContainerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChestStores>()
            .init_resource::<FurnaceStores>()
            .init_resource::<MillStores>()
            .init_resource::<OpenContainer>()
            .add_systems(Startup, spawn_container_panel)
            .add_systems(
                Update,
                (
                    try_open,
                    close_when_far,
                    container_click,
                    furnace_tick,
                    mill_grind,
                    mill_cleanup,
                )
                    .chain()
                    .run_if(not_paused),
            )
            .add_systems(Update, (paint_container_slots, update_container_ui));
    }
}

// --- Stores -------------------------------------------------------------

#[derive(Resource, Default)]
pub struct ChestStores(pub HashMap<IVec3, Vec<Option<Stack>>>);

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Furnace {
    pub input: Option<Stack>,
    pub fuel: Option<Stack>,
    pub output: Option<Stack>,
    pub progress: f32,
    pub burn: f32,
}

#[derive(Resource, Default)]
pub struct FurnaceStores(pub HashMap<IVec3, Furnace>);

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Mill {
    pub input: Option<Stack>,
    pub output: Option<Stack>,
    pub progress: f32,
}

#[derive(Resource, Default)]
pub struct MillStores(pub HashMap<IVec3, Mill>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Chest,
    Furnace,
    Mill,
}

pub struct Open {
    pub kind: ContainerKind,
    pub a: IVec3,
    /// Second half of a double chest.
    pub b: Option<IVec3>,
}

#[derive(Resource, Default)]
pub struct OpenContainer(pub Option<Open>);

impl OpenContainer {
    fn slot_count(&self) -> usize {
        match &self.0 {
            None => 0,
            Some(o) => match o.kind {
                ContainerKind::Furnace => 3,
                ContainerKind::Mill => 2,
                ContainerKind::Chest => {
                    if o.b.is_some() {
                        CHEST_SLOTS * 2
                    } else {
                        CHEST_SLOTS
                    }
                }
            },
        }
    }
}

fn smelt_output(item: Item) -> Option<Item> {
    match item {
        Item::Fish => Some(Item::CookedFish),
        Item::Meat => Some(Item::CookedMeat),
        Item::RedMeat => Some(Item::CookedRedMeat),
        Item::Mutton => Some(Item::CookedMutton),
        Item::WhiteMeat => Some(Item::CookedWhiteMeat),
        Item::Dough => Some(Item::Bread),
        Item::Block(Block::Sand) => Some(Item::Block(Block::Glass)),
        Item::Block(Block::Gravel) => Some(Item::Block(Block::Stone)),
        // Fire cobblestone back into smooth rock; bake clay balls into bricks.
        Item::Block(Block::Cobblestone) => Some(Item::Block(Block::Stone)),
        Item::ClayBall => Some(Item::Brick),
        Item::Block(Block::Wood) => Some(Item::Charcoal),
        _ => None,
    }
}

fn fuel_time(item: Item) -> Option<f32> {
    match item {
        Item::Charcoal => Some(14.0),
        Item::Block(Block::Wood) => Some(16.0),
        Item::Fat => Some(10.0),
        Item::Stick => Some(4.0),
        Item::Block(Block::Leaves) => Some(2.0),
        _ => None,
    }
}

/// Called by `interact::mining` when a chest/furnace is broken.
pub fn on_broken(
    pos: IVec3,
    chests: &mut ChestStores,
    furnaces: &mut FurnaceStores,
    inventory: &mut Inventory,
) {
    if let Some(store) = chests.0.remove(&pos) {
        for stack in store.into_iter().flatten() {
            inventory.add(stack.item, stack.count);
        }
    }
    if let Some(f) = furnaces.0.remove(&pos) {
        for stack in [f.input, f.fuel, f.output].into_iter().flatten() {
            inventory.add(stack.item, stack.count);
        }
    }
}

fn read_slot(
    open: &OpenContainer,
    chests: &ChestStores,
    furnaces: &FurnaceStores,
    mills: &MillStores,
    i: usize,
) -> Option<Stack> {
    let o = open.0.as_ref()?;
    match o.kind {
        ContainerKind::Mill => {
            let m = mills.0.get(&o.a)?;
            match i {
                0 => m.input,
                1 => m.output,
                _ => None,
            }
        }
        ContainerKind::Chest => {
            let (pos, local) = if i < CHEST_SLOTS {
                (o.a, i)
            } else {
                (o.b?, i - CHEST_SLOTS)
            };
            chests
                .0
                .get(&pos)
                .and_then(|v| v.get(local).copied().flatten())
        }
        ContainerKind::Furnace => {
            let f = furnaces.0.get(&o.a)?;
            match i {
                0 => f.input,
                1 => f.fuel,
                2 => f.output,
                _ => None,
            }
        }
    }
}

// --- Systems ----------------------------------------------------------

/// Builds the `Open` descriptor for a chest/furnace block (chests pair with a
/// touching neighbour for a double). `None` if `pos` is not a container.
pub fn open_at(world: &ChunkWorld, pos: IVec3) -> Option<Open> {
    match world.get_loaded(pos.x, pos.y, pos.z) {
        Some(Block::Chest) => {
            let mut b = None;
            for d in [
                IVec3::new(1, 0, 0),
                IVec3::new(-1, 0, 0),
                IVec3::new(0, 0, 1),
                IVec3::new(0, 0, -1),
            ] {
                let q = pos + d;
                if world.get_loaded(q.x, q.y, q.z) == Some(Block::Chest) {
                    b = Some(q);
                    break;
                }
            }
            Some(Open {
                kind: ContainerKind::Chest,
                a: pos,
                b,
            })
        }
        Some(Block::Furnace) => Some(Open {
            kind: ContainerKind::Furnace,
            a: pos,
            b: None,
        }),
        Some(Block::HandMill) => Some(Open {
            kind: ContainerKind::Mill,
            a: pos,
            b: None,
        }),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn try_open(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    inv_open: Res<InventoryOpen>,
    station_menu: Res<StationChoices>,
    mut open: ResMut<OpenContainer>,
    mut inventory: ResMut<Inventory>,
    cam_mode: Res<crate::firstperson::CameraMode>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    player_q: Query<&Transform, With<Player>>,
    world: Res<ChunkWorld>,
) {
    if inv_open.0
        || open.0.is_some()
        || !station_menu.0.is_empty()
        || !mouse.just_pressed(MouseButton::Left)
        // Shift + click is "break the block", handled by `interact::mining`.
        || keys.pressed(KeyCode::ShiftLeft)
        || keys.pressed(KeyCode::ShiftRight)
    {
        return;
    }
    if matches!(inventory.selected_item(), Some(Item::Block(_))) {
        return; // a block in hand means "place", not "open"
    }
    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), camera_q.single()) else {
        return;
    };
    let Some(cursor) = crate::firstperson::aim_point(window, &cam_mode) else {
        return;
    };
    if cursor.y > window.height() - HOTBAR_GUARD_PX {
        return;
    }
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else {
        return;
    };
    let Some(hit) = raycast_cell(&world, ray.origin, *ray.direction, RAY_MAX, None) else {
        return;
    };
    let pos = hit.cell;
    if let Some(pt) = player_q.iter().next() {
        if (pos.as_vec3() + Vec3::splat(0.5)).distance(pt.translation) > REACH {
            return;
        }
    }

    let Some(descriptor) = open_at(&world, pos) else {
        return;
    };
    open.0 = Some(descriptor);
    inventory.return_carried();
}

fn close_when_far(
    mut open: ResMut<OpenContainer>,
    mut inventory: ResMut<Inventory>,
    player_q: Query<&Transform, With<Player>>,
) {
    let Some(o) = &open.0 else {
        return;
    };
    let Some(pt) = player_q.iter().next() else {
        return;
    };
    if (o.a.as_vec3() + Vec3::splat(0.5)).distance(pt.translation) > REACH + 2.5 {
        open.0 = None;
        inventory.return_carried();
    }
}

#[allow(clippy::too_many_arguments)]
fn container_click(
    mouse: Res<ButtonInput<MouseButton>>,
    open: Res<OpenContainer>,
    mut inventory: ResMut<Inventory>,
    mut chests: ResMut<ChestStores>,
    mut furnaces: ResMut<FurnaceStores>,
    mut mills: ResMut<MillStores>,
    slots: Query<(&SlotKind, &Interaction)>,
) {
    let Some(o) = &open.0 else {
        return;
    };
    let left = mouse.just_pressed(MouseButton::Left);
    if !left && !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let Some(idx) = slots.iter().find_map(|(k, i)| {
        match (*k, *i) {
            (SlotKind::Container(idx), Interaction::Hovered | Interaction::Pressed) => Some(idx),
            _ => None,
        }
    }) else {
        return;
    };

    match o.kind {
        ContainerKind::Chest => {
            let (pos, local) = if idx < CHEST_SLOTS {
                (o.a, idx)
            } else {
                (o.b.unwrap_or(o.a), idx - CHEST_SLOTS)
            };
            let store = chests.0.entry(pos).or_insert_with(|| vec![None; CHEST_SLOTS]);
            if let Some(slot) = store.get_mut(local) {
                stack_click(slot, &mut inventory.carried, left);
            }
        }
        ContainerKind::Furnace => {
            let f = furnaces.0.entry(o.a).or_default();
            match idx {
                0 => stack_click(&mut f.input, &mut inventory.carried, left),
                1 => stack_click(&mut f.fuel, &mut inventory.carried, left),
                2 => {
                    if left && inventory.carried.is_none() {
                        inventory.carried = f.output.take();
                    }
                }
                _ => {}
            }
        }
        ContainerKind::Mill => {
            let m = mills.0.entry(o.a).or_default();
            match idx {
                0 => stack_click(&mut m.input, &mut inventory.carried, left),
                1 => {
                    if left && inventory.carried.is_none() {
                        inventory.carried = m.output.take();
                    }
                }
                _ => {}
            }
        }
    }
}

fn furnace_tick(time: Res<Time>, mut furnaces: ResMut<FurnaceStores>) {
    let dt = time.delta_secs();
    for f in furnaces.0.values_mut() {
        let Some(input) = f.input else {
            f.progress = 0.0;
            continue;
        };
        let Some(result) = smelt_output(input.item) else {
            f.progress = 0.0;
            continue;
        };
        let output_ok = match f.output {
            None => true,
            Some(o) => o.item == result && o.count < o.item.max_stack(),
        };
        if !output_ok {
            continue;
        }

        if f.burn <= 0.0 {
            if let Some(fuel) = f.fuel.as_mut() {
                if let Some(bt) = fuel_time(fuel.item) {
                    fuel.count -= 1;
                    f.burn += bt;
                    if fuel.count == 0 {
                        f.fuel = None;
                    }
                }
            }
        }
        if f.burn <= 0.0 {
            f.progress = (f.progress - dt).max(0.0);
            continue;
        }

        f.burn -= dt;
        f.progress += dt;
        if f.progress >= SMELT_TIME {
            f.progress = 0.0;
            if let Some(i) = f.input.as_mut() {
                i.count -= 1;
                if i.count == 0 {
                    f.input = None;
                }
            }
            match f.output.as_mut() {
                Some(o) => o.count += 1,
                None => {
                    f.output = Some(Stack {
                        item: result,
                        count: 1,
                    })
                }
            }
        }
    }
}

fn grind_output(item: Item) -> Option<Item> {
    match item {
        Item::Wheat => Some(Item::Flour),
        _ => None,
    }
}

/// While the open mill's "grind" button is held: advance progress, drain the
/// player's hunger, and turn one input into one output per `GRIND_TIME`.
fn mill_grind(
    time: Res<Time>,
    open: Res<OpenContainer>,
    mut mills: ResMut<MillStores>,
    mut stats: ResMut<Stats>,
    button: Query<&Interaction, With<MillGrindButton>>,
) {
    let Some(o) = &open.0 else {
        return;
    };
    if o.kind != ContainerKind::Mill {
        return;
    }
    let held = button
        .single()
        .map(|i| *i == Interaction::Pressed)
        .unwrap_or(false);
    if !held {
        return;
    }

    let m = mills.0.entry(o.a).or_default();
    let Some(input) = m.input else {
        m.progress = 0.0;
        return;
    };
    let Some(result) = grind_output(input.item) else {
        m.progress = 0.0;
        return;
    };
    let output_ok = match m.output {
        None => true,
        Some(out) => out.item == result && out.count < out.item.max_stack(),
    };
    if !output_ok {
        return;
    }

    let dt = time.delta_secs();
    m.progress += dt / GRIND_TIME;
    stats.hunger = (stats.hunger - MILL_HUNGER_RATE * dt).max(0.0);

    if m.progress >= 1.0 {
        m.progress = 0.0;
        if let Some(i) = m.input.as_mut() {
            i.count -= 1;
            if i.count == 0 {
                m.input = None;
            }
        }
        match m.output.as_mut() {
            Some(out) => out.count += 1,
            None => {
                m.output = Some(Stack {
                    item: result,
                    count: 1,
                })
            }
        }
    }
}

/// A mill block that was broken (no longer in the world) dumps its contents.
fn mill_cleanup(
    world: Res<ChunkWorld>,
    mut mills: ResMut<MillStores>,
    mut inventory: ResMut<Inventory>,
) {
    // Only act on loaded chunks: `None` means "unloaded", not "broken".
    let gone: Vec<IVec3> = mills
        .0
        .keys()
        .copied()
        .filter(|p| {
            world
                .get_loaded(p.x, p.y, p.z)
                .is_some_and(|b| b != Block::HandMill)
        })
        .collect();
    for pos in gone {
        if let Some(m) = mills.0.remove(&pos) {
            for stack in [m.input, m.output].into_iter().flatten() {
                inventory.add(stack.item, stack.count);
            }
        }
    }
}

// --- UI --------------------------------------------------------------

#[derive(Component)]
struct ContainerRoot;
#[derive(Component)]
struct ContainerTitle;
#[derive(Component)]
struct FurnaceStatus;
/// Hold this button to run the mill (only shown while a mill is open).
#[derive(Component)]
struct MillGrindButton;

fn spawn_container_panel(mut commands: Commands) {
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
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            Visibility::Hidden,
            ContainerRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.10, 0.13, 0.97)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextFont::from_font_size(14.0),
                    TextColor(Color::WHITE),
                    ContainerTitle,
                ));
                panel.spawn((
                    Text::new(""),
                    TextFont::from_font_size(12.0),
                    TextColor(Color::srgb(0.85, 0.85, 0.85)),
                    FurnaceStatus,
                ));
                panel
                    .spawn((
                        Button,
                        Node {
                            width: Val::Px(200.0),
                            height: Val::Px(34.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            display: Display::None,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.24, 0.22, 0.18)),
                        MillGrindButton,
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new("GRIND  (hold)"),
                            TextFont::from_font_size(13.0),
                            TextColor(Color::WHITE),
                        ));
                    });
                panel
                    .spawn(Node {
                        width: Val::Px(9.0 * CELL_STRIDE),
                        flex_wrap: FlexWrap::Wrap,
                        flex_direction: FlexDirection::Row,
                        row_gap: Val::Px(4.0),
                        column_gap: Val::Px(4.0),
                        ..default()
                    })
                    .with_children(|grid| {
                        for i in 0..MAX_CONTAINER_SLOTS {
                            cell(grid, SlotKind::Container(i));
                        }
                    });
                panel
                    .spawn(Node {
                        width: Val::Px(10.0 * CELL_STRIDE),
                        flex_wrap: FlexWrap::Wrap,
                        flex_direction: FlexDirection::Row,
                        row_gap: Val::Px(4.0),
                        column_gap: Val::Px(4.0),
                        ..default()
                    })
                    .with_children(|grid| {
                        for i in 0..SLOTS {
                            cell(grid, SlotKind::Backpack(i));
                        }
                    });
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn paint_container_slots(
    open: Res<OpenContainer>,
    chests: Res<ChestStores>,
    furnaces: Res<FurnaceStores>,
    mills: Res<MillStores>,
    server: Res<AssetServer>,
    mut cells: Query<(&SlotKind, &mut BackgroundColor, &mut Node, &mut ImageNode)>,
    mut counts: Query<(&SlotCount, &mut Text)>,
) {
    let n = open.slot_count();
    for (kind, mut bg, mut node, mut image) in &mut cells {
        let SlotKind::Container(i) = *kind else {
            continue;
        };
        node.display = if i < n { Display::Flex } else { Display::None };
        paint_icon(
            read_slot(&open, &chests, &furnaces, &mills, i),
            &server,
            &mut image,
            &mut bg,
            EMPTY_SLOT_COLOR,
        );
    }
    for (count, mut text) in &mut counts {
        let SlotKind::Container(i) = count.0 else {
            continue;
        };
        text.0 = stack_count_text(read_slot(&open, &chests, &furnaces, &mills, i));
    }
}

#[allow(clippy::too_many_arguments)]
fn update_container_ui(
    open: Res<OpenContainer>,
    furnaces: Res<FurnaceStores>,
    mills: Res<MillStores>,
    mut root: Query<&mut Visibility, With<ContainerRoot>>,
    mut title: Query<&mut Text, (With<ContainerTitle>, Without<FurnaceStatus>)>,
    mut status: Query<&mut Text, (With<FurnaceStatus>, Without<ContainerTitle>)>,
    mut grind_btn: Query<&mut Node, With<MillGrindButton>>,
) {
    if let Ok(mut vis) = root.single_mut() {
        *vis = if open.0.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    let is_mill = matches!(open.0.as_ref().map(|o| o.kind), Some(ContainerKind::Mill));
    if let Ok(mut node) = grind_btn.single_mut() {
        node.display = if is_mill { Display::Flex } else { Display::None };
    }

    let Some(o) = &open.0 else {
        return;
    };
    if let Ok(mut text) = title.single_mut() {
        text.0 = match o.kind {
            ContainerKind::Chest if o.b.is_some() => "Double Chest".into(),
            ContainerKind::Chest => "Chest".into(),
            ContainerKind::Furnace => {
                "Furnace    [1] ore   [2] fuel   [3] result".into()
            }
            ContainerKind::Mill => "Hand Mill    [1] to grind   [2] result".into(),
        };
    }
    if let Ok(mut text) = status.single_mut() {
        text.0 = match o.kind {
            ContainerKind::Furnace => {
                let f = furnaces.0.get(&o.a).cloned().unwrap_or_default();
                format!(
                    "Fundido: {:.0}%    Combustible restante: {:.0}s",
                    (f.progress / SMELT_TIME * 100.0).clamp(0.0, 100.0),
                    f.burn.max(0.0)
                )
            }
            ContainerKind::Mill => {
                let p = mills.0.get(&o.a).map(|m| m.progress).unwrap_or(0.0);
                format!("Molienda: {:.0}%", (p * 100.0).clamp(0.0, 100.0))
            }
            ContainerKind::Chest => String::new(),
        };
    }
}
