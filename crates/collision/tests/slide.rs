//! `move_and_slide`: the property and scenario tests of the collision spec
//! (`docs/subsystems/collision.md`).

mod common;

use common::{Solid, coord, deepest, depth, finite, mask, motion, shape, solids};
use nadir_collision::{
    Collider, ColliderKind, CollisionWorld, Contact, EPSILON, LayerMask, Obstacle, Shape, TileGrid,
    move_and_slide,
};
use nadir_core::testing::check_sized;
use nadir_core::{IVec2, Pcg32, Vec2};

/// The side of the square the property tests play in, in pixels.
const ARENA: f32 = 192.0;

/// A world, a mover in it, and its move.
#[derive(Debug)]
struct Case {
    hash_cell: f32,
    grid: TileGrid,
    colliders: Vec<(u64, Vec2, Collider)>,
    mover: u32,
    start: Vec2,
    delta: Vec2,
}

impl Case {
    fn world(&self) -> CollisionWorld {
        let mut world = CollisionWorld::with_cell_size(self.hash_cell);
        world.set_grid(self.grid.clone());
        world.rebuild(self.colliders.iter().copied());
        world
    }

    fn collider(&self) -> Collider {
        self.colliders[self.mover as usize].2
    }
}

/// A grid of `size`-dependent clutter: scattered solid cells, and a wall
/// or two of them, so that movers meet seams.
fn grid(rng: &mut Pcg32, size: u32) -> TileGrid {
    let cell = [1.0, 4.0, 8.0, 16.0, 10.7][rng.range(0..5u32) as usize];
    let cells = (ARENA / cell).ceil() as u32;
    let origin = Vec2::new(coord(rng, -8.0, 8.0), coord(rng, -8.0, 8.0));
    let mut grid = TileGrid::new(cells, cells, cell, origin);
    grid.layer = mask(rng);
    let cells = cells as i32;
    let scattered = size as i32 * cells * cells / 1000;
    for _ in 0..scattered {
        grid.set_solid(IVec2::new(rng.range(0..cells), rng.range(0..cells)), true);
    }
    for _ in 0..rng.range(0..3u32) {
        let (along, at) = (rng.range(0..cells), rng.range(0..cells));
        let length = rng.range(1..cells);
        for step in 0..length.min(cells - along) {
            let cell = if rng.range(0..2u32) == 0 {
                IVec2::new(along + step, at)
            } else {
                IVec2::new(at, along + step)
            };
            grid.set_solid(cell, true);
        }
    }
    grid
}

/// A case whose mover starts overlapping no solid it collides with:
/// anywhere, or a quarter of the time flush against one.
fn case(rng: &mut Pcg32, size: u32) -> Case {
    loop {
        let grid = grid(rng, size);
        let mut colliders: Vec<(u64, Vec2, Collider)> = (0..size / 4)
            .map(|id| {
                let position = Vec2::new(coord(rng, 0.0, ARENA), coord(rng, 0.0, ARENA));
                (u64::from(id), position, common::collider(rng, 24.0))
            })
            .collect();
        let mover = Collider {
            shape: shape(rng, 20.0),
            offset: Vec2::new(coord(rng, -3.0, 3.0), coord(rng, -3.0, 3.0)),
            layer: mask(rng),
            mask: mask(rng),
            kind: if rng.range(0..6u32) == 0 {
                ColliderKind::Trigger
            } else {
                ColliderKind::Solid
            },
        };
        let index = rng.range(0..colliders.len() as u32 + 1);
        colliders.insert(index as usize, (999, Vec2::ZERO, mover));
        let mut case = Case {
            hash_cell: [8.0, 16.0, 64.0][rng.range(0..3u32) as usize],
            grid,
            colliders,
            mover: index,
            start: Vec2::ZERO,
            delta: Vec2::new(motion(rng, 40.0), motion(rng, 40.0)),
        };
        for _ in 0..20 {
            let mut start = Vec2::new(coord(rng, 0.0, ARENA), coord(rng, 0.0, ARENA));
            let world = case.world();
            let others = blockers(&world, &mover, index);
            if rng.range(0..4u32) == 0
                && let Some(other) = rng.choose(&others)
            {
                start = flush_against(rng, mover, other);
            }
            case.colliders[index as usize].1 = start;
            let world = case.world();
            let others = blockers(&world, &mover, index);
            let clear = deepest(&others, mover.center(start), mover.shape)
                .is_none_or(|(depth, _)| depth <= 0.0);
            if clear {
                case.start = start;
                return case;
            }
        }
    }
}

