//! Items, a 50-slot inventory (10-wide hotbar + 4 backpack rows), tool rules,
//! a cursor-held stack for moving items, and a 2×2 (3×3 near a workbench)
//! crafting grid. Backpack + crafting panel toggle with `I`.
//!
//! `container` (chests / furnace / mill / campfire) lives here too, re-exported
//! flat at the crate root by `main.rs`.

pub mod container;

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::block::Block;
use crate::container::OpenContainer;
use crate::pause::not_paused;

pub const HOTBAR: usize = 10;
pub const ROWS: usize = 5;
pub const SLOTS: usize = HOTBAR * ROWS; // 50
pub const STACK_MAX: u32 = 99;
pub const CRAFT_SLOTS: usize = 9;

// --- Items ----------------------------------------------------------------

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Item {
    Block(Block),
    Flint,
    Stick,
    Fiber,
    Rope,
    Knife,
    Axe,
    Pick,
    Shovel,
    Arrow,
    FishingRod,
    Fish,
    CookedFish,
    Charcoal,
    // Animal drops.
    Meat,        // cerdo
    Fat,         // cerdo
    RedMeat,     // vaca
    Leather,     // vaca
    Mutton,      // oveja
    Wool,        // oveja
    WhiteMeat,   // pollo
    Feather,     // pollo
    CookedMeat,      // carne de cerdo asada
    CookedRedMeat,   // carne roja (vaca) asada
    CookedMutton,    // cordero (oveja) asado
    CookedWhiteMeat, // carne blanca (pollo) asada
    Bottle,          // botella de vidrio vacía
    WaterBottle, // botella con agua
    // Farming.
    Sickle,  // hoz de pedernal — ara la tierra
    Seeds,   // de cortar plantas silvestres; siembra trigo, atrae/alimenta gallinas
    Wheat,   // cultivado en tierra arada; atrae/alimenta vacas y ovejas
    Potato,  // silvestre; atrae/alimenta cerdos
    Flour,   // trigo molido en el molino manual
    Dough,   // harina + botella de agua
    Bread,   // masa cocinada en el horno
    // Masonry.
    ClayBall, // bodoque de arcilla — de picar bloques de arcilla; se cuece en ladrillo
    Brick,    // ladrillo — bodoque de arcilla cocido; 4 → bloque de Ladrillo
    Cement,   // cemento (mortero) — clic izq. sobre un bloque de Ladrillo lo vuelve Cemento
    // Water treatment (radiation / toxicity).
    Bucket,         // balde de madera vacío
    BucketRadRaw,   // balde de agua irradiada sin tratar
    BucketToxicRaw, // balde de agua tóxica sin tratar
    BucketHot,      // balde hervido en la fogata (aún no potable)
    BucketClean,    // balde de agua potable
    PurifyingPill,  // carbón activado — al agua hervida la vuelve potable
    Vodka,          // baja la intoxicación deprisa
    AntiRad,        // medicamento — baja la radiación deprisa
    Medkit,         // botiquín — cura salud (loot de ciudades en ruinas)
    Spoiled,        // comida podrida — comerla intoxica
}

/// What a stack's `wear` counter models.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WearKind {
    None,
    /// Tool durability — `wear` climbs each use; at `max_wear` the tool breaks.
    Tool,
    /// Food rot — `wear` climbs with time; at `max_wear` it turns to `Spoiled`.
    Rot,
    /// Metal rust — `wear` climbs slowly with time. Hook for future firearms /
    /// metal gear; no item returns this yet.
    #[allow(dead_code)]
    Rust,
}

