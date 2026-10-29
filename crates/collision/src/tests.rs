//! Tests across the crate's parts: a steady tick keeps its buffers, and
//! `move_and_slide`'s shortcuts change no move.

use nadir_core::{IVec2, Pcg32, Rect, Vec2};

use crate::slide::{FALLBACKS, Shortcuts, move_with};
use crate::{
    Collider, ColliderKind, CollisionWorld, Contact, LayerMask, Obstacle, Shape, TileGrid,
    Triggers, move_and_slide,
};

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

/// Both of `move_and_slide`'s shortcuts, as the game takes them.
const BOTH: Shortcuts = Shortcuts {
    gather: true,
    first_pass: true,
};

/// The gather, with each stopped sweep's contacts from a second pass.
const GATHER_ONLY: Shortcuts = Shortcuts {
    gather: true,
    first_pass: false,
};

/// Neither: each sweep asks the spatial hash, and finds its contacts in a
/// second pass, as `move_and_slide` did before ADR-0026.
const NEITHER: Shortcuts = Shortcuts {
    gather: false,
    first_pass: false,
};

/// Whether two coordinates are the same bits, a zero of either sign being
/// one zero, as the world hash has it (ADR-0025): the order the solids
/// come in can choose the sign of a zero stop.
fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a == 0.0 && b == 0.0)
}

/// A size for the shortcut test: often ordinary, sometimes 0 of either
/// sign, a hair, or negative, which is meaningless but must not change.
fn size(rng: &mut Pcg32, most: f32) -> f32 {
    match rng.range(0..12u32) {
        0 => 0.0,
        1 => -0.0,
        2 => rng.range(0.0..0.02),
        3 => -rng.range(0.0..3.0),
        _ => rng.range(0.5..most),
    }
}

/// A box or a circle of [`size`]s.
fn shape(rng: &mut Pcg32, most: f32) -> Shape {
    if rng.range(0..2u32) == 0 {
        Shape::Circle {
            radius: size(rng, most),
        }
    } else {
        Shape::Aabb {
            half: Vec2::new(size(rng, most), size(rng, most)),
        }
    }
}

