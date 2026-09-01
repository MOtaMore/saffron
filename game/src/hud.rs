//! Minimal on-screen debug readout.

use bevy::prelude::*;

use crate::chunk::chunk_of_pos;
use crate::daynight::GameClock;
use crate::interact::{BuildState, MiningState, TargetInfo};
use crate::item::{Inventory, Item, tool_hint};
use crate::player::{Player, PlayerBody};
use crate::streaming::ChunkWorld;
use crate::survival::Stats;
use crate::view::{SliceMode, ViewSlice};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, update_hud);
    }
}

#[derive(Component)]
struct HudText;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Text::new("Cargando mundo..."),
        TextFont::from_font_size(15.0),
        TextColor(Color::srgb(0.95, 0.97, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(12.0),
            ..default()
        },
        HudText,
    ));
}

#[allow(clippy::too_many_arguments)]
fn update_hud(
    time: Res<Time>,
    world: Res<ChunkWorld>,
    slice: Res<ViewSlice>,
    inventory: Res<Inventory>,
    build: Res<BuildState>,
    mining: Res<MiningState>,
    target: Res<TargetInfo>,
    clock: Res<GameClock>,
    stats: Res<Stats>,
    player: Query<(&Transform, &PlayerBody), With<Player>>,
    mut text: Query<&mut Text, With<HudText>>,
    mut fps: Local<f32>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let dt = time.delta_secs().max(1e-5);
    *fps = *fps * 0.9 + (1.0 / dt) * 0.1;

    let loaded = world.chunks.len();
    let capa = match slice.mode {
        SliceMode::Off => "completa".to_string(),
        SliceMode::Auto => format!("auto Y{} (sigue al jugador)", slice.cutoff),
        SliceMode::Manual => format!("manual Y{}", slice.cutoff),
    };
    let hand = match inventory.selected_item() {
        Some(Item::Block(b)) => format!(
            "{} x{}  (clic: colocar)",
            b.display_name(),
            inventory.count(Item::Block(b))
        ),
        Some(it) if it.tool().is_some() => format!("{}  (herramienta)", it.name()),
        Some(it) => format!("{} x{}", it.name(), inventory.count(it)),
        None => "vacia  (manten clic: minar)".to_string(),
    };
    let accion = if build.room_mode {
        match build.room_corner {
            None => "ESTANCIA: clic esquina 1".to_string(),
            Some(_) => "ESTANCIA: clic esquina 2".to_string(),
        }
    } else if let Some(p) = mining.progress() {
        format!("minando... {:.0}%", p * 100.0)
    } else if let Some(b) = target.block {
        if target.harvestable {
            format!("apuntas a: {}", b.display_name())
        } else {
            format!("apuntas a: {} — {}", b.display_name(), tool_hint(b))
        }
    } else {
        "—".to_string()
    };

    if let Ok((transform, _body)) = player.single() {
        let p = transform.translation;
        let c = chunk_of_pos(p);
        let sky = if clock.is_night() { "noche" } else { "dia" };
        text.0 = format!(
            "FPS {:.0}   Hora {} ({sky})\nPos   {:.1}, {:.1}, {:.1}\nChunk  {}, {}\nChunks cargados: {loaded}\nCapa: {capa}\nVida {:.0}  Hambre {:.0}  Sed {:.0}\nMano: {hand}\nAccion: {accion}\n\nEsc menu · I inventario · clic der: mover · Espacio: saltar · rueda: slot · Ctrl+rueda: zoom · B estancia\nShift correr · G comer/beber · K cutout · L capa auto · [ ] capa · \\ completa · Q/E rotar",
            *fps, clock.clock_string(), p.x, p.y, p.z, c.x, c.y, stats.health, stats.hunger, stats.thirst,
        );
    } else {
        text.0 = format!("FPS {:.0}\nGenerando terreno...\nChunks cargados: {loaded}", *fps);
    }
}