impl Item {
    /// glTF shown in the player's hand for this item, if any (`player.rs` picks
    /// the per-item transform). Everything else uses the generic textured cube.
    pub fn hand_model(self) -> Option<&'static str> {
        Some(match self {
            Item::FishingRod => "models/RudimentaryFishingrod.glb",
            Item::Knife => "models/FlintKnife.glb",
            Item::Axe => "models/FlintAxe.glb",
            Item::Pick => "models/FlintPickaxe.glb",
            Item::Shovel => "models/FlintShovel.glb",
            Item::Sickle => "models/FlintSickle.glb",
            _ => return None,
        })
    }

    /// `(hunger, thirst)` restored when eaten/drunk, if this item is consumable.
    /// `WaterBottle` also returns an empty `Bottle` — handled in `survival::consume`.
    pub fn food(self) -> Option<(f32, f32)> {
        Some(match self {
            Item::Meat | Item::RedMeat | Item::Mutton | Item::WhiteMeat => (12.0, 0.0),
            Item::CookedMeat
            | Item::CookedRedMeat
            | Item::CookedMutton
            | Item::CookedWhiteMeat => (30.0, 0.0),
            Item::Fish => (8.0, 2.0),
            Item::CookedFish => (20.0, 2.0),
            Item::WaterBottle => (0.0, 45.0),
            Item::Potato => (10.0, 0.0),
            Item::Bread => (35.0, 0.0),
            // Bucket drinks: all quench thirst; the raw / boiled ones also add
            // radiation or toxicity (handled in `survival::consume`).
            Item::BucketClean => (0.0, 60.0),
            Item::BucketHot => (0.0, 45.0),
            Item::BucketRadRaw | Item::BucketToxicRaw => (0.0, 38.0),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolKind {
    Axe,
    Pick,
    Shovel,
    Knife,
    Sickle,
}

impl Item {
    pub fn name(self) -> String {
        match self {
            Item::Block(b) => b.display_name().to_string(),
            Item::Flint => "Flint".into(),
            Item::Stick => "Stick".into(),
            Item::Fiber => "Plant Fiber".into(),
            Item::Rope => "Plant Rope".into(),
            Item::Knife => "Flint Knife".into(),
            Item::Axe => "Flint Axe".into(),
            Item::Pick => "Flint Pickaxe".into(),
            Item::Shovel => "Flint Shovel".into(),
            Item::Arrow => "Flint Arrow".into(),
            Item::FishingRod => "Crude Fishing Rod".into(),
            Item::Fish => "Fish".into(),
            Item::CookedFish => "Cooked Fish".into(),
            Item::Charcoal => "Charcoal".into(),
            Item::Meat => "Meat".into(),
            Item::Fat => "Fat".into(),
            Item::RedMeat => "Red Meat".into(),
            Item::Leather => "Leather".into(),
            Item::Mutton => "Mutton".into(),
            Item::Wool => "Wool".into(),
            Item::WhiteMeat => "White Meat".into(),
            Item::Feather => "Feather".into(),
            Item::CookedMeat => "Cooked Meat".into(),
            Item::CookedRedMeat => "Cooked Red Meat".into(),
            Item::CookedMutton => "Cooked Mutton".into(),
            Item::CookedWhiteMeat => "Cooked White Meat".into(),
            Item::Bottle => "Empty Bottle".into(),
            Item::WaterBottle => "Water Bottle".into(),
            Item::Sickle => "Flint Sickle".into(),
            Item::Seeds => "Seeds".into(),
            Item::Wheat => "Wheat".into(),
            Item::Potato => "Potato".into(),
            Item::Flour => "Flour".into(),
            Item::Dough => "Dough".into(),
            Item::Bread => "Bread".into(),
            Item::ClayBall => "Clay Ball".into(),
            Item::Brick => "Brick".into(),
            Item::Cement => "Cement".into(),
            Item::Bucket => "Wooden Bucket".into(),
            Item::BucketRadRaw => "Bucket of Irradiated Water".into(),
            Item::BucketToxicRaw => "Bucket of Toxic Water".into(),
            Item::BucketHot => "Bucket of Boiled Water".into(),
            Item::BucketClean => "Bucket of Clean Water".into(),
            Item::PurifyingPill => "Purifying Pill".into(),
            Item::Vodka => "Vodka".into(),
            Item::AntiRad => "Anti-Rad Meds".into(),
            Item::Medkit => "Medkit".into(),
            Item::Spoiled => "Rotten Food".into(),
        }
    }

    pub fn max_stack(self) -> u32 {
        match self {
            Item::Knife | Item::Axe | Item::Pick | Item::Shovel | Item::FishingRod
            | Item::Sickle => 1,
            // A filled bucket is a single unit.
            Item::BucketRadRaw
            | Item::BucketToxicRaw
            | Item::BucketHot
            | Item::BucketClean => 1,
            Item::Bucket => 4,
            _ => STACK_MAX,
        }
    }

    pub fn tool(self) -> Option<ToolKind> {
        match self {
            Item::Axe => Some(ToolKind::Axe),
            Item::Pick => Some(ToolKind::Pick),
            Item::Shovel => Some(ToolKind::Shovel),
            Item::Knife => Some(ToolKind::Knife),
            Item::Sickle => Some(ToolKind::Sickle),
            _ => None,
        }
    }

    /// Whether this food was cooked (rots much slower than raw).
    pub fn is_cooked_food(self) -> bool {
        matches!(
            self,
            Item::CookedMeat
                | Item::CookedRedMeat
                | Item::CookedMutton
                | Item::CookedWhiteMeat
                | Item::CookedFish
                | Item::Bread
        )
    }

    /// Which deterioration model a stack of this item follows.
    pub fn wear_kind(self) -> WearKind {
        match self {
            Item::Knife | Item::Axe | Item::Pick | Item::Shovel | Item::Sickle
            | Item::FishingRod => WearKind::Tool,
            Item::Meat | Item::RedMeat | Item::Mutton | Item::WhiteMeat | Item::Fish
            | Item::Potato => WearKind::Rot,
            _ if self.is_cooked_food() => WearKind::Rot,
            _ => WearKind::None,
        }
    }

    /// `wear` value at which the item is spent (tool breaks / food spoils), or
    /// `None` if it never wears.
    pub fn max_wear(self) -> Option<u16> {
        Some(match self.wear_kind() {
            WearKind::Tool => match self {
                Item::Pick => 250,
                Item::Axe => 200,
                Item::Shovel => 220,
                Item::Knife => 150,
                Item::Sickle => 140,
                Item::FishingRod => 80,
                _ => 180,
            },
            // "puntos de podredumbre" ≈ segundos de vida útil.
            WearKind::Rot => {
                if self.is_cooked_food() {
                    1000
                } else {
                    360
                }
            }
            WearKind::Rust => 600,
            WearKind::None => return None,
        })
    }

    pub fn as_block(self) -> Option<Block> {
        match self {
            Item::Block(b) => Some(b),
            _ => None,
        }
    }

    pub fn icon_color(self) -> Color {
        match self {
            Item::Block(b) => {
                let c = b.color();
                Color::srgb(c[0], c[1], c[2])
            }
            Item::Flint => Color::srgb(0.22, 0.22, 0.25),
            Item::Stick => Color::srgb(0.45, 0.31, 0.16),
            Item::Fiber => Color::srgb(0.55, 0.72, 0.35),
            Item::Rope => Color::srgb(0.78, 0.68, 0.45),
            Item::Knife => Color::srgb(0.62, 0.64, 0.68),
            Item::Axe => Color::srgb(0.50, 0.56, 0.64),
            Item::Pick => Color::srgb(0.44, 0.50, 0.60),
            Item::Shovel => Color::srgb(0.56, 0.60, 0.66),
            Item::Arrow => Color::srgb(0.82, 0.80, 0.72),
            Item::FishingRod => Color::srgb(0.60, 0.45, 0.28),
            Item::Fish => Color::srgb(0.55, 0.68, 0.78),
            Item::CookedFish => Color::srgb(0.78, 0.60, 0.42),
            Item::Charcoal => Color::srgb(0.16, 0.16, 0.18),
            Item::Meat => Color::srgb(0.86, 0.42, 0.44),
            Item::Fat => Color::srgb(0.94, 0.90, 0.80),
            Item::RedMeat => Color::srgb(0.72, 0.20, 0.22),
            Item::Leather => Color::srgb(0.55, 0.38, 0.22),
            Item::Mutton => Color::srgb(0.88, 0.55, 0.55),
            Item::Wool => Color::srgb(0.92, 0.92, 0.90),
            Item::WhiteMeat => Color::srgb(0.90, 0.80, 0.66),
            Item::Feather => Color::srgb(0.96, 0.96, 0.98),
            Item::CookedMeat => Color::srgb(0.60, 0.36, 0.22),
            Item::CookedRedMeat => Color::srgb(0.52, 0.26, 0.20),
            Item::CookedMutton => Color::srgb(0.64, 0.40, 0.30),
            Item::CookedWhiteMeat => Color::srgb(0.72, 0.52, 0.34),
            Item::Bottle => Color::srgb(0.78, 0.88, 0.90),
            Item::WaterBottle => Color::srgb(0.32, 0.62, 0.92),
            Item::Sickle => Color::srgb(0.58, 0.60, 0.64),
            Item::Seeds => Color::srgb(0.72, 0.62, 0.30),
            Item::Wheat => Color::srgb(0.87, 0.74, 0.28),
            Item::Potato => Color::srgb(0.80, 0.65, 0.38),
            Item::Flour => Color::srgb(0.95, 0.93, 0.87),
            Item::Dough => Color::srgb(0.88, 0.82, 0.65),
            Item::Bread => Color::srgb(0.76, 0.55, 0.28),
            Item::ClayBall => Color::srgb(0.58, 0.60, 0.66),
            Item::Brick => Color::srgb(0.70, 0.34, 0.26),
            Item::Cement => Color::srgb(0.66, 0.66, 0.64),
            Item::Bucket => Color::srgb(0.46, 0.34, 0.20),
            Item::BucketRadRaw => Color::srgb(0.30, 0.62, 0.46),
            Item::BucketToxicRaw => Color::srgb(0.42, 0.60, 0.18),
            Item::BucketHot => Color::srgb(0.55, 0.68, 0.80),
            Item::BucketClean => Color::srgb(0.40, 0.72, 0.95),
            Item::PurifyingPill => Color::srgb(0.90, 0.90, 0.86),
            Item::Vodka => Color::srgb(0.85, 0.88, 0.92),
            Item::AntiRad => Color::srgb(0.85, 0.70, 0.25),
            Item::Medkit => Color::srgb(0.88, 0.20, 0.20),
            Item::Spoiled => Color::srgb(0.34, 0.40, 0.20),
        }
    }

    /// Path to this item's inventory sprite, if it has one. Items without a
    /// sprite fall back to a flat [`Item::icon_color`] swatch.
    pub fn texture_path(self) -> Option<&'static str> {
        Some(match self {
            Item::Flint => "textures/items/flint.png",
            Item::Stick => "textures/items/stick.png",
            Item::Fiber => "textures/items/plant.png",
            Item::Rope => "textures/items/rope.png",
            Item::Fish => "textures/items/raw_fish.png",
            Item::CookedFish => "textures/items/cooked_fish.png",
            Item::Charcoal => "textures/items/charcoal.png",
            Item::Flour => "textures/items/flour.png",
            Item::Dough => "textures/items/dough.png",
            Item::Bread => "textures/items/bread.png",
            Item::Meat => "textures/items/meat.png",
            Item::Fat => "textures/items/fat.png",
            Item::RedMeat => "textures/items/read_meat.png",
            Item::Leather => "textures/items/leather.png",
            Item::Mutton => "textures/items/lamb_meat.png",
            Item::Wool => "textures/items/wool.png",
            Item::WhiteMeat => "textures/items/white_meat.png",
            Item::Feather => "textures/items/feather.png",
            Item::CookedMeat => "textures/items/meat-cooked.png",
            Item::CookedRedMeat => "textures/items/read_meat-cooked.png",
            Item::CookedMutton => "textures/items/lamb_meat-cooked.png",
            Item::CookedWhiteMeat => "textures/items/white_meat-cooked.png",
            Item::Bottle => "textures/items/bottle-empty.png",
            Item::WaterBottle => "textures/items/bottle-water.png",
            Item::Seeds => "textures/items/wheat_seeds.png",
            Item::Wheat => "textures/items/wheat.png",
            Item::Potato => "textures/items/potato.png",
            Item::Knife => "textures/tools/flint_knife.png",
            Item::Axe => "textures/tools/flint_axe.png",
            Item::Pick => "textures/tools/flint_pickaxe.png",
            Item::Shovel => "textures/tools/flint_shovel.png",
            Item::Sickle => "textures/tools/flint_sickle.png",
            Item::FishingRod => "textures/tools/rudimentary_fishingrod.png",
            Item::Block(Block::WoodPlanks) => "textures/blocks/wood_planks.png",
            _ => return None, // other blocks, Arrow
        })
    }

    /// Inventory sprite as `(path, optional pixel sub-rect)`. Falls back to
    /// [`Item::texture_path`] for regular items; for a placeable block it shows
    /// that block's **side face** texture (cropping the grass/wood spritesheets
    /// to their first tile). `None` = flat [`Item::icon_color`] swatch.
    pub fn inventory_sprite(self) -> Option<(&'static str, Option<Rect>)> {
        if let Some(path) = self.texture_path() {
            return Some((path, None));
        }
        match self {
            Item::Block(b) => block_side_sprite(b),
            _ => None,
        }
    }
}

/// The side-face texture used as a block's inventory icon. Grass and wood live
/// in horizontal spritesheets, so those return a 16×16 sub-rect on tile 0.
fn block_side_sprite(b: Block) -> Option<(&'static str, Option<Rect>)> {
    const GRASS: &str = "textures/blocks/grass-spritesheet.png";
    const WOOD: &str = "textures/blocks/wood-spritesheet.png";
    // nth 16px tile of a horizontal strip
    fn tile(n: f32) -> Option<Rect> {
        Some(Rect {
            min: Vec2::new(n * 16.0, 0.0),
            max: Vec2::new(n * 16.0 + 16.0, 16.0),
        })
    }
    Some(match b {
        Block::Grass => (GRASS, tile(0.0)),
        // bare dirt shows the grass block's underside on every face
        Block::Dirt => (GRASS, tile(2.0)),
        Block::Wood => (WOOD, tile(0.0)),
        Block::Leaves => ("textures/blocks/leaves.png", None),
        Block::Sand => ("textures/blocks/sand.png", None),
        Block::Stone => ("textures/blocks/stone.png", None),
        Block::Gravel => ("textures/blocks/gravel.png", None),
        Block::WoodPlanks => ("textures/blocks/wood_planks.png", None),
        Block::Farmland => ("textures/blocks/plowed_land.png", None),
        Block::Snow => ("textures/blocks/snow.png", None),
        Block::Glass => ("textures/blocks/glass.png", None),
        Block::Bedrock => ("textures/blocks/mother_rock.png", None),
        Block::Cobblestone => ("textures/blocks/cobblestone.png", None),
        Block::Clay => ("textures/blocks/clay.png", None),
        Block::Mud => ("textures/blocks/mud.png", None),
        Block::PolishedStone => ("textures/blocks/polished_stone.png", None),
        Block::StoneBrick => ("textures/blocks/stone_brick.png", None),
        Block::Bricks => ("textures/blocks/bricks.png", None),
        Block::Cement => ("textures/blocks/cement.png", None),
        _ => return None,
    })
}

/// Paints one inventory cell: a sprite when the item has one, otherwise a flat
/// colour swatch. `empty` is used when the slot holds nothing.
pub fn paint_icon(
    stack: Option<Stack>,
    server: &AssetServer,
    image: &mut ImageNode,
    bg: &mut BackgroundColor,
    empty: Color,
) {
    match stack.and_then(|s| s.item.inventory_sprite().map(|p| (s, p))) {
        Some((_, (path, rect))) => {
            *image = ImageNode::new(server.load(path));
            image.rect = rect;
            *bg = BackgroundColor(Color::NONE);
        }
        None => {
            // Invisible image; the swatch colour does the work.
            image.color = Color::NONE;
            *bg = BackgroundColor(match stack {
                Some(s) => s.item.icon_color(),
                None => empty,
            });
        }
    }
}

/// The tool that makes mining `block` *fast*. `None` = bare hands are fine.
pub fn ideal_tool(block: Block) -> Option<ToolKind> {
    match block {
        Block::Wood => Some(ToolKind::Axe),
        Block::Stone
        | Block::Cobblestone
        | Block::PolishedStone
        | Block::StoneBrick
        | Block::Bricks
        | Block::Cement => Some(ToolKind::Pick),
        Block::Dirt
        | Block::Grass
        | Block::Sand
        | Block::Snow
        | Block::Gravel
        | Block::Farmland
        | Block::Clay
        | Block::Mud => Some(ToolKind::Shovel),
        _ => None,
    }
}

/// Whether `block` can be broken at all with the currently held `tool`.
pub fn can_harvest(block: Block, tool: Option<ToolKind>) -> bool {
    match block {
        Block::Wood => tool == Some(ToolKind::Axe),
        Block::Stone
        | Block::Cobblestone
        | Block::PolishedStone
        | Block::StoneBrick
        | Block::Bricks
        | Block::Cement => tool == Some(ToolKind::Pick),
        Block::Dirt | Block::Grass | Block::Farmland => {
            matches!(tool, Some(ToolKind::Shovel) | Some(ToolKind::Pick))
        }
        _ => true,
    }
}

pub fn tool_hint(block: Block) -> &'static str {
    match block {
        Block::Wood => "needs an axe",
        Block::Stone
        | Block::Cobblestone
        | Block::PolishedStone
        | Block::StoneBrick
        | Block::Bricks
        | Block::Cement => "needs a pickaxe",
        Block::Dirt | Block::Grass | Block::Farmland => "needs a shovel or pickaxe",
        _ => "",
    }
}

// --- Crafting -----------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Station {
    Hand,
    Workbench,
}

