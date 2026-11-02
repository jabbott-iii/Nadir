// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! 2D renderer for the Nadir engine: sprite batching, y-sort, tilemap chunks,
//! camera, bitmap text, debug draw and passes.
//!
//! So far: the [`Texture`] asset (roadmap M3.2), the [`Atlas`] asset of a
//! cooked atlas's regions (M5.9), and the [`Renderer`]
//! (M3.3), which draws a [`RenderFrame`]'s sprites in pixel-perfect mode.
//! Its work on the CPU is the [`SpriteBatcher`], public so that it can be
//! measured without a GPU (M3.5). Since M4.5 it also draws [`TileLayer`]s,
//! in chunks, from the [`TileLayers`] the game keeps (ADR-0023).
//! `docs/subsystems/render.md` has the design.

mod atlas;
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

pub use atlas::{Atlas, AtlasClip, AtlasClipFrame, AtlasFrame, AtlasPage, AtlasSprite};
pub use batcher::SpriteBatcher;
pub use error::RenderError;
pub use frame::{Camera2D, Flip, RenderFrame, SortMode, SpriteDraw};
pub use instance::SPRITE_INSTANCE_FLOATS;
pub use renderer::{FrameStats, Output, RenderConfig, Renderer};
pub use texture::{DecodedTexture, Texture};
pub use tiles::{TILE_INSTANCE_FLOATS, Tile, TileLayer, TileLayerDraw, TileLayers, TileSet};
pub use view::Upscale;
