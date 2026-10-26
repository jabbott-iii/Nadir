//! Casts: [`CollisionWorld::segment`], [`CollisionWorld::sweep_aabb`] and
//! [`CollisionWorld::sweep_circle`], for projectiles and lines of sight.

use nadir_core::{IVec2, Rect, Vec2};

use crate::geometry::{self, Closed, Target};
use crate::{ColliderKind, CollisionWorld, EPSILON, LayerMask, Shape};

/// What a cast or a move ran into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Obstacle {
    /// A solid cell of the tile grid.
    Tile(IVec2),
    /// A solid collider, by index.
    Collider(u32),
}

/// Where a cast first meets a solid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepHit {
    /// What it meets.
    pub obstacle: Obstacle,
    /// When, as a fraction of the cast's motion: from 0 to 1.
    pub t: f32,
    /// Where the shape's center is then.
    pub position: Vec2,
    /// The unit normal of the surface it meets, facing the shape.
    pub normal: Vec2,
}

impl CollisionWorld {
    /// The first solid on a layer of `mask` that the segment from `start`
    /// to `end` meets, other than collider `ignore`: a line of sight. See
    /// [`sweep_aabb`](Self::sweep_aabb) for what counts.
    pub fn segment(
        &self,
        start: Vec2,
        end: Vec2,
        mask: LayerMask,
        ignore: Option<u32>,
    ) -> Option<SweepHit> {
        let point = Shape::Aabb { half: Vec2::ZERO };
        self.cast(start, point, end - start, mask, ignore)
    }

    /// The first solid on a layer of `mask` that the box of half size
    /// `half`, its center moving from `center` by `delta`, meets: solid
    /// tiles if the grid's layer is in `mask`, and solid colliders other
    /// than `ignore`. Thin walls stop it however fast it goes. A cast from
    /// an entity, such as a projectile from its shooter, should ignore the
    /// entity's own collider.
    ///
    /// - A shape it only grazes does not stop it.
    /// - A shape it starts inside stops it at once, facing back along
    ///   `delta`, if it moves toward that shape's center, and does not stop
    ///   it otherwise: a projectile leaves the shooter it starts in, but
    ///   does not go deeper into a wall it starts in.
    /// - Of two hits at the same time, a tile before a collider, then the
    ///   first cell row by row, or the lower index.
    /// - No motion, or a position or motion that is not finite, meets
    ///   nothing.
    pub fn sweep_aabb(
        &self,
        center: Vec2,
        half: Vec2,
        delta: Vec2,
        mask: LayerMask,
        ignore: Option<u32>,
    ) -> Option<SweepHit> {
        self.cast(center, Shape::Aabb { half }, delta, mask, ignore)
    }

    /// As [`sweep_aabb`](Self::sweep_aabb), for a circle of `radius`.
    pub fn sweep_circle(
        &self,
        center: Vec2,
        radius: f32,
        delta: Vec2,
        mask: LayerMask,
        ignore: Option<u32>,
    ) -> Option<SweepHit> {
        self.cast(center, Shape::Circle { radius }, delta, mask, ignore)
    }

    fn cast(
        &self,
        center: Vec2,
        shape: Shape,
        delta: Vec2,
        mask: LayerMask,
        ignore: Option<u32>,
    ) -> Option<SweepHit> {
        let finite = |v: Vec2| v.x.is_finite() && v.y.is_finite();
        if delta == Vec2::ZERO || !finite(center) || !finite(delta) {
            return None;
        }
        let mut best = Best::default();
        if self.grid().layer.intersects(mask) {
            self.cast_tiles(center, shape, delta, &mut best);
        }
        self.cast_colliders(center, shape, delta, mask, ignore, &mut best);
        best.hit.map(|(t, normal, obstacle)| SweepHit {
            obstacle,
            t,
            position: center + delta * t,
            normal,
        })
    }

