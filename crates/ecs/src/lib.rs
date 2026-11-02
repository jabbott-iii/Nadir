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
