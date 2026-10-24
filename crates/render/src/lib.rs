//! 2D renderer for the Nadir engine: sprite batching, y-sort, tilemap chunks,
//! camera, bitmap text, debug draw and passes.
//!
//! So far: the [`Texture`] asset (roadmap M3.2), and the [`Renderer`]
//! (M3.3), which draws a [`RenderFrame`]'s sprites in pixel-perfect mode.
//! Its work on the CPU is the [`SpriteBatcher`], public so that it can be
//! measured without a GPU (M3.5).
//! `docs/subsystems/render.md` has the design.

mod batcher;
mod error;
mod frame;
mod instance;
mod plan;
mod renderer;
mod shader;
mod sort;
mod texture;
mod view;

pub use batcher::SpriteBatcher;
pub use error::RenderError;
pub use frame::{Camera2D, Flip, RenderFrame, SortMode, SpriteDraw};
pub use instance::SPRITE_INSTANCE_FLOATS;
pub use renderer::{FrameStats, Output, RenderConfig, Renderer};
pub use texture::{DecodedTexture, Texture};
pub use view::Upscale;
