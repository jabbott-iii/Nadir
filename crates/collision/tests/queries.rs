//! The overlap queries and the casts against brute force over every
//! collider and tile, with colliders moved since the rebuild.

mod common;

use common::{coord, deepest, depth, finite, mask, motion, shape, solids};
use nadir_collision::{
    Collider, CollisionWorld, EPSILON, LayerMask, Obstacle, Shape, SweepHit, TileGrid,
};
use nadir_core::testing::check_sized;
use nadir_core::{IVec2, Pcg32, Rect, Vec2};

/// The side of the square the colliders start in, in pixels.
const ARENA: f32 = 256.0;

/// A world and the queries to ask it.
#[derive(Debug)]
struct Case {
    hash_cell: f32,
    grid: TileGrid,
    colliders: Vec<(u64, Vec2, Collider)>,
    /// Moves since the rebuild: a few within a cell, a few far.
    moves: Vec<(u32, Vec2)>,
    probes: Vec<Probe>,
}

/// One query.
#[derive(Debug)]
enum Probe {
    Point(Vec2, LayerMask),
    Aabb(Rect, LayerMask),
    Circle(Vec2, f32, LayerMask),
    Nearest(Vec2, f32, LayerMask, Option<u32>),
    Cast(Vec2, Shape, Vec2, LayerMask, Option<u32>),
}

fn case(rng: &mut Pcg32, size: u32) -> Case {
    let hash_cell = [4.0, 8.0, 16.0, 64.0][rng.range(0..4u32) as usize];
    let cell = [1.0, 8.0, 16.0][rng.range(0..3u32) as usize];
    let cells = (ARENA / cell) as u32;
    let mut grid = TileGrid::new(cells, cells, cell, Vec2::new(coord(rng, -4.0, 4.0), 0.0));
    grid.layer = mask(rng);
    for _ in 0..size as i32 * (cells * cells) as i32 / 2000 {
        let cells = cells as i32;
        grid.set_solid(IVec2::new(rng.range(0..cells), rng.range(0..cells)), true);
    }
    let count = size / 2;
    let colliders: Vec<(u64, Vec2, Collider)> = (0..count)
        .map(|id| {
            let position = Vec2::new(coord(rng, 0.0, ARENA), coord(rng, 0.0, ARENA));
            // Now and then one too big to hash.
            let most = if rng.range(0..25u32) == 0 {
                600.0
            } else {
                32.0
            };
            (u64::from(id) * 7, position, common::collider(rng, most))
        })
        .collect();
    let moves = (0..count / 3)
        .map(|_| {
            let index = rng.range(0..count);
            let reach = if rng.range(0..4u32) == 0 {
                80.0
            } else {
                hash_cell * 0.25
            };
            (index, Vec2::new(motion(rng, reach), motion(rng, reach)))
        })
        .collect();
    let mut case = Case {
        hash_cell,
        grid,
        colliders,
        moves,
        probes: Vec::new(),
    };
    let world = case.world();
    let anywhere = |rng: &mut Pcg32| {
        Vec2::new(
            coord(rng, -20.0, ARENA + 20.0),
            coord(rng, -20.0, ARENA + 20.0),
        )
    };
    for _ in 0..12 {
        // A third of the time, a point just inside the leading edge of a
        // moved collider: past the cells it was hashed in, if any.
        if let Some(&(index, by)) = rng.choose(&case.moves)
            && rng.range(0..3u32) == 0
        {
            let collider = world.collider(index);
            let center = collider.center(world.position(index));
            let toward = Vec2::new(by.x.signum(), by.y.signum());
            let edge = match collider.shape {
                Shape::Aabb { half } => {
                    center + toward * (half - Vec2::splat(0.01)).max(Vec2::ZERO)
                }
                Shape::Circle { radius } => {
                    center + by.normalize_or_zero() * (radius - 0.01).max(0.0)
                }
            };
            let probe = match rng.range(0..3u32) {
                0 => Probe::Point(edge, LayerMask::ALL),
                1 => Probe::Aabb(
                    Rect::from_center_size(edge, Vec2::splat(0.01)),
                    LayerMask::ALL,
                ),
                _ => Probe::Circle(edge, 0.01, LayerMask::ALL),
            };
            case.probes.push(probe);
            continue;
        }
        let probe = match rng.range(0..5u32) {
            0 => Probe::Point(anywhere(rng), mask(rng)),
            1 => {
                let min = anywhere(rng);
                let size = Vec2::new(motion(rng, 40.0).abs(), motion(rng, 40.0).abs());
                Probe::Aabb(Rect::from_min_size(min, size), mask(rng))
            }
            2 => Probe::Circle(anywhere(rng), coord(rng, 0.0, 40.0), mask(rng)),
            3 => {
                let ignore = (count > 0 && rng.range(0..2u32) == 0).then(|| rng.range(0..count));
                Probe::Nearest(anywhere(rng), coord(rng, 0.0, 60.0), mask(rng), ignore)
            }
            _ => {
                // A cast that starts clear of every solid it can meet.
                let (shape, mask) = match rng.range(0..3u32) {
                    0 => (Shape::Aabb { half: Vec2::ZERO }, mask(rng)),
                    _ => (shape(rng, 12.0), mask(rng)),
                };
                let ignore = (count > 0 && rng.range(0..2u32) == 0).then(|| rng.range(0..count));
                let solids = solids(&world, mask, ignore);
                let start = (0..20).map(|_| anywhere(rng)).find(|&start| {
                    deepest(&solids, start, shape).is_none_or(|(depth, _)| depth <= 0.0)
                });
                let Some(start) = start else { continue };
                let delta = Vec2::new(motion(rng, 300.0), motion(rng, 300.0));
                Probe::Cast(start, shape, delta, mask, ignore)
            }
        };
        case.probes.push(probe);
    }
    case
}