/// One world of the shortcut test: tiles, colliders of every kind, some of
/// them moved since the rebuild, and colliders too big for the hash.
/// Colliders in the hash, and movers other than the giants kept aside, stay
/// within ±131,072 px, where an f32 step is under `EPSILON`, and are at
/// most 15,000 px wide. Past that, a sweep's own query can miss
/// a solid by a step, which the gather finds (ADR-0026, and the backlog).
fn shortcut_world(rng: &mut Pcg32) -> (CollisionWorld, Vec<u32>) {
    let origin = Vec2::new(rng.range(-1.0..1.0), rng.range(-1.0..1.0))
        * [0.0, 100.0, 30_000.0, 100_000.0][rng.range(0..4u32) as usize];
    let tile = [1.0, 8.0, 13.7, 16.0, 32.0][rng.range(0..5u32) as usize];
    let (width, height) = (rng.range(4..30u32), rng.range(4..30u32));
    let mut grid = TileGrid::new(width, height, tile, origin);
    let solid = rng.range(0.0..0.4);
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            if rng.next_f32() < solid {
                grid.set_solid(IVec2::new(x, y), true);
            }
        }
    }
    if rng.range(0..4u32) == 0 {
        grid.layer = LayerMask::bit(rng.range(0..3u32));
    }
    let span = Vec2::new(width as f32, height as f32) * tile;
    let cell = [64.0, 16.0, 8.0, 200.0][rng.range(0..4u32) as usize];
    let mut world = CollisionWorld::with_cell_size(cell);
    world.set_grid(grid);

    // A crowd, sometimes packed into a few pixels, so that some moves have
    // more than 32 solids near and some sweeps more than 16 stopping them.
    let packed = rng.range(0..3u32) == 0;
    let most = [2.0, 8.0, 30.0][rng.range(0..3u32) as usize];
    let count = [1, 5, 40, 150][rng.range(0..4u32) as usize];
    let mut colliders: Vec<(u64, Vec2, Collider)> = (0..count)
        .map(|id| {
            let at = if packed {
                origin + span * 0.5 + Vec2::new(rng.range(-12.0..12.0), rng.range(-12.0..12.0))
            } else {
                origin + Vec2::new(rng.range(-0.1..1.1), rng.range(-0.1..1.1)) * span
            };
            let mut collider = Collider::solid(shape(rng, most));
            if rng.range(0..3u32) == 0 {
                collider.offset = Vec2::new(rng.range(-6.0..6.0), rng.range(-6.0..6.0));
            }
            if rng.range(0..5u32) == 0 {
                collider.kind = ColliderKind::Trigger;
            }
            if rng.range(0..4u32) == 0 {
                collider.layer = LayerMask::bit(rng.range(0..3u32));
            }
            if rng.range(0..4u32) == 0 {
                collider.mask =
                    LayerMask::bit(rng.range(0..3u32)) | LayerMask::bit(rng.range(0..3u32));
            }
            (id, at, collider)
        })
        .collect();
    // Zero-width walls on -0.0, where the order of the solids can choose
    // the sign of a zero stop.
    if rng.range(0..6u32) == 0 {
        for (id, y) in (1_000..).zip([-6.0, 0.0, 7.0]) {
            let wall = Collider {
                offset: Vec2::new(-0.0, 0.0),
                ..Collider::solid(Shape::Aabb {
                    half: Vec2::new(0.0, rng.range(1.0..8.0)),
                })
            };
            colliders.push((id, Vec2::new(-0.0, y), wall));
        }
        let wall = Collider::solid(Shape::Aabb {
            half: Vec2::new(0.0, rng.range(1.0..8.0)),
        });
        colliders.push((1_003, Vec2::new(0.0, -14.0), wall));
    }
    // Solids kept aside: over 1,024 cells, or giants whose edges round by
    // pixels, which the gather must keep whatever its region.
    let mut big = Vec::new();
    for id in 2_000..2_000 + rng.range(0..3u32) {
        let near = origin + span * rng.range(0.0..1.0);
        let (at, shape) = match rng.range(0..3u32) {
            0 => (
                near,
                Shape::Aabb {
                    half: Vec2::splat(rng.range(150.0..300.0)) * cell / 8.0,
                },
            ),
            1 => (
                Vec2::new(near.x, near.y + 1.0e9 + rng.range(0.0..64.0)),
                Shape::Aabb {
                    half: Vec2::splat(1.0e9),
                },
            ),
            _ => (
                Vec2::new(near.x + 2.0e20, near.y),
                Shape::Circle { radius: 2.0e20 },
            ),
        };
        big.push(colliders.len() as u32);
        colliders.push((u64::from(id), at, Collider::solid(shape)));
    }
    world.rebuild(colliders);

    // Moved since the rebuild: a few pixels, far, or to no number at all.
    let len = world.len() as u32;
    for _ in 0..rng.range(0..len / 2 + 1) {
        let index = rng.range(0..len);
        let at = world.position(index);
        let to = match rng.range(0..40u32) {
            0 => Vec2::new(f32::NAN, at.y),
            1 => Vec2::new(f32::INFINITY, at.y),
            2..=6 => at + Vec2::new(rng.range(-80.0..80.0), rng.range(-80.0..80.0)),
            _ => at + Vec2::new(rng.range(-15.0..15.0), rng.range(-15.0..15.0)) * rng.next_f32(),
        };
        world.set_position(index, to);
    }
    (world, big)
}

#[test]
fn the_shortcuts_change_no_move() {
    let mut rng = Pcg32::new(0x5eed, 48);
    let (mut a, mut b, mut c) = (Vec::new(), Vec::new(), Vec::new());
    let (mut met_big, mut stopped) = (0, 0);
    FALLBACKS.with(|taken| taken.set((0, 0)));
    for round in 0..600 {
        let (world, big) = shortcut_world(&mut rng);
        let len = world.len() as u32;
        for _ in 0..25 {
            let index = rng.range(0..len);
            let mut start = world.position(index);
            if rng.range(0..4u32) == 0 {
                start += Vec2::new(rng.range(-3.0..3.0), rng.range(-3.0..3.0));
            }
            let length = [0.0, 0.005, 0.5, 3.0, 20.0, 300.0][rng.range(0..6u32) as usize];
            let mut delta = Vec2::new(rng.range(-1.0..1.0), rng.range(-1.0..1.0)) * length;
            match rng.range(0..4u32) {
                0 => delta.x = 0.0,
                1 => delta.y = 0.0,
                _ => {}
            }
            let end = move_with(&world, index, start, delta, &mut a, BOTH);
            let second = move_with(&world, index, start, delta, &mut b, GATHER_ONLY);
            let swept = move_with(&world, index, start, delta, &mut c, NEITHER);
            let case = format!("round {round}: {index} from {start:?} by {delta:?}");
            // The first pass finds the contacts the second would, in order.
            assert!(
                end.x.to_bits() == second.x.to_bits() && end.y.to_bits() == second.y.to_bits(),
                "{case}: {end:?}, {second:?}"
            );
            assert_eq!(a, b, "{case}");
            // The gather meets what the sweeps' own queries would.
            assert!(
                same(end.x, swept.x) && same(end.y, swept.y),
                "{case}: {end:?}, {swept:?}"
            );
            assert!(
                a.len() == c.len() && a.iter().all(|contact| c.contains(contact)),
                "{case}: {a:?}, {c:?}"
            );

            stopped += usize::from(!a.is_empty());
            met_big += usize::from(a.iter().any(|contact| {
                matches!(contact.obstacle, Obstacle::Collider(other) if big.contains(&other))
            }));
        }
    }
    // Each path was taken: moves with more solids near than they gather,
    // left to the sweeps; sweeps that more solids would stop than they
    // remember, which take a second pass; and solids kept aside that
    // stopped moves.
    let (crowded, second) = FALLBACKS.with(|taken| taken.get());
    assert!(
        crowded > 0 && second > 0 && met_big > 0 && stopped > 1_000,
        "{crowded}, {second}, {met_big}, {stopped}"
    );
}