pub enum RecipeKind {
    /// Minecraft-style shaped: `rows` (top→bottom) written trimmed of empty
    /// border rows/cols, `' '` = empty cell, other chars resolved via `key`.
    /// The shape may sit anywhere in the grid; its horizontal mirror also matches.
    Shaped {
        rows: &'static [&'static str],
        key: &'static [(char, Item)],
    },
    /// Position-independent: the occupied cells (one item each) must be exactly
    /// this multiset.
    Shapeless { items: &'static [Item] },
}

pub struct Recipe {
    pub out: (Item, u32),
    pub kind: RecipeKind,
    pub station: Station,
}

// `#` maps to the block/item named in each recipe's key.
pub const RECIPES: &[Recipe] = &[
    // Troncos (`Block::Wood`) come from felling trees; refine them into Madera
    // (`Block::WoodPlanks`), which is what the workbench and chest need.
    Recipe {
        out: (Item::Block(Block::WoodPlanks), 4),
        kind: RecipeKind::Shaped {
            rows: &["#"],
            key: &[('#', Item::Block(Block::Wood))],
        },
        station: Station::Hand,
    },
    Recipe {
        out: (Item::Stick, 4),
        kind: RecipeKind::Shaped {
            rows: &["#", "#"],
            key: &[('#', Item::Block(Block::Wood))],
        },
        station: Station::Hand,
    },
    Recipe {
        out: (Item::Block(Block::Workbench), 1),
        kind: RecipeKind::Shaped {
            rows: &["##", "##"],
            key: &[('#', Item::Block(Block::WoodPlanks))],
        },
        station: Station::Hand,
    },
    Recipe {
        // Two panes of glass, one above the other → bottles for water.
        out: (Item::Bottle, 2),
        kind: RecipeKind::Shaped {
            rows: &["#", "#"],
            key: &[('#', Item::Block(Block::Glass))],
        },
        station: Station::Hand,
    },
    Recipe {
        out: (Item::Rope, 1),
        kind: RecipeKind::Shapeless {
            items: &[Item::Fiber, Item::Fiber, Item::Fiber],
        },
        station: Station::Hand,
    },
    Recipe {
        out: (Item::Block(Block::Torch), 4),
        kind: RecipeKind::Shapeless {
            items: &[Item::Stick, Item::Rope, Item::Charcoal],
        },
        station: Station::Hand,
    },
    Recipe {
        out: (Item::Knife, 1),
        kind: RecipeKind::Shaped {
            rows: &["f", "s"],
            key: &[('f', Item::Flint), ('s', Item::Stick)],
        },
        station: Station::Hand,
    },
    Recipe {
        out: (Item::Arrow, 4),
        kind: RecipeKind::Shaped {
            rows: &["fs"],
            key: &[('f', Item::Flint), ('s', Item::Stick)],
        },
        station: Station::Hand,
    },
    Recipe {
        // Only tool craftable in the 2×2 grid.
        out: (Item::Axe, 1),
        kind: RecipeKind::Shaped {
            rows: &["ff", "sf"],
            key: &[('f', Item::Flint), ('s', Item::Stick)],
        },
        station: Station::Hand,
    },
    // --- Workbench (3×3) ---
    Recipe {
        out: (Item::Pick, 1),
        kind: RecipeKind::Shaped {
            rows: &["fff", " s ", " s "],
            key: &[('f', Item::Flint), ('s', Item::Stick)],
        },
        station: Station::Workbench,
    },
    Recipe {
        out: (Item::Shovel, 1),
        kind: RecipeKind::Shaped {
            rows: &["f", "s", "s"],
            key: &[('f', Item::Flint), ('s', Item::Stick)],
        },
        station: Station::Workbench,
    },
    Recipe {
        out: (Item::FishingRod, 1),
        kind: RecipeKind::Shaped {
            rows: &["  s", " s ", "rf "],
            key: &[
                ('s', Item::Stick),
                ('r', Item::Rope),
                ('f', Item::Fiber),
            ],
        },
        station: Station::Workbench,
    },
    Recipe {
        out: (Item::Block(Block::Chest), 1),
        kind: RecipeKind::Shaped {
            rows: &["###", "# #", "###"],
            key: &[('#', Item::Block(Block::WoodPlanks))],
        },
        station: Station::Workbench,
    },
    Recipe {
        out: (Item::Block(Block::Furnace), 1),
        kind: RecipeKind::Shaped {
            rows: &["###", "# #", "###"],
            key: &[('#', Item::Block(Block::Cobblestone))],
        },
        station: Station::Workbench,
    },
    Recipe {
        out: (Item::Sickle, 1),
        kind: RecipeKind::Shaped {
            rows: &["ff", " s", " s"],
            key: &[('f', Item::Flint), ('s', Item::Stick)],
        },
        station: Station::Workbench,
    },
    Recipe {
        // A ring of stone around a stick handle/crank.
        out: (Item::Block(Block::HandMill), 1),
        kind: RecipeKind::Shaped {
            rows: &["###", "#s#", "###"],
            key: &[('#', Item::Block(Block::Cobblestone)), ('s', Item::Stick)],
        },
        station: Station::Workbench,
    },
    Recipe {
        // Flour mixed with a bottle of water.
        out: (Item::Dough, 1),
        kind: RecipeKind::Shapeless {
            items: &[Item::Flour, Item::WaterBottle],
        },
        station: Station::Hand,
    },
    // --- Masonry ---
    Recipe {
        // 4 clay balls pack back into a clay block.
        out: (Item::Block(Block::Clay), 1),
        kind: RecipeKind::Shaped {
            rows: &["##", "##"],
            key: &[('#', Item::ClayBall)],
        },
        station: Station::Hand,
    },
    Recipe {
        // 4 fired bricks → 4 brick blocks.
        out: (Item::Block(Block::Bricks), 4),
        kind: RecipeKind::Shaped {
            rows: &["##", "##"],
            key: &[('#', Item::Brick)],
        },
        station: Station::Hand,
    },
    Recipe {
        // 2×2 rock → 4 polished stone.
        out: (Item::Block(Block::PolishedStone), 4),
        kind: RecipeKind::Shaped {
            rows: &["##", "##"],
            key: &[('#', Item::Block(Block::Stone))],
        },
        station: Station::Workbench,
    },
    Recipe {
        // 2×2 polished stone → 4 stone bricks.
        out: (Item::Block(Block::StoneBrick), 4),
        kind: RecipeKind::Shaped {
            rows: &["##", "##"],
            key: &[('#', Item::Block(Block::PolishedStone))],
        },
        station: Station::Workbench,
    },
    Recipe {
        // 2 clay + 2 gravel, mixed → 4 cement (mortar). Left-click a Brick block
        // with it in hand to set it into a Cement block.
        out: (Item::Cement, 4),
        kind: RecipeKind::Shapeless {
            items: &[
                Item::Block(Block::Clay),
                Item::Block(Block::Clay),
                Item::Block(Block::Gravel),
                Item::Block(Block::Gravel),
            ],
        },
        station: Station::Workbench,
    },
    // --- Water treatment / medicine ---
    Recipe {
        // Wooden bucket: an L of three planks.
        out: (Item::Bucket, 1),
        kind: RecipeKind::Shaped {
            rows: &["##", "# "],
            key: &[('#', Item::Block(Block::WoodPlanks))],
        },
        station: Station::Hand,
    },
    Recipe {
        // Campfire: two logs over a couple of stones.
        out: (Item::Block(Block::Campfire), 1),
        kind: RecipeKind::Shaped {
            rows: &["ss", "##"],
            key: &[('s', Item::Stick), ('#', Item::Block(Block::Stone))],
        },
        station: Station::Hand,
    },
    Recipe {
        // Charcoal pressed into activated-carbon pills.
        out: (Item::PurifyingPill, 2),
        kind: RecipeKind::Shapeless {
            items: &[Item::Charcoal, Item::Charcoal],
        },
        station: Station::Workbench,
    },
    Recipe {
        // Homebrew: fermented potatoes.
        out: (Item::Vodka, 1),
        kind: RecipeKind::Shapeless {
            items: &[Item::Potato, Item::Potato, Item::Potato],
        },
        station: Station::Workbench,
    },
    Recipe {
        // Drop a purifying pill into the boiled bucket → drinkable water.
        out: (Item::BucketClean, 1),
        kind: RecipeKind::Shapeless {
            items: &[Item::BucketHot, Item::PurifyingPill],
        },
        station: Station::Hand,
    },
];

