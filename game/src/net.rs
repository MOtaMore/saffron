//! Hand-rolled TCP multiplayer — no networking crates. Length-prefixed JSON
//! frames over blocking sockets on background threads, bridged to Bevy through
//! `std::sync::mpsc` channels held in `NonSend` slot resources.
//!
//! What replicates (v1): the world seed, the block-edit overlay
//! (`ChunkWorld.edits`, so friends build on the same world), player
//! position/rotation, and chat. What does NOT yet: inventory, chests/furnaces,
//! animals, dropped items, fishing.
//!
//! Modes ([`NetMode`]): `Solo` (default), `Host` (listen server — you play and
//! friends join), `Client` (you joined someone), `Server` (headless dedicated,
//! launched with `--server`; see `main.rs`).

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::block::Block;
use crate::pause::{GameFlow, Paused};
use crate::player::{PLAYER_MODEL, PLAYER_MODEL_DROP, PLAYER_MODEL_SCALE, PendingPlayerSpawn, Player};
use crate::streaming::ChunkWorld;
use crate::worldgen::WorldSeed;

pub const DEFAULT_PORT: u16 = 25599;
const HOST_ID: PlayerId = 0;
const MOVE_HZ: f32 = 20.0;
const SERVER_AUTOSAVE_SECS: f32 = 30.0;
const MAX_FRAME: usize = 8 * 1024 * 1024;
const CHAT_KEEP: usize = 8;
const CHAT_MAX_LEN: usize = 160;

type PlayerId = u32;

// --- Mode -----------------------------------------------------------------

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NetMode {
    #[default]
    Solo,
    Host,
    Client,
    Server,
}

impl NetMode {
    pub fn networked(self) -> bool {
        !matches!(self, NetMode::Solo)
    }
    /// Host (listen) or dedicated server — anything that owns the world.
    pub fn is_server(self) -> bool {
        matches!(self, NetMode::Host | NetMode::Server)
    }
}

fn on_server(mode: Res<NetMode>) -> bool {
    mode.is_server()
}
fn on_client(mode: Res<NetMode>) -> bool {
    *mode == NetMode::Client
}
fn is_networked(mode: Res<NetMode>) -> bool {
    mode.networked()
}

// --- Wire protocol ------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
enum ClientMsg {
    Hello { name: String, skin: String },
    Move { pos: [f32; 3], yaw: f32, moving: bool },
    SetBlock { pos: [i32; 3], block: Block },
    Chat { text: String },
}

#[derive(Serialize, Deserialize, Clone)]
enum ServerMsg {
    Welcome {
        id: PlayerId,
        seed: u32,
        spawn: [f32; 3],
        edits: Vec<([i32; 3], Block)>,
    },
    Joined {
        id: PlayerId,
        name: String,
        skin: String,
    },
    Left {
        id: PlayerId,
    },
    Moved {
        id: PlayerId,
        pos: [f32; 3],
        yaw: f32,
        moving: bool,
    },
    Block {
        pos: [i32; 3],
        block: Block,
    },
    Chat {
        from: String,
        text: String,
    },
}

fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(msg).map_err(io::Error::other)?;
    w.write_all(&(bytes.len() as u32).to_le_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::other("frame too large"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

// --- Chat -------------------------------------------------------------

#[derive(Resource, Default)]
pub struct ChatLog {
    lines: Vec<String>,
    /// `Some` while the player is typing a message.
    input: Option<String>,
    /// Set by systems when a chat line should go out over the wire.
    outbox: Vec<String>,
}

impl ChatLog {
    pub fn capturing(&self) -> bool {
        self.input.is_some()
    }
    fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
        let overflow = self.lines.len().saturating_sub(CHAT_KEEP);
        if overflow > 0 {
            self.lines.drain(0..overflow);
        }
    }
}

// === Plugins ==========================================================

/// Threads + systems the dedicated server also needs. Gated on `NetMode` being
/// a server; safe to add to a headless `App`.
pub struct ServerCorePlugin;

impl Plugin for ServerCorePlugin {
    fn build(&self, app: &mut App) {
        app.init_non_send::<ServerSlot>().add_systems(
            Update,
            (
                server_start_listener,
                server_pump,
                server_replicate_edits,
                server_replicate_players,
                server_flush_chat,
                server_autosave,
            )
                .chain()
                .run_if(on_server),
        );
    }
}

/// The full client-facing multiplayer plugin (menus, chat UI, remote avatars,
/// client sync). Added to the normal windowed app only.
pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetMode>()
            .init_resource::<ChatLog>()
            .init_non_send::<ClientSlot>()
            .add_message::<JoinServer>()
            .add_plugins(ServerCorePlugin)
            .add_systems(Startup, (spawn_chat_ui, spawn_menu_status, maybe_auto_join))
            .add_systems(
                Update,
                (handle_join_server, client_connect_pump, menu_status_text)
                    .run_if(in_state(GameFlow::Menu)),
            )
            .add_systems(
                Update,
                (
                    client_game_pump,
                    client_replicate_edits,
                    client_send_move,
                    move_remote_players,
                    apply_remote_skins,
                )
                    .run_if(on_client)
                    .run_if(in_state(GameFlow::Playing)),
            )
            .add_systems(Update, (chat_capture, chat_ui_sync).run_if(is_networked));
    }
}

// === Server ===========================================================

enum Inbound {
    Connected(PlayerId, Sender<ServerMsg>),
    Msg(PlayerId, ClientMsg),
    Dropped(PlayerId),
}

struct Conn {
    out: Sender<ServerMsg>,
    name: String,
    skin: String,
    /// Latest reported transform and whether it has been broadcast yet.
    mv: Option<([f32; 3], f32, bool)>,
    mv_dirty: bool,
    hello: bool,
}

struct NetServer {
    inbound: Receiver<Inbound>,
    conns: HashMap<PlayerId, Conn>,
    edit_shadow: HashMap<IVec3, Block>,
    autosave: f32,
    /// Host mode only: last transform we sent for the local player.
    host_mv: Option<([f32; 3], f32, bool)>,
}

impl NetServer {
    fn broadcast(&self, msg: &ServerMsg) {
        for c in self.conns.values() {
            let _ = c.out.send(msg.clone());
        }
    }
    fn broadcast_except(&self, skip: PlayerId, msg: &ServerMsg) {
        for (id, c) in &self.conns {
            if *id != skip {
                let _ = c.out.send(msg.clone());
            }
        }
    }
}

#[derive(Default)]
struct ServerSlot(Option<NetServer>);

#[derive(Resource, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub seed: u32,
    pub motd: String,
}

impl ServerConfig {
    pub fn load_or_create() -> Self {
        #[derive(Serialize, Deserialize)]
        struct Raw {
            port: u16,
            seed: u32,
            motd: String,
        }
        let path = PathBuf::from("server.json");
        let mut raw: Raw = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Raw {
                port: DEFAULT_PORT,
                seed: 0,
                motd: "Servidor Aves".into(),
            });
        if raw.seed == 0 {
            raw.seed = fresh_seed();
        }
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&raw).unwrap_or_default(),
        );
        Self {
            port: raw.port,
            seed: raw.seed,
            motd: raw.motd,
        }
    }
}

fn server_world_path() -> PathBuf {
    PathBuf::from("server_world.json")
}

#[derive(Serialize, Deserialize, Default)]
struct ServerWorld {
    edits: Vec<([i32; 3], Block)>,
}

fn load_server_edits() -> HashMap<IVec3, Block> {
    std::fs::read_to_string(server_world_path())
        .ok()
        .and_then(|t| serde_json::from_str::<ServerWorld>(&t).ok())
        .map(|w| {
            w.edits
                .into_iter()
                .map(|([x, y, z], b)| (IVec3::new(x, y, z), b))
                .collect()
        })
        .unwrap_or_default()
}

fn save_server_edits(edits: &HashMap<IVec3, Block>) {
    let data = ServerWorld {
        edits: edits.iter().map(|(p, b)| ([p.x, p.y, p.z], *b)).collect(),
    };
    let _ = std::fs::write(
        server_world_path(),
        serde_json::to_string_pretty(&data).unwrap_or_default(),
    );
}

