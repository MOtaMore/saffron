//! Procedural terrain generation.
//!
//! Purely functional: `WorldGen::generate` takes a chunk coordinate and returns
//! fully-populated `ChunkData`, so it can run on a background task pool.
//!
//! Features: rolling base terrain, rare **mountain regions** with sharp snowy
//! ridgelines, lowland **rivers**, 3D **cave** tunnels + caverns, and vertical
//! **ravines** ("grietas") that break the surface.

use std::sync::Arc;

use bevy::prelude::*;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use crate::block::Block;
use crate::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, ChunkCoord, ChunkData};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Biome {
    Plains,
    Forest,
    Desert,
    Snow,
}

impl Biome {
    fn surface(self, height: i32, sea: i32) -> Block {
        if self != Biome::Snow && height <= sea + 1 {
            return Block::Sand; // beaches
        }
        match self {
            Biome::Desert => Block::Sand,
            Biome::Snow => Block::Snow,
            Biome::Plains | Biome::Forest => Block::Grass,
        }
    }

    fn filler(self) -> Block {
        match self {
            Biome::Desert => Block::Sand,
            _ => Block::Dirt,
        }
    }

    fn has_trees(self) -> bool {
        matches!(self, Biome::Plains | Biome::Forest)
    }

    fn tree_chance(self) -> f32 {
        match self {
            Biome::Forest => 0.06,
            Biome::Plains => 0.012,
            _ => 0.0,
        }
    }

    /// Nombre para el chat / comandos (los comandos van en inglés).
    pub fn en_name(self) -> &'static str {
        match self {
            Biome::Plains => "plains",
            Biome::Forest => "forest",
            Biome::Desert => "desert",
            Biome::Snow => "snow",
        }
    }

    /// Interpreta el argumento de `/biome <x>`.
    pub fn parse(s: &str) -> Option<Biome> {
        Some(match s.trim().to_lowercase().as_str() {
            "plains" | "plain" | "grassland" => Biome::Plains,
            "forest" | "woods" => Biome::Forest,
            "desert" | "sand" => Biome::Desert,
            "snow" | "snowy" => Biome::Snow,
            _ => return None,
        })
    }
}

/// The world seed. Set by the menu / a loaded save before entering `Playing`.
#[derive(Resource)]
pub struct WorldSeed(pub u32);

impl Default for WorldSeed {
    fn default() -> Self {
        Self(0x5EED_1234)
    }
}

/// Shared, cheap-to-clone handle to the world generator (built on `OnEnter`
/// `GameFlow::Playing` from `WorldSeed`).
#[derive(Resource, Clone)]
pub struct WorldGenHandle(pub Arc<WorldGen>);

pub struct WorldGen {
    seed: u32,
    height: Fbm<Perlin>,
    mtn_mask: Fbm<Perlin>,
    mountain: Fbm<Perlin>,
    biome: Fbm<Perlin>,
    river: Fbm<Perlin>,
    ravine: Fbm<Perlin>,
    ravine_mask: Perlin,
    snow: Perlin,
    rock: Perlin,
    gravel: Perlin,
    clay: Perlin,
    mud: Perlin,
    ruin: Perlin,
    contam: Perlin,
    cave_a: Perlin,
    cave_b: Perlin,
    cavern: Perlin,
    /// Structures with `spawn.weight > 0`, stamped into chunks during `generate`.
    structures: Arc<crate::structure::Library>,
    pub sea_level: i32,
}

/// Chunks (per axis) in one structure-placement region. A region gets at most
/// one structure, anchored so its footprint stays inside the region.
const REGION_CHUNKS: i32 = 4;
/// Regions that attempt a structure, per mille (before slope / Y-range checks).
const STRUCT_PER_MIL: u64 = 130;

/// Chunks per axis in one ruined-city region — one city at most, anchored so its
/// footprint never leaves the region (keeps chunk generation independent).
const CITY_REGION_CHUNKS: i32 = 16;
/// City regions that actually hold ruins, per mille (before the flatness gate).
const CITY_PER_MIL: u64 = 90;
/// Storey height of the apartment blocks.
const CITY_FLOOR_H: i32 = 3;
/// Apartment-block footprint (per side), in blocks.
const BUILD_MIN: i32 = 15;
const BUILD_MAX: i32 = 22;
/// Gap between two blocks of the *same* manzana (kept tight — claustrophobic).
const BUILD_GAP: i32 = 2;
/// Street width between one manzana and the next.
const STREET_W: i32 = 9;
/// Depth of a room; interior partitions sit every `ROOM_STEP + 1` blocks.
const ROOM_STEP: i32 = 3;
/// Most apartment blocks per axis inside one manzana (3×3 = 9 plots, 3..7 used).
const SUB_MAX: i32 = 3;
const BUILD_SLOT: i32 = BUILD_MAX + BUILD_GAP;
const MANZANA_SLOT: i32 = SUB_MAX * BUILD_SLOT;

/// Manzanas per axis in a city (2..3 each).
fn city_grid(ch: u64) -> (i32, i32) {
    (2 + ((ch >> 4) & 1) as i32, 2 + ((ch >> 7) & 1) as i32)
}

