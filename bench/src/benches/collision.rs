//! Collision benchmarks: the spatial hash's rebuild and queries, and a
//! tick of movement for 5,000 entities.

use std::hint::black_box;

use nadir_collision::{
    Collider, CollisionWorld, Contact, LayerMask, Shape, TileGrid, move_and_slide,
};
use nadir_core::{IVec2, Pcg32, Rect, Vec2};

use crate::harness::Bencher;

/// How many colliders each benchmark has.
const COLLIDERS: usize = 5_000;

/// The side of the square they stand in, in pixels.
const MAP: f32 = 2_048.0;

/// The side of a tile: 128 × 128 of them.
const TILE: f32 = 16.0;

/// One collider of the benchmarks: its id, position, collider and
/// velocity.
type Body = (u64, Vec2, Collider, Vec2);

/// A map of [`TILE`] px tiles, one in five solid, and [`COLLIDERS`] boxes
/// and circles from 10 to 14 px across, each near the middle of a tile
/// that is not, with a velocity of 2 to 4 px a tick: a crowd among walls,
/// where a move often meets a neighbour or a wall. The same every time.
fn scene() -> (TileGrid, Vec<Body>) {
    let mut rng = Pcg32::new(0x636f_6c6c, 0);
    let tiles = (MAP / TILE) as i32;
    let mut grid = TileGrid::new(tiles as u32, tiles as u32, TILE, Vec2::ZERO);
    let mut open = Vec::new();
    for y in 0..tiles {
        for x in 0..tiles {
            if rng.range(0..5u32) == 0 {
                grid.set_solid(IVec2::new(x, y), true);
            } else {
                open.push(IVec2::new(x, y));
            }
        }
    }
    rng.shuffle(&mut open);
    let bodies = open[..COLLIDERS]
        .iter()
        .zip(0u64..)
        .map(|(&cell, id)| {
            let half = Vec2::new(rng.range(5.0..7.0), rng.range(5.0..7.0));
            let shape = if id % 2 == 0 {
                Shape::Circle { radius: half.x }
            } else {
                Shape::Aabb { half }
            };
            let middle = grid.cell_rect(cell).center();
            let position = middle + Vec2::new(rng.range(-1.0..1.0), rng.range(-1.0..1.0));
            let angle = rng.range(0.0..std::f32::consts::TAU);
            let velocity = Vec2::from_angle(angle) * rng.range(2.0..4.0);
            (id, position, Collider::solid(shape), velocity)
        })
        .collect();
    (grid, bodies)
}

/// `collision_hash_5k`: what the facade does with the spatial hash each
/// tick, and more: rebuild it from [`COLLIDERS`] colliders, then ask which
/// colliders overlap each one's bounds grown by 16 px.
pub(crate) fn hash(bencher: &mut Bencher) {
    let (_, bodies) = scene();
    let mut world = CollisionWorld::new();
    let mut found = Vec::new();
    bencher.iter(|| {
        world.rebuild(
            black_box(&bodies)
                .iter()
                .map(|&(id, at, collider, _)| (id, at, collider)),
        );
        let mut total = 0;
        for &(_, at, collider, _) in &bodies {
            let bounds = collider.bounds(at);
            let grown = Rect::new(
                bounds.min - Vec2::splat(16.0),
                bounds.max + Vec2::splat(16.0),
            );
            world.aabb(grown, LayerMask::ALL, &mut found);
            total += found.len();
        }
        total
    });
}

/// The world of the movement benchmark: the [`scene`]'s crowd and walls.
fn slide_world() -> (CollisionWorld, Vec<Body>) {
    let (grid, bodies) = scene();
    let mut world = CollisionWorld::new();
    world.set_grid(grid);
    world.rebuild(
        bodies
            .iter()
            .map(|&(id, at, collider, _)| (id, at, collider)),
    );
    (world, bodies)
}

/// Moves every body by its velocity, and returns how many were stopped.
fn slide_all(world: &CollisionWorld, bodies: &[Body], contacts: &mut Vec<Contact>) -> usize {
    let mut stopped = 0;
    for (index, &(_, at, _, velocity)) in bodies.iter().enumerate() {
        let end = move_and_slide(world, index as u32, at, black_box(velocity), contacts);
        stopped += usize::from(end != at + velocity);
    }
    stopped
}

/// `collision_slide_5k`: a tick of movement. Each of the [`scene`]'s
/// [`COLLIDERS`] colliders moves 2 to 4 px with `move_and_slide`, among
/// the others and the walls; about one move in four is stopped, mostly by
/// a wall, sometimes by a neighbour. Positions are not written
/// back, so every iteration does the same work. The M4 exit budget is
/// 4 ms a tick for 5,000 entities on the min-spec machine, everything
/// included.
pub(crate) fn slide(bencher: &mut Bencher) {
    let (world, bodies) = slide_world();
    let mut contacts = Vec::new();
    bencher.iter(|| slide_all(&world, &bodies, &mut contacts));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Config;

    #[test]
    fn the_movement_benchmark_stops_a_share_of_its_moves() {
        let (world, bodies) = slide_world();
        let mut contacts = Vec::new();
        let stopped = slide_all(&world, &bodies, &mut contacts);
        // 1,186 when it was written.
        assert!(
            (900..=1_500).contains(&stopped),
            "{stopped} of {COLLIDERS} stopped"
        );
        // By walls, and by neighbours: 1,026 and 242 contacts.
        let (mut walls, mut neighbours) = (0, 0);
        for (index, &(_, at, _, velocity)) in bodies.iter().enumerate() {
            move_and_slide(&world, index as u32, at, velocity, &mut contacts);
            for contact in &contacts {
                match contact.obstacle {
                    nadir_collision::Obstacle::Tile(_) => walls += 1,
                    nadir_collision::Obstacle::Collider(_) => neighbours += 1,
                }
            }
        }
        assert!(
            walls > 600 && neighbours > 120,
            "{walls} wall and {neighbours} neighbour contacts"
        );
    }

    #[test]
    fn the_benchmarks_time_their_work() {
        for run in [hash, slide] {
            let mut bencher = Bencher::new(Config::fast());
            run(&mut bencher);
            let measurement = bencher.finish().unwrap();
            assert!(measurement.median_ns() > 0.0);
        }
    }
}
