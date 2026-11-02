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

//! Entity-component system for the Nadir engine: generational entities,
//! sparse-set storage, resources, events, commands and the schedule.
//!
//! It is small, safe and single-threaded (ADR-0006): components sit in a
//! [`SparseSet`] per type, behind a `RefCell`, in the [`World`]; systems
//! are plain functions that borrow what they need; structural changes go
//! through [`Commands`]; and the [`Schedule`] runs the systems in stage
//! order, then in the order they were added. `docs/subsystems/ecs.md` has
//! the design.
//!
//! The facade crate, `nadir_engine`, re-exports this crate as
//! `nadir::ecs`, and its prelude has the names most games use. Its `App`
//! holds the game's [`World`] and [`Schedule`] and runs them: a game adds
//! systems with `App::add_fixed_system` and `App::add_frame_system`, and
//! never calls [`Schedule::run_fixed`] itself. Use this crate directly to
//! run a world of your own, as tools and tests do. It depends only on
//! `nadir-core`.
//!
//! # The parts
//!
//! - **Entities.** An [`Entity`] is an identifier: a slot and a
//!   generation. [`World::spawn`] and [`Commands::spawn`] make one;
//!   [`World::is_alive`], [`World::despawn`] and [`World::entity_count`]
//!   ask about them and end them.
//! - **Components.** Any `'static` type that implements the marker trait
//!   [`Component`], registered with [`World::register`]. [`World::read`]
//!   and [`World::write`] borrow one type's [`SparseSet`], as [`Read`] and
//!   [`Write`]; [`SparseSet::get`], [`SparseSet::get_mut`],
//!   [`SparseSet::iter`] and [`SparseSet::iter_mut`] reach the components.
//!   [`EntityMut::with`] adds them while setting up.
//! - **Several types at once.** [`join2`] walks the entities that have
//!   two types, read-only; [`for_each_mut_with`] changes one type and
//!   reads another. Otherwise drive a loop from the set you change and look
//!   the others up with [`SparseSet::get`].
//! - **Resources.** One value per type: [`World::insert_resource`],
//!   [`World::resource`] ([`Res`]), [`World::resource_mut`] ([`ResMut`]) and
//!   [`World::contains_resource`].
//! - **Events.** [`World::add_event`] adds the resource [`Events<T>`];
//!   writers [`Events::send`], fixed-stage readers read
//!   [`Events::tick_events`], and frame-stage readers read
//!   [`Events::frame_events`].
//! - **Commands.** [`World::commands`] gives a system its [`Commands`]:
//!   spawn, despawn, insert and remove, and [`EntityCommands`] for one
//!   entity. [`World::apply_commands`] makes them, in the order they were
//!   queued.
//! - **The schedule.** [`Schedule`] holds [`FixedSystem`]s in the
//!   [`FixedStage`]s of each tick and [`FrameSystem`]s in the
//!   [`FrameStage`]s of each frame, each with an optional
//!   [`RunCondition`]. [`Schedule::run_fixed`] runs a tick and
//!   [`Schedule::run_frame`] a frame. [`Schedule::describe`] lists the
//!   systems in the order they run, as [`SystemInfo`] with its [`Stage`].
//!
//! # Example
//!
//! Two ticks and a frame: a system takes a hit point a tick, another moves
//! what has a velocity, and a frame system counts the deaths of both ticks.
//!
//! ```
//! use nadir_core::time::{Frame, Tick};
//! use nadir_ecs::{
//!     Component, Entity, Events, FixedStage, FrameStage, Schedule, World, for_each_mut_with,
//! };
//!
//! struct Position(f32);
//! struct Velocity(f32);
//! struct Health(i32);
//! impl Component for Position {}
//! impl Component for Velocity {}
//! impl Component for Health {}
//!
//! #[derive(Clone)]
//! struct Died(Entity);
//!
//! struct Deaths(usize);
//!
//! fn poison(world: &World, _: &Tick) {
//!     let mut commands = world.commands();
//!     let mut died = world.resource_mut::<Events<Died>>();
//!     for (entity, health) in world.write::<Health>().iter_mut() {
//!         health.0 -= 1;
//!         if health.0 <= 0 {
//!             died.send(Died(entity));
//!             commands.despawn(entity);
//!         }
//!     }
//! }
//!
//! fn movement(world: &World, tick: &Tick) {
//!     let velocity = world.read::<Velocity>();
//!     let mut position = world.write::<Position>();
//!     for_each_mut_with(&mut position, &velocity, |_, p, v| p.0 += v.0 * tick.dt);
//! }
//!
//! fn count_deaths(world: &World, _: &Frame) {
//!     let died = world.resource::<Events<Died>>().frame_events().len();
//!     world.resource_mut::<Deaths>().0 += died;
//! }
//!
//! let mut world = World::new();
//! world.register::<Position>();
//! world.register::<Velocity>();
//! world.register::<Health>();
//! world.add_event::<Died>();
//! world.insert_resource(Deaths(0));
//!
//! let runner = world
//!     .spawn()
//!     .with(Position(0.0))
//!     .with(Velocity(60.0))
//!     .with(Health(3))
//!     .id();
//! let rock = world.spawn().with(Position(5.0)).with(Health(1)).id();
//!
//! let mut schedule = Schedule::new();
//! schedule
//!     .add_fixed(FixedStage::Gameplay, "poison", poison)
//!     .add_fixed(FixedStage::Movement, "movement", movement)
//!     .add_frame(FrameStage::Ui, "count_deaths", count_deaths);
//!
//! let dt = 1.0 / 60.0;
//! for index in 0..2 {
//!     schedule.run_fixed(&mut world, &Tick { index, dt });
//! }
//! schedule.run_frame(&mut world, &Frame { dt: 2.0 * dt, alpha: 0.0 });
//!
//! assert!(world.is_alive(runner) && !world.is_alive(rock));
//! assert_eq!(world.resource::<Deaths>().0, 1);
//! let x = world.read::<Position>().get(runner).map_or(0.0, |p| p.0);
//! assert!((x - 2.0).abs() < 1e-5);
//! let order: Vec<String> = schedule.describe().map(|system| system.to_string()).collect();
//! assert_eq!(
//!     order,
//!     ["tick  Gameplay: poison", "tick  Movement: movement", "frame Ui: count_deaths"],
//! );
//! ```
//!
//! # Using it well
//!
//! - **Systems are function pointers,** `fn(&World, &Tick)` or
//!   `fn(&World, &Frame)`. A closure that captures nothing becomes one; a
//!   system's state lives in resources and components.
//! - **Borrows are checked at run time.** Borrowing a component type or a
//!   resource that is borrowed mutably, or borrowing it mutably while it is
//!   borrowed, panics with a message that names the type and the running
//!   system. So does reading a component type that is not registered, or a
//!   resource there is none of. Let every borrow end when its system
//!   returns, and borrow each type once per system.
//! - **Structural changes wait.** Inside systems, spawn, despawn, insert and
//!   remove through [`Commands`]. [`Schedule::run_fixed`] applies them after
//!   the tick's stages and [`Schedule::run_frame`] after the frame's. An
//!   entity from [`Commands::spawn`] is alive at once, and gets its
//!   components when the commands are applied. An insert for an entity
//!   that was despawned first is dropped. A command for a type that is not
//!   registered panics as it is queued. [`World::spawn`] and
//!   [`World::despawn`] change the world at once, for setup code and tests.
//! - **Stale entities read nothing.** A despawned entity's slot is reused
//!   with a new generation, and the old [`Entity`] finds no components
//!   there. Give [`SparseSet::insert`], through [`World::write`], only
//!   living entities: an insert for a dead one whose slot holds nothing in
//!   that set is kept, and no despawn removes it. Commands check that their
//!   entity is alive.
//! - **Events last a tick, or a frame.** The tick buffer is cleared as each
//!   tick starts, so a fixed-stage reader sees only what systems before it
//!   sent in its tick. The frame buffer holds everything sent since the
//!   last frame, in all its ticks, and is cleared after the frame stages.
//!   Order writers before readers, by stage or by the order they were
//!   added.
//! - **Determinism.** Systems run in a fixed order, one at a time. A set's
//!   iteration order depends only on its history of inserts and removes (a
//!   remove moves the last component into the gap), so the same history
//!   gives the same order on every run and every machine, as replays need.
//! - **Allocation.** Sets, command queues and event buffers keep what they
//!   have grown to, and freed entity slots are reused, so a steady pattern
//!   of spawns, despawns, commands and events allocates nothing once the
//!   buffers have grown.
//! - **Threading.** None: a [`World`] is neither `Send` nor `Sync`, and the
//!   name of the running system is kept per thread.
//! - **Profiling.** Each system runs inside `nadir_core::profile_scope!`
//!   under its name, which records only with `nadir-core`'s `profile`
//!   feature.

mod commands;
mod entity;
mod events;
mod join;
mod schedule;
mod sparse;
mod system;
#[cfg(test)]
mod tests;
mod world;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use commands::{Commands, EntityCommands};
pub use entity::Entity;
pub use events::Events;
pub use join::{Join2, for_each_mut_with, join2};
pub use schedule::{
    FixedStage, FixedSystem, FrameStage, FrameSystem, RunCondition, Schedule, Stage, SystemInfo,
};
pub use sparse::SparseSet;
pub use world::{Component, EntityMut, Read, Res, ResMut, World, Write};
