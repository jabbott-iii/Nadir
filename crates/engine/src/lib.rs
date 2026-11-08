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

//! The Nadir game engine: [`App`] and its main loop, the engine's
//! components and systems, and the subsystems a game uses.
//!
//! [Nadir](https://github.com/jabbott-iii/Nadir) is a 2D engine for
//! top-down games on Windows, macOS and Linux, written in Rust with no
//! third-party crates. This crate is its facade, the one a game depends on:
//! its package is `nadir_engine`, and code names it `nadir`. It re-exports
//! the subsystems as modules, and [`prelude`] holds what most games use. A
//! game needs only this crate:
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
//!
//! This is version 0.2, and the API will change before 1.0. The engine
//! draws on Linux (X11). On Windows and macOS a game runs, but its window
//! stays empty until roadmap tasks M2.6 and M2.7. Audio, text, gamepads,
//! saves and the debug tools are not built yet. The
//! [README](https://github.com/jabbott-iii/Nadir/blob/main/crates/engine/README.md)
//! is a guide to how the parts fit together, with a game that walks a hero
//! about a tile map.
//!
//! # The app and the main loop
//!
//! [`App`] holds the game's [`Config`], its world and its systems. A game
//! adds resources, actions and systems with its builder methods, sets its
//! world up through [`App::world_mut`], and calls [`App::run`], which opens
//! the log file and the window and runs the main loop until the window is
//! closed. Each frame, the loop:
//!
//! 1. hands the window's [`Event`]s to [`Input`](input::Input);
//! 2. runs the ticks that are due, [`TICK_HZ`] a second however fast the
//!    frames come: each latches the game's actions, then runs the tick
//!    stages;
//! 3. sets [`TickStats`], and runs the frame stages, which fill the
//!    [`RenderFrame`](render::RenderFrame);
//! 4. draws it, and makes the assets that the workers have loaded.
//!
//! A frame counts for at most 250 ms: after a stall the simulation falls
//! behind real time rather than running a burst of ticks.
//!
//! - [`App::update`] runs steps 1 to 3 without a window: tests, benchmarks
//!   and headless runs call it themselves.
//! - [`App::offscreen`] draws frames with no window: [`Offscreen::frame`]
//!   returns each frame's pixels, for recordings.
//! - [`Error`] is what ends [`App::run`].
//!
//! # The world, systems and stages
//!
//! The game's state is a [`World`](ecs::World) of entities, components and
//! resources, from [`ecs`]. Systems are plain functions: a
//! [`FixedSystem`](ecs::FixedSystem), `fn(&World, &Tick)`, runs in a
//! [`FixedStage`](ecs::FixedStage) of each tick, added with
//! [`App::add_fixed_system`]; a [`FrameSystem`](ecs::FrameSystem),
//! `fn(&World, &Frame)`, in a [`FrameStage`](ecs::FrameStage) of each
//! frame, added with [`App::add_frame_system`]. They run in stage order,
//! then in the order they were added, after the engine's own, which
//! [`App`] lists with the resources the engine adds.
//!
//! - [`App::insert_resource`] adds a resource; systems read it with
//!   [`World::resource`](ecs::World::resource) and write it with
//!   [`World::resource_mut`](ecs::World::resource_mut).
//! - A component implements [`Component`](ecs::Component), and is
//!   registered before use: with [`App::register_component`] if it is
//!   reflected (defined with [`scene::reflect_struct!`] and its siblings),
//!   or with [`World::register`](ecs::World::register).
//! - Systems spawn, despawn, add and remove through
//!   [`Commands`](ecs::Commands), which apply after the tick's stages or the
//!   frame's.
//! - [`App::add_event`] adds an event type, the resource
//!   [`Events`](ecs::Events): tick systems read a tick's events, and frame
//!   systems all those since the last frame.
//!
//! # The engine's components
//!
//! | Component | What it is |
//! | :- | :- |
//! | [`Transform`] | Where an entity is, in world pixels, +Y down |
//! | [`Velocity`] | How fast it moves, in pixels a second: the engine moves it each tick |
//! | [`Collider`] | What it collides with, solid or a trigger |
//! | [`Sprite`] | What it looks like, drawn at its `Transform` |
//! | [`Animator`] | Plays a [`Clip`] on its `Sprite` |
//! | [`Facing`] | Which way it faces: facing left mirrors its `Sprite` |
//! | [`Parent`] | Puts it at another entity's position, plus an offset |
//! | [`Player`] | A tag for the entity the player controls |
//! | [`SaveId`] | Its id in saves, which come with roadmap M9.4 |
//! | [`PrevTransform`] | Where it was at the start of the last tick, for drawing between ticks: the engine keeps it |
//!
//! [`Transform`], [`Velocity`], [`Player`], [`SaveId`], [`Facing`],
//! [`Sprite`], [`Collider`] and [`Animator`] are reflected, so prefabs and
//! levels can give them; [`Parent`] is not yet (roadmap M9.4), and
//! [`PrevTransform`] is the engine's own, and is never in data. A
//! [`Sprite`] is written with its texture's path, and a prefab may name a
//! frame of a cooked atlas instead, `{"atlas": "atlas/sprites.kbj",
//! "sprite": "sprites/hero", "frame": 0}`; a [`Collider`]'s layers by the
//! names [`App::collision_layers`] gives them, `["World", "People"]`; an
//! [`Animator`] as its clip's name in the [`Clips`]. The world hash leaves
//! out what `nadir::animate` writes: a sprite's texture, uv, size, pivot
//! and `flip.x` (ADR-0038).
//!
//! # Prefabs
//!
//! A prefab is a set of component values, cooked from
//! `assets/data/prefabs/<name>.jsonc` to `data/prefabs/<name>.kbj`, that
//! may extend another. The [`PrefabLibrary`](scene::PrefabLibrary)
//! resource loads prefabs and their bases, and spawns entities of them;
//! [`loading_context`] gives it a reflect context with what the engine's
//! components need, the asset server, the clips and the collision layers,
//! and [`reflect_context`] one for writing them, as a hash or a save
//! does.
//!
//! # Levels
//!
//! The cook makes a level file, `levels/<name>.klvl`, of each level of an
//! LDtk project. [`App::load_level`] (or [`load_level`] on a world) loads
//! one in place of the last: its prefabs are read and checked before it
//! returns, its assets load in `Scope::Level`, its tall tiles and entities
//! spawn, its tile layers go into the [`TileLayers`](render::TileLayers)
//! and its grid into the [`Collisions`], and the engine draws its tile
//! layers each frame, its background the clear colour. [`CurrentLevel`]
//! says what it made, and the next load removes it whole; a
//! [`LevelError`] says why one did not load (ADR-0038).
//!
//! # Collision
//!
//! [`Collisions`] is the resource of the colliders, which the engine
//! rebuilds and moves each tick: systems query its
//! [`world`](Collisions::world), a [`CollisionWorld`](collision::CollisionWorld),
//! and turn the collider indices it gives into entities with
//! [`Collisions::entity`]. Its tile grid, [`Collisions::set_grid`], is how
//! tiles collide. [`TriggerEnter`] and [`TriggerExit`] are the events of
//! colliders that start and stop overlapping a trigger. [`collision`] has
//! the shapes, the layers and the queries.
//!
//! # Animation
//!
//! A [`Clip`] is a list of [`ClipFrame`]s, each a region of a texture shown
//! for a time, maybe with its own [`Placement`]. It loops or plays once
//! ([`Playback`]). The resource [`Clips`] keeps the clips by handle and
//! name, and [`Clips::add_atlas`] adds a cooked atlas's, from Aseprite's
//! tags. An [`Animator`] plays one on its entity's [`Sprite`], and sends
//! [`AnimationFinished`] when a clip that plays once ends. [`ClipError`]
//! says why a clip could not be made or added.
//!
//! # Input, drawing and assets
//!
//! - [`input`]: the game's actions. An [`ActionSet`](input::ActionSet)
//!   names them, an [`ActionMap`](input::ActionMap) binds keys and mouse
//!   buttons to them, and [`App::add_actions`] adds both; tick systems read
//!   the resource [`ActionState`](input::ActionState).
//! - [`render`]: the resource [`RenderFrame`](render::RenderFrame), with
//!   its [`Camera2D`](render::Camera2D), which frame systems fill and the
//!   loop draws; the resource [`TileLayers`](render::TileLayers), whose
//!   layers a frame names in [`RenderFrame::tile_layers`](render::RenderFrame::tile_layers);
//!   and the [`Texture`](render::Texture) and [`Atlas`](render::Atlas)
//!   assets.
//! - [`assets`]: the resource [`AssetServer`](assets::AssetServer), which
//!   loads from [`Config::assets`] on worker threads, and frees assets by
//!   [`Scope`](assets::Scope).
//!
//! # Hot reload
//!
//! With the `dev` feature, [`App::run`] reloads the files that change while
//! the game runs, such as those `cargo xtask cook --watch` cooks again, and
//! nadir-render's shaders, from its `shaders/` folder
//! ([`SHADER_DIR`](render::SHADER_DIR)). Watchers look at the files' times
//! of change every 250 ms, and an edit is drawn a frame or two after it is
//! seen; a shader edited before the loop starts is seen at its next save.
//! The handles the game holds stay good (ADR-0043):
//!
//! - a texture's handle draws its new texels;
//! - an atlas's clips are replaced by name under their handles, so its
//!   animators play the new frames on from their time;
//! - a prefab is replaced in the [`PrefabLibrary`](scene::PrefabLibrary)
//!   under its name;
//! - the current level is made again from its files, behind the handles of
//!   its assets, tile layers and clips, once its file, an atlas or a prefab
//!   changed; its entities are spawned anew;
//! - a program is rebuilt from its shader files.
//!
//! A file that fails to load, a level that cannot be made again, or shaders
//! that do not compile or fit their program are logged, and what they
//! would replace stays. Each reload is logged with its time. What the game
//! copied out of an asset, such as an atlas's regions into its own sprites,
//! and the entities it spawned of prefabs itself, do not change.
//! [`App::offscreen`] and [`App::update`] never reload, so recordings stay
//! the same, and builds without `dev` hold none of the reload.
//!
//! # Replays and determinism
//!
//! [`App::play_replay`] plays a recording of the game's actions, made with
//! an [`InputRecorder`](input::InputRecorder), in place of the live input,
//! and the resource [`Replay`] says how far it got. [`world_hash`] and
//! [`App::world_hash`] hash the reflected components, so that a test can
//! check that a replay reaches the same world, tick after tick.
//!
//! # The modules
//!
//! | Module | Crate | What it holds |
//! | :- | :- | :- |
//! | [`assets`] | `nadir-assets` | Handles, the asset server, sources and scopes |
//! | [`collision`] | `nadir-collision` | Shapes, layers, the tile grid, move-and-slide, triggers, queries |
//! | [`ecs`] | `nadir-ecs` | The world, components, resources, events, commands, the schedule |
//! | [`input`] | `nadir-input` | Actions, bindings, their state, replays |
//! | [`render`] | `nadir-render` | The frame to draw, sprites, tiles, the camera, textures, atlases |
//! | [`scene`] | `nadir-scene` | Reflection: the macros that define reflected types, and the type registry |
//! | [`log`] | `nadir-core` | The log macros, `error!` to `trace!` |
//! | [`cli`] | `nadir-core` | A small parser of command lines |
//!
//! [`Vec2`](prelude::Vec2), [`IVec2`](prelude::IVec2), [`UVec2`](prelude::UVec2),
//! [`Rect`](prelude::Rect), [`Color`](prelude::Color), [`Key`](prelude::Key),
//! [`MouseButton`](prelude::MouseButton), [`Pcg32`](prelude::Pcg32) and the
//! time types [`Tick`](prelude::Tick), [`Frame`](prelude::Frame) and
//! [`Stopwatch`](prelude::Stopwatch) are in the [`prelude`]; the rest of
//! `nadir-core`, such as its matrices and linear colours, is a dependency
//! of its own. The OS
//! layer, the GPU layer and the codecs are the engine's own, and a game
//! does not use them, except for the window's [`Event`]s, which
//! [`App::update`] takes.
//!
//! # Examples
//!
//! A frame of one tick moves an entity by its velocity, with no window:
//!
//! ```
//! use std::time::Duration;
//!
//! use nadir::prelude::*;
//!
//! let mut app = App::new(Config::default());
//! let ball = app
//!     .world_mut()
//!     .spawn()
//!     .with(Transform::at(Vec2::ZERO))
//!     .with(Velocity(Vec2::new(60.0, 0.0)))
//!     .id();
//! // 1/60 of a second, rounded up to the nanosecond: one tick.
//! app.update(Duration::from_nanos(16_666_667), &[]);
//! let at = app.world().read::<Transform>().get(ball).map(|it| it.position);
//! assert_eq!(at, Some(Vec2::new(1.0, 0.0)));
//! ```
//!
//! A trigger counts who steps on it, in a tick system after the engine's
//! `nadir::triggers`:
//!
//! ```
//! use std::time::Duration;
//!
//! use nadir::prelude::*;
//!
//! /// How many times something stepped on the plate.
//! #[derive(Default)]
//! struct Steps(usize);
//!
//! fn count_steps(world: &World, _tick: &Tick) {
//!     let entered = world.resource::<Events<TriggerEnter>>().tick_events().len();
//!     world.resource_mut::<Steps>().0 += entered;
//! }
//!
//! let mut app = App::new(Config::default())
//!     .insert_resource(Steps::default())
//!     .add_fixed_system(FixedStage::PostMovement, "count_steps", count_steps);
//! let world = app.world_mut();
//! // A plate 8 pixels across at x = 20.
//! world
//!     .spawn()
//!     .with(Transform::at(Vec2::new(20.0, 0.0)))
//!     .with(Collider::trigger(Shape::Aabb { half: Vec2::splat(4.0) }));
//! // Someone who walks right across it, at 2 pixels a tick.
//! world
//!     .spawn()
//!     .with(Transform::at(Vec2::ZERO))
//!     .with(Velocity(Vec2::new(120.0, 0.0)))
//!     .with(Collider::solid(Shape::Aabb { half: Vec2::splat(2.0) }));
//! for _ in 0..30 {
//!     app.update(Duration::from_nanos(16_666_667), &[]);
//! }
//! assert_eq!(app.world().resource::<Steps>().0, 1);
//! ```
//!
//! An animator shows the frame of its clip that the simulated time has
//! reached. The texture need not have loaded: the clip writes its regions
//! into the sprite all the same.
//!
//! ```
//! use std::time::Duration;
//!
//! use nadir::prelude::*;
//!
//! let mut app = App::new(Config::default());
//! let world = app.world_mut();
//! let sheet = world.resource_mut::<AssetServer>().load::<Texture>("hero.qoi");
//! // The sheet's left and right halves, 100 ms each, over and over.
//! let half = |x: f32| {
//!     let uv = Rect::new(Vec2::new(x, 0.0), Vec2::new(x + 0.5, 1.0));
//!     ClipFrame::new(sheet, uv, Duration::from_millis(100))
//! };
//! let clip = Clip::new(vec![half(0.0), half(0.5)], Playback::Loop);
//! let walk = world.resource_mut::<Clips>().add(clip);
//! let hero = world
//!     .spawn()
//!     .with(Transform::at(Vec2::ZERO))
//!     .with(Sprite::new(sheet, Vec2::new(16.0, 16.0)))
//!     .with(Animator::new(walk))
//!     .id();
//! // 9 ticks, 150 ms: the second frame shows.
//! for _ in 0..9 {
//!     app.update(Duration::from_nanos(16_666_667), &[]);
//! }
//! let uv = app.world().read::<Sprite>().get(hero).map(|it| it.uv.min.x);
//! assert_eq!(uv, Some(0.5));
//! ```
//!
//! # Using it well
//!
//! - **Threads.** The world, the systems, the window and the GPU all live
//!   on the thread that runs the app, and systems run one at a time. The
//!   asset server reads and decodes files on 1 to 4 worker threads of its
//!   own. Call [`App::run`] on the main thread: macOS opens windows only
//!   there.
//! - **Borrows.** Storages and resources are borrowed at run time. A system
//!   that borrows one mutably while it is borrowed panics, with the
//!   system's name and the type.
//! - **Determinism.** Ticks have a fixed length, and the clock counts
//!   frames' lengths exactly, so the same frames give the same ticks. Keep
//!   gameplay in tick systems. What frames set, such as an
//!   [`Animator`]'s time, whether an asset has loaded, and the pointer and
//!   the wheel differ from run to run: [`world_hash`] gives the conditions
//!   under which a replay reproduces the world.
//! - **Assets.** Loads finish only in drawn frames, so [`App::update`]
//!   finishes none, and a tick system must not depend on whether an asset
//!   has loaded. Before the window opens there is no GPU device, so only
//!   assets that need none, such as an [`Atlas`](render::Atlas), can be
//!   made then, with [`AssetServer::load_now`](assets::AssetServer::load_now).
//! - **Allocation.** The engine's own systems and drawing do not allocate in
//!   a steady frame: its tests count a frame of 10,000 animated sprites and
//!   a tile layer with the `dev` feature's counter. Systems are function
//!   pointers; keep their buffers in resources and reuse them.
//! - **Errors and logs.** [`App::run`] logs an [`Error`] and shows it to the
//!   player before it returns it: on standard error on Linux, and in a
//!   message box on Windows or an alert on macOS. The [`log`] macros write to standard
//!   error, and [`App::run`] also writes the log to `nadir.log` in the
//!   game's logs folder, named by [`Config::studio`] and [`Config::game`].
//! - **Platforms.** On Linux the engine needs X11, or XWayland, and OpenGL
//!   3.3, which it loads at run time. On Windows and macOS it has no OpenGL
//!   context yet: [`App::run`] logs that and runs the game without drawing,
//!   and so without reloading, and [`App::offscreen`] fails.
//!
//! # Features
//!
//! - `dev`: development builds. The assets are the loose files of
//!   [`Config::assets`], not the cook's pack there, which builds without
//!   `dev` read when it is there (ADR-0041). A texture that failed to load
//!   shows a placeholder rather than nothing, and the global allocator counts
//!   allocations: the resource `FrameAllocs` holds the last frame's, and
//!   `thread_alloc_counts` and `process_alloc_counts` give the counts so
//!   far, as `AllocCounts`. No other crate in the program may then declare
//!   a `#[global_allocator]`. [`App::run`] reloads what changes while it
//!   runs ("Hot reload", above). Turn it on from a feature of the game,
//!   never from a normal dependency, so that release builds leave it out.

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod animation;
mod app;
mod collisions;
mod components;
mod error;
#[cfg(feature = "dev")]
mod frame_allocs;
mod glue;
#[cfg(feature = "dev")]
mod hot_reload;
mod level;
mod limiter;
mod log_file;
mod pack;
mod reflected;
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
pub use level::{CurrentLevel, LevelError, load_level, unload_level};
pub use reflected::{CollisionLayers, loading_context, reflect_context};
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
