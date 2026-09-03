//! Block / voxel type definitions and their basic material properties.

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum Block {
    #[default]
    Air,
    Grass,
    Dirt,
    Stone,
    Sand,
    Snow,
    Water,
    Wood,
    Leaves,
    Bedrock,
    Gravel,
    Workbench,
    Chest,
    Furnace,
    Glass,
    WoodPlanks,
    Torch,
    Farmland,
    WheatCrop,
    HandMill,
    /// Mined from `Stone`; smelts back into `Stone`.
    Cobblestone,
    /// Found in lake / river shallows; mining it drops `Item::ClayBall` ×4.
    Clay,
    /// Muddy river banks with no sand.
    Mud,
    /// 2×2 of `Stone` on a workbench → 4.
    PolishedStone,
    /// 2×2 of `PolishedStone` on a workbench → 4.
    StoneBrick,
    /// 2×2 of `Item::Brick` (smelted clay balls) → 4.
    Bricks,
    /// 2 `Clay` + 2 `Gravel` on a workbench → 4.
    Cement,
    /// Radioactive water — some rivers and ruined-city puddles. Drinking it raw
    /// spikes `Stats::radiation`.
    RadWater,
    /// Toxic water — stagnant inland pools. Drinking it raw spikes
    /// `Stats::toxicity`.
    ToxicWater,
    /// Campfire — a station that boils a bucket of contaminated water.
    Campfire,
}

/// Tile columns in the block texture atlas (see `block_atlas.rs`).
pub const ATLAS_COLS: u32 = 22;
const TILE_GRASS_SIDE: u32 = 0;
const TILE_GRASS_TOP: u32 = 1;
const TILE_GRASS_BOTTOM: u32 = 2;
const TILE_WOOD_SIDE: u32 = 3;
const TILE_WOOD_END: u32 = 4;
const TILE_LEAVES: u32 = 5;
const TILE_SAND: u32 = 6;
const TILE_STONE: u32 = 7;
const TILE_GRAVEL: u32 = 8;
const TILE_PLANKS: u32 = 9;
const TILE_PLOWED: u32 = 10;
const TILE_SNOW: u32 = 11;
const TILE_GLASS: u32 = 12;
const TILE_BEDROCK: u32 = 13;
/// Solid white — untextured blocks fall back to their vertex colour.
pub const TILE_BLANK: u32 = 14;
const TILE_COBBLESTONE: u32 = 15;
const TILE_CLAY: u32 = 16;
const TILE_MUD: u32 = 17;
const TILE_POLISHED_STONE: u32 = 18;
const TILE_STONE_BRICK: u32 = 19;
const TILE_BRICKS: u32 = 20;
const TILE_CEMENT: u32 = 21;

impl Block {
    /// Anything that is not air.
    #[inline]
    pub fn is_solid(self) -> bool {
        !matches!(self, Block::Air)
    }

    /// Has its own PNG texture in the atlas (as opposed to a flat colour).
    #[inline]
    pub fn is_textured(self) -> bool {
        matches!(
            self,
            Block::Grass
                | Block::Dirt
                | Block::Wood
                | Block::Leaves
                | Block::Sand
                | Block::Stone
                | Block::Gravel
                | Block::WoodPlanks
                | Block::Farmland
                | Block::Snow
                | Block::Glass
                | Block::Bedrock
                | Block::Cobblestone
                | Block::Clay
                | Block::Mud
                | Block::PolishedStone
                | Block::StoneBrick
                | Block::Bricks
                | Block::Cement
        )
    }

    /// Drawn as a prop (glTF model or small procedural mesh) instead of a voxel
    /// cube (`props.rs`).
    #[inline]
    pub fn renders_as_model(self) -> bool {
        matches!(
            self,
            Block::Workbench
                | Block::Chest
                | Block::Furnace
                | Block::Torch
                | Block::WheatCrop
                | Block::HandMill
                | Block::Campfire
        )
    }

    /// Water and its contaminated variants — meshed into the blended pass,
    /// passable, and treated as a drink source by `survival`.
    #[inline]
    pub fn is_waterlike(self) -> bool {
        matches!(self, Block::Water | Block::RadWater | Block::ToxicWater)
    }

