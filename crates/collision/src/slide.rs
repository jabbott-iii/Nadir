//! [`move_and_slide`]: kinematic movement that stops at solids and slides
//! along them.

use nadir_core::{IVec2, Rect, Vec2};

use crate::geometry::{self, Axis, Placed};
use crate::{Collider, ColliderKind, CollisionWorld, EPSILON, Obstacle, Shape};

/// The most substeps a move takes, however far it goes.
const MOST_SUBSTEPS: u32 = 16;

/// The most solid colliders a move gathers for its sweeps. A move near more
/// asks the spatial hash on each sweep instead. In a jammed crowd a move has
/// up to 6 near it, and a dash of 150 px up to 29.
const MOST_NEAR: usize = 32;

/// The most solids a sweep remembers as stopping it. A sweep that more
/// would stop, somewhere along it, finds its contacts in a second pass.
const MOST_AHEAD: usize = 16;

#[cfg(test)]
thread_local! {
    /// For the tests: how many moves had more solids near them than they
    /// gather, and how many sweeps took a second pass for their contacts.
    pub(crate) static FALLBACKS: std::cell::Cell<(u64, u64)> =
        const { std::cell::Cell::new((0, 0)) };
}

/// A solid that stopped a move along an axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// What stopped it.
    pub obstacle: Obstacle,
    /// The unit normal along the stopped axis, against the motion.
    pub normal: Vec2,
}

/// Moves collider `self_index`'s entity from `start` by `delta`, stopping
/// at solids and sliding along them, and returns where it ends. Fills
/// `out_contacts` with each solid that stopped it, once per solid and
/// normal.
///
/// The solids are the grid's solid tiles if the collider's mask has the
/// grid's layer, and the other solid colliders on a layer of its mask,
/// where the world has them now ([`CollisionWorld::set_position`]). A
/// trigger is stopped by tiles only: it detects colliders, and moves into
/// them to do so.
///
/// - It moves along X, then along Y, so a wall stops only the motion into
///   it. A solid it overlaps across the motion by [`EPSILON`] or less
///   does not stop it, so it slides along floors and walls of tiles
///   without snagging on the seams between them.
/// - A move longer than half the collider's smallest dimension is split
///   into up to 16 substeps, so a diagonal move follows its diagonal.
///   Each axis is swept, so nothing tunnels through a thin wall however
///   long the move.
/// - Each axis stops where the collider would start to overlap a solid
///   ahead of it. A solid it already overlaps lets it leave toward the
///   nearer side along the motion and stops it going deeper, so the
///   overlap never grows.
/// - If it starts overlapping no solid, it ends overlapping none by more
///   than [`EPSILON`], give or take one f32 step at its position: under
///   0.002 px within ±16,384 px of the origin, under 0.008 px within
///   ±65,536 px.
/// - No motion, or a position or motion that is not finite, returns
///   `start` and no contacts.
/// - The contacts come sweep by sweep, and within a sweep the tiles row by
///   row before the colliders, whose order is fixed but not specified.
///
/// A solid mover asks the spatial hash once for the solid colliders near
/// the whole move, and sweeps against those; one with more than 32 near
/// it, or of negative or not-a-number size, asks on each sweep instead
/// (ADR-0026).
///
/// # Panics
///
/// If there is no collider `self_index`.
pub fn move_and_slide(
    world: &CollisionWorld,
    self_index: u32,
    start: Vec2,
    delta: Vec2,
    out_contacts: &mut Vec<Contact>,
) -> Vec2 {
    move_with(
        world,
        self_index,
        start,
        delta,
        out_contacts,
        Shortcuts {
            gather: true,
            first_pass: true,
        },
    )
}

