//! 2D renderer for the Nadir engine: sprite batching, y-sort, tilemap chunks,
//! camera, bitmap text, debug draw and passes.
//!
//! So far (roadmap M3.2): the [`Texture`] asset. `docs/subsystems/render.md`
//! has the design.

mod texture;

pub use texture::{DecodedTexture, Texture};