fn region_hash(seed: u32, rx: i32, ry: i32) -> u64 {
    let mut z = (seed as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (rx as u64).wrapping_mul(0xD1B5_4A32_D192_ED03)
        ^ (ry as u64).wrapping_mul(0xA076_1D64_78BD_642F);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn city_hash(seed: u32, rx: i32, ry: i32) -> u64 {
    region_hash(seed ^ 0x00C1_7000, rx, ry)
}

/// `(anchor_x, anchor_z, width, depth)` of the ruined city in region `(rx, ry)`,
/// or `None` if that region has none. The city is a grid of *manzanas* (city
/// blocks) separated by streets, anchored fully inside its region so every chunk
/// can reconstruct it independently.
fn city_anchor(seed: u32, rx: i32, ry: i32) -> Option<(i32, i32, i32, i32)> {
    let ch = city_hash(seed, rx, ry);
    if ch % 1000 >= CITY_PER_MIL {
        return None;
    }
    let (mcols, mrows) = city_grid(ch);
    let city_w = mcols * MANZANA_SLOT + (mcols - 1) * STREET_W;
    let city_d = mrows * MANZANA_SLOT + (mrows - 1) * STREET_W;
    let span = CITY_REGION_CHUNKS * CHUNK_SIZE;
    let ax = rx * span + ((ch >> 20) % (span - city_w).max(1) as u64) as i32;
    let az = ry * span + ((ch >> 36) % (span - city_d).max(1) as u64) as i32;
    Some((ax, az, city_w, city_d))
}

/// Vertical material gradient of a ruined apartment block: a stone-brick base,
/// a cement midsection, plain brick up top. `k` is the storey index.
fn ruin_material(k: i32, standing: i32) -> Block {
    let third = (standing.max(1) + 2) / 3;
    if k < third {
        Block::StoneBrick
    } else if k < 2 * third {
        Block::Cement
    } else {
        Block::Bricks
    }
}

/// splitmix64 finalizer — a per-building spread from the city hash.
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Recorre columnas en anillos cuadrados (paso `step`) alrededor de `from` y
/// devuelve la más cercana que cumpla `hit`. `None` si ninguna en `max_rings`.
fn ring_nearest(
    from: IVec2,
    step: i32,
    max_rings: i32,
    mut hit: impl FnMut(i32, i32) -> bool,
) -> Option<IVec2> {
    if hit(from.x, from.y) {
        return Some(from);
    }
    for r in 1..=max_rings {
        let rr = r * step;
        let mut best: Option<(i64, IVec2)> = None;
        for i in -r..=r {
            let o = i * step;
            for (x, z) in [
                (from.x + o, from.y - rr),
                (from.x + o, from.y + rr),
                (from.x - rr, from.y + o),
                (from.x + rr, from.y + o),
            ] {
                if hit(x, z) {
                    let (dx, dz) = ((x - from.x) as i64, (z - from.y) as i64);
                    let d = dx * dx + dz * dz;
                    if best.map_or(true, |(bd, _)| d < bd) {
                        best = Some((d, IVec2::new(x, z)));
                    }
                }
            }
        }
        if best.is_some() {
            return best.map(|(_, p)| p);
        }
    }
    None
}

/// One resolved terrain column.
struct Column {
    /// Index of the topmost solid block (heightmap, before caves/ravines).
    height: i32,
    /// 0 = no river here, →1 towards a river's centreline.
    river_t: f32,
}

impl WorldGen {
    pub fn new(seed: u32, structures: Arc<crate::structure::Library>) -> Self {
        let height = Fbm::<Perlin>::new(seed)
            .set_octaves(5)
            .set_frequency(0.0042)
            .set_persistence(0.5)
            .set_lacunarity(2.05);
        let mtn_mask = Fbm::<Perlin>::new(seed.wrapping_add(71))
            .set_octaves(2)
            .set_frequency(0.0011);
        let mountain = Fbm::<Perlin>::new(seed.wrapping_add(151))
            .set_octaves(4)
            .set_frequency(0.0060)
            .set_persistence(0.55);
        let biome = Fbm::<Perlin>::new(seed.wrapping_add(101))
            .set_octaves(3)
            .set_frequency(0.0016);
        let river = Fbm::<Perlin>::new(seed.wrapping_add(211))
            .set_octaves(2)
            .set_frequency(0.00095);
        let ravine = Fbm::<Perlin>::new(seed.wrapping_add(307))
            .set_octaves(2)
            .set_frequency(0.0040);
        Self {
            seed,
            height,
            mtn_mask,
            mountain,
            biome,
            river,
            ravine,
            ravine_mask: Perlin::new(seed.wrapping_add(313)),
            snow: Perlin::new(seed.wrapping_add(404)),
            rock: Perlin::new(seed.wrapping_add(451)),
            gravel: Perlin::new(seed.wrapping_add(505)),
            clay: Perlin::new(seed.wrapping_add(521)),
            mud: Perlin::new(seed.wrapping_add(541)),
            ruin: Perlin::new(seed.wrapping_add(809)),
            contam: Perlin::new(seed.wrapping_add(877)),
            cave_a: Perlin::new(seed.wrapping_add(601)),
            cave_b: Perlin::new(seed.wrapping_add(619)),
            cavern: Perlin::new(seed.wrapping_add(637)),
            structures,
            sea_level: 48,
        }
    }

    /// Base rolling height + mountain peak contribution (no rivers yet).
    fn raw_height(&self, x: f64, z: f64) -> f64 {
        let n = self.height.get([x, z]); // ~ -1..1
        let base = 30.0 + (n * 0.5 + 0.5).clamp(0.0, 1.0).powf(1.22) * 58.0; // ~30..88

        // Mountains: only the upper slice of the mask noise becomes a range, and
        // ridged `mountain` noise makes sharp crests instead of round domes.
        let mask = ((self.mtn_mask.get([x, z]) * 0.5 + 0.5) - 0.42).max(0.0) / 0.58;
        let ridge = 1.0 - self.mountain.get([x, z]).abs(); // 0..1, ~1 on a crest
        let peak = mask.powf(1.2) * ridge.powf(1.8) * 52.0;

        base + peak
    }

    fn column(&self, x: i32, z: i32) -> Column {
        let (fx, fz) = (x as f64, z as f64);
        let raw = self.raw_height(fx, fz);
        let sea = self.sea_level as f64;

        // Rivers: only in already-low land, so they always hold water.
        let mut river_t = 0.0_f64;
        let mut h = raw;
        if raw < sea + 12.0 {
            let w = 0.028 + 0.020 * (self.river.get([fx * 3.1, fz * 3.1]) * 0.5 + 0.5);
            let d = self.river.get([fx, fz]).abs();
            if d < w {
                river_t = smoothstep(1.0 - d / w);
                h = lerp(raw, sea - 3.0, river_t);
            }
        }

        Column {
            height: (h.round() as i32).clamp(2, CHUNK_HEIGHT - 6),
            river_t: river_t as f32,
        }
    }

    /// Terrain surface height (index of the topmost solid block) at a world column.
    pub fn surface_height(&self, x: i32, z: i32) -> i32 {
        self.column(x, z).height
    }

    /// Dry, solid ground: surface above the waterline and not a river bed.
    pub fn is_land(&self, x: i32, z: i32) -> bool {
        let c = self.column(x, z);
        c.height > self.sea_level + 1 && c.river_t <= 0.0
    }

    /// Nearest dry-land column to `(ox, oz)`, spiralling outward. Used so the
    /// player never spawns underwater. Falls back to the origin column.
    pub fn find_land(&self, ox: i32, oz: i32) -> (i32, i32) {
        if self.is_land(ox, oz) {
            return (ox, oz);
        }
        for r in 1..128 {
            for d in -r..=r {
                for (x, z) in [
                    (ox + d, oz - r),
                    (ox + d, oz + r),
                    (ox - r, oz + d),
                    (ox + r, oz + d),
                ] {
                    if self.is_land(x, z) {
                        return (x, z);
                    }
                }
            }
        }
        (ox, oz)
    }

    // --- Búsquedas para los comandos de chat (`/bioma`, `/estructura`, …) ---

    /// Columna del bioma `want` más cercana a `from`, como `[x, y, z]` con `y`
    /// dos bloques por encima de la superficie (para caer encima). `None` si no
    /// aparece en el radio de búsqueda (~5 km).
    pub fn nearest_biome(&self, from: IVec2, want: Biome) -> Option<IVec3> {
        let p = ring_nearest(from, 8, 640, |x, z| {
            let h = self.surface_height(x, z);
            h > self.sea_level && self.biome_at(x, z, h) == want
        })?;
        Some(IVec3::new(p.x, self.surface_height(p.x, p.y) + 2, p.y))
    }

    /// Ancla de la estructura de librería generable más cercana. `None` si la
    /// librería está vacía o no hay ninguna en el radio.
    pub fn nearest_structure(&self, from: IVec2) -> Option<IVec3> {
        if self.structures.is_empty() {
            return None;
        }
        let span = REGION_CHUNKS * CHUNK_SIZE;
        self.nearest_region(from, span, 56, |rx, ry| self.structure_at_region(rx, ry))
    }

    /// Centro de la ciudad en ruinas más cercana (sobre tierra firme).
    pub fn nearest_ruined_city(&self, from: IVec2) -> Option<IVec3> {
        let span = CITY_REGION_CHUNKS * CHUNK_SIZE;
        self.nearest_region(from, span, 40, |rx, ry| {
            let (ax, az, w, d) = city_anchor(self.seed, rx, ry)?;
            let (cx, cz) = (ax + w / 2, az + d / 2);
            self.is_land(cx, cz)
                .then(|| IVec3::new(cx, self.surface_height(cx, cz) + 2, cz))
        })
    }

    /// Recorre las regiones de tamaño `span` en anillos crecientes desde la de
    /// `from` y devuelve el resultado no vacío más cercano.
    fn nearest_region(
        &self,
        from: IVec2,
        span: i32,
        max_rings: i32,
        mut at: impl FnMut(i32, i32) -> Option<IVec3>,
    ) -> Option<IVec3> {
        let (frx, fry) = (from.x.div_euclid(span), from.y.div_euclid(span));
        for r in 0..=max_rings {
            let mut best: Option<(i64, IVec3)> = None;
            for rx in frx - r..=frx + r {
                for ry in fry - r..=fry + r {
                    let edge = rx == frx - r || rx == frx + r || ry == fry - r || ry == fry + r;
                    if r > 0 && !edge {
                        continue;
                    }
                    if let Some(p) = at(rx, ry) {
                        let (dx, dz) = ((p.x - from.x) as i64, (p.z - from.y) as i64);
                        let d = dx * dx + dz * dz;
                        if best.map_or(true, |(bd, _)| d < bd) {
                            best = Some((d, p));
                        }
                    }
                }
            }
            if best.is_some() {
                return best.map(|(_, p)| p);
            }
        }
        None
    }

    /// El ancla `[x, y, z]` de la estructura que `stamp_structures` colocaría en
    /// la región `(rx, ry)`, o `None` (mismo dado + puertas que el generador).
    fn structure_at_region(&self, rx: i32, ry: i32) -> Option<IVec3> {
        let lib = &*self.structures;
        if lib.is_empty() {
            return None;
        }
        let h = region_hash(self.seed, rx, ry);
        if h % 1000 >= STRUCT_PER_MIL {
            return None;
        }
        let s = lib.pick(((h >> 12) & 0xFFFF) as f32 / 65536.0)?;
        let [sx, sy, sz] = s.size;
        if sx <= 0 || sy <= 0 || sz <= 0 {
            return None;
        }
        let span = REGION_CHUNKS * CHUNK_SIZE;
        let ax = rx * span + ((h >> 24) % (span - sx).max(1) as u64) as i32;
        let az = ry * span + ((h >> 40) % (span - sz).max(1) as u64) as i32;
        let hs = [
            self.surface_height(ax, az),
            self.surface_height(ax + sx - 1, az),
            self.surface_height(ax, az + sz - 1),
            self.surface_height(ax + sx - 1, az + sz - 1),
            self.surface_height(ax + sx / 2, az + sz / 2),
        ];
        let lo = *hs.iter().min().unwrap();
        let hi = *hs.iter().max().unwrap();
        let r = &s.spawn;
        if hi - lo > r.max_slope || lo < r.min_y || lo > r.max_y {
            return None;
        }
        let ay = (lo - r.sink).max(1);
        Some(IVec3::new(ax, ay + 2, az))
    }

    /// Which flavour of water fills a submerged cell: mostly plain, but a
    /// fraction of rivers run **irradiated** and stagnant inland pools turn
    /// **toxic** in patches (deterministic — the post-apocalyptic creep).
    fn water_at(&self, x: i32, z: i32, h: i32, is_river: bool) -> Block {
        let (fx, fz) = (x as f64, z as f64);
        if is_river {
            if self.contam.get([fx * 0.0016, fz * 0.0016]) > 0.30 {
                return Block::RadWater;
            }
        } else if h < self.sea_level - 1
            && self.contam.get([fx * 0.03 + 137.0, fz * 0.03 - 61.0]) > 0.55
        {
            return Block::ToxicWater;
        }
        Block::Water
    }

    /// Altitude (noisy) above which the surface turns to snow.
    pub fn snowline(&self, x: i32, z: i32) -> f64 {
        82.0 + (self.snow.get([x as f64 * 0.02, z as f64 * 0.02]) * 0.5 + 0.5) * 12.0
    }

    pub fn biome_at(&self, x: i32, z: i32, height: i32) -> Biome {
        if height as f64 >= self.snowline(x, z) {
            return Biome::Snow;
        }
        let t = (self.biome.get([x as f64, z as f64]) * 0.5 + 0.5) as f32;
        if t < 0.30 {
            Biome::Desert
        } else if t > 0.68 {
            Biome::Forest
        } else {
            Biome::Plains
        }
    }

    /// `true` if the voxel at `(x, y, z)` should be hollowed into a cave.
    fn cave_carve(&self, x: f64, y: f64, z: f64) -> bool {
        // Worm tunnels: near the shared zero-set of two 3D fields.
        let a = self.cave_a.get([x * 0.028, y * 0.046, z * 0.028]);
        let b = self.cave_b.get([x * 0.028, y * 0.046, z * 0.028]);
        if a * a + b * b < 0.020 {
            return true;
        }
        // Blobby caverns, progressively more common towards bedrock.
        let bias = ((42.0 - y).max(0.0)) * 0.004;
        self.cavern.get([x * 0.020, y * 0.030, z * 0.020]) > 0.58 - bias
    }

    /// If this column is inside a ravine, the Y of its floor.
    fn ravine_floor(&self, x: f64, z: f64, h: i32) -> Option<i32> {
        if self.ravine_mask.get([x * 0.0055, z * 0.0055]) < 0.55 {
            return None; // ravines only in scattered regions
        }
        let half = 0.014;
        let d = self.ravine.get([x, z]).abs();
        if d >= half {
            return None;
        }
        let t = 1.0 - d / half; // 0 at the lip, 1 at the crack's centre
        let depth = (16.0 + 24.0 * t) as i32;
        Some((h - depth).max(6))
    }

    pub fn generate(&self, coord: ChunkCoord) -> ChunkData {
        let mut data = ChunkData::empty(coord);
        let ox = coord.x * CHUNK_SIZE;
        let oz = coord.y * CHUNK_SIZE;
        let sea = self.sea_level;

        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let wx = ox + lx;
                let wz = oz + lz;
                let (fx, fz) = (wx as f64, wz as f64);

                let col = self.column(wx, wz);
                let h = col.height;
                let is_river = col.river_t > 0.0;
                let biome = self.biome_at(wx, wz, h);
                let snowline = self.snowline(wx, wz);
                let ravine_floor = self.ravine_floor(fx, fz, h);

                // Below this Y, caves may hollow the rock. Kept under the sea
                // floor near the coast so oceans don't leak through.
                let cave_ceiling = if h < sea + 2 {
                    (sea - 4).min(h - 3)
                } else {
                    h - 3
                };

                for y in 0..CHUNK_HEIGHT {
                    let mut block = if y == 0 {
                        Block::Bedrock
                    } else if y > h {
                        if y <= sea {
                            self.water_at(wx, wz, h, is_river)
                        } else {
                            Block::Air
                        }
                    } else if y == h {
                        let mut s = if is_river {
                            Block::Sand // river bed
                        } else {
                            let mut s = biome.surface(h, sea);
                            // Wind-scoured rock breaking through a snow cap.
                            if biome == Biome::Snow
                                && (h as f64) >= snowline + 6.0
                                && self.rock.get([fx * 0.09, fz * 0.09]) > 0.33
                            {
                                s = Block::Stone;
                            }
                            s
                        };
                        // Clay pockets in the sand of lake / river shallows and
                        // beaches (patchy).
                        if s == Block::Sand
                            && h <= sea + 1
                            && self.clay.get([fx * 0.075, fz * 0.075]) > 0.5
                        {
                            s = Block::Clay;
                        }
                        // Muddy river banks: grassy ground right beside a river,
                        // where the beach rule laid down no sand.
                        if s == Block::Grass
                            && (0.02f32..0.55).contains(&col.river_t)
                            && self.mud.get([fx * 0.11, fz * 0.11]) > 0.0
                        {
                            s = Block::Mud;
                        }
                        s
                    } else if y >= h - 3 {
                        // High ground is rocky right under the surface.
                        if h >= 84 { Block::Stone } else { biome.filler() }
                    } else {
                        Block::Stone
                    };

                    // Gravel outcrops near the surface: the bootstrap flint source.
                    if matches!(block, Block::Stone | Block::Dirt | Block::Sand)
                        && y > h - 10
                        && y <= h
                    {
                        let g = self.gravel.get([fx * 0.085, y as f64 * 0.085, fz * 0.085]);
                        if g > 0.42 {
                            block = Block::Gravel;
                        }
                    }

                    // --- Carve caves --------------------------------------
                    if y >= 4
                        && y < cave_ceiling
                        && matches!(
                            block,
                            Block::Stone | Block::Dirt | Block::Sand | Block::Gravel
                        )
                        && self.cave_carve(fx, y as f64, fz)
                    {
                        block = Block::Air;
                    }

                    // --- Carve ravines ----------------------------------
                    if let Some(floor) = ravine_floor {
                        if y > floor
                            && y <= h + 1
                            && !matches!(block, Block::Bedrock | Block::Water)
                        {
                            block = Block::Air;
                        }
                    }

                    if block != Block::Air {
                        data.set(lx, y, lz, block);
                    }
                }

                // Trees are kept fully inside the chunk to avoid cross-chunk writes.
                if biome.has_trees()
                    && !is_river
                    && (3..CHUNK_SIZE - 3).contains(&lx)
                    && (3..CHUNK_SIZE - 3).contains(&lz)
                    && h > sea
                    && data.get(lx, h, lz) == Block::Grass
                {
                    if hash2(wx, wz) < biome.tree_chance() {
                        let trunk = 4 + (hash2(wx ^ 7, wz ^ 13) * 3.0) as i32;
                        place_tree(&mut data, lx, h, lz, trunk);
                    }
                }
            }
        }

        self.stamp_structures(coord, &mut data);
        self.stamp_ruins(coord, &mut data);
        data
    }

    /// Stamps the slice of a **ruined Soviet-style city** overlapping this chunk,
    /// when the chunk's city-region rolls one. A cramped grid of near-identical
    /// concrete apartment blocks (khrushchyovka) on narrow streets, half of them
    /// collapsed — the claustrophobic early-USSR / *Samosbor* mood. Built from
    /// the new masonry blocks (cobblestone, cement, polished stone, stone brick,
    /// brick). Fully deterministic from the seed, exactly like `stamp_structures`:
    /// every client rebuilds the same city with no networking or save state.
    fn stamp_ruins(&self, coord: ChunkCoord, data: &mut ChunkData) {
        let (rx, ry) = (
            coord.x.div_euclid(CITY_REGION_CHUNKS),
            coord.y.div_euclid(CITY_REGION_CHUNKS),
        );
        let Some((ax, az, city_w, city_d)) = city_anchor(self.seed, rx, ry) else {
            return;
        };
        let ch = city_hash(self.seed, rx, ry);
        let (mcols, mrows) = city_grid(ch);

        let (cx0, cz0) = (coord.x * CHUNK_SIZE, coord.y * CHUNK_SIZE);
        // Bail unless this chunk actually overlaps the city footprint.
        if cx0 + CHUNK_SIZE <= ax
            || cx0 >= ax + city_w
            || cz0 + CHUNK_SIZE <= az
            || cz0 >= az + city_d
        {
            return;
        }

        // Reject the whole city only if its heart is in the water — each block
        // levels to its own footing, so rolling ground is fine.
        if !self.is_land(ax + city_w / 2, az + city_d / 2) {
            return;
        }

        let local = |wx: i32, wz: i32| -> Option<(i32, i32)> {
            let (lx, lz) = (wx - cx0, wz - cz0);
            ((0..CHUNK_SIZE).contains(&lx) && (0..CHUNK_SIZE).contains(&lz)).then_some((lx, lz))
        };
        let put = |data: &mut ChunkData, lx: i32, wy: i32, lz: i32, b: Block| {
            if (1..CHUNK_HEIGHT).contains(&wy) {
                data.set(lx, wy, lz, b);
            }
        };
        // Ruin noise: a cell survives when the field is *below* `keep`.
        let hole = |wx: i32, wy: i32, wz: i32, keep: f64| -> bool {
            self.ruin
                .get([wx as f64 * 0.28, wy as f64 * 0.28, wz as f64 * 0.28])
                > keep
        };

        // --- Ground cover: grass over the manzanas, gravel streets between --
        let period = MANZANA_SLOT + STREET_W;
        for wx in ax..ax + city_w {
            for wz in az..az + city_d {
                let Some((lx, lz)) = local(wx, wz) else {
                    continue;
                };
                let g = self.surface_height(wx, wz);
                if g <= self.sea_level {
                    continue; // leave open water alone
                }
                let in_manzana = (wx - ax).rem_euclid(period) < MANZANA_SLOT
                    && (wz - az).rem_euclid(period) < MANZANA_SLOT;
                let cover = if in_manzana {
                    Block::Grass
                } else if self.ruin.get([wx as f64 * 0.25, wz as f64 * 0.25]) > 0.74 {
                    Block::RadWater // irradiated puddle on the street
                } else {
                    Block::Gravel // path between the manzanas
                };
                put(data, lx, g, lz, cover);
            }
        }

        // --- Manzanas (city blocks) -----------------------------------
        let span_range = (BUILD_MAX - BUILD_MIN + 1) as u64;
        for mbc in 0..mcols {
            for mbr in 0..mrows {
                let m_x0 = ax + mbc * (MANZANA_SLOT + STREET_W);
                let m_z0 = az + mbr * (MANZANA_SLOT + STREET_W);
                if cx0 + CHUNK_SIZE <= m_x0
                    || cx0 >= m_x0 + MANZANA_SLOT
                    || cz0 + CHUNK_SIZE <= m_z0
                    || cz0 >= m_z0 + MANZANA_SLOT
                {
                    continue; // this chunk doesn't touch the manzana
                }

                let mh = mix64(ch ^ ((mbc as u64) << 16) ^ ((mbr as u64) << 32) ^ 0x0B10_C0DE);
                let per_manzana = 3 + (mh % 5) as i32; // 3..7 apartment blocks
                let sub_cols = if per_manzana <= 6 { 2 } else { SUB_MAX };
                let sub_rows = (per_manzana + sub_cols - 1) / sub_cols;

                for sc in 0..sub_cols {
                    for sr in 0..sub_rows {
                        if sr * sub_cols + sc >= per_manzana {
                            continue; // empty plot in the manzana
                        }
                        let bh =
                            mix64(mh ^ ((sc as u64) << 8) ^ ((sr as u64) << 24) ^ 0xB1D6_5EED);
                        let bw = BUILD_MIN + ((bh & 0xFF) % span_range) as i32;
                        let bd = BUILD_MIN + (((bh >> 8) & 0xFF) % span_range) as i32;
                        let bx0 = m_x0 + sc * BUILD_SLOT;
                        let bz0 = m_z0 + sr * BUILD_SLOT;
                        let bx1 = bx0 + bw - 1;
                        let bz1 = bz0 + bd - 1;
                        if cx0 + CHUNK_SIZE <= bx0
                            || cx0 > bx1
                            || cz0 + CHUNK_SIZE <= bz0
                            || cz0 > bz1
                        {
                            continue; // building clear of this chunk
                        }

                        let floors = 4 + ((bh >> 16) & 0xFF) % 9; // 4..12
                        let dmg = ((bh >> 24) & 0xFF) as f64 / 256.0;
                        let razed = ((bh >> 40) & 0xFF) < 26; // ~10 %

                        // Level each building to its own footing.
                        let bcs = [
                            self.surface_height(bx0, bz0),
                            self.surface_height(bx1, bz0),
                            self.surface_height(bx0, bz1),
                            self.surface_height(bx1, bz1),
                            self.surface_height((bx0 + bx1) / 2, (bz0 + bz1) / 2),
                        ];
                        let blo = *bcs.iter().min().unwrap();
                        let bhi = *bcs.iter().max().unwrap();
                        if blo <= self.sea_level + 1 || bhi - blo > 9 {
                            continue; // in the water, or ground too broken
                        }
                        let base_y = bhi - 1;

                        // Concrete podium down to the terrain.
                        for wx in bx0..=bx1 {
                            for wz in bz0..=bz1 {
                                let Some((lx, lz)) = local(wx, wz) else {
                                    continue;
                                };
                                let g = self.surface_height(wx, wz);
                                let mut wy = base_y;
                                while wy >= 1 && wy > g - 4 {
                                    put(data, lx, wy, lz, Block::Cobblestone);
                                    wy -= 1;
                                }
                            }
                        }

                        if razed {
                            for wx in bx0..=bx1 {
                                for wz in bz0..=bz1 {
                                    let Some((lx, lz)) = local(wx, wz) else {
                                        continue;
                                    };
                                    let n = self.ruin.get([wx as f64 * 0.4, wz as f64 * 0.4]);
                                    let pile = ((n * 0.5 + 0.5) * 3.0) as i32;
                                    for k in 0..pile {
                                        put(data, lx, base_y + k, lz, Block::Cobblestone);
                                    }
                                }
                            }
                            continue;
                        }

                        let standing =
                            ((floors as f64) * (1.0 - dmg * 0.6)).ceil().max(1.0) as i32;
                        let top = base_y + standing * CITY_FLOOR_H;

                        // Hollow the interior (clear terrain up through the high corner).
                        for wx in bx0 + 1..bx1 {
                            for wz in bz0 + 1..bz1 {
                                let Some((lx, lz)) = local(wx, wz) else {
                                    continue;
                                };
                                for wy in base_y..=(top + 1).max(bhi + 1) {
                                    put(data, lx, wy, lz, Block::Air);
                                }
                            }
                        }

                        // Perimeter walls — material graded by height, plus
                        // windows, a ground-floor doorway and shelled-out gaps.
                        for wy in base_y..=top {
                            let k = (wy - base_y) / CITY_FLOOR_H;
                            let m = ruin_material(k, standing);
                            let level = (wy - base_y).rem_euclid(CITY_FLOOR_H);
                            let up = (wy - base_y) as f64 / (top - base_y).max(1) as f64;
                            for wx in bx0..=bx1 {
                                for wz in bz0..=bz1 {
                                    if wx != bx0 && wx != bx1 && wz != bz0 && wz != bz1 {
                                        continue;
                                    }
                                    let Some((lx, lz)) = local(wx, wz) else {
                                        continue;
                                    };
                                    if wz == bz0 && wy < base_y + 3 && wx == bx0 + bw / 2 {
                                        put(data, lx, wy, lz, Block::Air); // doorway
                                        continue;
                                    }
                                    let along = if wx == bx0 || wx == bx1 {
                                        wz - bz0
                                    } else {
                                        wx - bx0
                                    };
                                    if level == 1 && wy > base_y + 1 && along % 3 == 1 {
                                        put(data, lx, wy, lz, Block::Air); // window
                                        continue;
                                    }
                                    if hole(wx, wy, wz, 0.62 - dmg * 0.35 - up * 0.15) {
                                        continue; // shelled-out masonry
                                    }
                                    put(data, lx, wy, lz, m);
                                }
                            }
                        }

                        // Cramped Soviet interior: a full grid of thin partitions
                        // (rooms `ROOM_STEP` deep), a 1-wide corridor down the long
                        // axis, and a doorway through the middle of every wall run.
                        let hall_along_x = bw >= bd;
                        let hall = if hall_along_x { bz0 + bd / 2 } else { bx0 + bw / 2 };
                        let step = ROOM_STEP + 1;
                        for k in 0..standing {
                            let fy = base_y + k * CITY_FLOOR_H;
                            let m = ruin_material(k, standing);
                            for wy in (fy + 1)..(fy + CITY_FLOOR_H) {
                                for wx in (bx0 + 1)..bx1 {
                                    for wz in (bz0 + 1)..bz1 {
                                        let gx = (wx - bx0) % step == 0;
                                        let gz = (wz - bz0) % step == 0;
                                        if !gx && !gz {
                                            continue; // inside a room
                                        }
                                        if (hall_along_x && wz == hall)
                                            || (!hall_along_x && wx == hall)
                                        {
                                            continue; // main corridor
                                        }
                                        let door = if gx {
                                            (wz - bz0) % step == step / 2
                                        } else {
                                            (wx - bx0) % step == step / 2
                                        };
                                        if door && wy - fy <= 2 {
                                            continue; // doorway
                                        }
                                        if hole(wx, wy, wz, 0.82 - dmg * 0.3) {
                                            continue;
                                        }
                                        let Some((lx, lz)) = local(wx, wz) else {
                                            continue;
                                        };
                                        put(data, lx, wy, lz, m);
                                    }
                                }
                            }
                        }

                        // Storey slabs — the ground floor is solid, upper floors
                        // cave in progressively, and the roof is always at least
                        // half gone (holes widen with damage and height).
                        for k in 0..=standing {
                            let fy = base_y + k * CITY_FLOOR_H;
                            if fy > top {
                                break;
                            }
                            let m = ruin_material((k - 1).max(0), standing);
                            let keep = if k == 0 {
                                0.98
                            } else if k == standing {
                                dmg * 0.5 // roof: about half collapsed, worse with damage
                            } else {
                                let frac = k as f64 / standing.max(1) as f64;
                                0.18 + frac * 0.5 + dmg * 0.25
                            };
                            for wx in bx0..=bx1 {
                                for wz in bz0..=bz1 {
                                    let Some((lx, lz)) = local(wx, wz) else {
                                        continue;
                                    };
                                    if k == 0 {
                                        put(data, lx, fy, lz, Block::Cement);
                                    } else if !hole(wx, fy, wz, keep) {
                                        put(data, lx, fy, lz, m);
                                    }
                                }
                            }
                        }

                        // Stairwell in an interior corner — two steps and a hole
                        // through each slab above, so every storey is reachable.
                        let (sx0, sz0) = (bx0 + 1, bz0 + 1);
                        for k in 0..standing {
                            let fy = base_y + k * CITY_FLOOR_H;
                            let m = ruin_material(k, standing);
                            for dz in 0..2 {
                                for s in 0i32..2 {
                                    if let Some((lx, lz)) = local(sx0 + s, sz0 + dz) {
                                        put(data, lx, fy + 1 + s, lz, m); // step
                                        put(data, lx, fy + 2 + s, lz, Block::Air);
                                        put(data, lx, fy + 3 + s, lz, Block::Air);
                                    }
                                }
                                for dx in 0..3 {
                                    if let Some((lx, lz)) = local(sx0 + dx, sz0 + dz) {
                                        put(data, lx, fy + CITY_FLOOR_H, lz, Block::Air);
                                    }
                                }
                            }
                        }

                        // A chest or two on the ground floor — empty for now
                        // (loot comes later). They still open like any chest.
                        let n_chests = (bh >> 50) % 3; // 0..2
                        let spots = [
                            (bx1 - 1, bz0 + 1),
                            (bx0 + 1, bz1 - 1),
                            (bx1 - 1, bz1 - 1),
                        ];
                        for &(cwx, cwz) in spots.iter().take(n_chests as usize) {
                            if let Some((lx, lz)) = local(cwx, cwz) {
                                put(data, lx, base_y + 1, lz, Block::Chest);
                                put(data, lx, base_y + 2, lz, Block::Air);
                                put(data, lx, base_y + 3, lz, Block::Air);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Stamps the slice of any library structure whose footprint overlaps this
    /// chunk. Fully deterministic from the seed, so every client (and reloads)
    /// generates the same layout without any networking or save state.
    fn stamp_structures(&self, coord: ChunkCoord, data: &mut ChunkData) {
        let lib = &*self.structures;
        if lib.is_empty() {
            return;
        }
        let (rx, ry) = (
            coord.x.div_euclid(REGION_CHUNKS),
            coord.y.div_euclid(REGION_CHUNKS),
        );
        let h = region_hash(self.seed, rx, ry);
        if h % 1000 >= STRUCT_PER_MIL {
            return;
        }
        let Some(s) = lib.pick(((h >> 12) & 0xFFFF) as f32 / 65536.0) else {
            return;
        };
        let [sx, sy, sz] = s.size;
        if sx <= 0 || sy <= 0 || sz <= 0 {
            return;
        }

        // Anchor inside the region so the footprint never crosses into another.
        let span = REGION_CHUNKS * CHUNK_SIZE;
        let rmin_x = rx * span;
        let rmin_z = ry * span;
        let ax = rmin_x + ((h >> 24) % (span - sx).max(1) as u64) as i32;
        let az = rmin_z + ((h >> 40) % (span - sz).max(1) as u64) as i32;

        // Surface + slope gate over the footprint.
        let hs = [
            self.surface_height(ax, az),
            self.surface_height(ax + sx - 1, az),
            self.surface_height(ax, az + sz - 1),
            self.surface_height(ax + sx - 1, az + sz - 1),
            self.surface_height(ax + sx / 2, az + sz / 2),
        ];
        let lo = *hs.iter().min().unwrap();
        let hi = *hs.iter().max().unwrap();
        let r = &s.spawn;
        if hi - lo > r.max_slope || lo < r.min_y || lo > r.max_y {
            return;
        }
        let ay = (lo - r.sink).max(1);

        let (cx, cz) = (coord.x * CHUNK_SIZE, coord.y * CHUNK_SIZE);
        let local = |wx: i32, wz: i32| -> Option<(i32, i32)> {
            let (lx, lz) = (wx - cx, wz - cz);
            ((0..CHUNK_SIZE).contains(&lx) && (0..CHUNK_SIZE).contains(&lz)).then_some((lx, lz))
        };
        let put = |data: &mut ChunkData, lx: i32, wy: i32, lz: i32, b: Block| {
            if (1..CHUNK_HEIGHT).contains(&wy) {
                data.set(lx, wy, lz, b);
            }
        };

        for wx in ax..ax + sx {
            for wz in az..az + sz {
                let Some((lx, lz)) = local(wx, wz) else {
                    continue;
                };
                if r.clear {
                    for wy in ay..(ay + sy) {
                        put(data, lx, wy, lz, Block::Air);
                    }
                }
                if r.fill_below {
                    let ground = self.surface_height(wx, wz);
                    let mut wy = ay - 1;
                    while wy >= 1 && wy >= ground {
                        put(data, lx, wy, lz, Block::Dirt);
                        wy -= 1;
                    }
                }
            }
        }
        for &([bx, by, bz], b) in &s.blocks {
            if b == Block::Air {
                continue;
            }
            if let Some((lx, lz)) = local(ax + bx, az + bz) {
                put(data, lx, ay + by, lz, b);
            }
        }
    }
}

/// A dry-land spawn point near the world origin for `seed`, as `[x, y, z]`
/// (`y` is a few blocks above the surface so the player drops onto it). Builds a
/// throwaway generator — cheap, called once per join. Used by the dedicated
/// server, which never builds a [`WorldGenHandle`].
pub fn land_spawn(seed: u32) -> [f32; 3] {
    let wg = WorldGen::new(seed, Arc::new(crate::structure::Library::default()));
    let (x, z) = wg.find_land(0, 0);
    [x as f32 + 0.5, wg.surface_height(x, z) as f32 + 3.0, z as f32 + 0.5]
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn place_tree(data: &mut ChunkData, x: i32, ground: i32, z: i32, trunk: i32) {
    let top = (ground + trunk).min(CHUNK_HEIGHT - 4);
    for y in (ground + 1)..=top {
        data.set(x, y, z, Block::Wood);
    }
    for dy in -1..=2 {
        let ly = top + dy;
        let radius = if dy <= 0 { 2 } else { 1 };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if dx * dx + dz * dz > radius * radius + 1 {
                    continue;
                }
                let (lx, lz) = (x + dx, z + dz);
                if (0..CHUNK_SIZE).contains(&lx)
                    && (0..CHUNK_SIZE).contains(&lz)
                    && (0..CHUNK_HEIGHT).contains(&ly)
                    && data.get(lx, ly, lz) == Block::Air
                {
                    data.set(lx, ly, lz, Block::Leaves);
                }
            }
        }
    }
}

/// Deterministic integer hash mapped to `[0, 1)`.
fn hash2(x: i32, z: i32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x1656_67b1)
        .wrapping_add((z as u32).wrapping_mul(0x2545_1d31));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h as f32) / (u32::MAX as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::{Library, SpawnRule, Structure};

    fn lib_with_one() -> Library {
        let s = Structure {
            format: crate::structure::FORMAT.into(),
            version: 1,
            name: "t".into(),
            size: [6, 5, 6],
            author: String::new(),
            notes: String::new(),
            spawn: SpawnRule {
                weight: 10.0,
                min_y: 0,
                max_y: 250,
                max_slope: 200,
                ..SpawnRule::default()
            },
            blocks: (0..6)
                .flat_map(|x| (0..6).map(move |z| ([x, 0, z], Block::Stone)))
                .chain([([2, 1, 2], Block::Wood), ([2, 2, 2], Block::WoodPlanks)])
                .collect(),
        };
        Library::build([s])
    }

    #[test]
    fn structures_stamp_without_panic_and_are_deterministic() {
        use std::sync::Arc;
        let wg = WorldGen::new(0x1234, Arc::new(lib_with_one()));
        let mut found = 0usize;
        for cx in -14..14 {
            for cy in -14..14 {
                let d = wg.generate(IVec2::new(cx, cy));
                for y in 1..CHUNK_HEIGHT {
                    for z in 0..CHUNK_SIZE {
                        for x in 0..CHUNK_SIZE {
                            if matches!(d.get(x, y, z), Block::WoodPlanks) {
                                found += 1;
                            }
                        }
                    }
                }
                // Regenerating the same chunk must be byte-identical.
                let d2 = wg.generate(IVec2::new(cx, cy));
                for y in 0..CHUNK_HEIGHT {
                    for z in 0..CHUNK_SIZE {
                        for x in 0..CHUNK_SIZE {
                            assert_eq!(d.get(x, y, z), d2.get(x, y, z));
                        }
                    }
                }
            }
        }
        assert!(found > 0, "no structure blocks generated");
    }

    #[test]
    fn ruined_city_is_deterministic_and_built_from_masonry() {
        use std::sync::Arc;
        let seed = 0xACE1_2345u32;
        let wg = WorldGen::new(seed, Arc::new(Library::default()));

        // Find a city-region that actually builds (rolled + heart on dry land).
        let mut target = None;
        'scan: for rx in -20..20 {
            for ry in -20..20 {
                if let Some((ax, az, w, d)) = city_anchor(seed, rx, ry) {
                    if wg.is_land(ax + w / 2, az + d / 2) {
                        target = Some((rx, ry));
                        break 'scan;
                    }
                }
            }
        }
        let (rx, ry) = target.expect("no buildable city region found");
        let (ax, az, cw, cd) = city_anchor(seed, rx, ry).unwrap();

        // Assemble the city's bounding box into one array so we can check
        // neighbours across chunk seams.
        let (x0, z0) = (ax - 4, az - 4);
        let (sx, sz) = (cw as usize + 8, cd as usize + 8);
        let (y0, sy) = (20usize, 110usize);
        let idx = |x: usize, y: usize, z: usize| (y * sz + z) * sx + x;
        let mut vox = vec![Block::Air; sx * sy * sz];

        for cx in x0.div_euclid(CHUNK_SIZE)..=(x0 + sx as i32).div_euclid(CHUNK_SIZE) {
            for cz in z0.div_euclid(CHUNK_SIZE)..=(z0 + sz as i32).div_euclid(CHUNK_SIZE) {
                let a = wg.generate(IVec2::new(cx, cz));
                let b = wg.generate(IVec2::new(cx, cz));
                for ly in 0..CHUNK_HEIGHT {
                    for lz in 0..CHUNK_SIZE {
                        for lx in 0..CHUNK_SIZE {
                            let blk = a.get(lx, ly, lz);
                            assert_eq!(blk, b.get(lx, ly, lz), "non-deterministic ruin");
                            let gx = cx * CHUNK_SIZE + lx - x0;
                            let gz = cz * CHUNK_SIZE + lz - z0;
                            let gy = ly - y0 as i32;
                            if (0..sx as i32).contains(&gx)
                                && (0..sz as i32).contains(&gz)
                                && (0..sy as i32).contains(&gy)
                            {
                                vox[idx(gx as usize, gy as usize, gz as usize)] = blk;
                            }
                        }
                    }
                }
            }
        }

        let is_masonry = |b: Block| {
            matches!(
                b,
                Block::Cobblestone
                    | Block::Cement
                    | Block::PolishedStone
                    | Block::StoneBrick
                    | Block::Bricks
            )
        };

        let mut masonry = 0usize;
        let mut interior = 0usize; // air cells walled in on all four sides + a floor
        for y in 1..sy - 1 {
            for z in 4..sz - 4 {
                for x in 4..sx - 4 {
                    let b = vox[idx(x, y, z)];
                    if is_masonry(b) {
                        masonry += 1;
                    }
                    if b == Block::Air
                        && is_masonry(vox[idx(x, y - 1, z)])
                        && (1..=4).any(|d| is_masonry(vox[idx(x - d, y, z)]))
                        && (1..=4).any(|d| is_masonry(vox[idx(x + d, y, z)]))
                        && (1..=4).any(|d| is_masonry(vox[idx(x, y, z - d)]))
                        && (1..=4).any(|d| is_masonry(vox[idx(x, y, z + d)]))
                    {
                        interior += 1;
                    }
                }
            }
        }
        assert!(masonry > 400, "ruined city produced too little masonry: {masonry}");
        assert!(interior > 60, "ruined buildings are not hollow: {interior} room cells");
    }
}



