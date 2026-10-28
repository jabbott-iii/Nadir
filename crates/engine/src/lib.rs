//! Nadir, a 2D top-down game engine for desktop. This facade crate provides
//! `App`, plugins, the main loop and re-exports of the engine's subsystems.
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
//! (ADR-0024). The game's tile layers are the
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
mod tick_stats;

pub use nadir_core::{cli, log};
pub use nadir_platform::Event;
#[cfg(feature = "dev")]
pub use nadir_platform::{AllocCounts, process_alloc_counts, thread_alloc_counts};
pub use {
    nadir_assets as assets, nadir_collision as collision, nadir_ecs as ecs, nadir_input as input,
    nadir_render as render, nadir_scene as scene,
};

pub use animation::{AnimationFinished, Animator, Clip, ClipFrame, Clips, Playback};
pub use app::{App, Config, TICK_HZ};
pub use collisions::{Collisions, TriggerEnter, TriggerExit};
pub use components::{
    Collider, Facing, Parent, Player, PrevTransform, SaveId, Sprite, Transform, Velocity,
};
pub use error::Error;
#[cfg(feature = "dev")]
pub use frame_allocs::FrameAllocs;
pub use tick_stats::TickStats;

/// What most games use, for one `use nadir::prelude::*`.
pub mod prelude {
    pub use nadir_assets::{AssetServer, Handle, LoadState};
    pub use nadir_collision::{LayerMask, Shape, TileGrid};
    pub use nadir_core::time::{Frame, Stopwatch, Tick};
    pub use nadir_core::{Color, IVec2, Key, MouseButton, Rect, UVec2, Vec2};
    pub use nadir_ecs::{
        Commands, Component, Entity, Events, FixedStage, FrameStage, Res, ResMut, World,
    };
    pub use nadir_input::{ActionKind, ActionMap, ActionSet, ActionState, Binding, Input};
    pub use nadir_render::{
        Camera2D, Flip, FrameStats, RenderFrame, SortMode, SpriteDraw, Texture, Tile, TileLayer,
        TileLayerDraw, TileLayers, TileSet,
    };

    pub use crate::{
        AnimationFinished, Animator, App, Clip, ClipFrame, Clips, Collider, Collisions, Config,
        Facing, Parent, Playback, Player, PrevTransform, SaveId, Sprite, TickStats, Transform,
        TriggerEnter, TriggerExit, Velocity,
    };
}