    /// Atlas tile for a face, given that face's Y-normal component.
    pub fn face_tile(self, normal_y: f32) -> u32 {
        match self {
            Block::Grass => {
                if normal_y > 0.5 {
                    TILE_GRASS_TOP
                } else if normal_y < -0.5 {
                    TILE_GRASS_BOTTOM
                } else {
                    TILE_GRASS_SIDE
                }
            }
            // Bare dirt = the grass block's underside on every face.
            Block::Dirt => TILE_GRASS_BOTTOM,
            Block::Wood => {
                if normal_y.abs() > 0.5 {
                    TILE_WOOD_END
                } else {
                    TILE_WOOD_SIDE
                }
            }
            Block::Leaves => TILE_LEAVES,
            Block::Sand => TILE_SAND,
            Block::Stone => TILE_STONE,
            Block::Gravel => TILE_GRAVEL,
            Block::WoodPlanks => TILE_PLANKS,
            Block::Farmland => TILE_PLOWED,
            Block::Snow => TILE_SNOW,
            Block::Glass => TILE_GLASS,
            Block::Bedrock => TILE_BEDROCK,
            Block::Cobblestone => TILE_COBBLESTONE,
            Block::Clay => TILE_CLAY,
            Block::Mud => TILE_MUD,
            Block::PolishedStone => TILE_POLISHED_STONE,
            Block::StoneBrick => TILE_STONE_BRICK,
            Block::Bricks => TILE_BRICKS,
            Block::Cement => TILE_CEMENT,
            _ => TILE_BLANK,
        }
    }

    /// Opaque blocks completely hide the faces of the blocks behind them.
    /// Model blocks are *not* opaque so the ground/walls around them keep their
    /// faces (the glTF model covers the cell visually).
    #[inline]
    pub fn is_opaque(self) -> bool {
        !matches!(
            self,
            Block::Air
                | Block::Water
                | Block::RadWater
                | Block::ToxicWater
                | Block::Glass
                | Block::Workbench
                | Block::Chest
                | Block::Furnace
                | Block::Torch
                | Block::WheatCrop
                | Block::Campfire
        )
    }

    /// Blocks the player physically bumps into (water, torches and crops are
    /// passable).
    #[inline]
    pub fn is_collidable(self) -> bool {
        !matches!(
            self,
            Block::Air
                | Block::Water
                | Block::RadWater
                | Block::ToxicWater
                | Block::Torch
                | Block::WheatCrop
                | Block::Campfire
        )
    }

    /// Base albedo, roughly linear RGB.
    pub fn color(self) -> [f32; 3] {
        match self {
            Block::Air => [0.0, 0.0, 0.0],
            Block::Grass => [0.28, 0.62, 0.24],
            Block::Dirt => [0.42, 0.30, 0.18],
            Block::Stone => [0.44, 0.45, 0.48],
            Block::Sand => [0.80, 0.72, 0.44],
            Block::Snow => [0.90, 0.93, 0.97],
            Block::Water => [0.16, 0.34, 0.62],
            Block::Wood => [0.36, 0.25, 0.14],
            Block::Leaves => [0.20, 0.47, 0.20],
            Block::Bedrock => [0.10, 0.10, 0.12],
            Block::Gravel => [0.40, 0.39, 0.37],
            Block::Workbench => [0.52, 0.36, 0.20],
            Block::Chest => [0.55, 0.40, 0.22],
            Block::Furnace => [0.34, 0.34, 0.36],
            Block::Glass => [0.72, 0.82, 0.88],
            Block::WoodPlanks => [0.62, 0.46, 0.27],
            Block::Torch => [0.95, 0.60, 0.25],
            Block::Farmland => [0.36, 0.24, 0.15],
            Block::WheatCrop => [0.55, 0.68, 0.22],
            Block::HandMill => [0.50, 0.49, 0.47],
            Block::Cobblestone => [0.42, 0.42, 0.44],
            Block::Clay => [0.60, 0.62, 0.66],
            Block::Mud => [0.30, 0.24, 0.18],
            Block::PolishedStone => [0.50, 0.51, 0.54],
            Block::StoneBrick => [0.46, 0.47, 0.49],
            Block::Bricks => [0.62, 0.30, 0.24],
            Block::Cement => [0.66, 0.66, 0.64],
            Block::RadWater => [0.22, 0.55, 0.40],
            Block::ToxicWater => [0.34, 0.52, 0.16],
            Block::Campfire => [0.95, 0.55, 0.20],
        }
    }