impl Case {
    fn world(&self) -> CollisionWorld {
        self.world_after(&[])
    }

    /// The world, built after a rebuild with `before`: the same colliders
    /// in a hash table of another size and layout.
    fn world_after(&self, before: &[(u64, Vec2, Collider)]) -> CollisionWorld {
        let mut world = CollisionWorld::with_cell_size(self.hash_cell);
        world.set_grid(self.grid.clone());
        world.rebuild(before.iter().copied());
        world.rebuild(self.colliders.iter().copied());
        for &(index, by) in &self.moves {
            world.set_position(index, world.position(index) + by);
        }
        world
    }
}

/// Many colliders over a wide area, to grow a hash table.
fn crowd() -> Vec<(u64, Vec2, Collider)> {
    let mut rng = Pcg32::new(0x6372_6f77, 0);
    (0..3000)
        .map(|id| {
            let at = Vec2::new(rng.range(-3000.0..3000.0), rng.range(-3000.0..3000.0));
            (id, at, Collider::solid(Shape::Circle { radius: 3.0 }))
        })
        .collect()
}

/// Every collider on a layer of `mask` that `probe` overlaps, by brute
/// force, in index order.
fn overlapping(world: &CollisionWorld, center: Vec2, probe: Shape, mask: LayerMask) -> Vec<u32> {
    (0..world.len() as u32)
        .filter(|&index| {
            let collider = world.collider(index);
            collider.layer.intersects(mask)
                && depth(
                    center,
                    probe,
                    collider.center(world.position(index)),
                    collider.shape,
                ) > 0.0
        })
        .collect()
}

/// The same set, with no repeats.
fn same_set(found: &[u32], expected: &[u32]) -> bool {
    let mut sorted = found.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.len() == found.len() && sorted == expected
}

/// The distance from `point` to the shape at `center`.
fn distance(point: Vec2, center: Vec2, shape: Shape) -> f32 {
    let apart = (point - center).abs();
    match shape {
        Shape::Aabb { half } => (apart - half).max(Vec2::ZERO).length(),
        Shape::Circle { radius } => (apart.length() - radius).max(0.0),
    }
}