/// Spins up the accept loop the first frame we are a server and don't have one.
fn server_start_listener(
    mut slot: NonSendMut<ServerSlot>,
    mode: Res<NetMode>,
    config: Option<Res<ServerConfig>>,
    mut world: ResMut<ChunkWorld>,
) {
    if slot.0.is_some() {
        return;
    }
    let port = config.as_deref().map(|c| c.port).unwrap_or(DEFAULT_PORT);

    // The dedicated server keeps the authoritative world in `edits`.
    if *mode == NetMode::Server {
        let saved = load_server_edits();
        if !saved.is_empty() {
            info!("mundo del servidor: {} bloques editados", saved.len());
            world.edits = saved;
        }
    }

    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            error!("no se pudo abrir el puerto {port}: {e}");
            // Insert an empty server so we don't retry every frame forever.
            slot.0 = Some(NetServer {
                inbound: channel().1,
                conns: HashMap::new(),
                edit_shadow: HashMap::new(),
                autosave: 0.0,
                host_mv: None,
            });
            return;
        }
    };
    let motd = config.as_deref().map(|c| c.motd.as_str()).unwrap_or("");
    info!("servidor escuchando en el puerto {port}  ·  {motd}");

    let (tx, rx) = channel::<Inbound>();
    thread::spawn(move || accept_loop(listener, tx));

    slot.0 = Some(NetServer {
        inbound: rx,
        conns: HashMap::new(),
        edit_shadow: world.edits.clone(),
        autosave: 0.0,
        host_mv: None,
    });
}

fn accept_loop(listener: TcpListener, tx: Sender<Inbound>) {
    static NEXT_ID: AtomicU32 = AtomicU32::new(1);
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let _ = stream.set_nodelay(true);
        let Ok(read_stream) = stream.try_clone() else {
            continue;
        };

        let (out_tx, out_rx) = channel::<ServerMsg>();
        if tx.send(Inbound::Connected(id, out_tx)).is_err() {
            return;
        }

        // Writer.
        let mut write_stream = stream;
        thread::spawn(move || {
            while let Ok(msg) = out_rx.recv() {
                if write_frame(&mut write_stream, &msg).is_err() {
                    break;
                }
            }
            let _ = write_stream.shutdown(std::net::Shutdown::Both);
        });

        // Reader.
        let reader_tx = tx.clone();
        thread::spawn(move || {
            let mut stream = read_stream;
            loop {
                match read_frame::<_, ClientMsg>(&mut stream) {
                    Ok(msg) => {
                        if reader_tx.send(Inbound::Msg(id, msg)).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = reader_tx.send(Inbound::Dropped(id));
                        break;
                    }
                }
            }
        });
    }
}

fn default_spawn() -> [f32; 3] {
    [0.5, 80.0, 0.5]
}

#[allow(clippy::too_many_arguments)]
fn server_pump(
    mut slot: NonSendMut<ServerSlot>,
    seed: Res<WorldSeed>,
    mut world: ResMut<ChunkWorld>,
    mut chat: ResMut<ChatLog>,
) {
    let Some(server) = slot.0.as_mut() else {
        return;
    };
    loop {
        match server.inbound.try_recv() {
            Ok(Inbound::Connected(id, out)) => {
                server.conns.insert(
                    id,
                    Conn {
                        out,
                        name: format!("Jugador {id}"),
                        skin: "motamore_skin".into(),
                        mv: None,
                        mv_dirty: false,
                        hello: false,
                    },
                );
            }
            Ok(Inbound::Msg(id, msg)) => {
                handle_client_msg(id, msg, server, &seed, &mut world, &mut chat);
            }
            Ok(Inbound::Dropped(id)) => {
                if let Some(c) = server.conns.remove(&id) {
                    server.broadcast(&ServerMsg::Left { id });
                    chat.push_line(format!("{} salió", c.name));
                }
            }
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
        }
    }
}