/// The grid as a `dim×dim` array of items (counts ignored).
fn grid_items(craft: &[Option<Stack>], dim: usize) -> Vec<Option<Item>> {
    (0..dim * dim)
        .map(|i| craft.get(i).copied().flatten().map(|s| s.item))
        .collect()
}

/// `(min_row, min_col, rows, cols)` of the occupied region, or `None` if empty.
fn content_bbox(cells: &[Option<Item>], dim: usize) -> Option<(usize, usize, usize, usize)> {
    let (mut min_r, mut max_r, mut min_c, mut max_c) = (dim, 0usize, dim, 0usize);
    let mut any = false;
    for r in 0..dim {
        for c in 0..dim {
            if cells[r * dim + c].is_some() {
                any = true;
                min_r = min_r.min(r);
                max_r = max_r.max(r);
                min_c = min_c.min(c);
                max_c = max_c.max(c);
            }
        }
    }
    any.then_some((min_r, min_c, max_r - min_r + 1, max_c - min_c + 1))
}

fn resolve(ch: char, key: &[(char, Item)]) -> Option<Item> {
    (ch != ' ')
        .then(|| key.iter().find(|(k, _)| *k == ch).map(|(_, it)| *it))
        .flatten()
}

fn shaped_matches(cells: &[Option<Item>], dim: usize, rows: &[&str], key: &[(char, Item)]) -> bool {
    let Some((base_r, base_c, crows, ccols)) = content_bbox(cells, dim) else {
        return false;
    };
    let prows = rows.len();
    let pcols = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    if crows != prows || ccols != pcols {
        return false;
    }
    let pat = |r: usize, c: usize| rows[r].chars().nth(c).and_then(|ch| resolve(ch, key));

    [false, true].iter().any(|&mirror| {
        (0..prows).all(|r| {
            (0..pcols).all(|c| {
                let pc = if mirror { pcols - 1 - c } else { c };
                pat(r, pc) == cells[(base_r + r) * dim + (base_c + c)]
            })
        })
    })
}

