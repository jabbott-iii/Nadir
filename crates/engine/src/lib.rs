//! Nadir, a 2D top-down game engine for desktop. This facade crate provides
//! `App`, plugins, the main loop and re-exports of the engine's subsystems.
//!
//! So far there is [`App`] with the main loop: it opens the window, steps
//! the simulation [`TICK_HZ`] times a second, and ends when the window is
//! closed. A game needs only this crate:
//!
//! ```no_run
//! use nadir::prelude::*;
//!
//! fn main() -> Result<(), nadir::Error> {
//!     App::new(Config::default())
//!         .add_fixed_system("hello", |tick| {
//!             if tick.index == 0 {
//!                 nadir::log::info!("the first tick");
//!             }
//!         })
//!         .run()
//! }
//! ```

mod app;
mod error;
mod limiter;

pub use nadir_core::log;

pub use app::{App, Config, TICK_HZ};
pub use error::Error;

/// What most games use, for one `use nadir::prelude::*`.
pub mod prelude {
    pub use nadir_core::time::{Frame, Stopwatch, Tick};
    pub use nadir_core::{UVec2, Vec2};

    pub use crate::{App, Config};
}
