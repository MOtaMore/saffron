//! Comandos de chat (`/...`). Funcionan igual en un jugador y en multijugador:
//! `net::chat_capture` detecta las líneas que empiezan por `/` y las manda aquí
//! en vez de enviarlas por la red. Los comandos y sus respuestas van en inglés,
//! como el resto del texto in-game.
//!
//! Disponibles:
//! - `/help`             — lista los comandos
//! - `/pos`              — muestra tus coordenadas
//! - `/biome <name>`     — te lleva al bioma más cercano (plains, forest, desert, snow)
//! - `/structure`        — te lleva a la estructura de librería más cercana
//! - `/city`             — te lleva a la ciudad en ruinas más cercana
//! - `/spawn`            — te devuelve al punto de aparición del mundo
//!
//! Todo teletransporte aterriza **sobre la superficie** (nunca dentro de un
//! bloque): se escanea la columna real del destino, generándola si el chunk
//! todavía no está cargado.

use bevy::prelude::*;

use crate::block::Block;
use crate::chunk::{CHUNK_HEIGHT, CHUNK_SIZE};
use crate::net::ChatLog;
use crate::pause::GameFlow;
use crate::player::{Player, PlayerBody};
use crate::streaming::ChunkWorld;
use crate::worldgen::{Biome, WorldGen, WorldGenHandle};

/// Una línea de chat que empezaba por `/`.
#[derive(Message)]
pub struct ChatCommand(pub String);

pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ChatCommand>().add_systems(
            Update,
            run_commands.run_if(in_state(GameFlow::Playing)),
        );
    }
}

fn run_commands(
    mut evs: MessageReader<ChatCommand>,
    mut chat: ResMut<ChatLog>,
    world: Res<ChunkWorld>,
    world_gen: Option<Res<WorldGenHandle>>,
    mut player: Query<(&mut Transform, &mut PlayerBody), With<Player>>,
) {
    for ev in evs.read() {
        let line = ev.0.trim_start_matches('/').trim().to_string();
        let mut it = line.split_whitespace();
        let cmd = it.next().unwrap_or("").to_lowercase();
        let arg = it.next().unwrap_or("");

        let (Ok((mut tf, mut body)), Some(wg)) = (player.single_mut(), world_gen.as_deref()) else {
            chat.push_line("* Command unavailable right now.");
            continue;
        };
        let wg = &wg.0;
        let from = tf.translation;
        let here = IVec2::new(from.x.floor() as i32, from.z.floor() as i32);

        // Teletransporta a la columna `(x, z)`, aterrizando sobre la superficie.
        let mut go = |x: i32, z: i32| {
            let y = safe_landing(&world, wg, x, z);
            tf.translation = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
            body.velocity = Vec3::ZERO;
        };

        match cmd.as_str() {
            "" => {}
            "help" | "?" | "commands" => {
                chat.push_line("* /pos  ·  /biome <name>  ·  /structure  ·  /city  ·  /spawn");
                chat.push_line("* biomes: plains, forest, desert, snow");
            }
            "pos" | "coords" | "where" => {
                chat.push_line(format!(
                    "* You are at X {}  Y {}  Z {}",
                    from.x.floor() as i32,
                    from.y.floor() as i32,
                    from.z.floor() as i32
                ));
            }
            "biome" => {
                let Some(want) = Biome::parse(arg) else {
                    chat.push_line("* Usage: /biome <plains|forest|desert|snow>");
                    continue;
                };
                match wg.nearest_biome(here, want) {
                    Some(p) => {
                        go(p.x, p.z);
                        chat.push_line(format!(
                            "* Warped to the nearest {} ({}, {}).",
                            want.en_name(),
                            p.x,
                            p.z
                        ));
                    }
                    None => chat.push_line(format!("* No {} found nearby.", want.en_name())),
                }
            }
            "structure" | "struct" => match wg.nearest_structure(here) {
                Some(p) => {
                    go(p.x, p.z);
                    chat.push_line(format!("* Warped to the nearest structure ({}, {}).", p.x, p.z));
                }
                None => chat.push_line(
                    "* This world has no spawnable structures (add .json to assets/structures/).",
                ),
            },
            "city" | "ruins" => match wg.nearest_ruined_city(here) {
                Some(p) => {
                    go(p.x, p.z);
                    chat.push_line(format!(
                        "* Warped to the nearest ruined city ({}, {}).",
                        p.x, p.z
                    ));
                }
                None => chat.push_line("* No ruined city nearby."),
            },
            "spawn" | "home" => {
                let (lx, lz) = wg.find_land(0, 0);
                go(lx, lz);
                chat.push_line("* Back to the world spawn.");
            }
            other => {
                chat.push_line(format!("* Unknown command: /{other}   (try /help)"));
            }
        }
    }
}

/// La `y` (índice de bloque) del primer hueco de aire sobre el bloque sólido más
/// alto de la columna `(x, z)` — es decir, la superficie donde debe quedar el
/// jugador. Usa los chunks cargados; si el destino aún no está cargado, genera
/// su `ChunkData` para conocer la altura real (incluidos edificios / estructuras).
fn safe_landing(world: &ChunkWorld, wg: &WorldGen, x: i32, z: i32) -> i32 {
    let loaded = (1..CHUNK_HEIGHT).any(|y| world.get_loaded(x, y, z).is_some());
    if loaded {
        for y in (1..CHUNK_HEIGHT).rev() {
            if let Some(b) = world.get_loaded(x, y, z) {
                if b != Block::Air && b != Block::Water {
                    return y + 1;
                }
            }
        }
        return wg.surface_height(x, z) + 1;
    }

    let cd = wg.generate(IVec2::new(x.div_euclid(CHUNK_SIZE), z.div_euclid(CHUNK_SIZE)));
    let (lx, lz) = (x.rem_euclid(CHUNK_SIZE), z.rem_euclid(CHUNK_SIZE));
    for y in (1..CHUNK_HEIGHT).rev() {
        let b = cd.get(lx, y, lz);
        if b != Block::Air && b != Block::Water {
            return y + 1;
        }
    }
    wg.surface_height(x, z) + 1
}
