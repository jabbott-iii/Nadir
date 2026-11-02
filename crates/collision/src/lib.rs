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

//! Kinematic collision and spatial queries for top-down 2D games: shapes,
//! solid tiles, a spatial hash, move-and-slide, casts, overlap queries and
//! triggers.
//!
//! It is the collision of [Nadir](https://github.com/jabbott-iii/Nadir),
//! a plain library over `nadir-core`, with no rigid-body physics
//! (ADR-0011). Games reach it through the `nadir_engine` facade, as
//! `nadir::collision`: each tick the facade copies the entities' colliders
//! into a [`CollisionWorld`], moves the entities that have both a velocity
//! and a collider with [`move_and_slide`] (one with a velocity and no
//! collider moves freely), sends the [`Triggers`]' pairs as events, and gives
//! systems the world, in its `Collisions` resource, for casts and queries.
//! Use this crate directly to drive a world of your own, as below.
//!
//! The design is in
//! [`docs/subsystems/collision.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/collision.md),
//! and the choices made where it was open in ADR-0021 and ADR-0026.
//!
//! # What is here
//!
//! - **Colliders:** a [`Shape`], a box or a circle; a [`Collider`], that
//!   shape with its offset from the entity's position, its layer and mask
//!   ([`LayerMask`]), and its [`ColliderKind`], solid or trigger.
//!   [`Collider::solid`] and [`Collider::trigger`] make one on every layer.
//! - **Tiles:** a [`TileGrid`], a solid bit per square cell, with a cell
//!   size, an origin and a layer. The world holds one
//!   ([`CollisionWorld::set_grid`], [`CollisionWorld::grid_mut`]); there
//!   are no colliders per tile.
//! - **The world:** [`CollisionWorld::rebuild`] replaces the colliders and
//!   rebuilds the spatial hash, once a tick. Each collider comes with an id
//!   of the caller's, which lasts as long as the caller wants, and gets an
//!   index, its place in the list, which lasts until the next rebuild.
//!   [`CollisionWorld::set_position`] moves one between rebuilds, so that
//!   the movers after it see where it went.
//! - **Movement:** [`move_and_slide`] moves a collider and gives where it
//!   ends, with the [`Contact`]s that stopped it. [`EPSILON`] is its
//!   tolerance.
//! - **Casts:** [`CollisionWorld::segment`] (a line of sight),
//!   [`CollisionWorld::sweep_aabb`] and [`CollisionWorld::sweep_circle`]
//!   (a moving shape, such as a projectile) give the first [`SweepHit`]:
//!   the [`Obstacle`], when, where and its normal.
//! - **Overlap queries:** [`CollisionWorld::point`],
//!   [`CollisionWorld::aabb`] and [`CollisionWorld::circle`] fill a list of
//!   colliders' indices; [`CollisionWorld::nearest`] gives the nearest one.
//! - **Triggers:** [`Triggers::update`], once a tick, then
//!   [`Triggers::entered`] and [`Triggers::exited`]: the [`TriggerPair`]s,
//!   by id, that began and stopped overlapping.
//!
//! # Example
//!
//! A tick by hand: a room with a wall of tiles, a player and a barrel. The
//! player walks down onto the barrel, then looks right through the wall.
//!
//! ```
//! use nadir_collision::{
//!     Collider, CollisionWorld, LayerMask, Obstacle, Shape, TileGrid, move_and_slide,
//! };
//! use nadir_core::{IVec2, Vec2};
//!
//! // 10 × 10 cells of 16 px, with a wall down column 6, from x = 96 to 112.
//! let mut grid = TileGrid::new(10, 10, 16.0, Vec2::ZERO);
//! for y in 0..10 {
//!     grid.set_solid(IVec2::new(6, y), true);
//! }
//! let mut world = CollisionWorld::new();
//! world.set_grid(grid);
//!
//! // The colliders of this tick: each an id of yours (an entity's, say),
//! // its entity's position and the collider. Collider `i` is the `i`th.
//! let player = Collider {
//!     // From the entity's position, its feet, to the box's center.
//!     offset: Vec2::new(0.0, -4.0),
//!     ..Collider::solid(Shape::Aabb { half: Vec2::splat(4.0) })
//! };
//! let barrel = Collider::solid(Shape::Circle { radius: 6.0 });
//! world.rebuild([
//!     (1, Vec2::new(40.0, 60.0), player),
//!     (2, Vec2::new(40.0, 100.0), barrel),
//! ]);
//! assert_eq!(world.id(1), 2);
//!
//! // The player walks 40 px down, and stops with its feet on the barrel's
//! // top, at y = 94.
//! let mut contacts = Vec::new();
//! let end = move_and_slide(&world, 0, Vec2::new(40.0, 60.0), Vec2::new(0.0, 40.0), &mut contacts);
//! assert_eq!(end, Vec2::new(40.0, 94.0));
//! assert_eq!(contacts[0].obstacle, Obstacle::Collider(1));
//! assert_eq!(contacts[0].normal, Vec2::new(0.0, -1.0));
//! // Tell the world, so that later moves and queries see it there.
//! world.set_position(0, end);
//!
//! // A line of sight to the right, ignoring the player's own collider,
//! // meets the wall.
//! let eye = Vec2::new(40.0, 90.0);
//! let hit = world
//!     .segment(eye, Vec2::new(150.0, 90.0), LayerMask::ALL, Some(0))
//!     .expect("the wall is in the way");
//! assert_eq!(hit.obstacle, Obstacle::Tile(IVec2::new(6, 5)));
//!
//! // What is within 12 px of the barrel's center: both, by index. Reuse
//! // the list from tick to tick.
//! let mut found = Vec::new();
//! world.circle(Vec2::new(40.0, 100.0), 12.0, LayerMask::ALL, &mut found);
//! assert_eq!(found, [0, 1]);
//! assert_eq!(
//!     world.nearest(Vec2::new(40.0, 120.0), 32.0, LayerMask::ALL, None),
//!     Some(1)
//! );
//! # fn shared<T: Send + Sync>() {}
//! # shared::<CollisionWorld>();
//! ```
//!
//! # Who meets whom
//!
//! A collider's `layer` is what it is and its `mask` what it collides with
//! or detects, as sets of 32 layers. Name them once, with
//! [`LayerMask::bit`]:
//!
//! - A solid collider stops a mover whose mask has its layer.
//! - The tiles stop a mover whose mask has the grid's
//!   [`layer`](TileGrid::layer), every layer unless it is set.
//! - A trigger stops nothing, and a trigger that moves is stopped by tiles
//!   only. It detects any other collider, solid or trigger, whose layer is
//!   in its mask.
//! - Overlap queries match every collider whose layer is in the query's
//!   mask; casts stop at solids only.
//!
//! ```
//! use nadir_collision::{Collider, CollisionWorld, LayerMask, Shape, TileGrid, move_and_slide};
//! use nadir_core::{IVec2, Vec2};
//!
//! const WALLS: LayerMask = LayerMask::bit(0);
//! const PLAYER: LayerMask = LayerMask::bit(1);
//! const GHOSTS: LayerMask = LayerMask::bit(2);
//!
//! let mut grid = TileGrid::new(8, 8, 16.0, Vec2::ZERO);
//! grid.set_solid(IVec2::new(4, 3), true);
//! grid.layer = WALLS;
//! let mut world = CollisionWorld::new();
//! world.set_grid(grid);
//!
//! // A ghost collides with the player, not with walls.
//! let ghost = Collider {
//!     layer: GHOSTS,
//!     mask: PLAYER,
//!     ..Collider::solid(Shape::Circle { radius: 4.0 })
//! };
//! let start = Vec2::new(40.0, 56.0);
//! world.rebuild([(1, start, ghost)]);
//! let mut contacts = Vec::new();
//! let end = move_and_slide(&world, 0, start, Vec2::new(60.0, 0.0), &mut contacts);
//! assert_eq!(end, Vec2::new(100.0, 56.0));
//! assert!(contacts.is_empty());
//! ```
//!
//! # Triggers
//!
//! [`Triggers`] compares each update's overlapping pairs with the last
//! one's. Pairs are by id, so a pair that goes on overlapping is not
//! reported again when the colliders' order changes, and a collider that
//! leaves the world ends its pairs.
//!
//! ```
//! use nadir_collision::{Collider, CollisionWorld, Shape, TriggerPair, Triggers};
//! use nadir_core::Vec2;
//!
//! let door = Collider::trigger(Shape::Circle { radius: 8.0 });
//! let player = Collider::solid(Shape::Aabb { half: Vec2::splat(4.0) });
//! let mut world = CollisionWorld::new();
//! let mut triggers = Triggers::new();
//!
//! world.rebuild([(10, Vec2::ZERO, door), (20, Vec2::new(50.0, 0.0), player)]);
//! triggers.update(&world);
//! assert!(triggers.entered().is_empty());
//!
//! // The player walks up to the door.
//! world.rebuild([(10, Vec2::ZERO, door), (20, Vec2::new(6.0, 0.0), player)]);
//! triggers.update(&world);
//! let pair = TriggerPair { trigger: 10, other: 20 };
//! assert_eq!(triggers.entered(), [pair]);
//!
//! // And leaves the world.
//! world.rebuild([(10, Vec2::ZERO, door)]);
//! triggers.update(&world);
//! assert_eq!(triggers.exited(), [pair]);
//! ```
//!
//! # Using it well
//!
//! - **Units:** pixels, +X right and +Y down. A collider's position is its
//!   entity's, its feet, and its `offset` goes from there to the shape's
//!   center.
//! - **Touching is not overlapping:** shapes that only touch do not
//!   overlap, and a point on an edge is outside. A mover against a wall
//!   stays put when pushed into it, and leaves it freely.
//! - **Tiles:** cells outside the grid are not solid, so a level needs
//!   walls round its edge. Neighbouring cells share their edges exactly
//!   ([`TileGrid::cell_rect`]), so a mover slides along a wall of tiles
//!   without snagging on the seams.
//! - **Speed:** a move is split into up to 16 substeps, and each axis is
//!   swept, so nothing tunnels through a thin wall however far it goes;
//!   nor does a cast. Hash in cells ([`CollisionWorld::with_cell_size`],
//!   64 px by [default](CollisionWorld::DEFAULT_CELL_SIZE)) at least twice
//!   the size of a typical collider. A collider over 1,024 cells, or one
//!   moved more than a quarter of a cell since the rebuild, is checked by
//!   every query until the next rebuild.
//! - **Determinism:** results come in the same order on every run: no
//!   result depends on a hash map's order. Queries give colliders kept
//!   aside first, then those moved far, then cell by cell, row by row, by
//!   index ([`CollisionWorld`] has the details). Ties go to the lower
//!   index, or to a tile before a collider.
//! - **Allocation:** [`CollisionWorld::rebuild`] and [`Triggers::update`]
//!   keep their buffers, and queries and moves fill a `Vec` you pass in, so
//!   a steady tick that reuses them allocates nothing.
//! - **Errors:** nothing here returns one. Sizes must be finite and not
//!   negative: others give meaningless results, but nothing panics or
//!   returns NaN for them. A move or cast with no motion, or with a
//!   position or motion that is not finite, does nothing. What panics is
//!   misuse, as each function's `# Panics` says: an index that is not a
//!   collider's, a cell outside the grid, a cell size not above 0, a layer
//!   past 31.
//! - **Threads:** [`CollisionWorld`] is `Send` and `Sync`, and queries,
//!   casts and [`move_and_slide`] take it by shared reference.
//! - **Not yet:** A* and flow fields (roadmap M9.5), and tiles other than
//!   full cells, such as half cells and diagonal walls (backlog).

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

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
