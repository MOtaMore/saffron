//! Discord Rich Presence. Runs the Discord IPC on its own thread; the Bevy side
//! only samples a small [`Snapshot`] of the player's state every few seconds and
//! sends it over a channel, so a missing/!running Discord never stalls the game.
//!
//! **Setup:** create an app at <https://discord.com/developers/applications>,
//! copy its *Application ID*, and provide it via the `AVES_DISCORD_APP_ID`
//! environment variable or a `game/discord.json` = `{ "app_id": "..." }`.
//! Upload art named `logo`, `day` and `night` under *Rich Presence → Art Assets*.
//! With no id the feature is simply disabled.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::time::Real;
use discord_rich_presence::activity::{Activity, Assets, Party, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use serde::Deserialize;

use crate::net::{CurrentServer, NetMode, RemotePlayer};
use crate::pause::GameFlow;
use crate::save::CurrentWorld;
use crate::survival::Stats;

/// Fallback App ID compiled in. Leave empty to rely on env / `discord.json`.
const DEFAULT_APP_ID: &str = "";

pub struct DiscordPlugin;

impl Plugin for DiscordPlugin {
    fn build(&self, app: &mut App) {
        let link = match load_app_id() {
            Some(id) => {
                let (tx, rx) = channel::<Snapshot>();
                let _ = thread::Builder::new()
                    .name("discord-rpc".into())
                    .spawn(move || worker(rx, id));
                DiscordLink(Some(tx))
            }
            None => {
                info!(
                    "Discord Rich Presence desactivado (define AVES_DISCORD_APP_ID \
                     o game/discord.json con {{\"app_id\": \"…\"}})"
                );
                DiscordLink(None)
            }
        };
        app.insert_resource(link)
            .init_resource::<SessionStart>()
            .add_systems(OnEnter(GameFlow::Playing), mark_session_start)
            .add_systems(Update, sample_presence);
    }
}

#[derive(Resource)]
struct DiscordLink(Option<Sender<Snapshot>>);

/// Unix seconds when the current world/session was entered (0 = in menu).
#[derive(Resource, Default)]
struct SessionStart(i64);

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mark_session_start(mut session: ResMut<SessionStart>) {
    session.0 = now_unix();
}

// === What we tell Discord ==========================================

#[derive(Clone, PartialEq)]
struct Snapshot {
    details: String,
    state: String,
    large_text: String,
    small_image: Option<&'static str>,
    small_text: String,
    party: Option<(i32, i32)>,
    since: i64,
}

impl Snapshot {
    fn menu() -> Self {
        Snapshot {
            details: "En el menú".into(),
            state: "Preparando la aventura".into(),
            large_text: "Aves — Supervivencia 2.5D".into(),
            small_image: None,
            small_text: String::new(),
            party: None,
            since: 0,
        }
    }
}

fn day_phase(t: f32) -> &'static str {
    match t.rem_euclid(1.0) {
        x if x < 0.23 => "Noche",
        x if x < 0.30 => "Amanecer",
        x if x < 0.70 => "Día",
        x if x < 0.77 => "Atardecer",
        _ => "Noche",
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_presence(
    time: Res<Time<Real>>,
    mut acc: Local<f32>,
    link: Res<DiscordLink>,
    flow: Res<State<GameFlow>>,
    mode: Res<NetMode>,
    server: Res<CurrentServer>,
    world: Res<CurrentWorld>,
    stats: Res<Stats>,
    clock: Res<crate::daynight::GameClock>,
    session: Res<SessionStart>,
    remotes: Query<(), With<RemotePlayer>>,
) {
    let Some(tx) = link.0.as_ref() else {
        return;
    };
    *acc += time.delta_secs();
    if *acc < 3.0 {
        return;
    }
    *acc = 0.0;

    let snap = if matches!(flow.get(), GameFlow::Menu) {
        Snapshot::menu()
    } else {
        let stat_line = if stats.health <= 0.5 {
            "☠ Derrotado".to_string()
        } else {
            format!(
                "❤ {}    🍖 {}    💧 {}",
                stats.health.round() as i32,
                stats.hunger.round() as i32,
                stats.thirst.round() as i32,
            )
        };
        let night = clock.is_night();
        let small_image = Some(if night { "night" } else { "day" });
        let small_text = format!("{} · {}", day_phase(clock.t), clock.clock_string());

        if mode.networked() {
            let n = remotes.iter().count() as i32 + 1;
            let addr = server
                .0
                .clone()
                .unwrap_or_else(|| "servidor".to_string());
            Snapshot {
                details: format!("En línea · {addr}"),
                state: format!("{n} jugando    ·    {stat_line}"),
                large_text: "Aves — Multijugador".into(),
                small_image,
                small_text,
                party: Some((n, n.max(2))),
                since: session.0,
            }
        } else {
            let name = world
                .0
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Mundo")
                .to_string();
            Snapshot {
                details: format!("Mundo: {name}"),
                state: stat_line,
                large_text: "Aves — Supervivencia 2.5D".into(),
                small_image,
                small_text,
                party: None,
                since: session.0,
            }
        }
    };

    let _ = tx.send(snap);
}

// === Config ========================================================

fn load_app_id() -> Option<String> {
    if let Ok(v) = std::env::var("AVES_DISCORD_APP_ID") {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    #[derive(Deserialize)]
    struct FileCfg {
        app_id: String,
    }
    if let Ok(text) = std::fs::read_to_string("discord.json") {
        if let Ok(cfg) = serde_json::from_str::<FileCfg>(&text) {
            let id = cfg.app_id.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    let compiled = DEFAULT_APP_ID.trim();
    (!compiled.is_empty()).then(|| compiled.to_string())
}

// === IPC worker thread =============================================

fn worker(rx: Receiver<Snapshot>, app_id: String) {
    let Ok(mut client) = DiscordIpcClient::new(&app_id) else {
        warn!("Discord RPC: app id inválido");
        return;
    };

    let mut connected = false;
    let mut retry_at = Instant::now();
    let mut current = Snapshot::menu();
    let mut pushed: Option<Snapshot> = None;
    let mut last_push = Instant::now() - Duration::from_secs(120);

    loop {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(s) => current = s,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(s) = rx.try_recv() {
            current = s;
        }

        if !connected {
            if Instant::now() < retry_at {
                continue;
            }
            match client.connect() {
                Ok(()) => connected = true,
                Err(_) => {
                    retry_at = Instant::now() + Duration::from_secs(15);
                    continue;
                }
            }
        }

        let changed = pushed.as_ref() != Some(&current);
        let stale = last_push.elapsed() >= Duration::from_secs(60);
        if !(changed || stale) || last_push.elapsed() < Duration::from_secs(5) {
            continue; // Discord rate-limits presence updates
        }

        if push_activity(&mut client, &current).is_ok() {
            pushed = Some(current.clone());
            last_push = Instant::now();
        } else {
            connected = false;
            retry_at = Instant::now() + Duration::from_secs(15);
        }
    }

    let _ = client.close();
}

fn push_activity(
    client: &mut DiscordIpcClient,
    s: &Snapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut assets = Assets::new().large_image("logo").large_text(&s.large_text);
    if let Some(small) = s.small_image {
        assets = assets.small_image(small).small_text(&s.small_text);
    }

    let mut activity = Activity::new()
        .state(&s.state)
        .details(&s.details)
        .assets(assets);

    if s.since > 0 {
        activity = activity.timestamps(Timestamps::new().start(s.since));
    }
    if let Some((cur, max)) = s.party {
        activity = activity.party(Party::new().size([cur, max]));
    }

    client.set_activity(activity)
}