/// Which of [`move_and_slide`]'s shortcuts a move takes. Tests turn them
/// off to check that they change nothing:
///
/// - the first pass changes neither where a move ends nor its contacts,
///   their order included;
/// - the gather changes neither where it ends nor the set of its contacts,
///   but in ADR-0026's two cases: the sign of a zero, and where rounding
///   exceeds `EPSILON`, far from the origin or at a solid hundreds of
///   thousands of pixels wide.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shortcuts {
    /// Gather the solid colliders near the move once, rather than ask the
    /// spatial hash on each sweep.
    pub(crate) gather: bool,
    /// Take a sweep's contacts from the pass that finds where it stops,
    /// rather than from a second pass.
    pub(crate) first_pass: bool,
}

/// [`move_and_slide`], taking the `shortcuts` given.
pub(crate) fn move_with(
    world: &CollisionWorld,
    self_index: u32,
    start: Vec2,
    delta: Vec2,
    out_contacts: &mut Vec<Contact>,
    shortcuts: Shortcuts,
) -> Vec2 {
    out_contacts.clear();
    let collider = *world.collider(self_index);
    let finite = |v: Vec2| v.x.is_finite() && v.y.is_finite();
    if delta == Vec2::ZERO || !finite(start) || !finite(delta) {
        return start;
    }
    let half = collider.shape.half_extents();
    let step = half.x.min(half.y);
    let substeps = if step > 0.0 {
        (delta.length() / step).ceil().min(MOST_SUBSTEPS as f32) as u32
    } else {
        MOST_SUBSTEPS
    }
    .max(1);

    let mut near = Near::new();
    if collider.kind == ColliderKind::Solid {
        if shortcuts.gather {
            near.gather(world, self_index, &collider, start, delta);
        } else {
            near.per_sweep = true;
        }
    }
    let mover = Mover {
        world,
        index: self_index,
        collider,
        near: &near,
        first_pass: shortcuts.first_pass,
    };
    let mut position = start;
    for substep in 1..=substeps {
        let target = if substep == substeps {
            start + delta
        } else {
            start + delta * (substep as f32 / substeps as f32)
        };
        for axis in [Axis::X, Axis::Y] {
            let to = axis.of(target);
            if to != axis.of(position) {
                let reached = mover.slide(position, axis, to, out_contacts);
                position = axis.with(position, reached);
            }
        }
    }
    position
}

/// The collider being moved.
struct Mover<'a> {
    world: &'a CollisionWorld,
    index: u32,
    collider: Collider,
    /// The solid colliders near the move.
    near: &'a Near,
    /// Whether a sweep takes its contacts from its first pass when it can.
    first_pass: bool,
}

/// The solid colliders near a move, gathered once for all its sweeps: each
/// one's index, and its shape where it is.
struct Near {
    solids: [(u32, Placed); MOST_NEAR],
    len: usize,
    /// Whether the sweeps ask the spatial hash themselves instead: the move
    /// had more than [`MOST_NEAR`] solids near it, or the gather could not
    /// vouch for its region.
    per_sweep: bool,
}

impl Near {
    /// None gathered yet.
    fn new() -> Self {
        // All zeros, so that filling the array is one memset.
        let none = Placed::new(Vec2::ZERO, Shape::Aabb { half: Vec2::ZERO });
        Self {
            solids: [(0, none); MOST_NEAR],
            len: 0,
            per_sweep: false,
        }
    }

    /// The solids gathered.
    fn solids(&self) -> &[(u32, Placed)] {
        &self.solids[..self.len]
    }

