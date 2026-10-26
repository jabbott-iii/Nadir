//! Shared by the integration tests: a geometry oracle written apart from
//! the crate's own (overlap depths, not rays or reaches), and generators
//! of awkward values.

#![allow(dead_code)] // Each test file uses its own share of these.

use nadir_collision::{Collider, ColliderKind, CollisionWorld, LayerMask, Obstacle, Shape};
use nadir_core::{IVec2, Pcg32, Vec2};

/// How far the two shapes overlap: the least distance that would part
/// them, or 0 or less if they do not overlap.
pub(crate) fn depth(a_center: Vec2, a: Shape, b_center: Vec2, b: Shape) -> f32 {
    let apart = (b_center - a_center).abs();
    match (a, b) {
        (Shape::Aabb { half: a }, Shape::Aabb { half: b }) => {
            (a.x + b.x - apart.x).min(a.y + b.y - apart.y)
        }
        (Shape::Circle { radius: a }, Shape::Circle { radius: b }) => a + b - apart.length(),
        (Shape::Aabb { half }, Shape::Circle { radius })
        | (Shape::Circle { radius }, Shape::Aabb { half }) => {
            if apart.x < half.x && apart.y < half.y {
                radius + (half.x - apart.x).min(half.y - apart.y)
            } else {
                radius - (apart - half).max(Vec2::ZERO).length()
            }
        }
    }
}

/// A solid that can stop a mover or a cast: where its center is, and its
/// shape.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Solid {
    pub(crate) obstacle: Obstacle,
    pub(crate) center: Vec2,
    pub(crate) shape: Shape,
}

/// The solids of `world` that stop something whose mask is `mask`,
/// skipping collider `skip`: the solid tiles if the grid's layer is in the
/// mask, and the solid colliders on a layer of it.
pub(crate) fn solids(world: &CollisionWorld, mask: LayerMask, skip: Option<u32>) -> Vec<Solid> {
    let mut solids = Vec::new();
    let grid = world.grid();
    if grid.layer.intersects(mask) {
        let half = Vec2::splat(grid.cell_size() * 0.5);
        for y in 0..grid.height() as i32 {
            for x in 0..grid.width() as i32 {
                let cell = IVec2::new(x, y);
                if grid.is_solid(cell) {
                    solids.push(Solid {
                        obstacle: Obstacle::Tile(cell),
                        center: grid.cell_rect(cell).center(),
                        shape: Shape::Aabb { half },
                    });
                }
            }
        }
    }
    for index in 0..world.len() as u32 {
        let collider = world.collider(index);
        if Some(index) == skip
            || collider.kind != ColliderKind::Solid
            || !mask.intersects(collider.layer)
        {
            continue;
        }
        solids.push(Solid {
            obstacle: Obstacle::Collider(index),
            center: collider.center(world.position(index)),
            shape: collider.shape,
        });
    }
    solids
}

/// The deepest overlap of `shape` at `center` with any of `solids`, and
/// which.
pub(crate) fn deepest(solids: &[Solid], center: Vec2, shape: Shape) -> Option<(f32, Obstacle)> {
    solids
        .iter()
        .map(|solid| {
            (
                depth(center, shape, solid.center, solid.shape),
                solid.obstacle,
            )
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

/// A coordinate in `lo..hi`: half the time on a grid of 1/8 px, so that
/// shapes touch exactly, edges line up with cells and components are
/// exactly 0; otherwise anywhere.
pub(crate) fn coord(rng: &mut Pcg32, lo: f32, hi: f32) -> f32 {
    let value = rng.range(lo..hi);
    if rng.range(0..2u32) == 0 {
        (value * 8.0).round() / 8.0
    } else {
        value
    }
}

/// A motion component in `-reach..reach`: a quarter of the time exactly 0.
pub(crate) fn motion(rng: &mut Pcg32, reach: f32) -> f32 {
    if rng.range(0..4u32) == 0 {
        0.0
    } else {
        coord(rng, -reach, reach)
    }
}

/// A box or a circle, from 1/2 px to `most` px across each way.
pub(crate) fn shape(rng: &mut Pcg32, most: f32) -> Shape {
    if rng.range(0..2u32) == 0 {
        Shape::Aabb {
            half: Vec2::new(coord(rng, 0.25, most / 2.0), coord(rng, 0.25, most / 2.0)),
        }
    } else {
        Shape::Circle {
            radius: coord(rng, 0.25, most / 2.0),
        }
    }
}

/// One of a few masks, from none to all.
pub(crate) fn mask(rng: &mut Pcg32) -> LayerMask {
    [
        LayerMask::ALL,
        LayerMask::ALL,
        LayerMask::bit(0),
        LayerMask::bit(1),
        LayerMask::bit(0) | LayerMask::bit(2),
        LayerMask::NONE,
    ][rng.range(0..6u32) as usize]
}

/// A collider: mostly solid, on a random layer, with a small offset.
pub(crate) fn collider(rng: &mut Pcg32, most: f32) -> Collider {
    Collider {
        shape: shape(rng, most),
        offset: Vec2::new(coord(rng, -2.0, 2.0), coord(rng, -2.0, 2.0)),
        layer: mask(rng),
        mask: mask(rng),
        kind: if rng.range(0..5u32) == 0 {
            ColliderKind::Trigger
        } else {
            ColliderKind::Solid
        },
    }
}

/// Whether both components are finite.
pub(crate) fn finite(v: Vec2) -> bool {
    v.x.is_finite() && v.y.is_finite()
}