fn shapeless_matches(cells: &[Option<Item>], items: &[Item]) -> bool {
    let mut grid: Vec<Item> = cells.iter().flatten().copied().collect();
    if grid.len() != items.len() {
        return false;
    }
    for &want in items {
        match grid.iter().position(|&g| g == want) {
            Some(i) => {
                grid.swap_remove(i);
            }
            None => return false,
        }
    }
    grid.is_empty()
}

fn matching_recipe(craft: &[Option<Stack>], dim: usize, near_workbench: bool) -> Option<&'static Recipe> {
    let cells = grid_items(craft, dim);
    if cells.iter().all(Option::is_none) {
        return None;
    }
    RECIPES.iter().find(|recipe| {
        let station_ok = match recipe.station {
            Station::Hand => true,
            Station::Workbench => near_workbench,
        };
        station_ok
            && match &recipe.kind {
                RecipeKind::Shaped { rows, key } => shaped_matches(&cells, dim, rows, key),
                RecipeKind::Shapeless { items } => shapeless_matches(&cells, items),
            }
    })
}

// --- Inventory --------------------------------------------------------

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Stack {
    pub item: Item,
    pub count: u32,
    /// Generic deterioration counter, meaning set by `Item::wear_kind`:
    /// tool durability spent, food rot, or (later) rust. `0` = pristine / fresh.
    /// The item is used up / spoiled at `Item::max_wear`.
    #[serde(default)]
    pub wear: u16,
}

impl Stack {
    pub fn new(item: Item, count: u32) -> Self {
        Stack { item, count, wear: 0 }
    }

    /// 0..1, how worn this stack is (0 = pristine). `1.0` if it has no wear model.
    pub fn wear_frac(&self) -> f32 {
        match self.item.max_wear() {
            Some(m) if m > 0 => (self.wear as f32 / m as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

#[derive(Resource)]
pub struct Inventory {
    pub slots: Vec<Option<Stack>>,
    pub selected: usize,
    /// The stack currently "on the cursor" (being moved).
    pub carried: Option<Stack>,
    pub craft: [Option<Stack>; CRAFT_SLOTS],
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; SLOTS],
            selected: 0,
            carried: None,
            craft: [None; CRAFT_SLOTS],
        }
    }
}

impl Inventory {
    pub fn selected_item(&self) -> Option<Item> {
        self.slots[self.selected].map(|s| s.item)
    }

    pub fn count(&self, item: Item) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.item == item)
            .map(|s| s.count)
            .sum()
    }

    /// Adds *fresh* items, topping up existing pristine stacks then filling empty
    /// slots. Won't merge onto a worn/rotting stack (keeps its `wear` honest).
    pub fn add(&mut self, item: Item, mut n: u32) {
        let max = item.max_stack();
        for slot in self.slots.iter_mut() {
            if n == 0 {
                return;
            }
            if let Some(st) = slot {
                if st.item == item && st.count < max && st.wear == 0 {
                    let take = (max - st.count).min(n);
                    st.count += take;
                    n -= take;
                }
            }
        }
        for slot in self.slots.iter_mut() {
            if n == 0 {
                return;
            }
            if slot.is_none() {
                let take = n.min(max);
                *slot = Some(Stack::new(item, take));
                n -= take;
            }
        }
    }

    /// Adds `amount` of wear to the selected slot's item; empties the slot if it
    /// breaks. Returns `true` if it just broke.
    pub fn wear_selected(&mut self, amount: u16) -> bool {
        let Some(st) = &mut self.slots[self.selected] else {
            return false;
        };
        let Some(max) = st.item.max_wear() else {
            return false;
        };
        st.wear = st.wear.saturating_add(amount);
        if st.wear >= max {
            self.slots[self.selected] = None;
            return true;
        }
        false
    }

    pub fn take(&mut self, item: Item, mut n: u32) -> u32 {
        let mut removed = 0;
        for slot in self.slots.iter_mut() {
            if n == 0 {
                break;
            }
            if let Some(st) = slot {
                if st.item == item {
                    let take = st.count.min(n);
                    st.count -= take;
                    n -= take;
                    removed += take;
                    if st.count == 0 {
                        *slot = None;
                    }
                }
            }
        }
        removed
    }

    /// Puts a whole stack back, keeping its `wear` (worn/rotting stays worn).
    pub fn add_stack(&mut self, s: Stack) {
        if s.wear == 0 {
            self.add(s.item, s.count);
            return;
        }
        if let Some(slot) = self.slots.iter_mut().find(|x| x.is_none()) {
            *slot = Some(s);
        } else if let Some(st) = self.slots.iter_mut().flatten().find(|st| st.item == s.item) {
            st.wear = blend_wear(st.wear, st.count, s.wear, s.count);
            st.count += s.count;
        }
        // else: inventory full — overflow is dropped, same as `add`.
    }

    pub fn return_carried(&mut self) {
        if let Some(c) = self.carried.take() {
            self.add_stack(c);
        }
    }

    fn return_grid_from(&mut self, from: usize) {
        for i in from..CRAFT_SLOTS {
            if let Some(s) = self.craft[i].take() {
                self.add_stack(s);
            }
        }
    }

    /// Empties the cursor and the whole crafting grid back into the pockets.
    pub fn stow_all(&mut self) {
        self.return_carried();
        self.return_grid_from(0);
    }

    /// Shaped/shapeless alike: consume exactly one item from every occupied cell
    /// of the `dim×dim` region, then drop the result on the cursor. Water bottles
    /// used as an ingredient come back empty (e.g. flour + water → dough).
    fn do_craft(&mut self, recipe: &Recipe, dim: usize) {
        let mut emptied_bottles = 0u32;
        for slot in self.craft[..dim * dim].iter_mut() {
            if let Some(st) = slot {
                if st.item == Item::WaterBottle {
                    emptied_bottles += 1;
                }
                st.count -= 1;
                if st.count == 0 {
                    *slot = None;
                }
            }
        }
        if emptied_bottles > 0 {
            self.add(Item::Bottle, emptied_bottles);
        }
        let (item, amount) = recipe.out;
        match &mut self.carried {
            Some(c) if c.item == item => c.count += amount,
            None => self.carried = Some(Stack::new(item, amount)),
            Some(_) => {}
        }
    }
}

/// Weighted-average two `wear` values so merging stacks keeps rot / durability
/// honest (the merged food ages at the blend of both).
fn blend_wear(a: u16, an: u32, b: u16, bn: u32) -> u16 {
    let total = an + bn;
    if total == 0 {
        return a;
    }
    ((a as u32 * an + b as u32 * bn) / total) as u16
}

