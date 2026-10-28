//! 2D renderer for the Nadir engine: sprite batching, y-sort, tilemap chunks,
//! camera, bitmap text, debug draw and passes.
//!
//! So far: the [`Texture`] asset (roadmap M3.2), and the [`Renderer`]
//! (M3.3), which draws a [`RenderFrame`]'s sprites in pixel-perfect mode.
//! Its work on the CPU is the [`SpriteBatcher`], public so that it can be
//! measured without a GPU (M3.5). Since M4.5 it also draws [`TileLayer`]s,
//! in chunks, from the [`TileLayers`] the game keeps (ADR-0023).
//! `docs/subsystems/render.md` has the design.

mod batcher;
mod chunks;
mod error;
mod frame;
mod instance;
mod plan;
mod renderer;
mod shader;
mod sort;
mod texture;
mod tiles;
mod view;

pub use batcher::SpriteBatcher;
pub use error::RenderError;
pub use frame::{Camera2D, Flip, RenderFrame, SortMode, SpriteDraw};
pub use instance::SPRITE_INSTANCE_FLOATS;
pub use renderer::{FrameStats, Output, RenderConfig, Renderer};
pub use texture::{DecodedTexture, Texture};
pub use tiles::{TILE_INSTANCE_FLOATS, Tile, TileLayer, TileLayerDraw, TileLayers, TileSet};
pub use view::Upscale;
