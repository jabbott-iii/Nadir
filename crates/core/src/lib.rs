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

//! Foundation types for the Nadir engine: 2D math, color, fixed-step time,
//! logging, random numbers, hashing, generational handles, key names,
//! profile scopes, a command-line parser and a property-test harness.
//!
//! This crate is the bottom layer of
//! [Nadir](https://github.com/jabbott-iii/Nadir), and most of the engine's
//! other crates build on it. It depends on nothing but the standard library,
//! and it has no `unsafe` and no OS-specific code: the same code runs on
//! Windows, macOS and Linux.
//!
//! A game built on [`nadir_engine`](https://docs.rs/nadir_engine) reaches
//! part of it through the facade: [`log`] as `nadir::log`, [`cli`] as
//! `nadir::cli`, and [`Vec2`], [`IVec2`], [`UVec2`], [`Rect`], [`Color`],
//! [`Key`], [`MouseButton`], [`Pcg32`], [`Tick`], [`Frame`] and
//! [`Stopwatch`] in `nadir::prelude`. Everything else here needs a direct
//! dependency on `nadir-core`. The design is in
//! [`docs/subsystems/core.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/core.md).
//!
//! # Contents
//!
//! The types used most are re-exported at the crate root. The rest stay in
//! their modules.
//!
//! - **2D math**, in [`math`]:
//!   - [`Vec2`] (`f32`), [`IVec2`] (`i32`) and [`UVec2`] (`u32`), with the
//!     arithmetic operators and conversions to and from `[T; 2]`;
//!   - [`Rect`] (`f32`) and [`IRect`] (`i32`), half-open rectangles;
//!   - [`Affine2`], a compact 2D transform, and [`Mat3`], the same as a
//!     3×3 matrix for where a full matrix is needed, such as a shader
//!     uniform;
//!   - the scalar helpers [`math::lerp`], [`math::approx_eq`],
//!     [`math::clamp`], [`math::wrap_angle`] and [`math::angle_delta`].
//! - **Color**, in [`color`]: [`Color`] (8-bit sRGB, straight alpha), how
//!   colors are authored, and [`LinearColor`] (`f32` linear light), where
//!   blending happens (ADR-0013). [`Color::from_hex`] parses `#RRGGBB` and
//!   `#RRGGBBAA`, and [`color::srgb_to_linear`] and
//!   [`color::linear_to_srgb`] are the exact transfer functions.
//! - **Time**, in [`time`]: [`FixedClock`] turns frames of any length into
//!   [`Tick`]s of one fixed length, and tells each [`Frame`] how far it lies
//!   between the last two ticks ([`Frame::alpha`]). [`Stopwatch`] measures
//!   the real time that the frames take.
//! - **Logging**, in [`log`]: the macros [`log::error!`], [`log::warn!`],
//!   [`log::info!`], [`log::debug!`] and [`log::trace!`]; a filter with a
//!   default level ([`log::set_level`]) and overrides per module
//!   ([`log::set_target_level`]); and three sinks: stderr
//!   ([`log::set_stderr`]), a file that rotates by size ([`log::open_file`]
//!   with a [`log::FileConfig`]), and a ring buffer of the last
//!   [`log::RING_CAPACITY`] lines ([`log::tail`]).
//! - **Random numbers**, in [`rng`]: [`Pcg32`], with [`Pcg32::range`] over
//!   `u32`, `i32` and `f32` ranges ([`rng::SampleRange`]),
//!   [`Pcg32::shuffle`] and [`Pcg32::choose`].
//! - **Hashing**, in [`hash`]: [`hash::fnv1a64`] for hashes that are
//!   stored, [`hash::crc32`] for integrity checks, and [`FastHashMap`] and
//!   [`FastHashSet`], built on [`hash::FxLikeHasher`], for maps whose order
//!   must not change from run to run.
//! - **Handles**, in [`handle`]: [`GenArena`] stores values in reusable
//!   slots and names each one by a [`GenIndex`], which stops resolving
//!   once its value is removed. [`Handle`] tags an index with the type it
//!   refers to.
//! - **Input names**, in [`keys`]: [`Key`], a physical key numbered by its
//!   USB HID usage, and [`MouseButton`]. The platform layer turns OS events
//!   into these. Names for gamepad buttons and axes are not here yet; they
//!   come with gamepad support (roadmap M8).
//! - **Profiling**, in [`profile`]: [`profile_scope!`] times the rest of a
//!   block, and [`profile::take_frame`] collects the calling thread's
//!   [`profile::ScopeRecord`]s. Recording needs the `profile` feature.
//! - **Command lines**, in [`cli`]: [`cli::Args`], a parser with no schema,
//!   for `--flag`, `--key value` and positional arguments. Nadir's tools
//!   and the sandbox game use it.
//! - **Calendar time**, in [`civil`]: [`civil::DateTime`], UTC, for log
//!   lines and file names.
//! - **Property tests**, in [`testing`]: [`testing::check`] and
//!   [`testing::check_sized`] run a property over many seeded random
//!   inputs. A failure panics with the seed to run it again with
//!   (`NADIR_SEED`).
//! - **Errors**, in [`error`]: [`CoreError`].
//!
//! # Conventions
//!
//! World space is +X right and **+Y down**, which matches tile editors and
//! screen space. Units are pixels at zoom 1. Angles are radians, and a
//! positive angle turns clockwise on screen. All math is `f32`.
//!
//! # Examples
//!
//! Transforms and rectangles:
//!
//! ```
//! use std::f32::consts::FRAC_PI_2;
//!
//! use nadir_core::{Affine2, IRect, IVec2, Mat3, Rect, Vec2};
//!
//! // A quarter turn clockwise takes +X (right) to +Y (down), and then the
//! // translation applies.
//! let transform =
//!     Affine2::from_scale_angle_translation(Vec2::ONE, FRAC_PI_2, Vec2::new(100.0, 0.0));
//! let point = transform.transform_point(Vec2::X);
//! assert!(point.approx_eq(Vec2::new(100.0, 1.0), 1e-5));
//!
//! let back = transform.inverse().expect("a rotation can be undone");
//! assert!(back.transform_point(point).approx_eq(Vec2::X, 1e-5));
//!
//! // The same transform as a matrix, for a shader uniform.
//! let matrix = Mat3::from(transform);
//! assert!(matrix.transform_point(Vec2::X).approx_eq(point, 1e-5));
//!
//! // Rectangles contain their `min` corner but not their `max`, so
//! // rectangles that only share an edge do not intersect.
//! let tiles = IRect::from_min_size(IVec2::ZERO, IVec2::new(4, 3));
//! assert!(tiles.contains(IVec2::new(3, 2)));
//! assert!(!tiles.contains(IVec2::new(4, 2)));
//! let left = Rect::new(Vec2::ZERO, Vec2::splat(10.0));
//! let right = Rect::new(Vec2::new(10.0, 0.0), Vec2::new(20.0, 10.0));
//! assert!(!left.intersects(right));
//! ```
//!
//! Handles, a deterministic map, and stable hashes:
//!
//! ```
//! use nadir_core::hash::{crc32, fnv1a64};
//! use nadir_core::{FastHashMap, GenArena, Handle};
//!
//! struct Texture {
//!     width: u32,
//! }
//!
//! let mut textures = GenArena::new();
//! let grass: Handle<Texture> = Handle::new(textures.insert(Texture { width: 64 }));
//! assert_eq!(textures.get(grass.id()).map(|t| t.width), Some(64));
//!
//! // Removing frees the slot. The next insert reuses it under a new
//! // generation, so the old index no longer resolves.
//! textures.remove(grass.id());
//! let sand = textures.insert(Texture { width: 32 });
//! assert_eq!(sand.index(), grass.id().index());
//! assert!(textures.get(grass.id()).is_none());
//!
//! let mut by_name: FastHashMap<&str, Handle<Texture>> = FastHashMap::default();
//! by_name.insert("sand", Handle::new(sand));
//!
//! // These never change, so they are safe to store on disk.
//! assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
//! assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
//! ```
//!
//! A main loop, in outline: the one that nadir_engine's `App` runs is built
//! the same way. It logs to a file, steps the simulation at 60 Hz whatever
//! the frame rate, and collects the profile scopes each frame:
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use nadir_core::log::{self, FileConfig, info};
//! use nadir_core::profile::{self, ScopeRecord};
//! use nadir_core::{FixedClock, Stopwatch, profile_scope};
//!
//! fn main() -> std::io::Result<()> {
//!     // Start a fresh file each run, and keep the last three beside it.
//!     let config = FileConfig {
//!         rotate_on_open: true,
//!         ..FileConfig::default()
//!     };
//!     log::open_file("game.log", config)?;
//!
//!     let mut clock = FixedClock::new(60, Duration::from_millis(250));
//!     let mut stopwatch = Stopwatch::start();
//!     let mut scopes: Vec<ScopeRecord> = Vec::new();
//!     loop {
//!         let frame = clock.begin_frame(stopwatch.lap());
//!         while let Some(tick) = clock.consume_tick() {
//!             profile_scope!("simulate");
//!             if tick.index % 600 == 0 {
//!                 info!("tick {}", tick.index);
//!             }
//!         }
//!         // Draw here, blending the last two ticks by `frame.alpha`.
//!         let _ = frame.alpha;
//!         // Pass the same `Vec` each frame, and profiling does not allocate.
//!         profile::take_frame(&mut scopes);
//!     }
//! }
//! ```
//!
//! The modules have more: [`log`], [`time`], [`cli`], [`testing`] and
//! [`profile_scope!`] each have an example of their own.
//!
//! # Errors and panics
//!
//! - Only parsing fails. [`Color::from_hex`] and the methods of
//!   [`cli::Args`] return a [`CoreError`], which implements `Display` and
//!   [`std::error::Error`]. [`log::open_file`] returns the
//!   [`std::io::Error`] it met.
//! - Other functions give an answer for any input, rather than fail:
//!   [`Affine2::inverse`] and [`Mat3::inverse`] return `None` for a
//!   transform that cannot be undone, [`GenArena::get`] returns `None` for
//!   a stale index, [`math::clamp`] accepts `min > max`, and
//!   [`Pcg32::range`] returns the start of an empty range.
//! - Only these panic: [`GenArena::insert`], if the arena would need more
//!   than `u32::MAX` slots; [`testing::check`] and
//!   [`testing::check_sized`], on a failing case, as a test should; and,
//!   where overflow checks are on, as in debug builds, the arithmetic
//!   operators of [`IVec2`] and [`UVec2`], on overflow, as Rust's integers
//!   do.
//!
//! # Determinism
//!
//! - [`Pcg32`] gives the same numbers for the same seed and stream on every
//!   OS, and matches the output of the PCG reference implementation. Give
//!   each system its own stream from one world seed, so that a new random
//!   call in one does not shift the numbers another sees. It is not
//!   cryptographically secure, and it is not `Copy`, so that a generator is
//!   not duplicated by accident.
//! - [`FixedClock`] does not read the time: the caller says how long each
//!   frame took, so the same frame lengths give the same ticks on every
//!   machine. It counts in whole units, so the ticks keep exactly to their
//!   rate.
//! - [`FastHashMap`] and [`FastHashSet`] have no per-process random state.
//!   On one target, a map's iteration order depends only on what was done
//!   to it. The order can differ between targets, such as x86_64 and
//!   aarch64, so code whose results must match on every system should not
//!   iterate one (ADR-0025). [`hash::FxLikeHasher`]'s output may change
//!   between versions of this crate: store [`hash::fnv1a64`] instead.
//! - `+`, `-`, `*`, `/` and `sqrt` on `f32` round the same way on every
//!   system. The functions that call `sin`, `cos` or `atan2`
//!   ([`Vec2::from_angle`], [`Vec2::angle`], [`Vec2::rotate`],
//!   [`Affine2::from_angle`], [`Affine2::from_scale_angle_translation`])
//!   and the sRGB conversions, which call `powf`, may differ in the last
//!   bit from one system to another. Nadir's own tick systems do not use
//!   them, so that a replay hashes alike on every system (ADR-0025).
//!
//! # Threads and allocation
//!
//! - There is one logger per process, and any thread can log. It takes a
//!   lock and may write to a file, so do not log from the audio callback.
//!   It formats into reused buffers, and once its ring buffer is full each
//!   new line reuses the oldest line's allocation. [`log::tail`] returns
//!   new strings.
//! - Profile scopes are recorded per thread. [`profile::take_frame`]
//!   collects only the calling thread's, and each thread keeps at most
//!   [`profile::MAX_RECORDS_PER_THREAD`] between calls. Collecting from
//!   other threads, and the profiler that shows the records, are roadmap
//!   M7.3.
//! - [`GenArena`] reuses freed slots before it grows, so a steady pattern
//!   of inserts and removes does not allocate.
//! - [`Handle`] is `Copy`, and `Send + Sync` whatever it refers to.
//! - The math, color, time and random-number types never allocate.
//!
//! # Features
//!
//! - `profile`: [`profile_scope!`] records how long each scope ran, for
//!   [`profile::take_frame`] to collect. Off by default, when
//!   `profile_scope!` expands to nothing, its name is not evaluated, and
//!   `take_frame` yields no records, so call sites need no `cfg` of their
//!   own. Cargo builds this crate once, with every feature that any crate
//!   in the build asks for, so turning it on also times each system that
//!   nadir-ecs's schedule runs.

pub mod civil;
pub mod cli;
pub mod color;
pub mod error;
pub mod handle;
pub mod hash;
pub mod keys;
pub mod log;
pub mod math;
pub mod profile;
pub mod rng;
pub mod testing;
pub mod time;

#[cfg(test)]
mod test_support;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use color::{Color, LinearColor};
pub use error::CoreError;
pub use handle::{GenArena, GenIndex, Handle};
pub use hash::{FastHashMap, FastHashSet};
pub use keys::{Key, MouseButton};
pub use math::{Affine2, IRect, IVec2, Mat3, Rect, UVec2, Vec2};
pub use rng::Pcg32;
pub use time::{FixedClock, Frame, Stopwatch, Tick};
