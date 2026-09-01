//! Assembles the block texture atlas at runtime from the PNGs in
//! `assets/textures/blocks/` and binds it to the chunk material once ready.
//!
//! Atlas = one row of 15 tiles of 16×16 (see `block::ATLAS_COLS`):
//! `0..3` grass side/top/bottom · `3..5` wood side/end · `5` leaves · `6` sand
//! `7` stone · `8` gravel · `9` wood planks · `10` plowed land · `11` snow
//! `12` glass · `13` bedrock (roca madre) · `14` blank white.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::chunk_material::{ChunkMaterial, ChunkMaterialHandle, GlassMaterialHandle};

const TILE: usize = 16;
const COLS: usize = 15;

pub struct BlockAtlasPlugin;

impl Plugin for BlockAtlasPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_sources)
            .add_systems(Update, assemble_atlas);
    }
}

/// `(handle, source column count, first atlas column)`.
#[derive(Resource)]
struct AtlasSources {
    entries: Vec<(Handle<Image>, usize, usize)>,
    done: bool,
}

fn load_sources(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(AtlasSources {
        entries: vec![
            (server.load("textures/blocks/grass-spritesheet.png"), 3, 0),
            (server.load("textures/blocks/wood-spritesheet.png"), 2, 3),
            (server.load("textures/blocks/leaves.png"), 1, 5),
            (server.load("textures/blocks/sand.png"), 1, 6),
            (server.load("textures/blocks/stone.png"), 1, 7),
            (server.load("textures/blocks/gravel.png"), 1, 8),
            (server.load("textures/blocks/wood_planks.png"), 1, 9),
            (server.load("textures/blocks/plowed_land.png"), 1, 10),
            (server.load("textures/blocks/snow.png"), 1, 11),
            (server.load("textures/blocks/glass.png"), 1, 12),
            (server.load("textures/blocks/mother_rock.png"), 1, 13),
        ],
        done: false,
    });
}

fn assemble_atlas(
    mut sources: ResMut<AtlasSources>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ChunkMaterial>>,
    handle: Option<Res<ChunkMaterialHandle>>,
    glass_handle: Option<Res<GlassMaterialHandle>>,
) {
    if sources.done {
        return;
    }
    let Some(handle) = handle else {
        return;
    };
    if sources
        .entries
        .iter()
        .any(|(h, ..)| images.get(h).map(|i| i.data.is_none()).unwrap_or(true))
    {
        return; // still loading
    }

    let width = COLS * TILE;
    let mut data = vec![255u8; width * TILE * 4]; // starts white (last tile = blank)

    for (h, cols, first_col) in &sources.entries {
        let src = images.get(h).unwrap();
        let src_w = src.width() as usize;
        let Some(bytes) = src.data.as_ref() else {
            continue;
        };
        for col in 0..*cols {
            let dst_col = first_col + col;
            for row in 0..TILE {
                let src_off = (row * src_w + col * TILE) * 4;
                let dst_off = (row * width + dst_col * TILE) * 4;
                data[dst_off..dst_off + TILE * 4]
                    .copy_from_slice(&bytes[src_off..src_off + TILE * 4]);
            }
        }
    }

    let mut atlas = Image::new(
        Extent3d {
            width: width as u32,
            height: TILE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    atlas.sampler = ImageSampler::nearest();
    let atlas_handle = images.add(atlas);

    if let Some(mut material) = materials.get_mut(&handle.0) {
        material.base.base_color_texture = Some(atlas_handle.clone());
    }
    if let Some(gh) = glass_handle {
        if let Some(mut material) = materials.get_mut(&gh.0) {
            material.base.base_color_texture = Some(atlas_handle);
        }
    }
    sources.done = true;
    info!("Block atlas assembled");
}