fn handle_client_msg(
    id: PlayerId,
    msg: ClientMsg,
    server: &mut NetServer,
    seed: &WorldSeed,
    world: &mut ChunkWorld,
    chat: &mut ChatLog,
) {
    match msg {
        ClientMsg::Hello { name, skin } => {
            let name = sanitize(&name, 24);
            // Tell the newcomer about the world and everyone already here.
            let welcome = ServerMsg::Welcome {
                id,
                seed: seed.0,
                spawn: default_spawn(),
                edits: world
                    .edits
                    .iter()
                    .map(|(p, b)| ([p.x, p.y, p.z], *b))
                    .collect(),
            };
            let existing: Vec<ServerMsg> = server
                .conns
                .iter()
                .filter(|(other, c)| **other != id && c.hello)
                .map(|(other, c)| ServerMsg::Joined {
                    id: *other,
                    name: c.name.clone(),
                    skin: c.skin.clone(),
                })
                .collect();
            if let Some(c) = server.conns.get_mut(&id) {
                c.name = name.clone();
                c.skin = sanitize(&skin, 40);
                c.hello = true;
                let _ = c.out.send(welcome);
                for m in existing {
                    let _ = c.out.send(m);
                }
            }
            let skin = server.conns.get(&id).map(|c| c.skin.clone()).unwrap_or_default();
            server.broadcast_except(
                id,
                &ServerMsg::Joined {
                    id,
                    name: name.clone(),
                    skin,
                },
            );
            chat.push_line(format!("{name} entró"));
        }
        ClientMsg::Move { pos, yaw, moving } => {
            if let Some(c) = server.conns.get_mut(&id) {
                c.mv = Some((pos, yaw, moving));
                c.mv_dirty = true;
            }
        }
        ClientMsg::SetBlock { pos, block } => {
            let p = IVec3::new(pos[0], pos[1], pos[2]);
            world.edits.insert(p, block);
            world.set_block(p.x, p.y, p.z, block); // no-op on a chunkless server
            server.edit_shadow.insert(p, block);
            server.broadcast(&ServerMsg::Block { pos, block });
        }
        ClientMsg::Chat { text } => {
            let text = sanitize(&text, CHAT_MAX_LEN);
            if text.is_empty() {
                return;
            }
            let from = server
                .conns
                .get(&id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("Jugador {id}"));
            server.broadcast(&ServerMsg::Chat {
                from: from.clone(),
                text: text.clone(),
            });
            chat.push_line(format!("<{from}> {text}"));
        }
    }
}

/// Broadcast any change to `world.edits` that did not originate from a client
/// (i.e. the host player's own building).
fn server_replicate_edits(mut slot: NonSendMut<ServerSlot>, world: Res<ChunkWorld>) {
    let Some(server) = slot.0.as_mut() else {
        return;
    };
    let mut changes: Vec<([i32; 3], Block)> = Vec::new();
    for (p, b) in &world.edits {
        if server.edit_shadow.get(p) != Some(b) {
            server.edit_shadow.insert(*p, *b);
            changes.push(([p.x, p.y, p.z], *b));
        }
    }
    for (pos, block) in changes {
        server.broadcast(&ServerMsg::Block { pos, block });
    }
}

fn server_replicate_players(
    mut slot: NonSendMut<ServerSlot>,
    mode: Res<NetMode>,
    host_q: Query<&Transform, With<Player>>,
) {
    let Some(server) = slot.0.as_mut() else {
        return;
    };

    // Fan out each client's latest move to the others.
    let updates: Vec<(PlayerId, [f32; 3], f32, bool)> = server
        .conns
        .iter_mut()
        .filter_map(|(id, c)| {
            if c.mv_dirty {
                c.mv_dirty = false;
                c.mv.map(|(p, y, m)| (*id, p, y, m))
            } else {
                None
            }
        })
        .collect();
    for (id, pos, yaw, moving) in updates {
        server.broadcast_except(id, &ServerMsg::Moved { id, pos, yaw, moving });
    }

    // Host's own avatar.
    if *mode == NetMode::Host {
        if let Ok(tf) = host_q.single() {
            let pos = tf.translation.to_array();
            let (yaw, _, _) = tf.rotation.to_euler(EulerRot::YXZ);
            if server.host_mv.map(|(p, ..)| p) != Some(pos) {
                server.host_mv = Some((pos, yaw, true));
                server.broadcast(&ServerMsg::Moved {
                    id: HOST_ID,
                    pos,
                    yaw,
                    moving: true,
                });
            }
        }
    }
}

