//! Collision and navigation for the Nadir engine: shapes, spatial hash,
//! move-and-slide, queries, A* and flow fields.
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