    /// Per-block vertex alpha. Water and glass are translucent; both are meshed
    /// into a separate blended pass so this does not interact with the terrain's
    /// alpha-to-coverage cutout.
    pub fn alpha(self) -> f32 {
        match self {
            Block::Water => 0.5,
            Block::RadWater => 0.55,
            Block::ToxicWater => 0.6,
            Block::Glass => 0.32,
            _ => 1.0,
        }
    }

    /// Can the player mine this block?
    #[inline]
    pub fn is_breakable(self) -> bool {
        !matches!(
            self,
            Block::Air | Block::Water | Block::RadWater | Block::ToxicWater | Block::Bedrock
        )
    }

    /// The item obtained when this block is broken (usually itself).
    pub fn drop_item(self) -> Option<Block> {
        match self {
            Block::Grass => Some(Block::Dirt),
            Block::Dirt => Some(Block::Dirt),
            // Rock breaks into cobblestone; smelt it back to stone.
            Block::Stone => Some(Block::Cobblestone),
            Block::Cobblestone => Some(Block::Cobblestone),
            // Clay is special-cased in `interact::grant_drop` (drops 4 clay balls).
            Block::Clay => Some(Block::Clay),
            Block::Mud => Some(Block::Mud),
            Block::PolishedStone => Some(Block::PolishedStone),
            Block::StoneBrick => Some(Block::StoneBrick),
            Block::Bricks => Some(Block::Bricks),
            Block::Cement => Some(Block::Cement),
            Block::Campfire => Some(Block::Campfire),
            Block::Sand => Some(Block::Sand),
            Block::Snow => Some(Block::Snow),
            Block::Wood => Some(Block::Wood),
            Block::Workbench => Some(Block::Workbench),
            Block::Chest => Some(Block::Chest),
            Block::Furnace => Some(Block::Furnace),
            Block::Glass => Some(Block::Glass),
            Block::WoodPlanks => Some(Block::WoodPlanks),
            Block::Torch => Some(Block::Torch),
            Block::HandMill => Some(Block::HandMill),
            Block::Farmland => Some(Block::Dirt),
            Block::Gravel => Some(Block::Gravel), // may yield flint instead (see interact)
            // Leaves crumble away without dropping anything; a wheat crop's
            // drop depends on how grown it is (see `farming::harvest_crops`).
            Block::Leaves
            | Block::Air
            | Block::Water
            | Block::RadWater
            | Block::ToxicWater
            | Block::Bedrock
            | Block::WheatCrop => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Block::Air => "Air",
            Block::Grass => "Grass",
            Block::Dirt => "Dirt",
            Block::Stone => "Stone",
            Block::Sand => "Sand",
            Block::Snow => "Snow",
            Block::Water => "Water",
            Block::Wood => "Log",
            Block::Leaves => "Leaves",
            Block::Bedrock => "Bedrock",
            Block::Gravel => "Gravel",
            Block::Workbench => "Workbench",
            Block::Chest => "Chest",
            Block::Furnace => "Furnace",
            Block::Glass => "Glass",
            Block::WoodPlanks => "Wood Planks",
            Block::Torch => "Torch",
            Block::Farmland => "Farmland",
            Block::WheatCrop => "Wheat (crop)",
            Block::HandMill => "Hand Mill",
            Block::Cobblestone => "Cobblestone",
            Block::Clay => "Clay",
            Block::Mud => "Mud",
            Block::PolishedStone => "Polished Stone",
            Block::StoneBrick => "Stone Bricks",
            Block::Bricks => "Bricks",
            Block::Cement => "Cement",
            Block::RadWater => "Irradiated Water",
            Block::ToxicWater => "Toxic Water",
            Block::Campfire => "Campfire",
        }
    }
}