fn server_flush_chat(mut slot: NonSendMut<ServerSlot>, mut chat: ResMut<ChatLog>) {
    let Some(server) = slot.0.as_mut() else {
        return;
    };
    let outgoing: Vec<String> = chat.outbox.drain(..).collect();
    for text in outgoing {
        server.broadcast(&ServerMsg::Chat {
            from: "Host".into(),
            text: text.clone(),
        });
        chat.push_line(format!("<Host> {text}"));
    }
}

fn server_autosave(
    time: Res<Time>,
    mut slot: NonSendMut<ServerSlot>,
    mode: Res<NetMode>,
    world: Res<ChunkWorld>,
) {
    if *mode != NetMode::Server {
        return;
    }
    let Some(server) = slot.0.as_mut() else {
        return;
    };
    server.autosave += time.delta_secs();
    if server.autosave >= SERVER_AUTOSAVE_SECS {
        server.autosave = 0.0;
        save_server_edits(&world.edits);
    }
}

// === Client ===========================================================

enum ClientEvent {
    Connected,
    Failed(String),
    Msg(ServerMsg),
    Dropped,
}

pub enum ClientState {
    Connecting,
    Handshaking,
    Playing,
    Failed(String),
    Lost,
}

struct NetClient {
    inbound: Receiver<ClientEvent>,
    out: Sender<ClientMsg>,
    state: ClientState,
    edit_shadow: HashMap<IVec3, Block>,
    my_id: PlayerId,
    move_accum: f32,
    last_move: Option<[f32; 3]>,
}

impl NetClient {
    fn send(&self, msg: ClientMsg) {
        let _ = self.out.send(msg);
    }
}

#[derive(Default)]
struct ClientSlot(Option<NetClient>);

/// Accept `host`, `host:port`, or a pasted `tcp://host:port`; default the port
/// to [`DEFAULT_PORT`] when it is missing.
pub fn normalize_addr(addr: &str) -> String {
    let a = addr
        .trim()
        .trim_start_matches("tcp://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if a.contains(':') {
        a.to_string()
    } else {
        format!("{a}:{DEFAULT_PORT}")
    }
}

fn spawn_client(addr: String) -> NetClient {
    let addr = normalize_addr(&addr);
    let (in_tx, in_rx) = channel::<ClientEvent>();
    let (out_tx, out_rx) = channel::<ClientMsg>();

    thread::spawn(move || {
        let stream = match TcpStream::connect(&addr) {
            Ok(s) => s,
            Err(e) => {
                let _ = in_tx.send(ClientEvent::Failed(e.to_string()));
                return;
            }
        };
        let _ = stream.set_nodelay(true);
        let Ok(read_stream) = stream.try_clone() else {
            let _ = in_tx.send(ClientEvent::Failed("try_clone".into()));
            return;
        };
        let _ = in_tx.send(ClientEvent::Connected);

        // Writer.
        let mut write_stream = stream;
        thread::spawn(move || {
            while let Ok(msg) = out_rx.recv() {
                if write_frame(&mut write_stream, &msg).is_err() {
                    break;
                }
            }
        });

        // Reader (this thread).
        let mut stream = read_stream;
        loop {
            match read_frame::<_, ServerMsg>(&mut stream) {
                Ok(msg) => {
                    if in_tx.send(ClientEvent::Msg(msg)).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = in_tx.send(ClientEvent::Dropped);
                    break;
                }
            }
        }
    });

    NetClient {
        inbound: in_rx,
        out: out_tx,
        state: ClientState::Connecting,
        edit_shadow: HashMap::new(),
        my_id: 0,
        move_accum: 0.0,
        last_move: None,
    }
}