/// One click's worth of item juggling between a slot and the cursor stack.
pub fn stack_click(target: &mut Option<Stack>, carried: &mut Option<Stack>, left: bool) {
    if left {
        match (carried.take(), target.take()) {
            (None, None) => {}
            (None, Some(t)) => *carried = Some(t),
            (Some(c), None) => *target = Some(c),
            (Some(mut c), Some(mut t)) if c.item == t.item => {
                let room = c.item.max_stack().saturating_sub(t.count);
                let moved = room.min(c.count);
                if moved > 0 {
                    t.wear = blend_wear(t.wear, t.count, c.wear, moved);
                }
                t.count += moved;
                c.count -= moved;
                *target = Some(t);
                *carried = (c.count > 0).then_some(c);
            }
            (Some(c), Some(t)) => {
                *target = Some(c);
                *carried = Some(t);
            }
        }
    } else {
        match (*carried, target.take()) {
            (Some(mut c), None) => {
                *target = Some(Stack { item: c.item, count: 1, wear: c.wear });
                c.count -= 1;
                *carried = (c.count > 0).then_some(c);
            }
            (Some(mut c), Some(mut t)) if t.item == c.item && t.count < c.item.max_stack() => {
                t.wear = blend_wear(t.wear, t.count, c.wear, 1);
                t.count += 1;
                c.count -= 1;
                *target = Some(t);
                *carried = (c.count > 0).then_some(c);
            }
            (Some(_), Some(t)) => *target = Some(t),
            (None, Some(t)) => {
                let half = t.count.div_ceil(2);
                *carried = Some(Stack { item: t.item, count: half, wear: t.wear });
                let rem = t.count - half;
                *target =
                    (rem > 0).then_some(Stack { item: t.item, count: rem, wear: t.wear });
            }
            (None, None) => {}
        }
    }
}

#[derive(Resource, Default)]
pub struct InventoryOpen(pub bool);

/// Set by `station.rs` when the crafting panel was opened *at a workbench*
/// (unlocks the 3×3 grid). Auto-cleared when the panel closes.
#[derive(Resource, Default)]
pub struct AtWorkbench(pub bool);

#[derive(Resource)]
pub struct CraftState {
    pub near_workbench: bool,
    pub dim: usize,
    result: Option<(Item, u32)>,
    recipe: Option<&'static Recipe>,
}

impl Default for CraftState {
    fn default() -> Self {
        Self {
            near_workbench: false,
            dim: 2,
            result: None,
            recipe: None,
        }
    }
}

// --- Plugin ----------------------------------------------------------

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inventory>()
            .init_resource::<InventoryOpen>()
            .init_resource::<AtWorkbench>()
            .init_resource::<CraftState>()
            .add_systems(
                Startup,
                (spawn_hotbar, spawn_inventory_panel, spawn_overlays),
            )
            .add_systems(
                Update,
                (
                    hotbar_select,
                    hotbar_scroll,
                    toggle_inventory,
                    update_crafting,
                    inventory_click,
                    tick_spoilage,
                )
                    .chain()
                    .run_if(not_paused),
            )
            .add_systems(
                Update,
                (
                    update_hotbar,
                    paint_slots,
                    paint_counts,
                    update_craft_ui,
                    update_carried_icon,
                    inventory_tooltip,
                    hotbar_name_popup,
                    sync_inventory_visibility,
                    clear_at_workbench,
                ),
            );
    }
}

/// Ages perishable stacks in the player's inventory once a second: `Rot` items
/// climb toward `max_wear` and turn into `Spoiled`; `Rust` items climb slowly
/// (hook for future metal gear). Chests / furnaces don't preserve — food kept
/// in them still rots.
fn tick_spoilage(time: Res<Time>, mut inv: ResMut<Inventory>, mut acc: Local<f32>) {
    *acc += time.delta_secs();
    if *acc < 1.0 {
        return;
    }
    let steps = *acc as u16;
    *acc -= steps as f32;

    // Destructure so the three field borrows are provably disjoint.
    let Inventory {
        slots,
        craft,
        carried,
        ..
    } = &mut *inv;
    for slot in slots
        .iter_mut()
        .chain(craft.iter_mut())
        .chain(std::iter::once(carried))
    {
        let Some(st) = slot else {
            continue;
        };
        let Some(max) = st.item.max_wear() else {
            continue;
        };
        match st.item.wear_kind() {
            WearKind::Rot => {
                st.wear = st.wear.saturating_add(steps);
                if st.wear >= max {
                    *slot = Some(Stack::new(Item::Spoiled, st.count));
                }
            }
            WearKind::Rust => {
                st.wear = st.wear.saturating_add(steps).min(max);
            }
            _ => {}
        }
    }
}

const KEY_ROW: [KeyCode; HOTBAR] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];
const KEY_NUMPAD: [KeyCode; HOTBAR] = [
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
    KeyCode::Numpad0,
];

fn hotbar_select(keys: Res<ButtonInput<KeyCode>>, mut inventory: ResMut<Inventory>) {
    for i in 0..HOTBAR {
        if keys.just_pressed(KEY_ROW[i]) || keys.just_pressed(KEY_NUMPAD[i]) {
            inventory.selected = i;
        }
    }
}

/// Plain mouse wheel cycles the selected slot (Ctrl+wheel is camera zoom).
fn hotbar_scroll(
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: MessageReader<MouseWheel>,
    mut inventory: ResMut<Inventory>,
) {
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        wheel.clear();
        return;
    }
    let scroll: f32 = wheel.read().map(|e| e.y).sum();
    if scroll.abs() < 0.01 {
        return;
    }
    let step = if scroll > 0.0 { -1 } else { 1 };
    inventory.selected = (inventory.selected as i32 + step).rem_euclid(HOTBAR as i32) as usize;
}

fn toggle_inventory(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::keybinds::Keybinds>,
    mut open: ResMut<InventoryOpen>,
    mut container: ResMut<OpenContainer>,
    mut inventory: ResMut<Inventory>,
) {
    if !binds.just_pressed(&keys, crate::keybinds::Action::Inventory) {
        return;
    }
    if container.0.is_some() {
        container.0 = None;
        inventory.return_carried();
        return;
    }
    open.0 = !open.0;
    if !open.0 {
        inventory.stow_all();
    }
}

/// `InventoryRoot` visibility is derived from the resource so any system can
/// close the panel by clearing `InventoryOpen`.
fn sync_inventory_visibility(
    open: Res<InventoryOpen>,
    mut root: Query<&mut Visibility, With<InventoryRoot>>,
) {
    if let Ok(mut vis) = root.single_mut() {
        *vis = if open.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn update_crafting(
    open: Res<InventoryOpen>,
    at_workbench: Res<AtWorkbench>,
    mut inventory: ResMut<Inventory>,
    mut craft_state: ResMut<CraftState>,
) {
    if !open.0 {
        craft_state.near_workbench = false;
        craft_state.dim = 2;
        craft_state.result = None;
        craft_state.recipe = None;
        return;
    }
    let bench = at_workbench.0;
    craft_state.near_workbench = bench;
    craft_state.dim = if bench { 3 } else { 2 };
    if !bench {
        inventory.return_grid_from(4);
    }
    let recipe = matching_recipe(&inventory.craft, craft_state.dim, bench);
    craft_state.result = recipe.map(|r| r.out);
    craft_state.recipe = recipe;
}

/// `AtWorkbench` only makes sense while the panel is open.
fn clear_at_workbench(open: Res<InventoryOpen>, mut at_workbench: ResMut<AtWorkbench>) {
    if !open.0 && at_workbench.0 {
        at_workbench.0 = false;
    }
}

fn inventory_click(
    open: Res<InventoryOpen>,
    container: Res<OpenContainer>,
    mouse: Res<ButtonInput<MouseButton>>,
    craft_state: Res<CraftState>,
    mut inventory: ResMut<Inventory>,
    slots: Query<(&SlotKind, &Interaction)>,
) {
    // Also active while a chest/furnace panel is up, for its backpack grid.
    if !open.0 && container.0.is_none() {
        return;
    }
    let left = mouse.just_pressed(MouseButton::Left);
    let right = mouse.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    let Some(kind) = slots
        .iter()
        .find(|(_, i)| matches!(**i, Interaction::Hovered | Interaction::Pressed))
        .map(|(k, _)| *k)
    else {
        return;
    };

    let n = craft_state.dim * craft_state.dim;
    let dim = craft_state.dim;
    let inv = inventory.as_mut();
    match kind {
        SlotKind::Result => {
            if !left {
                return;
            }
            let (Some((out_item, _)), Some(recipe)) = (craft_state.result, craft_state.recipe) else {
                return;
            };
            let can = match inv.carried {
                None => true,
                Some(c) => c.item == out_item,
            };
            if can {
                inv.do_craft(recipe, dim);
            }
        }
        SlotKind::Backpack(i) => stack_click(&mut inv.slots[i], &mut inv.carried, left),
        SlotKind::Craft(i) => {
            if i < n {
                stack_click(&mut inv.craft[i], &mut inv.carried, left);
            }
        }
        SlotKind::Container(_) => {} // handled in `container.rs`
    }
}

// --- Hotbar UI -----------------------------------------------------

#[derive(Component)]
struct HotbarCell(usize);
#[derive(Component)]
struct HotbarSwatch(usize);
#[derive(Component)]
struct HotbarCount(usize);

/// Root of the hotbar row — hidden while the structure editor is open.
#[derive(Component)]
pub struct HotbarRoot;

fn spawn_hotbar(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            HotbarRoot,
        ))
        .with_children(|bar| {
            bar.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|row| {
                for i in 0..HOTBAR {
                    row.spawn((
                        Node {
                            width: Val::Px(44.0),
                            height: Val::Px(48.0),
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(2.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.08, 0.08, 0.08, 0.7)),
                        HotbarCell(i),
                    ))
                    .with_children(|cell| {
                        cell.spawn((
                            Text::new(format!("{}", (i + 1) % 10)),
                            TextFont::from_font_size(10.0),
                            TextColor(Color::srgb(0.6, 0.6, 0.6)),
                        ));
                        cell.spawn((
                            Node {
                                width: Val::Px(24.0),
                                height: Val::Px(24.0),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            ImageNode { color: Color::NONE, ..default() },
                            HotbarSwatch(i),
                        ));
                        cell.spawn((
                            Text::new(""),
                            TextFont::from_font_size(12.0),
                            TextColor(Color::WHITE),
                            HotbarCount(i),
                        ));
                    });
                }
            });
        });
}