/// Checks a cast by sampling overlaps along it, apart from the ray maths:
/// nothing in the way before the hit, and touching what it hit, at the
/// hit.
fn check_cast(
    world: &CollisionWorld,
    (start, shape, delta, mask, ignore): (Vec2, Shape, Vec2, LayerMask, Option<u32>),
    hit: Option<SweepHit>,
) -> Result<(), String> {
    let solids = solids(world, mask, ignore);
    let end = hit.map_or(1.0, |hit| hit.t);
    if !(0.0..=1.0).contains(&end) {
        return Err(format!("{hit:?} is outside the cast"));
    }
    // Back off a little before the hit, where rounding may already touch.
    let margin = if delta == Vec2::ZERO {
        0.0
    } else {
        2.0 * EPSILON / delta.length()
    };
    for sample in 0..=512 {
        let t = (end - margin).max(0.0) * sample as f32 / 512.0;
        if let Some((depth, obstacle)) = deepest(&solids, start + delta * t, shape)
            && depth > EPSILON
        {
            return Err(format!(
                "{hit:?} passes {depth} px into {obstacle:?} at t = {t}"
            ));
        }
    }
    let Some(hit) = hit else {
        return Ok(());
    };
    if !finite(hit.position)
        || (hit.normal.length() - 1.0).abs() > 1e-4
        || hit.normal.dot(delta) > 0.0
    {
        return Err(format!("{hit:?} has a bad position or normal"));
    }
    let Some(solid) = solids.iter().find(|solid| solid.obstacle == hit.obstacle) else {
        return Err(format!("{hit:?} hit something it should not meet"));
    };
    let touch = depth(start + delta * hit.t, shape, solid.center, solid.shape);
    if touch.abs() > EPSILON {
        return Err(format!("{hit:?} is {touch} px from touching it"));
    }
    // And it was going in, not grazing: a moment later it overlaps a
    // solid. A point on the seam between two tiles overlaps neither, so
    // this holds for shapes with a size.
    let span = 8.0 * EPSILON / delta.length();
    if shape.half_extents() != Vec2::ZERO && hit.t + span <= 1.0 {
        let goes_in = (1..=8).any(|sample| {
            let t = hit.t + span * sample as f32 / 8.0;
            deepest(&solids, start + delta * t, shape).is_some_and(|(depth, _)| depth > 0.0)
        });
        if !goes_in {
            return Err(format!("{hit:?} only grazes what it hit"));
        }
    }
    Ok(())
}

fn check_case(case: &Case) -> Result<(), String> {
    let world = case.world();
    let again = case.world_after(&crowd());
    let mut out = vec![99];
    let mut repeat = Vec::new();
    for probe in &case.probes {
        match *probe {
            Probe::Point(point, mask) => {
                world.point(point, mask, &mut out);
                again.point(point, mask, &mut repeat);
                let expected = overlapping(&world, point, Shape::Aabb { half: Vec2::ZERO }, mask);
                if !same_set(&out, &expected) {
                    return Err(format!("{probe:?}: {out:?}, not {expected:?}"));
                }
            }
            Probe::Aabb(rect, mask) => {
                world.aabb(rect, mask, &mut out);
                again.aabb(rect, mask, &mut repeat);
                let half = rect.size() * 0.5;
                let expected = if rect.is_empty() {
                    Vec::new()
                } else {
                    overlapping(&world, rect.min + half, Shape::Aabb { half }, mask)
                };
                if !same_set(&out, &expected) {
                    return Err(format!("{probe:?}: {out:?}, not {expected:?}"));
                }
            }
            Probe::Circle(center, radius, mask) => {
                world.circle(center, radius, mask, &mut out);
                again.circle(center, radius, mask, &mut repeat);
                let expected = overlapping(&world, center, Shape::Circle { radius }, mask);
                if !same_set(&out, &expected) {
                    return Err(format!("{probe:?}: {out:?}, not {expected:?}"));
                }
            }
            Probe::Nearest(point, radius, mask, ignore) => {
                let found = world.nearest(point, radius, mask, ignore);
                let expected = (0..world.len() as u32)
                    .filter(|&index| {
                        Some(index) != ignore && world.collider(index).layer.intersects(mask)
                    })
                    .map(|index| {
                        let collider = world.collider(index);
                        let at = collider.center(world.position(index));
                        (distance(point, at, collider.shape), index)
                    })
                    .filter(|&(distance, _)| distance <= radius)
                    .min_by(|a, b| a.partial_cmp(b).expect("distances are numbers"))
                    .map(|(_, index)| index);
                if found != expected {
                    return Err(format!("{probe:?}: {found:?}, not {expected:?}"));
                }
                continue;
            }
            Probe::Cast(start, shape, delta, mask, ignore) => {
                let cast = |world: &CollisionWorld| match shape {
                    Shape::Aabb { half } if half == Vec2::ZERO => {
                        world.segment(start, start + delta, mask, ignore)
                    }
                    Shape::Aabb { half } => world.sweep_aabb(start, half, delta, mask, ignore),
                    Shape::Circle { radius } => {
                        world.sweep_circle(start, radius, delta, mask, ignore)
                    }
                };
                let (hit, repeated) = (cast(&world), cast(&again));
                if hit != repeated {
                    return Err(format!(
                        "{probe:?}: {hit:?} in one world, {repeated:?} in its twin"
                    ));
                }
                check_cast(&world, (start, shape, delta, mask, ignore), hit)
                    .map_err(|error| format!("{probe:?}: {error}"))?;
                continue;
            }
        }
        if out != repeat {
            return Err(format!(
                "{probe:?}: {out:?} in one world, {repeat:?} in its twin"
            ));
        }
    }
    Ok(())
}