/// Menu-state: finish the handshake, then hand off to `GameFlow::Playing` once
/// the `Welcome` (seed + edit overlay) has landed.
fn client_connect_pump(
    mut slot: NonSendMut<ClientSlot>,
    mut mode: ResMut<NetMode>,
    skin: Res<crate::skins::SkinChoice>,
    mut seed: ResMut<WorldSeed>,
    mut world: ResMut<ChunkWorld>,
    mut next: ResMut<NextState<GameFlow>>,
    mut commands: Commands,
) {
    let Some(client) = slot.0.as_mut() else {
        return;
    };
    loop {
        match client.inbound.try_recv() {
            Ok(ClientEvent::Connected) => {
                client.state = ClientState::Handshaking;
                client.send(ClientMsg::Hello {
                    name: whoami(),
                    skin: skin.0.clone(),
                });
            }
            Ok(ClientEvent::Failed(e)) => {
                client.state = ClientState::Failed(e);
            }
            Ok(ClientEvent::Dropped) => {
                client.state = ClientState::Failed("conexión cerrada".into());
            }
            Ok(ClientEvent::Msg(ServerMsg::Welcome {
                id,
                seed: world_seed,
                spawn,
                edits,
            })) => {
                client.my_id = id;
                seed.0 = world_seed;
                world.edits = edits
                    .into_iter()
                    .map(|([x, y, z], b)| (IVec3::new(x, y, z), b))
                    .collect();
                world.prop_blocks.clear();
                client.edit_shadow = world.edits.clone();
                *mode = NetMode::Client;
                commands.insert_resource(PendingPlayerSpawn {
                    pos: Vec3::from_array(spawn),
                });
                client.state = ClientState::Playing;
                next.set(GameFlow::Playing);
            }
            // Anything else before Welcome is safe to drop.
            Ok(ClientEvent::Msg(_)) => {}
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
        }
    }
}

#[derive(Component)]
struct RemotePlayer {
    id: PlayerId,
    target: Vec3,
    yaw: f32,
}

#[derive(Component)]
struct RemoteModel {
    skin: String,
    painted: bool,
}

#[allow(clippy::too_many_arguments)]
fn client_game_pump(
    mut slot: NonSendMut<ClientSlot>,
    server: Res<AssetServer>,
    mut world: ResMut<ChunkWorld>,
    mut chat: ResMut<ChatLog>,
    mut commands: Commands,
    mut remotes: Query<(Entity, &mut RemotePlayer)>,
) {
    let Some(client) = slot.0.as_mut() else {
        return;
    };
    let mut events = Vec::new();
    loop {
        match client.inbound.try_recv() {
            Ok(ev) => events.push(ev),
            Err(_) => break,
        }
    }
    for ev in events {
        match ev {
            ClientEvent::Dropped => {
                client.state = ClientState::Lost;
                chat.push_line("Se perdió la conexión con el servidor");
            }
            ClientEvent::Msg(ServerMsg::Joined { id, name, skin }) => {
                if id == client.my_id {
                    continue;
                }
                spawn_remote(&mut commands, &server, id, skin, default_spawn(), 0.0);
                chat.push_line(format!("{name} entró"));
            }
            ClientEvent::Msg(ServerMsg::Left { id }) => {
                for (entity, remote) in &remotes {
                    if remote.id == id {
                        commands.entity(entity).despawn();
                    }
                }
            }
            ClientEvent::Msg(ServerMsg::Moved {
                id,
                pos,
                yaw,
                moving: _,
            }) => {
                if id == client.my_id {
                    continue;
                }
                let mut found = false;
                for (_, mut remote) in &mut remotes {
                    if remote.id == id {
                        remote.target = Vec3::from_array(pos);
                        remote.yaw = yaw;
                        found = true;
                    }
                }
                if !found {
                    spawn_remote(&mut commands, &server, id, "motamore_skin".into(), pos, yaw);
                }
            }
            ClientEvent::Msg(ServerMsg::Block { pos, block }) => {
                let p = IVec3::new(pos[0], pos[1], pos[2]);
                world.edits.insert(p, block);
                world.set_block(p.x, p.y, p.z, block);
                client.edit_shadow.insert(p, block);
            }
            ClientEvent::Msg(ServerMsg::Chat { from, text }) => {
                chat.push_line(format!("<{from}> {text}"));
            }
            _ => {}
        }
    }
}

fn spawn_remote(
    commands: &mut Commands,
    server: &AssetServer,
    id: PlayerId,
    skin: String,
    pos: [f32; 3],
    yaw: f32,
) -> Entity {
    let target = Vec3::from_array(pos);
    commands
        .spawn((
            RemotePlayer { id, target, yaw },
            Transform::from_translation(target),
            Visibility::default(),
        ))
        .with_children(|c| {
            c.spawn((
                RemoteModel {
                    skin,
                    painted: false,
                },
                WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(PLAYER_MODEL))),
                Transform::from_xyz(0.0, PLAYER_MODEL_DROP, 0.0)
                    .with_scale(Vec3::splat(PLAYER_MODEL_SCALE))
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
            ));
        })
        .id()
}

