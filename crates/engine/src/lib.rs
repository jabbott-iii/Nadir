//! Nadir, a 2D top-down game engine for desktop. This facade crate provides
//! `App`, plugins, the main loop and re-exports of the engine's subsystems.
//!
//! So far there is [`App`] with the main loop: it opens the log file and
//! the window, latches the game's actions and steps the simulation
//! [`TICK_HZ`] times a second, and draws each frame's sprites. Systems share
//! state through [`Resources`]. With the `dev` feature, the global
//! allocator counts allocations, and the resource `FrameAllocs` holds the
//! last frame's. A game needs only this crate:
//!
//! ```no_run
//! use nadir::prelude::*;
//!
//! fn main() -> Result<(), nadir::Error> {
//!     App::new(Config::default())
//!         .add_fixed_system("hello", |_, tick| {
//!             if tick.index == 0 {
//!                 nadir::log::info!("the first tick");
//!             }
//!         })
//!         .run()
//! }
//! ```

mod app;
mod error;
#[cfg(feature = "dev")]
mod frame_allocs;
mod interpolated;
mod limiter;
mod log_file;
mod resources;

pub use nadir_core::{cli, log};
pub use nadir_platform::Event;
#[cfg(feature = "dev")]
pub use nadir_platform::{AllocCounts, process_alloc_counts, thread_alloc_counts};
pub use {nadir_assets as assets, nadir_input as input, nadir_render as render};

pub use app::{App, Config, TICK_HZ};
pub use error::Error;
#[cfg(feature = "dev")]
pub use frame_allocs::FrameAllocs;
pub use interpolated::{Interpolated, Lerp};
pub use resources::Resources;

/// What most games use, for one `use nadir::prelude::*`.
pub mod prelude {
    pub use nadir_assets::{AssetServer, Handle, LoadState};
    pub use nadir_core::time::{Frame, Stopwatch, Tick};
    pub use nadir_core::{Color, Key, MouseButton, Rect, UVec2, Vec2};
    pub use nadir_input::{ActionKind, ActionMap, ActionSet, ActionState, Binding, Input};
    pub use nadir_render::{
        Camera2D, Flip, FrameStats, RenderFrame, SortMode, SpriteDraw, Texture,
    };

    pub use crate::{App, Config, Interpolated, Resources};
}