#[test]
fn queries_and_casts_match_brute_force() {
    check_sized("queries against brute force", 300, case, check_case);
}

#[test]
fn nearest_counts_a_collider_exactly_at_the_radius() {
    let mut world = CollisionWorld::with_cell_size(16.0);
    let post = Collider::solid(Shape::Aabb {
        half: Vec2::splat(2.0),
    });
    world.rebuild([(0, Vec2::new(26.0, 0.0), post)]);
    // Its near edge is at x = 24, 24 from the point.
    assert_eq!(
        world.nearest(Vec2::ZERO, 24.0, LayerMask::ALL, None),
        Some(0)
    );
    assert_eq!(world.nearest(Vec2::ZERO, 23.99, LayerMask::ALL, None), None);
    assert_eq!(
        world.nearest(Vec2::new(26.0, 0.0), 1.0, LayerMask::ALL, Some(0)),
        None
    );
}

/// A world of one grid, with solid `cells`, and the colliders.
fn tiles(
    cell: f32,
    origin: Vec2,
    cells: &[(i32, i32)],
    colliders: &[(u64, Vec2, Collider)],
) -> CollisionWorld {
    let mut grid = TileGrid::new(64, 64, cell, origin);
    for &(x, y) in cells {
        grid.set_solid(IVec2::new(x, y), true);
    }
    let mut world = CollisionWorld::new();
    world.set_grid(grid);
    world.rebuild(colliders.iter().copied());
    world
}

fn obstacle(hit: Option<SweepHit>) -> Option<Obstacle> {
    hit.map(|hit| hit.obstacle)
}

#[test]
fn queries_give_colliders_kept_aside_then_moved_far_then_by_cell() {
    let small = Collider::solid(Shape::Aabb {
        half: Vec2::splat(2.0),
    });
    let huge = Collider::solid(Shape::Aabb {
        half: Vec2::splat(5000.0),
    });
    let mut world = CollisionWorld::with_cell_size(16.0);
    world.rebuild([
        (10, Vec2::splat(100.0), small),
        (11, Vec2::ZERO, huge),
        (12, Vec2::splat(500.0), small),
        (13, Vec2::splat(900.0), small),
    ]);
    // Moved far, 3 before 2.
    world.set_position(3, Vec2::new(100.5, 100.0));
    world.set_position(2, Vec2::new(99.5, 100.0));
    let mut out = Vec::new();
    world.point(Vec2::splat(100.0), LayerMask::ALL, &mut out);
    assert_eq!(out, [1, 3, 2, 0]);
    world.aabb(
        Rect::from_center_size(Vec2::splat(100.0), Vec2::ONE),
        LayerMask::ALL,
        &mut out,
    );
    assert_eq!(out, [1, 3, 2, 0]);
    world.circle(Vec2::splat(100.0), 0.5, LayerMask::ALL, &mut out);
    assert_eq!(out, [1, 3, 2, 0]);
    // A rect as wide as f32 allows finds everything.
    let everything = Rect::new(Vec2::splat(f32::MIN), Vec2::splat(f32::MAX));
    world.aabb(everything, LayerMask::ALL, &mut out);
    assert_eq!(out, [1, 3, 2, 0]);
}