fn move_remote_players(time: Res<Time>, mut remotes: Query<(&mut Transform, &RemotePlayer)>) {
    let t = 1.0 - (-14.0 * time.delta_secs()).exp();
    for (mut tf, remote) in &mut remotes {
        tf.translation = tf.translation.lerp(remote.target, t);
        let want = Quat::from_rotation_y(remote.yaw);
        tf.rotation = tf.rotation.slerp(want, t);
    }
}

fn apply_remote_skins(
    server: Res<AssetServer>,
    children_q: Query<&Children>,
    mesh_q: Query<&MeshMaterial3d<StandardMaterial>, With<Mesh3d>>,
    mut models: Query<(Entity, &mut RemoteModel)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (root, mut model) in &mut models {
        if model.painted {
            continue;
        }
        let skin = server.load(format!("textures/player_skin/{}.png", model.skin));
        let painted = crate::player::repaint_skin(root, &skin, &children_q, &mesh_q, &mut materials);
        if painted > 0 {
            model.painted = true;
        }
    }
}

fn client_replicate_edits(mut slot: NonSendMut<ClientSlot>, world: Res<ChunkWorld>) {
    let Some(client) = slot.0.as_mut() else {
        return;
    };
    let mut changes = Vec::new();
    for (p, b) in &world.edits {
        if client.edit_shadow.get(p) != Some(b) {
            client.edit_shadow.insert(*p, *b);
            changes.push(([p.x, p.y, p.z], *b));
        }
    }
    for (pos, block) in changes {
        client.send(ClientMsg::SetBlock { pos, block });
    }
}

fn client_send_move(
    time: Res<Time>,
    mut slot: NonSendMut<ClientSlot>,
    player_q: Query<&Transform, With<Player>>,
) {
    let Some(client) = slot.0.as_mut() else {
        return;
    };
    client.move_accum += time.delta_secs();
    if client.move_accum < 1.0 / MOVE_HZ {
        return;
    }
    client.move_accum = 0.0;
    let Ok(tf) = player_q.single() else {
        return;
    };
    let pos = tf.translation.to_array();
    let moving = client.last_move.map(|p| p != pos).unwrap_or(true);
    client.last_move = Some(pos);
    let (yaw, _, _) = tf.rotation.to_euler(EulerRot::YXZ);
    client.send(ClientMsg::Move { pos, yaw, moving });
}

// === Menu wiring ======================================================

/// Fired by the "Multijugador" screen to connect to a server by address.
#[derive(Message)]
pub struct JoinServer(pub String);

/// Set by `--connect <addr>` on the command line: auto-join on startup.
#[derive(Resource)]
pub struct AutoJoin(pub String);

#[derive(Component)]
struct MenuStatus;

fn spawn_menu_status(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(24.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        GlobalZIndex(201),
        Text::new(""),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb(0.85, 0.9, 1.0)),
        MenuStatus,
    ));
}

fn menu_status_text(slot: NonSend<ClientSlot>, mut text: Query<&mut Text, With<MenuStatus>>) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    text.0 = match slot.0.as_ref().map(|c| &c.state) {
        None => String::new(),
        Some(ClientState::Connecting | ClientState::Handshaking) => "Conectando…".into(),
        Some(ClientState::Playing) => "Conectado".into(),
        Some(ClientState::Lost) => "Conexión perdida".into(),
        Some(ClientState::Failed(e)) => format!("No se pudo conectar: {e}"),
    };
}

fn maybe_auto_join(
    auto: Option<Res<AutoJoin>>,
    mut slot: NonSendMut<ClientSlot>,
    mut mode: ResMut<NetMode>,
) {
    if let Some(auto) = auto {
        info!("--connect {}", auto.0);
        *mode = NetMode::Client;
        slot.0 = Some(spawn_client(auto.0.clone()));
    }
}