fn update_hotbar(
    inventory: Res<Inventory>,
    server: Res<AssetServer>,
    mut cells: Query<(&HotbarCell, &mut BackgroundColor), Without<HotbarSwatch>>,
    mut swatches: Query<(&HotbarSwatch, &mut BackgroundColor, &mut ImageNode), Without<HotbarCell>>,
    mut counts: Query<(&HotbarCount, &mut Text)>,
) {
    for (cell, mut bg) in &mut cells {
        *bg = if cell.0 == inventory.selected {
            BackgroundColor(Color::srgba(0.95, 0.85, 0.30, 0.9))
        } else {
            BackgroundColor(Color::srgba(0.08, 0.08, 0.08, 0.7))
        };
    }
    for (swatch, mut bg, mut image) in &mut swatches {
        paint_icon(
            inventory.slots.get(swatch.0).copied().flatten(),
            &server,
            &mut image,
            &mut bg,
            Color::NONE,
        );
    }
    for (count, mut text) in &mut counts {
        text.0 = stack_count_text(inventory.slots.get(count.0).copied().flatten());
    }
}

// --- Backpack + crafting panel ------------------------------------

pub const EMPTY_SLOT_COLOR: Color = Color::srgba(0.18, 0.18, 0.20, 0.9);
pub const CELL_PX: f32 = 30.0;
pub const CELL_STRIDE: f32 = 34.0;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Backpack(usize),
    Craft(usize),
    Result,
    /// Slot `i` of whatever container (chest / furnace) is currently open.
    Container(usize),
}

#[derive(Component)]
pub struct SlotCount(pub SlotKind);
#[derive(Component)]
struct InventoryRoot;
#[derive(Component)]
struct CraftGrid;
#[derive(Component)]
struct CraftHint;

/// One of the six body parts in the inventory's Fallout-style paper doll.
/// Index order: 0 head · 1 torso · 2 left arm · 3 right arm · 4 left leg · 5 right
/// leg. `survival::update_paper_doll` tints these from `Stats::limbs`.
#[derive(Component)]
pub struct LimbNode(pub usize);
#[derive(Component)]
pub struct LimbText(pub usize);

fn limb(
    doll: &mut ChildSpawnerCommands,
    i: usize,
    label: &str,
    (l, t, w, h): (f32, f32, f32, f32),
) {
    doll.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(l),
            top: Val::Px(t),
            width: Val::Px(w),
            height: Val::Px(h),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.2, 0.7, 0.25)),
        LimbNode(i),
    ))
    .with_children(|c| {
        c.spawn((
            Text::new(label.to_string()),
            TextFont::from_font_size(10.0),
            TextColor(Color::srgb(0.05, 0.05, 0.05)),
            LimbText(i),
        ));
    });
}

pub fn cell(commands: &mut ChildSpawnerCommands, kind: SlotKind) {
    commands
        .spawn((
            Button,
            Node {
                width: Val::Px(CELL_PX),
                height: Val::Px(CELL_PX),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            BackgroundColor(EMPTY_SLOT_COLOR),
            ImageNode { color: Color::NONE, ..default() },
            kind,
        ))
        .with_children(|c| {
            c.spawn((
                Text::new(""),
                TextFont::from_font_size(10.0),
                TextColor(Color::WHITE),
                SlotCount(kind),
            ));
        });
}

fn spawn_inventory_panel(mut commands: Commands) {
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
            InventoryRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(16.0),
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.10, 0.13, 0.97)),
            ))
            .with_children(|panel| {
                // Crafting row: grid + arrow + result.
                panel.spawn((
                    Text::new("Cuadricula 2x2"),
                    TextFont::from_font_size(12.0),
                    TextColor(Color::srgb(0.8, 0.8, 0.8)),
                    CraftHint,
                ));
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: Val::Px(2.0 * CELL_STRIDE),
                                flex_wrap: FlexWrap::Wrap,
                                flex_direction: FlexDirection::Row,
                                row_gap: Val::Px(4.0),
                                column_gap: Val::Px(4.0),
                                ..default()
                            },
                            CraftGrid,
                        ))
                        .with_children(|grid| {
                            for i in 0..CRAFT_SLOTS {
                                cell(grid, SlotKind::Craft(i));
                            }
                        });
                        row.spawn((
                            Text::new("->"),
                            TextFont::from_font_size(18.0),
                            TextColor(Color::WHITE),
                        ));
                        cell(row, SlotKind::Result);

                        // Fallout-style paper doll: 6 coloured boxes laid out
                        // like a body, tinted by limb health (`survival.rs`).
                        row.spawn(Node {
                            width: Val::Px(170.0),
                            height: Val::Px(192.0),
                            margin: UiRect::left(Val::Px(24.0)),
                            ..default()
                        })
                        .with_children(|doll| {
                            limb(doll, 0, "HEAD", (65.0, 0.0, 40.0, 34.0));
                            limb(doll, 2, "L", (8.0, 42.0, 34.0, 66.0));
                            limb(doll, 1, "TORSO", (50.0, 38.0, 70.0, 74.0));
                            limb(doll, 3, "R", (128.0, 42.0, 34.0, 66.0));
                            limb(doll, 4, "L", (54.0, 116.0, 28.0, 74.0));
                            limb(doll, 5, "R", (88.0, 116.0, 28.0, 74.0));
                        });
                    });

                // Backpack grid.
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

fn slot_stack(inventory: &Inventory, craft_state: &CraftState, kind: SlotKind) -> Option<Stack> {
    match kind {
        SlotKind::Backpack(i) => inventory.slots.get(i).copied().flatten(),
        SlotKind::Craft(i) => inventory.craft.get(i).copied().flatten(),
        SlotKind::Result => craft_state.result.map(|(item, count)| Stack::new(item, count)),
        SlotKind::Container(_) => None, // painted by `container.rs`
    }
}