#[test]
fn giants_stop_moves_as_the_sweeps_have_them() {
    // Rounding at a giant's size, not the mover's, decides where these
    // moves stop:
    // - a floor 2e9 px across, whose top edge rounds to 512, and a mover
    //   falling onto it: kept aside in cells of 64 px, hashed in 441 cells
    //   of 1e8 px;
    // - a circle of radius 1e20, whose reach overflows, beside a mover going
    //   toward it, kept aside;
    // - in cells of 1e-6 px, a box 8e8 px across whose corners both saturate
    //   to the last cell there is, so that it is hashed in one cell.
    let mover = Collider::solid(Shape::Aabb {
        half: Vec2::splat(40.0),
    });
    let floor = Collider::solid(Shape::Aabb {
        half: Vec2::splat(1.0e9),
    });
    let small = Collider::solid(Shape::Aabb {
        half: Vec2::splat(5.0),
    });
    let moon = Collider::solid(Shape::Circle { radius: 1.0e20 });
    let block = Collider::solid(Shape::Aabb {
        half: Vec2::splat(20.0),
    });
    let wall = Collider::solid(Shape::Aabb {
        half: Vec2::splat(402_653_184.0),
    });
    let cases = [
        (
            CollisionWorld::DEFAULT_CELL_SIZE,
            mover,
            Vec2::new(0.0, 400.0),
            floor,
            Vec2::new(0.0, 1.0e9 + 500.0),
            Vec2::new(0.0, 60.0),
        ),
        (
            1.0e8,
            mover,
            Vec2::new(0.0, 400.0),
            floor,
            Vec2::new(0.0, 1.0e9 + 500.0),
            Vec2::new(0.0, 60.0),
        ),
        (
            CollisionWorld::DEFAULT_CELL_SIZE,
            small,
            Vec2::ZERO,
            moon,
            Vec2::new(2.0e20, 0.0),
            Vec2::new(10.0, 0.0),
        ),
        (
            1.0e-6,
            block,
            Vec2::new(2_900.0, 5_000.0),
            wall,
            Vec2::splat(402_656_224.0),
            Vec2::new(115.0, 0.0),
        ),
    ];
    for (cell, mover, start, giant, at, delta) in cases {
        let mut world = CollisionWorld::with_cell_size(cell);
        world.rebuild([(0, start, mover), (1, at, giant)]);
        let (mut gathered, mut swept) = (Vec::new(), Vec::new());
        let end = move_and_slide(&world, 0, start, delta, &mut gathered);
        assert_eq!(end, move_with(&world, 0, start, delta, &mut swept, NEITHER));
        assert_eq!(gathered, swept);
        assert!(
            end != start + delta && gathered.len() == 1,
            "{end:?}, {gathered:?}"
        );
    }
}

#[test]
fn a_solid_whose_edge_rounds_onto_the_gathers_still_stops_a_move() {
    // At 100,000 px an f32 step is 2^-7. The mover's box ends, rounded to
    // even, exactly where the solid's begins, but the sweep's own arithmetic
    // stops it at 100,000: the gather's margin keeps the solid.
    let mover = Collider::solid(Shape::Aabb {
        half: Vec2::new(0.511_718_75, 4.0),
    });
    let solid = Collider::solid(Shape::Aabb {
        half: Vec2::new(0.5, 4.0),
    });
    // 99,999.757 812 5 and 100,001.015 625: exact in f32, as sums of exact
    // parts.
    let start = Vec2::new(100_000.0 - 0.242_187_5, 0.0);
    let mut world = CollisionWorld::new();
    world.rebuild([
        (0, start, mover),
        (1, Vec2::new(100_001.0 + 0.015_625, 0.0), solid),
    ]);
    for shortcuts in [BOTH, NEITHER] {
        let mut contacts = Vec::new();
        let end = move_with(
            &world,
            0,
            start,
            Vec2::new(0.25, 0.0),
            &mut contacts,
            shortcuts,
        );
        assert_eq!(end, Vec2::new(100_000.0, 0.0));
        let normal = Vec2::new(-1.0, 0.0);
        assert_eq!(
            contacts,
            [Contact {
                obstacle: Obstacle::Collider(1),
                normal
            }]
        );
    }
}