#[test]
fn ties_go_to_tiles_row_by_row_then_to_the_lower_index() {
    // Two tiles met at the same time: the first row's.
    let world = tiles(16.0, Vec2::ZERO, &[(1, 0), (0, 1)], &[]);
    let hit = world.sweep_aabb(
        Vec2::splat(4.0),
        Vec2::splat(2.0),
        Vec2::splat(20.0),
        LayerMask::ALL,
        None,
    );
    assert_eq!(obstacle(hit), Some(Obstacle::Tile(IVec2::new(1, 0))));
    // Two colliders in the same place: the lower index.
    let post = Collider::solid(Shape::Aabb {
        half: Vec2::splat(4.0),
    });
    let world = tiles(
        16.0,
        Vec2::ZERO,
        &[],
        &[
            (0, Vec2::new(30.0, 0.0), post),
            (1, Vec2::new(30.0, 0.0), post),
        ],
    );
    let hit = world.segment(Vec2::ZERO, Vec2::new(60.0, 0.0), LayerMask::ALL, None);
    assert_eq!(obstacle(hit), Some(Obstacle::Collider(0)));
}

#[test]
fn a_tile_cast_finds_the_earliest_tile_of_its_step() {
    // The first step finds (5, 4) through its margin, but (4, 5) is met
    // first, in the next step.
    let world = tiles(16.0, Vec2::ZERO, &[(5, 4), (4, 5)], &[]);
    let hit = world.sweep_aabb(
        Vec2::new(75.852_14, 61.699_29),
        Vec2::splat(4.0),
        Vec2::new(1.0, 100.0),
        LayerMask::ALL,
        None,
    );
    assert_eq!(
        obstacle(hit),
        Some(Obstacle::Tile(IVec2::new(4, 5))),
        "{hit:?}"
    );
}

#[test]
fn a_line_along_a_seam_inside_a_wall_is_stopped_and_one_along_its_face_passes() {
    for cell in [1.0, 16.0, 10.7, 0.7] {
        for origin in [Vec2::ZERO, Vec2::new(0.3, -4.1)] {
            // A wall two tiles tall, and one two tiles wide.
            let world = tiles(cell, origin, &[(20, 10), (20, 11), (40, 30), (41, 30)], &[]);
            let grid = world.grid();
            let seam = grid.cell_rect(IVec2::new(20, 11)).min.y;
            let face = grid.cell_rect(IVec2::new(20, 10)).min.y;
            let across = |y: f32| {
                world.segment(
                    Vec2::new(origin.x, y),
                    Vec2::new(origin.x + 64.0 * cell, y),
                    LayerMask::ALL,
                    None,
                )
            };
            assert_eq!(
                obstacle(across(seam)),
                Some(Obstacle::Tile(IVec2::new(20, 10))),
                "cell {cell} at {origin:?}"
            );
            assert_eq!(across(face), None, "cell {cell} at {origin:?}");
            let seam = grid.cell_rect(IVec2::new(41, 30)).min.x;
            let face = grid.cell_rect(IVec2::new(40, 30)).min.x;
            let down = |x: f32| {
                world.segment(
                    Vec2::new(x, origin.y),
                    Vec2::new(x, origin.y + 64.0 * cell),
                    LayerMask::ALL,
                    None,
                )
            };
            assert_eq!(
                obstacle(down(seam)),
                Some(Obstacle::Tile(IVec2::new(40, 30))),
                "cell {cell} at {origin:?}"
            );
            assert_eq!(down(face), None, "cell {cell} at {origin:?}");
        }
    }
}