    /// Gathers the solid colliders that collider `index` could meet moving
    /// from `start` by `delta`: those on a layer of its mask, other than
    /// itself, whose bounds overlap a region around the move.
    ///
    /// Every position the move passes is between `start` and
    /// `start + delta` on each axis, so each sweep's box is inside the
    /// box around the collider's bounds at both ends. A solid stops a sweep
    /// only if it overlaps that sweep's box, but the stop is worked out
    /// from centers and half extents, and rounds differently from the
    /// bounds. So the region is that box grown by a margin of 1 px plus
    /// 2^-16 (128 f32 steps) of the largest coordinate rounding could
    /// involve: the box's own, plus the widest collider in the hash. A
    /// solid that matters is near the box, so its coordinates are no
    /// larger. Colliders kept aside, which can be wider, are all kept. A
    /// mover whose size is negative or not a number, or whose box is not
    /// finite, is left to the sweeps.
    fn gather(
        &mut self,
        world: &CollisionWorld,
        index: u32,
        collider: &Collider,
        start: Vec2,
        delta: Vec2,
    ) {
        let (from, to) = (collider.bounds(start), collider.bounds(start + delta));
        let (min, max) = (from.min.min(to.min), from.max.max(to.max));
        let half = collider.shape.half_extents();
        let finite =
            min.x.is_finite() && min.y.is_finite() && max.x.is_finite() && max.y.is_finite();
        // Written so that NaN is left to the sweeps.
        if !(half.x >= 0.0 && half.y >= 0.0 && finite) {
            self.per_sweep = true;
            return;
        }
        let largest = min.abs().max(max.abs());
        let widest = world.widest_hashed();
        let margin = Vec2::splat(1.0 + (largest.x.max(largest.y) + widest) / 65_536.0);
        let region = Rect::new(min - margin, max + margin);
        world.near_candidates(region, |other, aside| {
            let solid = world.collider(other);
            if self.per_sweep
                || other == index
                || solid.kind != ColliderKind::Solid
                || !collider.mask.intersects(solid.layer)
            {
                return;
            }
            let placed = world.placed(other);
            // The size taken as positive: a negative one (meaningless) can
            // still stop a move, but never from further than this.
            let half = placed.shape.half_extents().abs();
            let bounds = Rect::new(placed.center - half, placed.center + half);
            if !aside && !geometry::boxes_overlap(bounds, region) {
                return;
            }
            if self.len == MOST_NEAR {
                self.per_sweep = true;
                #[cfg(test)]
                FALLBACKS.with(|taken| taken.set((taken.get().0 + 1, taken.get().1)));
                return;
            }
            self.solids[self.len] = (other, placed);
            self.len += 1;
        });
    }
}