pub fn stack_count_text(stack: Option<Stack>) -> String {
    match stack {
        Some(s) if s.count > 1 => s.count.to_string(),
        _ => String::new(),
    }
}

fn paint_slots(
    inventory: Res<Inventory>,
    craft_state: Res<CraftState>,
    server: Res<AssetServer>,
    mut cells: Query<(&SlotKind, &mut BackgroundColor, &mut ImageNode)>,
) {
    for (kind, mut bg, mut image) in &mut cells {
        if matches!(kind, SlotKind::Container(_)) {
            continue; // `container.rs` owns these
        }
        paint_icon(
            slot_stack(&inventory, &craft_state, *kind),
            &server,
            &mut image,
            &mut bg,
            EMPTY_SLOT_COLOR,
        );
    }
}

fn paint_counts(
    inventory: Res<Inventory>,
    craft_state: Res<CraftState>,
    mut counts: Query<(&SlotCount, &mut Text)>,
) {
    for (count, mut text) in &mut counts {
        if matches!(count.0, SlotKind::Container(_)) {
            continue;
        }
        text.0 = stack_count_text(slot_stack(&inventory, &craft_state, count.0));
    }
}

fn update_craft_ui(
    craft_state: Res<CraftState>,
    mut grid: Query<&mut Node, (With<CraftGrid>, Without<SlotKind>)>,
    mut craft_cells: Query<(&SlotKind, &mut Node), Without<CraftGrid>>,
    mut hint: Query<&mut Text, With<CraftHint>>,
) {
    let dim = craft_state.dim;
    if let Ok(mut node) = grid.single_mut() {
        node.width = Val::Px(dim as f32 * CELL_STRIDE);
    }
    for (kind, mut node) in &mut craft_cells {
        if let SlotKind::Craft(i) = kind {
            if *i >= 4 {
                node.display = if dim == 3 { Display::Flex } else { Display::None };
            }
        }
    }
    if let Ok(mut text) = hint.single_mut() {
        text.0 = if craft_state.near_workbench {
            "3x3 grid (workbench nearby)".into()
        } else {
            "2x2 grid  -  get near a workbench for 3x3".into()
        };
    }
}

// --- Cursor stack + tooltips -----------------------------------------

#[derive(Component)]
struct CarriedIcon;
#[derive(Component)]
struct CarriedCount;
#[derive(Component)]
struct Tooltip;
#[derive(Component)]
struct TooltipText;
#[derive(Component)]
struct HotbarNamePopup;
#[derive(Component)]
struct HotbarNameText;

fn spawn_overlays(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(26.0),
                height: Val::Px(26.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            BackgroundColor(Color::NONE),
            ImageNode { color: Color::NONE, ..default() },
            GlobalZIndex(100),
            Pickable::IGNORE, // must not eat clicks meant for the slot underneath
            Visibility::Hidden,
            CarriedIcon,
        ))
        .with_children(|c| {
            c.spawn((
                Text::new(""),
                TextFont::from_font_size(10.0),
                TextColor(Color::WHITE),
                CarriedCount,
            ));
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.88)),
            GlobalZIndex(101),
            Pickable::IGNORE,
            Visibility::Hidden,
            Tooltip,
        ))
        .with_children(|t| {
            t.spawn((
                Text::new(""),
                TextFont::from_font_size(13.0),
                TextColor(Color::WHITE),
                TooltipText,
            ));
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(66.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
            Visibility::Hidden,
            HotbarNamePopup,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ))
            .with_children(|pill| {
                pill.spawn((
                    Text::new(""),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::WHITE),
                    HotbarNameText,
                ));
            });
        });
}

fn update_carried_icon(
    open: Res<InventoryOpen>,
    container: Res<OpenContainer>,
    inventory: Res<Inventory>,
    server: Res<AssetServer>,
    windows: Query<&Window>,
    mut icon: Query<
        (&mut Node, &mut Visibility, &mut BackgroundColor, &mut ImageNode),
        With<CarriedIcon>,
    >,
    mut count: Query<&mut Text, With<CarriedCount>>,
) {
    let (Ok((mut node, mut visibility, mut bg, mut image)), Ok(mut text)) =
        (icon.single_mut(), count.single_mut())
    else {
        return;
    };
    match (open.0 || container.0.is_some(), inventory.carried) {
        (true, Some(stack)) => {
            paint_icon(Some(stack), &server, &mut image, &mut bg, Color::NONE);
            text.0 = stack_count_text(Some(stack));
            if let Ok(window) = windows.single() {
                if let Some(cursor) = window.cursor_position() {
                    node.left = Val::Px(cursor.x - 13.0);
                    node.top = Val::Px(cursor.y - 13.0);
                }
            }
            *visibility = Visibility::Visible;
        }
        _ => *visibility = Visibility::Hidden,
    }
}

fn inventory_tooltip(
    open: Res<InventoryOpen>,
    container: Res<OpenContainer>,
    inventory: Res<Inventory>,
    craft_state: Res<CraftState>,
    windows: Query<&Window>,
    slots: Query<(&SlotKind, &Interaction)>,
    mut tip: Query<(&mut Node, &mut Visibility), With<Tooltip>>,
    mut tip_text: Query<&mut Text, With<TooltipText>>,
) {
    let (Ok((mut node, mut visibility)), Ok(mut text)) = (tip.single_mut(), tip_text.single_mut())
    else {
        return;
    };

    let hovered = (open.0 || container.0.is_some()).then(|| {
        slots
            .iter()
            .find(|(_, i)| matches!(**i, Interaction::Hovered | Interaction::Pressed))
            .map(|(k, _)| *k)
    });

    let stack = hovered
        .flatten()
        .filter(|_| inventory.carried.is_none())
        .and_then(|kind| slot_stack(&inventory, &craft_state, kind));

    match stack {
        Some(stack) => {
            text.0 = stack.item.name();
            match stack.item.wear_kind() {
                WearKind::Tool => {
                    text.0
                        .push_str(&format!("  ({:.0}% dur.)", (1.0 - stack.wear_frac()) * 100.0));
                }
                WearKind::Rot => {
                    let f = 1.0 - stack.wear_frac();
                    let extra = if f > 0.25 {
                        format!("  ({:.0}% fresh)", f * 100.0)
                    } else {
                        "  (going off!)".to_string()
                    };
                    text.0.push_str(&extra);
                }
                WearKind::Rust if stack.wear > 0 => {
                    text.0
                        .push_str(&format!("  ({:.0}% rusted)", stack.wear_frac() * 100.0));
                }
                _ => {}
            }
            if let Ok(window) = windows.single() {
                if let Some(cursor) = window.cursor_position() {
                    node.left = Val::Px(cursor.x + 14.0);
                    node.top = Val::Px(cursor.y + 14.0);
                }
            }
            *visibility = Visibility::Visible;
        }
        None => *visibility = Visibility::Hidden,
    }
}

fn hotbar_name_popup(
    time: Res<Time>,
    open: Res<InventoryOpen>,
    inventory: Res<Inventory>,
    mut last: Local<Option<(usize, Option<Item>)>>,
    mut remaining: Local<f32>,
    mut popup: Query<&mut Visibility, With<HotbarNamePopup>>,
    mut popup_text: Query<&mut Text, With<HotbarNameText>>,
) {
    let current = (inventory.selected, inventory.selected_item());
    if (*last).map_or(true, |l| l != current) {
        *last = Some(current);
        *remaining = if current.1.is_some() { 2.0 } else { 0.0 };
    }
    *remaining = (*remaining - time.delta_secs()).max(0.0);

    let (Ok(mut visibility), Ok(mut text)) = (popup.single_mut(), popup_text.single_mut()) else {
        return;
    };
    match current.1 {
        Some(item) if !open.0 && *remaining > 0.0 => {
            text.0 = item.name();
            *visibility = Visibility::Visible;
        }
        _ => *visibility = Visibility::Hidden,
    }
}