    /// Casts against the solid tiles, a step of at most one cell at a
    /// time, so that a long cast looks only at the cells along it, and
    /// stops at the first step that meets one.
    fn cast_tiles(&self, center: Vec2, shape: Shape, delta: Vec2, best: &mut Best) {
        let grid = self.grid();
        let cell_size = grid.cell_size();
        let size = Vec2::new(grid.width() as f32, grid.height() as f32) * cell_size;
        let half = shape.half_extents();
        let area = Rect::new(grid.origin() - half, grid.origin() + size + half);
        let Some((first, last)) = clip(center, delta, area) else {
            return;
        };
        let across = (last - first) * delta.x.abs().max(delta.y.abs());
        let steps = tile_steps(across, cell_size, grid.width(), grid.height());
        for step in 0..steps {
            let from = first + (last - first) * (step as f32 / steps as f32);
            let to = if step + 1 == steps {
                last
            } else {
                first + (last - first) * ((step + 1) as f32 / steps as f32)
            };
            let (a, b) = (center + delta * from, center + delta * to);
            let swept = Rect::new(a.min(b) - half, a.max(b) + half);
            let swept = Rect::new(
                swept.min - Vec2::splat(EPSILON),
                swept.max + Vec2::splat(EPSILON),
            );
            if let Some((min, max)) = grid.cells_touching(swept) {
                for y in min.y..=max.y {
                    for x in min.x..=max.x {
                        let cell = IVec2::new(x, y);
                        if !grid.is_solid(cell) {
                            continue;
                        }
                        let tile = Target::Box(grid.cell_rect(cell));
                        let solid = |dx: i32, dy: i32| grid.is_solid(cell + IVec2::new(dx, dy));
                        let closed = Closed {
                            x: [solid(-1, 0), solid(1, 0)],
                            y: [solid(0, -1), solid(0, 1)],
                        };
                        if let Some((t, normal)) =
                            geometry::cast(shape, center, delta, tile, closed)
                        {
                            best.offer(t, normal, Obstacle::Tile(cell));
                        }
                    }
                }
            }
            if best.hit.is_some_and(|(t, ..)| t <= to) {
                return;
            }
        }
    }

    /// Casts against the solid colliders under the whole cast.
    fn cast_colliders(
        &self,
        center: Vec2,
        shape: Shape,
        delta: Vec2,
        mask: LayerMask,
        ignore: Option<u32>,
        best: &mut Best,
    ) {
        let half = shape.half_extents() + Vec2::splat(EPSILON);
        let end = center + delta;
        let swept = Rect::new(center.min(end) - half, center.max(end) + half);
        self.candidates(swept, |index| {
            let collider = self.collider(index);
            if Some(index) == ignore
                || collider.kind != ColliderKind::Solid
                || !collider.layer.intersects(mask)
            {
                return;
            }
            let other = Target::of(self.placed(index));
            if let Some((t, normal)) =
                geometry::cast(shape, center, delta, other, Closed::default())
            {
                best.offer(t, normal, Obstacle::Collider(index));
            }
        });
    }
}

/// The earliest hit so far.
#[derive(Default)]
struct Best {
    hit: Option<(f32, Vec2, Obstacle)>,
}

impl Best {
    /// Keeps the hit if it is earlier, or as early and first by
    /// [`rank`].
    fn offer(&mut self, t: f32, normal: Vec2, obstacle: Obstacle) {
        let better = match self.hit {
            None => true,
            Some((best_t, _, best)) => t < best_t || (t == best_t && rank(obstacle) < rank(best)),
        };
        if better {
            self.hit = Some((t, normal, obstacle));
        }
    }
}

/// The order of obstacles hit at the same time: tiles, row by row, then
/// colliders by index.
fn rank(obstacle: Obstacle) -> (u8, i32, i64) {
    match obstacle {
        Obstacle::Tile(cell) => (0, cell.y, i64::from(cell.x)),
        Obstacle::Collider(index) => (1, 0, i64::from(index)),
    }
}

/// How many steps a cast takes through a grid of `width` × `height` cells
/// of `cell_size`, going `across` pixels along its larger axis: one per
/// cell. A cast through the grid crosses no more cells than it is wide and
/// tall, but a shape larger than the grid covers it all at every step, so
/// the steps stop there: fewer steps only end the search later.
fn tile_steps(across: f32, cell_size: f32, width: u32, height: u32) -> u32 {
    let most = 2.0 * (width as f32 + height as f32) + 2.0;
    let steps = (across / cell_size).ceil();
    // Written so that NaN takes one step.
    if steps >= 1.0 {
        steps.min(most) as u32
    } else {
        1
    }
}

