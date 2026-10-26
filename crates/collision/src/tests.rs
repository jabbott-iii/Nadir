//! Tests across the crate's parts: a steady tick keeps its buffers.

use nadir_core::{IVec2, Pcg32, Rect, Vec2};

use crate::{Collider, CollisionWorld, LayerMask, Shape, TileGrid, Triggers, move_and_slide};

/// The capacity of every buffer a tick uses: the world's, the triggers',
/// and those of the caller's `others`.
fn capacities(world: &CollisionWorld, triggers: &Triggers, others: [usize; 3]) -> Vec<usize> {
    let mut capacities = world.capacities();
    capacities.extend(triggers.capacities());
    capacities.extend(others);
    capacities
}

#[test]
fn a_steady_tick_keeps_every_buffer() {
    let mut rng = Pcg32::new(0x636f_6c6c, 0);
    let mut grid = TileGrid::new(64, 64, 16.0, Vec2::ZERO);
    for _ in 0..600 {
        grid.set_solid(IVec2::new(rng.range(0..64), rng.range(0..64)), true);
    }
    let mut world = CollisionWorld::new();
    world.set_grid(grid);
    let mut colliders: Vec<(u64, Vec2, Collider)> = (0..2_000u64)
        .map(|id| {
            let shape = if id % 2 == 0 {
                Shape::Circle { radius: 4.0 }
            } else {
                Shape::Aabb {
                    half: Vec2::new(3.0, 5.0),
                }
            };
            let collider = if id % 10 == 0 {
                Collider::trigger(Shape::Circle { radius: 12.0 })
            } else {
                Collider::solid(shape)
            };
            let position = Vec2::new(rng.range(0.0..1024.0), rng.range(0.0..1024.0));
            (id, position, collider)
        })
        .collect();
    // A huge collider, always kept aside.
    colliders.push((
        9_999,
        Vec2::splat(512.0),
        Collider::trigger(Shape::Aabb {
            half: Vec2::splat(4000.0),
        }),
    ));
    let velocities: Vec<Vec2> = (0..colliders.len())
        .map(|_| Vec2::new(rng.range(-3.0..3.0), rng.range(-3.0..3.0)))
        .collect();

    let mut triggers = Triggers::new();
    let mut out = [Vec::new(), Vec::new()];
    let mut contacts = Vec::new();
    let mut warm = Vec::new();
    for tick in 0..60 {
        // The same work each tick, back and forth.
        let sign = if tick % 2 == 0 { 1.0 } else { -1.0 };
        world.rebuild(colliders.iter().copied());
        for (index, &velocity) in velocities.iter().enumerate() {
            let index = index as u32;
            let start = world.position(index);
            let end = move_and_slide(&world, index, start, velocity * sign, &mut contacts);
            world.set_position(index, end);
        }
        // A fast mover, moved far.
        world.set_position(3, world.position(3) + Vec2::new(300.0 * sign, 0.0));
        triggers.update(&world);
        for index in 0..100 {
            let at = world.position(index);
            world.aabb(
                Rect::from_center_size(at, Vec2::splat(40.0)),
                LayerMask::ALL,
                &mut out[0],
            );
            world.circle(at, 30.0, LayerMask::ALL, &mut out[1]);
            let _ = world.segment(at, at + Vec2::new(200.0, 50.0), LayerMask::ALL, Some(index));
        }
        if tick == 10 {
            warm = capacities(
                &world,
                &triggers,
                [out[0].capacity(), out[1].capacity(), contacts.capacity()],
            );
        }
    }
    assert!(!triggers.overlapping().is_empty());
    let others = [out[0].capacity(), out[1].capacity(), contacts.capacity()];
    assert_eq!(capacities(&world, &triggers, others), warm);
}

#[test]
fn a_lighter_tick_keeps_a_heavier_ticks_buffers() {
    let mut world = CollisionWorld::with_cell_size(16.0);
    let mut triggers = Triggers::new();
    let zone = Collider::trigger(Shape::Circle { radius: 10.0 });
    let body = Collider::solid(Shape::Aabb {
        half: Vec2::splat(3.0),
    });
    let crowd = |count: u32| {
        (0..count).map(move |i| {
            let at = Vec2::new((i % 40) as f32 * 8.0, (i / 40) as f32 * 8.0);
            (u64::from(i), at, if i % 4 == 0 { zone } else { body })
        })
    };
    // The caller's buffers, made big enough beforehand.
    let mut out = Vec::with_capacity(512);
    let mut contacts = Vec::with_capacity(64);

    world.rebuild(crowd(1600));
    for index in 0..40 {
        world.set_position(index, world.position(index) + Vec2::new(40.0, 0.0));
    }
    triggers.update(&world);
    let heavy = capacities(&world, &triggers, [0; 3]);

    world.rebuild(crowd(100));
    world.set_position(1, Vec2::new(500.0, 500.0));
    triggers.update(&world);
    triggers.update(&world);
    let light = capacities(&world, &triggers, [0; 3]);
    for (index, (light, heavy)) in light.iter().zip(&heavy).enumerate() {
        assert!(
            light >= heavy,
            "buffer {index} shrank from {heavy} to {light}"
        );
    }

    world.aabb(
        Rect::new(Vec2::ZERO, Vec2::splat(100.0)),
        LayerMask::ALL,
        &mut out,
    );
    world.circle(Vec2::splat(20.0), 30.0, LayerMask::ALL, &mut out);
    world.point(Vec2::splat(20.0), LayerMask::ALL, &mut out);
    let start = world.position(2);
    move_and_slide(&world, 2, start, Vec2::new(3.0, 0.0), &mut contacts);
    assert!(!contacts.is_empty());
    assert_eq!((out.capacity(), contacts.capacity()), (512, 64));
}
