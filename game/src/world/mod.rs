//! Voxel world: block definitions, chunk data, procedural generation, meshing,
//! streaming, the shareable structure format, ground scatter, glTF props, the
//! camera view-slice and the day/night cycle. Also the wildlife (`animal`),
//! which lives and breeds in this world.
//!
//! Re-exported flat at the crate root by `main.rs`, so the rest of the code
//! keeps using `crate::block::…`, `crate::worldgen::…` and so on.

pub mod animal;
pub mod block;
pub mod block_atlas;
pub mod chunk;
pub mod chunk_material;
pub mod daynight;
pub mod mesher;
pub mod props;
pub mod scatter;
pub mod streaming;
pub mod structure;
pub mod view;
pub mod worldgen;
