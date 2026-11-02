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

//! Collision for the Nadir engine: shapes, a spatial hash, move-and-slide,
//! sweeps and queries. A* and flow fields, for navigation, come with roadmap
//! M9.5.
//!
//! It is kinematic, with no rigid-body physics (ADR-0011), and a plain
//! library over `nadir-core`: the facade copies the ECS's colliders into a
//! [`CollisionWorld`] each tick, moves entities with [`move_and_slide`],
//! sends the [`Triggers`]' pairs as events, and casts and queries for
//! gameplay. `docs/subsystems/collision.md` has the design, and ADR-0021
//! the choices made where it was open.
//!
//! - Colliders are boxes and circles ([`Shape`]) on layers
//!   ([`LayerMask`]). Tiles collide through a [`TileGrid`] of solid cells.
//! - Shapes that only touch do not overlap.
//! - Results come in the same order on every run, and a steady tick
//!   allocates nothing.

mod geometry;
mod grid;
mod hash;
mod shape;
mod slide;
mod sweep;
#[cfg(test)]
mod tests;
mod trigger;
mod world;

pub use grid::TileGrid;
pub use shape::{Collider, ColliderKind, LayerMask, Shape};
pub use slide::{Contact, move_and_slide};
pub use sweep::{Obstacle, SweepHit};
pub use trigger::{TriggerPair, Triggers};
pub use world::CollisionWorld;

/// How far, in pixels, a collider may overlap a solid after
/// [`move_and_slide`]: rounding leaves less, and a mover overlapping a
/// solid across its motion by no more than this slides along it.
pub const EPSILON: f32 = 0.01;
