//! Entity-component system for the Nadir engine: generational entities,
//! sparse-set storage, resources, events, commands and the schedule.
//!
//! It is small, safe and single-threaded (ADR-0006): components sit in a
//! [`SparseSet`] per type, behind a `RefCell`, in the [`World`]; systems
//! are plain functions that borrow what they need; structural changes go
//! through [`Commands`]; and the [`Schedule`] runs the systems in stage
//! order, then in the order they were added. `docs/subsystems/ecs.md` has
//! the design.

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

pub use commands::{Commands, EntityCommands};
pub use entity::Entity;
pub use events::Events;
pub use join::{Join2, for_each_mut_with, join2};
pub use schedule::{
    FixedStage, FixedSystem, FrameStage, FrameSystem, RunCondition, Schedule, Stage, SystemInfo,
};
pub use sparse::SparseSet;
pub use world::{Component, EntityMut, Read, Res, ResMut, World, Write};
