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

//! Nadir, a 2D top-down game engine for desktop. This facade crate provides
//! `App`, the main loop and re-exports of the engine's subsystems.
//!
//! So far there is [`App`] with the main loop: it opens the log file and
//! the window, latches the game's actions, steps the simulation
//! [`TICK_HZ`] times a second, and draws each frame. The game's state is a
//! [`World`](nadir_ecs::World) of entities, components and resources, and
//! its systems run in stages (`docs/subsystems/ecs.md`). The engine's own
//! components ([`Transform`], [`Velocity`], [`Sprite`], [`Collider`]...)
//! and systems interpolate, move and collide entities, send trigger
//! events, and draw sprites (ADR-0022). An [`Animator`] plays a [`Clip`]
//! from [`Clips`] on its entity's sprite, and [`Facing`] mirrors it
//! (ADR-0024); a cooked atlas's clips, from Aseprite's tags, come with
//! [`Clips::add_atlas`] (ADR-0035). [`App::play_replay`] plays recorded actions in place of the
//! live input, and [`App::world_hash`] hashes the reflected world, for
//! determinism tests (ADR-0025). [`App::offscreen`] draws the game with
//! no window, a frame at a time, for recordings such as the demo page's
//! (ADR-0029). The game's tile layers are the
//! resource [`TileLayers`](nadir_render::TileLayers), which the renderer
//! draws from (ADR-0023). With the `dev` feature, the global
//! allocator counts allocations, and the resource `FrameAllocs` holds the
//! last frame's. A game needs only this crate:
//!
//! ```no_run
//! use nadir::prelude::*;
//!
//! fn main() -> Result<(), nadir::Error> {
//!     App::new(Config::default())
//!         .add_fixed_system(FixedStage::Gameplay, "hello", |_, tick| {
//!             if tick.index == 0 {
//!                 nadir::log::info!("the first tick");
//!             }
//!         })
//!         .run()
//! }
//! ```

mod animation;
mod app;
mod collisions;
mod components;
mod error;
#[cfg(feature = "dev")]
mod frame_allocs;
mod glue;
mod limiter;
mod log_file;
mod replay;
mod tick_stats;

pub use nadir_core::{cli, log};
pub use nadir_platform::Event;
#[cfg(feature = "dev")]
pub use nadir_platform::{AllocCounts, process_alloc_counts, thread_alloc_counts};
pub use {
    nadir_assets as assets, nadir_collision as collision, nadir_ecs as ecs, nadir_input as input,
    nadir_render as render, nadir_scene as scene,
};

pub use animation::{
    AnimationFinished, Animator, Clip, ClipError, ClipFrame, Clips, Placement, Playback,
};
pub use app::{App, Config, Offscreen, TICK_HZ};
pub use collisions::{Collisions, TriggerEnter, TriggerExit};
pub use components::{
    Collider, Facing, Parent, Player, PrevTransform, SaveId, Sprite, Transform, Velocity,
};
pub use error::Error;
#[cfg(feature = "dev")]
pub use frame_allocs::FrameAllocs;
pub use replay::{Replay, world_hash};
pub use tick_stats::TickStats;

/// What most games use, for one `use nadir::prelude::*`.
pub mod prelude {
    pub use nadir_assets::{AssetServer, FinalizeCtx, Handle, LoadState};
    pub use nadir_collision::{LayerMask, Shape, TileGrid};
    pub use nadir_core::time::{Frame, Stopwatch, Tick};
    pub use nadir_core::{Color, IVec2, Key, MouseButton, Pcg32, Rect, UVec2, Vec2};
    pub use nadir_ecs::{
        Commands, Component, Entity, Events, FixedStage, FrameStage, Res, ResMut, World,
    };
    pub use nadir_input::{ActionKind, ActionMap, ActionSet, ActionState, Binding, Input};
    pub use nadir_render::{
        Atlas, AtlasClip, AtlasClipFrame, AtlasFrame, AtlasPage, AtlasSprite, Camera2D, Flip,
        FrameStats, RenderFrame, SortMode, SpriteDraw, Texture, Tile, TileLayer, TileLayerDraw,
        TileLayers, TileSet,
    };

    pub use crate::{
        AnimationFinished, Animator, App, Clip, ClipError, ClipFrame, Clips, Collider, Collisions,
        Config, Facing, Parent, Placement, Playback, Player, PrevTransform, SaveId, Sprite,
        TickStats, Transform, TriggerEnter, TriggerExit, Velocity,
    };
}