#[test]
fn a_cast_along_a_colliders_edge_or_past_its_corner_grazes_it() {
    let post = Collider::solid(Shape::Aabb {
        half: Vec2::splat(8.0),
    });
    let world = tiles(16.0, Vec2::ZERO, &[], &[(0, Vec2::ZERO, post)]);
    let all = LayerMask::ALL;
    assert_eq!(
        world.segment(Vec2::new(-20.0, -8.0), Vec2::new(20.0, -8.0), all, None),
        None
    );
    assert_eq!(
        world.sweep_aabb(
            Vec2::new(-20.0, -10.0),
            Vec2::splat(2.0),
            Vec2::new(40.0, 0.0),
            all,
            None
        ),
        None
    );
    assert_eq!(
        world.sweep_circle(
            Vec2::new(-20.0, -10.0),
            2.0,
            Vec2::new(40.0, 0.0),
            all,
            None
        ),
        None
    );
    // Past the corner, diagonally, touching it.
    assert_eq!(
        world.segment(Vec2::new(-16.0, 0.0), Vec2::new(0.0, -16.0), all, None),
        None
    );
}

#[test]
fn circle_casts_along_what_they_rest_against_pass() {
    // A circle that move_and_slide left against a wall of tiles, cast
    // along the wall: past every seam inside it.
    let mut grid = TileGrid::new(64, 64, 16.0, Vec2::ZERO);
    for y in 0..64 {
        grid.set_solid(IVec2::new(40, y), true);
    }
    let ball = Collider::solid(Shape::Circle { radius: 4.3 });
    let mut world = CollisionWorld::new();
    world.set_grid(grid);
    world.rebuild([(0, Vec2::new(600.0, 100.0), ball)]);
    let mut contacts = Vec::new();
    let rest = nadir_collision::move_and_slide(
        &world,
        0,
        Vec2::new(600.0, 100.0),
        Vec2::new(60.0, 0.0),
        &mut contacts,
    );
    assert!((rest.x - (640.0 - 4.3)).abs() < 1e-3, "{rest:?}");
    world.set_position(0, rest);
    for delta in [Vec2::new(0.0, 200.0), Vec2::new(0.0, -90.0)] {
        assert_eq!(
            world.sweep_circle(rest, 4.3, delta, LayerMask::ALL, Some(0)),
            None,
            "{delta:?}"
        );
    }
    // The same against a wall of boxes edge to edge.
    let brick = Collider::solid(Shape::Aabb {
        half: Vec2::splat(8.0),
    });
    let wall: Vec<_> = (0..20)
        .map(|i| (i, Vec2::new(648.0, 8.0 + 16.0 * i as f32), brick))
        .collect();
    let world = tiles(16.0, Vec2::ZERO, &[], &wall);
    let hit = world.sweep_circle(
        Vec2::new(640.0 - 4.3, 20.0),
        4.3,
        Vec2::new(0.0, 250.0),
        LayerMask::ALL,
        None,
    );
    assert_eq!(hit, None);
    // A box that move_and_slide left a hair inside a wall, by rounding.
    let mut grid = TileGrid::new(64, 64, 8.0, Vec2::new(-2.132_841, -3.726_627_3));
    for y in 0..64 {
        grid.set_solid(IVec2::new(40, y), true);
    }
    let mover = Collider {
        offset: Vec2::new(0.323_165_9, 0.340_005_87),
        ..Collider::solid(Shape::Aabb {
            half: Vec2::new(3.899_109, 2.907_826_7),
        })
    };
    let mut world = CollisionWorld::new();
    world.set_grid(grid);
    let start = Vec2::new(287.867_16, 96.273_38);
    world.rebuild([(0, start, mover)]);
    let rest =
        nadir_collision::move_and_slide(&world, 0, start, Vec2::new(60.0, 0.0), &mut contacts);
    let center = mover.center(rest);
    let hit = world.sweep_aabb(
        center,
        Vec2::new(3.899_109, 2.907_826_7),
        Vec2::new(0.0, 150.0),
        LayerMask::ALL,
        Some(0),
    );
    assert_eq!(hit, None);
}