fn handle_join_server(
    mut events: MessageReader<JoinServer>,
    mut slot: NonSendMut<ClientSlot>,
    mut mode: ResMut<NetMode>,
) {
    let Some(JoinServer(addr)) = events.read().last() else {
        return;
    };
    *mode = NetMode::Client;
    slot.0 = Some(spawn_client(addr.clone()));
}

// === Chat UI ==========================================================

#[derive(Component)]
struct ChatRoot;
#[derive(Component)]
struct ChatLines;
#[derive(Component)]
struct ChatInputPill;
#[derive(Component)]
struct ChatInputText;

fn spawn_chat_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(70.0),
                width: Val::Px(460.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                ..default()
            },
            Visibility::Hidden,
            ChatRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(""),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgb(0.9, 0.93, 0.96)),
                ChatLines,
            ));
            root.spawn((
                Node {
                    margin: UiRect::top(Val::Px(4.0)),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                Visibility::Hidden,
                ChatInputPill,
            ))
            .with_children(|pill| {
                pill.spawn((
                    Text::new(""),
                    TextFont::from_font_size(14.0),
                    TextColor(Color::srgb(1.0, 0.95, 0.6)),
                    ChatInputText,
                ));
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn chat_capture(
    mut keys: MessageReader<KeyboardInput>,
    mut chat: ResMut<ChatLog>,
    mode: Res<NetMode>,
    paused: Res<Paused>,
    flow: Res<State<GameFlow>>,
    slot_c: NonSend<ClientSlot>,
) {
    if !mode.networked() || paused.0 || !matches!(flow.get(), GameFlow::Playing) {
        chat.input = None;
        return;
    }
    for ev in keys.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match (&mut chat.input, &ev.logical_key) {
            (slot @ None, Key::Enter) => *slot = Some(String::new()),
            (Some(_), Key::Enter) => {
                let text = chat
                    .input
                    .take()
                    .map(|t| sanitize(&t, CHAT_MAX_LEN))
                    .unwrap_or_default();
                if !text.is_empty() {
                    send_chat(*mode, slot_c.0.as_ref(), &mut chat, text);
                }
            }
            (Some(_), Key::Escape) => chat.input = None,
            (Some(buf), Key::Backspace) => {
                buf.pop();
            }
            (Some(buf), Key::Space) => {
                if buf.len() < CHAT_MAX_LEN {
                    buf.push(' ');
                }
            }
            (Some(buf), Key::Character(s)) => {
                for ch in s.chars() {
                    if !ch.is_control() && buf.len() < CHAT_MAX_LEN {
                        buf.push(ch);
                    }
                }
            }
            _ => {}
        }
    }
}

fn send_chat(mode: NetMode, client: Option<&NetClient>, chat: &mut ChatLog, text: String) {
    match mode {
        NetMode::Client => {
            if let Some(client) = client {
                client.send(ClientMsg::Chat { text: text.clone() });
            }
            chat.push_line(format!("<yo> {text}"));
        }
        // Host / dedicated: hand to `server_flush_chat`.
        _ => chat.outbox.push(text),
    }
}

fn chat_ui_sync(
    chat: Res<ChatLog>,
    mut root: Query<&mut Visibility, (With<ChatRoot>, Without<ChatInputPill>)>,
    mut pill: Query<&mut Visibility, (With<ChatInputPill>, Without<ChatRoot>)>,
    mut lines: Query<&mut Text, (With<ChatLines>, Without<ChatInputText>)>,
    mut field: Query<&mut Text, (With<ChatInputText>, Without<ChatLines>)>,
) {
    if let Ok(mut vis) = root.single_mut() {
        *vis = if chat.lines.is_empty() && chat.input.is_none() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
    if let Ok(mut vis) = pill.single_mut() {
        *vis = if chat.input.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if let Ok(mut text) = lines.single_mut() {
        text.0 = chat.lines.join("\n");
    }
    if let Ok(mut text) = field.single_mut() {
        text.0 = match &chat.input {
            Some(buf) => format!("> {buf}_"),
            None => String::new(),
        };
    }
}

// === Helpers ==========================================================

fn sanitize(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

fn whoami() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .map(|n| sanitize(&n, 24))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Jugador".into())
}

fn fresh_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let mut x = n ^ (n >> 33);
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x as u32
}