impl Mover<'_> {
    /// Moves the entity at `position` along `axis` to `to`, or as far as
    /// the solids let it, and returns the coordinate it reaches.
    fn slide(&self, position: Vec2, axis: Axis, to: f32, contacts: &mut Vec<Contact>) -> f32 {
        let from = axis.of(position);
        let end = axis.with(position, to);
        let center = self.collider.center(position);
        let (start, finish) = (axis.of(center), axis.of(self.collider.center(end)));
        let forward = finish > start;
        let (here, there) = (self.collider.bounds(position), self.collider.bounds(end));
        let swept = Rect::new(
            here.min.min(there.min) - Vec2::splat(EPSILON),
            here.max.max(there.max) + Vec2::splat(EPSILON),
        );

        // Where each solid ahead would stop the center, if it does.
        let stop_at = |other: f32, reach: f32| {
            if forward {
                (other > start && other - reach < finish).then(|| (other - reach).max(start))
            } else {
                (other < start && other + reach > finish).then(|| (other + reach).min(start))
            }
        };
        let mut stop = finish;
        // The solids that would stop it and where, in the order found: its
        // contacts are those that stop it within EPSILON of where it stops.
        let mut ahead = [(Obstacle::Tile(IVec2::ZERO), 0.0); MOST_AHEAD];
        let mut count = 0;
        self.solids(center, axis, swept, |obstacle, other, reach| {
            if let Some(at) = stop_at(other, reach) {
                stop = if forward { stop.min(at) } else { stop.max(at) };
                if let Some(slot) = ahead.get_mut(count) {
                    *slot = (obstacle, at);
                }
                count += 1;
            }
        });
        if stop == finish {
            return to;
        }

        let normal = axis.unit(if forward { -1.0 } else { 1.0 });
        let mut touch = |obstacle, at: f32| {
            if (at - stop).abs() <= EPSILON {
                let contact = Contact { obstacle, normal };
                if !contacts.contains(&contact) {
                    contacts.push(contact);
                }
            }
        };
        if self.first_pass && count <= MOST_AHEAD {
            for &(obstacle, at) in &ahead[..count] {
                touch(obstacle, at);
            }
        } else {
            #[cfg(test)]
            if self.first_pass {
                FALLBACKS.with(|taken| taken.set((taken.get().0, taken.get().1 + 1)));
            }
            // The same solids in the same order, so the same contacts.
            self.solids(center, axis, swept, |obstacle, other, reach| {
                if let Some(at) = stop_at(other, reach) {
                    touch(obstacle, at);
                }
            });
        }
        // Back from the center to the entity, and never past either end.
        let reached = stop - axis.of(self.collider.offset);
        reached.clamp(from.min(to), from.max(to))
    }

    /// Calls `visit` with each solid under `swept` that the collider at
    /// `center` overlaps across `axis` by more than [`EPSILON`]: the solid,
    /// its center along the axis, and how far the collider's center
    /// reaches past it along the axis while they overlap. The tiles come
    /// row by row, then the colliders the move gathered, or else those the
    /// spatial hash gives for `swept`.
    fn solids(
        &self,
        center: Vec2,
        axis: Axis,
        swept: Rect,
        mut visit: impl FnMut(Obstacle, f32, f32),
    ) {
        let shape = self.collider.shape;
        let mut offer = |obstacle, other: Vec2, other_shape| {
            let across = (axis.across(other) - axis.across(center)).abs();
            if let Some(reach) = geometry::reach(shape, other_shape, axis, across) {
                visit(obstacle, axis.of(other), reach);
            }
        };

        let grid = self.world.grid();
        if grid.layer.intersects(self.collider.mask)
            && let Some((min, max)) = grid.cells_touching(swept)
        {
            let half = grid.cell_size() * 0.5;
            // A mover no more than EPSILON either side of its center,
            // across the axis, overlaps neither of two solid tiles by more
            // than EPSILON when it is on the seam between them, and would
            // slip along it: for it, each tile is one with the solid tiles
            // beside it across the axis, and only the tiles it is across
            // from count. Neighbouring cells share their edges exactly, so
            // some tile of a wall is across from every point along it.
            let thin = axis.across(shape.half_extents()) <= EPSILON;
            let across_unit = axis.with(Vec2::ONE, 0.0);
            let step = across_unit.as_ivec2();
            for y in min.y..=max.y {
                for x in min.x..=max.x {
                    let cell = IVec2::new(x, y);
                    if !grid.is_solid(cell) {
                        continue;
                    }
                    let rect = grid.cell_rect(cell);
                    let mut tile = Shape::Aabb {
                        half: rect.size() * 0.5,
                    };
                    let mut middle = rect.center();
                    if thin {
                        let at = axis.across(center);
                        let (low, high) = (axis.across(rect.min), axis.across(rect.max));
                        if !(at >= low && at <= high) {
                            continue;
                        }
                        let before = f32::from(u8::from(grid.is_solid(cell - step)));
                        let after = f32::from(u8::from(grid.is_solid(cell + step)));
                        middle += across_unit * ((after - before) * half);
                        tile = Shape::Aabb {
                            half: axis.with(Vec2::splat(half * (1.0 + before + after)), half),
                        };
                    }
                    offer(Obstacle::Tile(cell), middle, tile);
                }
            }
        }

        if self.collider.kind == ColliderKind::Trigger {
            return;
        }
        if !self.near.per_sweep {
            for &(index, placed) in self.near.solids() {
                offer(Obstacle::Collider(index), placed.center, placed.shape);
            }
            return;
        }
        self.world.candidates(swept, |index| {
            let other = self.world.collider(index);
            if index == self.index
                || other.kind != ColliderKind::Solid
                || !self.collider.mask.intersects(other.layer)
            {
                return;
            }
            let placed = self.world.placed(index);
            offer(Obstacle::Collider(index), placed.center, placed.shape);
        });
    }
}