/// The solids that stop `mover`: tiles and, unless it is a trigger, solid
/// colliders.
fn blockers(world: &CollisionWorld, mover: &Collider, index: u32) -> Vec<Solid> {
    let mut all = solids(world, mover.mask, Some(index));
    if mover.kind == ColliderKind::Trigger {
        all.retain(|solid| matches!(solid.obstacle, Obstacle::Tile(_)));
    }
    all
}

/// A position for `mover` touching `other` on a random side, along an
/// axis.
fn flush_against(rng: &mut Pcg32, mover: Collider, other: &Solid) -> Vec2 {
    let reach = mover.shape.half_extents() + other.shape.half_extents();
    let side = if rng.range(0..2u32) == 0 { -1.0 } else { 1.0 };
    let center = if rng.range(0..2u32) == 0 {
        Vec2::new(other.center.x + side * reach.x, other.center.y)
    } else {
        Vec2::new(other.center.x, other.center.y + side * reach.y)
    };
    center - mover.offset
}

/// Whether `value` is between `a` and `b`, either way round.
fn between(value: f32, a: f32, b: f32) -> bool {
    value >= a.min(b) && value <= a.max(b)
}

fn check_move(case: &Case) -> Result<(), String> {
    let world = case.world();
    let mover = case.collider();
    let solids = blockers(&world, &mover, case.mover);
    let mut contacts = vec![Contact {
        obstacle: Obstacle::Tile(IVec2::ZERO),
        normal: Vec2::ZERO,
    }];
    let target = case.start + case.delta;
    let end = move_and_slide(&world, case.mover, case.start, case.delta, &mut contacts);

    if !finite(end) {
        return Err(format!("ended at {end:?}"));
    }
    if case.delta == Vec2::ZERO && (end != case.start || !contacts.is_empty()) {
        return Err(format!("no motion, but ended at {end:?} with {contacts:?}"));
    }
    if let Some((depth, obstacle)) = deepest(&solids, mover.center(end), mover.shape)
        && depth > EPSILON
    {
        return Err(format!("ended {depth} px into {obstacle:?}, at {end:?}"));
    }
    if !between(end.x, case.start.x, target.x) || !between(end.y, case.start.y, target.y) {
        return Err(format!("ended at {end:?}, outside the move to {target:?}"));
    }

    // Nothing under the swept box, by more than EPSILON: it gets all the
    // way, exactly.
    let swept = mover.bounds(case.start).union(mover.bounds(target));
    let half = swept.size() * 0.5 + Vec2::splat(EPSILON);
    let hull = Shape::Aabb { half };
    let open = solids
        .iter()
        .all(|solid| depth(swept.center(), hull, solid.center, solid.shape) <= 0.0);
    if open && end != target {
        return Err(format!(
            "nothing in the way, but ended at {end:?}, not {target:?}"
        ));
    }

    for contact in &contacts {
        if !solids
            .iter()
            .any(|solid| solid.obstacle == contact.obstacle)
        {
            return Err(format!(
                "{contact:?} is not a solid the mover collides with"
            ));
        }
    }
    let axes = [
        (end.x != target.x, Vec2::new(-case.delta.x.signum(), 0.0)),
        (end.y != target.y, Vec2::new(0.0, -case.delta.y.signum())),
    ];
    for (short, normal) in axes {
        if !short {
            continue;
        }
        let stoppers: Vec<_> = contacts.iter().filter(|c| c.normal == normal).collect();
        if stoppers.is_empty() {
            return Err(format!(
                "stopped short at {end:?} with no contact facing {normal:?}"
            ));
        }
        // It stops touching what stopped it, unless it moved across after
        // being stopped: the last motion is along Y, or along X alone.
        if normal.y != 0.0 || case.delta.y == 0.0 {
            let further = mover.center(end) - normal * (2.0 * EPSILON);
            let touching = stoppers.iter().any(|contact| {
                solids.iter().any(|solid| {
                    solid.obstacle == contact.obstacle
                        && depth(further, mover.shape, solid.center, solid.shape) > 0.0
                })
            });
            if !touching {
                return Err(format!("stopped short at {end:?}, apart from {stoppers:?}"));
            }
        }
    }
    // In a move of one substep, every contact stopped it: it touches each
    // at the end, unless it moved across after being stopped.
    let half = mover.shape.half_extents();
    let step = half.x.min(half.y);
    if step > 0.0 && case.delta.length() <= step {
        for contact in &contacts {
            if contact.normal.x != 0.0 && case.delta.y != 0.0 {
                continue;
            }
            let further = mover.center(end) - contact.normal * (2.0 * EPSILON);
            let touching = solids.iter().any(|solid| {
                solid.obstacle == contact.obstacle
                    && depth(further, mover.shape, solid.center, solid.shape) > 0.0
            });
            if !touching {
                return Err(format!(
                    "{contact:?} did not stop the move, which ended at {end:?}"
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn random_moves_among_random_solids_never_end_inside_one() {
    check_sized("move_and_slide among random solids", 400, case, check_move);
}

/// A world of one grid and one mover, collider 0, at `start`.
fn alone(grid: TileGrid, mover: Collider, start: Vec2) -> CollisionWorld {
    let mut world = CollisionWorld::new();
    world.set_grid(grid);
    world.rebuild([(0, start, mover)]);
    world
}

/// The awkward movers the scenarios use: sizes, offsets and grid origins
/// that round.
fn awkward_movers() -> [Collider; 2] {
    let offset = Vec2::new(0.3, -0.1);
    [
        Collider {
            offset,
            ..Collider::solid(Shape::Aabb {
                half: Vec2::new(3.7, 4.3),
            })
        },
        Collider {
            offset,
            ..Collider::solid(Shape::Circle { radius: 3.7 })
        },
    ]
}

#[test]
fn sliding_along_a_wall_of_tiles_never_stops_at_a_seam() {
    for cell in [1.0, 16.0] {
        for mover in awkward_movers() {
            for (wall_along_y, into, along) in [
                (true, 1.0, 1.0),
                (true, -1.0, -1.0),
                (false, 1.0, -1.0),
                (false, -1.0, 1.0),
            ] {
                // A wall one tile thick, 400 px long, 40 px from the start
                // across it.
                let tiles = (480.0 / cell) as u32;
                let mut grid = TileGrid::new(tiles, tiles, cell, Vec2::new(0.1, 0.3));
                let middle = Vec2::splat(240.0);
                let across = if wall_along_y { Vec2::X } else { Vec2::Y };
                let wall = grid.cell_at(middle + across * (into * 40.0));
                for step in -(200.0 / cell) as i32..(200.0 / cell) as i32 {
                    let cell = if wall_along_y {
                        IVec2::new(wall.x, wall.y + step)
                    } else {
                        IVec2::new(wall.x + step, wall.y)
                    };
                    grid.set_solid(cell, true);
                }
                let mut world = alone(grid, mover, middle);
                // Pressing into the wall, and moving along it.
                let delta = if wall_along_y {
                    Vec2::new(into * 1.7, along * 1.3)
                } else {
                    Vec2::new(along * 1.3, into * 1.7)
                };
                let mut position = middle;
                let mut contacts = Vec::new();
                let mut touched = false;
                for tick in 0..140 {
                    let end = move_and_slide(&world, 0, position, delta, &mut contacts);
                    let (moved, expected) = if wall_along_y {
                        (end.y, position.y + delta.y)
                    } else {
                        (end.x, position.x + delta.x)
                    };
                    assert_eq!(
                        moved, expected,
                        "cell {cell}, {mover:?}, tick {tick}: stopped along the wall at {end:?}"
                    );
                    touched |= !contacts.is_empty();
                    position = end;
                    world.set_position(0, position);
                }
                assert!(touched, "cell {cell}, {mover:?}: it never reached the wall");
            }
        }
    }
}

#[test]
fn a_diagonal_move_into_an_inside_corner_settles_without_jitter() {
    for mover in awkward_movers() {
        // A wall on the right, and a floor below it.
        let mut grid = TileGrid::new(20, 20, 16.0, Vec2::new(0.1, 0.3));
        for i in 0..20 {
            grid.set_solid(IVec2::new(15, i), true);
            grid.set_solid(IVec2::new(i, 15), true);
        }
        let corner = grid.cell_rect(IVec2::new(15, 15)).min;
        let mut world = alone(grid, mover, Vec2::splat(40.0));
        let mut position = Vec2::splat(40.0);
        let mut contacts = Vec::new();
        let mut history = Vec::new();
        for _ in 0..120 {
            position = move_and_slide(&world, 0, position, Vec2::splat(2.5), &mut contacts);
            world.set_position(0, position);
            history.push(position);
        }
        let settled = history[history.len() - 1];
        assert!(
            history[history.len() - 30..].iter().all(|&p| p == settled),
            "{mover:?} jittered: {:?}",
            &history[history.len() - 30..]
        );
        // Against both walls.
        let reach = mover.shape.half_extents();
        let center = mover.center(settled);
        assert!(
            (corner.x - reach.x - center.x).abs() <= EPSILON,
            "{mover:?} at {center:?}"
        );
        assert!(
            (corner.y - reach.y - center.y).abs() <= EPSILON,
            "{mover:?} at {center:?}"
        );
        let normals: Vec<Vec2> = contacts.iter().map(|contact| contact.normal).collect();
        assert!(normals.contains(&Vec2::new(-1.0, 0.0)) && normals.contains(&Vec2::new(0.0, -1.0)));
    }
}

#[test]
fn a_fast_projectile_does_not_tunnel_through_a_1_px_wall() {
    let thin_collider = || {
        let mut world = CollisionWorld::new();
        let wall = Collider::solid(Shape::Aabb {
            half: Vec2::new(0.5, 100.0),
        });
        let bullet = Collider::solid(Shape::Circle { radius: 2.0 });
        world.rebuild([
            (0, Vec2::new(100.5, 50.0), wall),
            (1, Vec2::new(0.0, 50.0), bullet),
        ]);
        world
    };
    let thin_tiles = || {
        let mut grid = TileGrid::new(200, 200, 1.0, Vec2::ZERO);
        for y in 0..200 {
            grid.set_solid(IVec2::new(100, y), true);
        }
        let bullet = Collider::solid(Shape::Circle { radius: 2.0 });
        alone(grid, bullet, Vec2::new(0.0, 50.0))
    };
    for (world, bullet) in [(thin_collider(), 1), (thin_tiles(), 0)] {
        let start = Vec2::new(0.0, 50.0);
        for delta in [Vec2::new(500.0, 0.0), Vec2::new(500.0, 37.0)] {
            let near = |hit: Option<nadir_collision::SweepHit>, reach: f32| {
                let hit = hit.unwrap_or_else(|| panic!("the wall stops it: {delta:?}, {reach}"));
                assert!((hit.position.x - (100.0 - reach)).abs() < 1e-3, "{hit:?}");
                assert_eq!(hit.normal, Vec2::new(-1.0, 0.0));
            };
            let ignore = Some(bullet);
            near(
                world.sweep_circle(start, 2.0, delta, LayerMask::ALL, ignore),
                2.0,
            );
            near(
                world.sweep_aabb(start, Vec2::ONE, delta, LayerMask::ALL, ignore),
                1.0,
            );
            near(
                world.segment(start, start + delta, LayerMask::ALL, ignore),
                0.0,
            );
            let mut contacts = Vec::new();
            let end = move_and_slide(&world, bullet, start, delta, &mut contacts);
            assert!((end.x - 98.0).abs() <= EPSILON, "{end:?}");
            assert!(!contacts.is_empty());
        }
    }
}

#[test]
fn a_diagonal_move_follows_its_substeps_past_a_corner() {
    // A box of half 2 at the origin, past a block's corner: moving X then
    // Y in one step would stop at the block; along the diagonal, it clears
    // it.
    let mut world = CollisionWorld::new();
    let block = Collider::solid(Shape::Aabb {
        half: Vec2::new(2.0, 5.5),
    });
    let mover = Collider::solid(Shape::Aabb {
        half: Vec2::splat(2.0),
    });
    world.rebuild([(0, Vec2::new(8.0, -4.5), block), (1, Vec2::ZERO, mover)]);
    let mut contacts = Vec::new();
    let end = move_and_slide(&world, 1, Vec2::ZERO, Vec2::splat(10.0), &mut contacts);
    assert_eq!(end, Vec2::splat(10.0));
    assert!(contacts.is_empty());
}

#[test]
fn a_mover_leaves_what_it_touches_and_what_it_starts_in() {
    let boxed = |half: f32| {
        Collider::solid(Shape::Aabb {
            half: Vec2::splat(half),
        })
    };
    // A wall of tiles whose face is on a cell boundary, at x = 64, and a
    // wall collider whose face is at x = 200.
    let mut grid = TileGrid::new(8, 8, 16.0, Vec2::ZERO);
    for y in 0..8 {
        grid.set_solid(IVec2::new(4, y), true);
    }
    let wall = Collider::solid(Shape::Aabb {
        half: Vec2::new(8.0, 64.0),
    });
    let mut world = CollisionWorld::new();
    world.set_grid(grid);
    world.rebuild([
        (0, Vec2::new(40.0, 40.0), boxed(4.0)),
        (1, Vec2::new(208.0, 40.0), wall),
    ]);
    let mut contacts = Vec::new();

    for (from, into) in [(Vec2::new(40.0, 40.0), 1.0), (Vec2::new(110.0, 40.0), -1.0)] {
        // Up against the tiles from either side, then away.
        let pressed = move_and_slide(&world, 0, from, Vec2::new(into * 50.0, 0.0), &mut contacts);
        let face = if into > 0.0 { 64.0 - 4.0 } else { 80.0 + 4.0 };
        assert_eq!(pressed.x, face, "{pressed:?}");
        assert_eq!(contacts.len(), 1);
        for away in [Vec2::new(-into * 5.0, 0.0), Vec2::new(-into * 5.0, 3.0)] {
            let end = move_and_slide(&world, 0, pressed, away, &mut contacts);
            assert_eq!(end, pressed + away);
            assert!(contacts.is_empty());
        }
    }

    // The same against the collider.
    let pressed = move_and_slide(
        &world,
        0,
        Vec2::new(150.0, 40.0),
        Vec2::new(60.0, 0.0),
        &mut contacts,
    );
    assert_eq!(pressed.x, 196.0);
    assert_eq!(
        contacts,
        [Contact {
            obstacle: Obstacle::Collider(1),
            normal: Vec2::new(-1.0, 0.0)
        }]
    );
    let end = move_and_slide(&world, 0, pressed, Vec2::new(-5.0, 2.0), &mut contacts);
    assert_eq!(end, pressed + Vec2::new(-5.0, 2.0));

    // Starting 3 px into the collider: it leaves by the near side, but
    // goes no deeper.
    let inside = Vec2::new(199.0, 40.0);
    let end = move_and_slide(&world, 0, inside, Vec2::new(-10.0, 0.0), &mut contacts);
    assert_eq!(end, inside + Vec2::new(-10.0, 0.0));
    let end = move_and_slide(&world, 0, inside, Vec2::new(10.0, 0.0), &mut contacts);
    assert_eq!(end, inside);
    assert_eq!(contacts.len(), 1);
}

#[test]
fn layers_decide_what_stops_a_mover() {
    let mut grid = TileGrid::new(8, 8, 16.0, Vec2::ZERO);
    for y in 0..8 {
        grid.set_solid(IVec2::new(4, y), true);
    }
    grid.layer = LayerMask::bit(0);
    let ghost = Collider {
        mask: LayerMask::bit(1),
        ..Collider::solid(Shape::Circle { radius: 4.0 })
    };
    let walker = Collider {
        mask: LayerMask::bit(0),
        ..ghost
    };
    let hazard = Collider {
        kind: ColliderKind::Trigger,
        ..walker
    };
    let crate_on_layer_0 = Collider {
        layer: LayerMask::bit(0),
        ..Collider::solid(Shape::Aabb {
            half: Vec2::splat(4.0),
        })
    };
    let mut world = CollisionWorld::new();
    world.set_grid(grid);
    world.rebuild([
        (0, Vec2::new(20.0, 20.0), ghost),
        (1, Vec2::new(20.0, 60.0), walker),
        (2, Vec2::new(20.0, 100.0), hazard),
        (3, Vec2::new(40.0, 100.0), crate_on_layer_0),
    ]);
    let mut contacts = Vec::new();
    let right = Vec2::new(80.0, 0.0);
    // The ghost's mask misses the tiles' layer.
    assert_eq!(
        move_and_slide(&world, 0, Vec2::new(20.0, 20.0), right, &mut contacts),
        Vec2::new(100.0, 20.0)
    );
    // The walker's does not.
    assert_eq!(
        move_and_slide(&world, 1, Vec2::new(20.0, 60.0), right, &mut contacts),
        Vec2::new(60.0, 60.0)
    );
    // A trigger passes through the crate on its mask's layer, and stops
    // at the tiles.
    assert_eq!(
        move_and_slide(&world, 2, Vec2::new(20.0, 100.0), right, &mut contacts),
        Vec2::new(60.0, 100.0)
    );
    assert_eq!(
        contacts,
        [Contact {
            obstacle: Obstacle::Tile(IVec2::new(4, 6)),
            normal: Vec2::new(-1.0, 0.0)
        }]
    );
}

#[test]
fn shapes_of_no_size_or_negative_size_give_numbers() {
    let mut grid = TileGrid::new(8, 8, 16.0, Vec2::ZERO);
    grid.set_solid(IVec2::new(4, 4), true);
    let shapes = [
        Shape::Aabb { half: Vec2::ZERO },
        Shape::Aabb {
            half: Vec2::new(-3.0, 2.0),
        },
        Shape::Circle { radius: -1.0 },
        Shape::Circle { radius: 0.0 },
    ];
    for shape in shapes {
        let world = alone(grid.clone(), Collider::solid(shape), Vec2::splat(10.0));
        let mut contacts = Vec::new();
        let end = move_and_slide(
            &world,
            0,
            Vec2::splat(10.0),
            Vec2::splat(100.0),
            &mut contacts,
        );
        assert!(finite(end), "{shape:?}: {end:?}");
        let hit = world.sweep_aabb(
            Vec2::splat(10.0),
            shape.half_extents(),
            Vec2::splat(100.0),
            LayerMask::ALL,
            Some(0),
        );
        assert!(
            hit.is_none_or(|hit| finite(hit.position) && hit.t.is_finite()),
            "{shape:?}: {hit:?}"
        );
    }
}

#[test]
fn a_mover_thinner_than_epsilon_does_not_slip_along_a_seam() {
    // A wall of 1 px tiles at x = 100; the mover runs exactly along the
    // line between two of its rows, and just off it.
    let mut grid = TileGrid::new(200, 100, 1.0, Vec2::ZERO);
    for y in 0..100 {
        grid.set_solid(IVec2::new(100, y), true);
    }
    for half in [0.0, 0.004, 0.008] {
        for y in [50.0, 50.003, 49.996] {
            let mover = Collider::solid(Shape::Aabb {
                half: Vec2::splat(half),
            });
            let start = Vec2::new(10.0, y);
            let world = alone(grid.clone(), mover, start);
            let mut contacts = Vec::new();
            let end = move_and_slide(&world, 0, start, Vec2::new(300.0, 0.0), &mut contacts);
            assert_eq!(end.x, 100.0 - half, "half {half} at y {y}: {end:?}");
            // Near the line, it touches both rows.
            let rows: Vec<IVec2> = contacts
                .iter()
                .map(|contact| match contact.obstacle {
                    Obstacle::Tile(cell) => cell,
                    Obstacle::Collider(_) => panic!("{contact:?}"),
                })
                .collect();
            assert!(
                !rows.is_empty()
                    && rows
                        .iter()
                        .all(|cell| *cell == IVec2::new(100, 49) || *cell == IVec2::new(100, 50)),
                "half {half} at y {y}: {contacts:?}"
            );
            // Along the wall's face, it passes every seam.
            let beside = Vec2::new(100.0 - half, y);
            let world = alone(grid.clone(), mover, beside);
            let end = move_and_slide(&world, 0, beside, Vec2::new(0.0, 20.0), &mut contacts);
            assert_eq!(end, beside + Vec2::new(0.0, 20.0), "half {half} at y {y}");
        }
    }
}

#[test]
fn a_thin_mover_passes_the_ends_of_a_wall_and_finds_the_seams_that_round() {
    // A wall of 1 px tiles at x = 100, rows 0 to 49 only: along its top and
    // bottom ends, a point passes.
    let mut grid = TileGrid::new(200, 100, 1.0, Vec2::ZERO);
    for y in 0..50 {
        grid.set_solid(IVec2::new(100, y), true);
    }
    let point = Collider::solid(Shape::Aabb { half: Vec2::ZERO });
    for y in [0.0, 50.0] {
        let start = Vec2::new(10.0, y);
        let world = alone(grid.clone(), point, start);
        let mut contacts = Vec::new();
        let end = move_and_slide(&world, 0, start, Vec2::new(300.0, 0.0), &mut contacts);
        assert_eq!(end, start + Vec2::new(300.0, 0.0), "at y {y}");
        assert!(contacts.is_empty(), "at y {y}: {contacts:?}");
    }
    // Cells of 0.7 px from y = -4: neighbouring rows round to the same
    // edge, and a point on it does not slip between them.
    let mut grid = TileGrid::new(64, 256, 0.7, Vec2::new(0.0, -4.0));
    grid.set_solid(IVec2::new(20, 6), true);
    grid.set_solid(IVec2::new(20, 7), true);
    let seam = grid.cell_rect(IVec2::new(20, 7)).min.y;
    assert_eq!(seam, grid.cell_rect(IVec2::new(20, 6)).max.y);
    for y in [seam, seam - 1e-6, seam + 1e-6] {
        let start = Vec2::new(0.0, y);
        let world = alone(grid.clone(), point, start);
        let mut contacts = Vec::new();
        let end = move_and_slide(&world, 0, start, Vec2::new(42.0, 0.0), &mut contacts);
        assert_eq!(end.x, 14.0, "at y {y}: {end:?}");
    }
}

#[test]
fn the_tolerance_across_the_motion_is_epsilon() {
    // A block that overlaps the mover's rows by 0.008 px lets it pass; by
    // 0.015 px, it stops it. The same for tiles.
    let mover = Collider::solid(Shape::Aabb {
        half: Vec2::splat(4.0),
    });
    let block = Collider::solid(Shape::Aabb {
        half: Vec2::splat(4.0),
    });
    for (overlap, passes) in [(0.008, true), (0.015, false)] {
        let mut world = CollisionWorld::new();
        world.rebuild([
            (0, Vec2::ZERO, mover),
            (1, Vec2::new(20.0, 8.0 - overlap), block),
        ]);
        let mut contacts = Vec::new();
        let end = move_and_slide(&world, 0, Vec2::ZERO, Vec2::new(20.0, 0.0), &mut contacts);
        let expected = if passes { 20.0 } else { 12.0 };
        assert_eq!(end, Vec2::new(expected, 0.0), "overlap {overlap}");
        // A cast along the same line agrees.
        let hit = world.sweep_aabb(
            Vec2::ZERO,
            Vec2::splat(4.0),
            Vec2::new(20.0, 0.0),
            LayerMask::ALL,
            Some(0),
        );
        assert_eq!(hit.is_none(), passes, "overlap {overlap}: {hit:?}");

        // Row 0 starts `overlap` above the mover's bottom edge, at y = 8.
        let mut grid = TileGrid::new(16, 16, 16.0, Vec2::new(0.0, 8.0 - overlap));
        grid.set_solid(IVec2::new(2, 0), true);
        let start = Vec2::new(4.0, 4.0);
        let world = alone(grid, mover, start);
        let end = move_and_slide(&world, 0, start, Vec2::new(40.0, 0.0), &mut contacts);
        let expected = if passes { 44.0 } else { 28.0 };
        assert_eq!(end, Vec2::new(expected, 4.0), "tiles, overlap {overlap}");
        let hit = world.sweep_aabb(
            start,
            Vec2::splat(4.0),
            Vec2::new(40.0, 0.0),
            LayerMask::ALL,
            Some(0),
        );
        assert_eq!(hit.is_none(), passes, "tiles, overlap {overlap}: {hit:?}");
    }
}

#[test]
fn substeps_count_from_the_smallest_dimension() {
    // A tall mover past a tall block's corner, diagonally: in steps of
    // under half its width it clears the block; in one step, X first
    // would stop it.
    let mut world = CollisionWorld::new();
    let tall = Collider::solid(Shape::Aabb {
        half: Vec2::new(2.0, 10.0),
    });
    world.rebuild([(0, Vec2::new(8.0, -19.0), tall), (1, Vec2::ZERO, tall)]);
    let mut contacts = Vec::new();
    let end = move_and_slide(&world, 1, Vec2::ZERO, Vec2::splat(7.0), &mut contacts);
    assert_eq!(end, Vec2::splat(7.0));
    assert!(contacts.is_empty(), "{contacts:?}");
}

#[test]
fn a_motion_or_start_that_is_not_a_number_moves_nothing() {
    let mut grid = TileGrid::new(8, 8, 16.0, Vec2::ZERO);
    grid.set_solid(IVec2::new(4, 1), true);
    let world = alone(
        grid,
        Collider::solid(Shape::Circle { radius: 4.0 }),
        Vec2::splat(20.0),
    );
    let start = Vec2::splat(20.0);
    for delta in [
        Vec2::new(f32::NAN, 0.0),
        Vec2::new(f32::INFINITY, 0.0),
        Vec2::new(0.0, f32::NEG_INFINITY),
    ] {
        let mut contacts = vec![];
        assert_eq!(
            move_and_slide(&world, 0, start, delta, &mut contacts),
            start,
            "{delta:?}"
        );
        assert!(contacts.is_empty());
    }
    let nowhere = Vec2::new(f32::NAN, 20.0);
    let mut contacts = vec![];
    let end = move_and_slide(&world, 0, nowhere, Vec2::new(5.0, 0.0), &mut contacts);
    assert!(end.x.is_nan() && end.y == 20.0 && contacts.is_empty());
}

#[test]
fn two_movers_on_the_same_spot_can_part() {
    let block = Collider::solid(Shape::Aabb {
        half: Vec2::splat(4.0),
    });
    let mut world = CollisionWorld::new();
    world.rebuild([(0, Vec2::splat(50.0), block), (1, Vec2::splat(50.0), block)]);
    let mut contacts = Vec::new();
    let end = move_and_slide(
        &world,
        0,
        Vec2::splat(50.0),
        Vec2::new(10.0, 0.0),
        &mut contacts,
    );
    assert_eq!(end, Vec2::new(60.0, 50.0));
    assert!(contacts.is_empty());
    // A cast that starts inside a box and moves across its center, not
    // toward it, passes.
    let hit = world.segment(
        Vec2::new(50.0, 53.0),
        Vec2::new(70.0, 53.0),
        LayerMask::ALL,
        None,
    );
    assert_eq!(hit, None);
}

#[test]
fn contacts_are_only_what_stopped_the_move() {
    let mover = Collider::solid(Shape::Aabb {
        half: Vec2::splat(2.0),
    });
    let wall = Collider::solid(Shape::Aabb {
        half: Vec2::new(1.0, 10.0),
    });
    let far = Collider::solid(Shape::Aabb {
        half: Vec2::new(2.0, 20.0),
    });
    let mut world = CollisionWorld::new();
    // A wall the mover meets; one half a pixel further on, which it does
    // not; and one 30 px on.
    world.rebuild([
        (0, Vec2::ZERO, mover),
        (1, Vec2::new(51.0, 0.0), wall),
        (2, Vec2::new(51.5, 5.0), wall),
        (3, Vec2::new(90.0, 0.0), far),
    ]);
    let mut contacts = Vec::new();
    let end = move_and_slide(&world, 0, Vec2::ZERO, Vec2::new(100.0, 0.0), &mut contacts);
    assert_eq!(end, Vec2::new(48.0, 0.0));
    assert_eq!(
        contacts,
        [Contact {
            obstacle: Obstacle::Collider(1),
            normal: Vec2::new(-1.0, 0.0)
        }]
    );
}

#[test]
fn colliders_and_tiles_start_on_every_layer() {
    let grid = TileGrid::new(8, 8, 16.0, Vec2::ZERO);
    assert_eq!(grid.layer, LayerMask::ALL);
    for collider in [
        Collider::solid(Shape::Circle { radius: 1.0 }),
        Collider::trigger(Shape::Circle { radius: 1.0 }),
    ] {
        assert_eq!(
            (collider.layer, collider.mask),
            (LayerMask::ALL, LayerMask::ALL)
        );
    }
    // So a mover whose mask is any one layer stops at a default grid's
    // tiles.
    let mut grid = grid;
    grid.set_solid(IVec2::new(3, 0), true);
    let mover = Collider {
        mask: LayerMask::bit(3),
        ..Collider::solid(Shape::Aabb {
            half: Vec2::splat(4.0),
        })
    };
    let world = alone(grid, mover, Vec2::new(8.0, 8.0));
    let mut contacts = Vec::new();
    let end = move_and_slide(
        &world,
        0,
        Vec2::new(8.0, 8.0),
        Vec2::new(40.0, 0.0),
        &mut contacts,
    );
    assert_eq!(end, Vec2::new(44.0, 8.0));
}

/// How far a box mover of `half` at `center` overlaps `tile`, in f64.
fn overlap64(center: (f64, f64), half: Vec2, tile: nadir_core::Rect) -> f64 {
    let (hx, hy) = (f64::from(half.x), f64::from(half.y));
    let across = |c: f64, h: f64, min: f32, max: f32| {
        (c + h).min(f64::from(max)) - (c - h).max(f64::from(min))
    };
    across(center.0, hx, tile.min.x, tile.max.x).min(across(center.1, hy, tile.min.y, tile.max.y))
}

#[test]
fn far_from_the_origin_the_overlap_bound_holds_to_within_one_f32_step() {
    // A tile overlapping a mover's rows by about EPSILON, which it passes:
    // it ends no more than EPSILON into it, plus a step of f32 at that
    // distance from the origin.
    let mut rng = Pcg32::new(0x0066_6172, 0);
    for (far, step) in [(16_384.0f32, 0.002f64), (65_536.0, 0.008)] {
        let mut worst = 0.0f64;
        for _ in 0..20_000 {
            let by = Vec2::new(rng.range(-far..far), rng.range(-far..far));
            let cell = [16.0f32, 10.7, 8.0][rng.range(0..3u32) as usize];
            let half = Vec2::new(rng.range(2.0..9.0), rng.range(2.0..9.0));
            let offset = Vec2::new(rng.range(-2.0..2.0), rng.range(-2.0..2.0));
            let mover = Collider {
                offset,
                ..Collider::solid(Shape::Aabb { half })
            };
            let mut grid = TileGrid::new(8, 8, cell, by);
            grid.set_solid(IVec2::new(4, 4), true);
            let tile = grid.cell_rect(IVec2::new(4, 4));
            let over = EPSILON + rng.range(-0.004..0.004);
            let start =
                Vec2::new(tile.min.x - half.x - cell * 1.5, tile.min.y - half.y + over) - offset;
            let world = alone(grid, mover, start);
            let mut contacts = Vec::new();
            let end = move_and_slide(&world, 0, start, Vec2::new(cell * 3.0, 0.0), &mut contacts);
            let center = (
                f64::from(end.x) + f64::from(offset.x),
                f64::from(end.y) + f64::from(offset.y),
            );
            worst = worst.max(overlap64(center, half, tile));
        }
        assert!(worst <= f64::from(EPSILON) + step, "±{far}: {worst}");
    }
}