/// The part of `0..=1` for which `origin + delta * t` is inside the closed
/// `area`, if any.
fn clip(origin: Vec2, delta: Vec2, area: Rect) -> Option<(f32, f32)> {
    let mut first = 0.0f32;
    let mut last = 1.0f32;
    for (from, step, min, max) in [
        (origin.x, delta.x, area.min.x, area.max.x),
        (origin.y, delta.y, area.min.y, area.max.y),
    ] {
        if step == 0.0 {
            if !(from >= min && from <= max) {
                return None;
            }
            continue;
        }
        let (a, b) = ((min - from) / step, (max - from) / step);
        first = first.max(a.min(b));
        last = last.min(a.max(b));
    }
    (first <= last).then_some((first, last))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Collider, TileGrid};

    #[test]
    fn a_long_cast_finds_the_first_tile_along_it() {
        let mut world = CollisionWorld::new();
        let mut grid = TileGrid::new(256, 256, 16.0, Vec2::ZERO);
        grid.set_solid(IVec2::new(200, 10), true);
        grid.set_solid(IVec2::new(250, 10), true);
        world.set_grid(grid);
        let start = Vec2::new(8.0, 168.0);
        let hit = world.segment(start, Vec2::new(4000.0, 168.0), LayerMask::ALL, None);
        let hit = hit.expect("it meets the wall");
        assert_eq!(hit.obstacle, Obstacle::Tile(IVec2::new(200, 10)));
        assert!(
            hit.position.approx_eq(Vec2::new(3200.0, 168.0), 1e-3),
            "{hit:?}"
        );
        assert_eq!(hit.normal, Vec2::new(-1.0, 0.0));
        // Outside the grid's layer, tiles do not stop it.
        world.grid_mut().layer = LayerMask::bit(0);
        assert_eq!(
            world.segment(start, Vec2::new(4000.0, 168.0), LayerMask::bit(1), None),
            None
        );
    }

    #[test]
    fn at_the_same_time_a_tile_comes_before_a_collider() {
        let mut world = CollisionWorld::new();
        let mut grid = TileGrid::new(4, 4, 16.0, Vec2::ZERO);
        grid.set_solid(IVec2::new(2, 0), true);
        world.set_grid(grid);
        let wall = Collider::solid(Shape::Aabb {
            half: Vec2::new(8.0, 8.0),
        });
        world.rebuild([(0, Vec2::new(40.0, 24.0), wall)]);
        // A box 32 tall, centered between the tile and the collider.
        let hit = world.sweep_aabb(
            Vec2::new(8.0, 16.0),
            Vec2::new(4.0, 16.0),
            Vec2::new(40.0, 0.0),
            LayerMask::ALL,
            None,
        );
        assert_eq!(
            hit.map(|hit| hit.obstacle),
            Some(Obstacle::Tile(IVec2::new(2, 0)))
        );
    }

    #[test]
    fn a_cast_that_does_not_move_meets_nothing() {
        let mut world = CollisionWorld::new();
        let wall = Collider::solid(Shape::Circle { radius: 8.0 });
        world.rebuild([(0, Vec2::ZERO, wall)]);
        let ray = |center, delta| world.sweep_circle(center, 1.0, delta, LayerMask::ALL, None);
        assert_eq!(ray(Vec2::new(-20.0, 0.0), Vec2::ZERO), None);
        let nan = Vec2::new(f32::NAN, 0.0);
        assert_eq!(ray(nan, Vec2::X), None);
        assert_eq!(ray(Vec2::new(-20.0, 0.0), nan), None);
        assert!(ray(Vec2::new(-20.0, 0.0), Vec2::new(40.0, 0.0)).is_some());
        assert_eq!(
            world.sweep_circle(
                Vec2::new(-20.0, 0.0),
                1.0,
                Vec2::new(40.0, 0.0),
                LayerMask::ALL,
                Some(0)
            ),
            None
        );
    }

    #[test]
    fn a_cast_takes_a_step_per_cell_up_to_twice_the_grids_size() {
        assert_eq!(tile_steps(100.0, 16.0, 64, 64), 7);
        assert_eq!(tile_steps(0.0, 16.0, 64, 64), 1);
        // A shape so large that the whole cast is over the grid.
        assert_eq!(tile_steps(3e38, 1.0, 24, 24), 98);
        assert_eq!(tile_steps(f32::NAN, 1.0, 24, 24), 1);
    }

    #[test]
    fn a_huge_cast_looks_only_along_the_grid() {
        let mut world = CollisionWorld::new();
        let mut grid = TileGrid::new(64, 64, 16.0, Vec2::ZERO);
        grid.set_solid(IVec2::new(63, 63), true);
        world.set_grid(grid);
        world.rebuild([(
            0,
            Vec2::splat(-500.0),
            Collider::solid(Shape::Circle { radius: 4.0 }),
        )]);
        // Diagonally from far outside the grid, a long way past it: it
        // must not take a step per cell of the whole cast.
        let start = Vec2::splat(-1e7);
        let hit = world.segment(start, Vec2::splat(1e9), LayerMask::ALL, None);
        assert_eq!(hit.map(|hit| hit.obstacle), Some(Obstacle::Collider(0)));
        let past = world.segment(
            Vec2::splat(1.0),
            Vec2::new(1e30, 2e30),
            LayerMask::ALL,
            None,
        );
        assert_eq!(past, None);
    }
}