#[test]
fn long_casts_meet_what_they_pass_through() {
    let ball = Collider::solid(Shape::Circle { radius: 8.0 });
    let world = tiles(16.0, Vec2::ZERO, &[], &[(0, Vec2::ZERO, ball)]);
    let all = LayerMask::ALL;
    assert!(
        world
            .segment(
                Vec2::new(-60_000.0, 0.0),
                Vec2::new(60_000.0, 0.0),
                all,
                None
            )
            .is_some()
    );
    assert!(
        world
            .segment(
                Vec2::new(-30_000.0, -29_997.0),
                Vec2::new(30_000.0, 30_003.0),
                all,
                None
            )
            .is_some()
    );
    let far = Collider::solid(Shape::Circle { radius: 10.0 });
    let world = tiles(16.0, Vec2::ZERO, &[], &[(0, Vec2::new(100.0, 0.0), far)]);
    for delta in [Vec2::new(1e18, 0.0), Vec2::new(1e30, 1.0)] {
        let hit = world
            .sweep_circle(Vec2::ZERO, 2.0, delta, all, None)
            .expect("it meets the ball");
        assert!(
            hit.normal.approx_eq(Vec2::new(-1.0, 0.0), 1e-6),
            "{delta:?}: {hit:?}"
        );
        assert!(
            world.segment(Vec2::ZERO, delta, all, None).is_some(),
            "{delta:?}"
        );
    }
    // From inside, moving deeper, the normal is still a unit vector.
    let hit = world
        .segment(Vec2::new(95.0, 0.0), Vec2::new(1e20, 0.0), all, None)
        .expect("it starts inside");
    assert_eq!((hit.t, hit.normal), (0.0, Vec2::new(-1.0, 0.0)));
}

#[test]
fn a_huge_rect_keeps_the_colliders_precision() {
    let post = Collider::solid(Shape::Aabb {
        half: Vec2::splat(2.0),
    });
    let world = tiles(
        16.0,
        Vec2::ZERO,
        &[],
        &[
            (0, Vec2::ZERO, post),
            (1, Vec2::new(100.0, 0.0), post),
            (2, Vec2::new(-3.0, 0.0), post),
        ],
    );
    let mut out = Vec::new();
    for far in [1e6, 1e9, 1e12, 1e30, f32::MAX] {
        world.aabb(
            Rect::new(Vec2::splat(-5.0), Vec2::splat(far)),
            LayerMask::ALL,
            &mut out,
        );
        out.sort_unstable();
        assert_eq!(out, [0, 1, 2], "to {far}");
        world.aabb(
            Rect::new(Vec2::splat(-far), Vec2::splat(5.0)),
            LayerMask::ALL,
            &mut out,
        );
        out.sort_unstable();
        assert_eq!(out, [0, 2], "from -{far}");
        // Its edge 1 px clear of a post.
        world.aabb(
            Rect::new(Vec2::new(-far, -far), Vec2::new(-6.0, far)),
            LayerMask::ALL,
            &mut out,
        );
        assert!(out.is_empty(), "from -{far}: {out:?}");
    }
}

#[test]
fn a_cast_from_inside_with_a_motion_too_long_to_square_still_knows_its_way() {
    let post = Collider::solid(Shape::Aabb {
        half: Vec2::splat(8.0),
    });
    let world = tiles(16.0, Vec2::ZERO, &[], &[(0, Vec2::ZERO, post)]);
    // Toward the center, though delta · (start - center) overflows to NaN.
    let start = Vec2::new(2.0, 3.0);
    let hit = world.segment(start, start + Vec2::new(3e38, -3e38), LayerMask::ALL, None);
    let hit = hit.expect("it moves toward the center");
    assert_eq!(hit.t, 0.0);
    assert!(
        hit.normal
            .approx_eq(Vec2::new(-1.0, 1.0) * std::f32::consts::FRAC_1_SQRT_2, 1e-6)
    );
    // Away from it, it leaves.
    let away = world.segment(start, start + Vec2::new(3e38, 3e38), LayerMask::ALL, None);
    assert_eq!(away, None);
}
