//! [`CollisionWorld`]: a tick's colliders, the tile grid, and the overlap
//! queries.

use nadir_core::{Rect, Vec2};

use crate::geometry::{self, Placed};
use crate::hash::SpatialHash;
use crate::{Collider, LayerMask, Shape, TileGrid};

/// What collides this tick: the colliders, each with the caller's id for
/// it and its entity's position, over a [`TileGrid`].
///
/// [`rebuild`](Self::rebuild) replaces the colliders and rebuilds the
/// spatial hash; the facade does it once a tick. A collider's index is
/// its place in that list, so it holds only until the next rebuild; its
/// id is the caller's and holds as long as the caller wants, which is how
/// [`Triggers`](crate::Triggers) tell pairs apart from one tick to the
/// next.
///
/// Every query gives its results in the same order on every run, whatever
/// the hash's layout: first any collider too big to hash (over 1,024
/// cells, or not finite) in index order, then those moved far since the
/// rebuild ([`set_position`](Self::set_position)) in the order they were
/// moved, then cell by cell, row by row, and in index order within a cell.
pub struct CollisionWorld {
    ids: Vec<u64>,
    positions: Vec<Vec2>,
    colliders: Vec<Collider>,
    /// Where each collider was when the hash was built.
    built_at: Vec<Vec2>,
    /// Whether each collider has moved far since then, so that queries
    /// look for it in [`Self::far`] rather than in the hash.
    is_far: Vec<bool>,
    /// The colliders moved far since then, in the order they were moved.
    far: Vec<u32>,
    /// The most any other collider has moved along each axis since then.
    drift: Vec2,
    grid: TileGrid,
    hash: SpatialHash,
}

impl CollisionWorld {
    /// The side of a spatial hash cell unless the world is made with
    /// another: 64 pixels. Use at least twice the size of a typical
    /// collider.
    pub const DEFAULT_CELL_SIZE: f32 = 64.0;

    /// A world with no colliders and an empty grid, hashing in cells of
    /// [`DEFAULT_CELL_SIZE`](Self::DEFAULT_CELL_SIZE).
    pub fn new() -> Self {
        Self::with_cell_size(Self::DEFAULT_CELL_SIZE)
    }

    /// A world with no colliders and an empty grid, hashing in cells of
    /// `cell_size` pixels.
    ///
    /// # Panics
    ///
    /// If `cell_size` is not a finite number above 0.
    pub fn with_cell_size(cell_size: f32) -> Self {
        assert!(
            cell_size.is_finite() && cell_size > 0.0,
            "spatial hash cells must have a finite size above 0, not {cell_size}"
        );
        Self {
            ids: Vec::new(),
            positions: Vec::new(),
            colliders: Vec::new(),
            built_at: Vec::new(),
            is_far: Vec::new(),
            far: Vec::new(),
            drift: Vec2::ZERO,
            grid: TileGrid::empty(),
            hash: SpatialHash::new(cell_size),
        }
    }

    /// The tile grid.
    pub fn grid(&self) -> &TileGrid {
        &self.grid
    }

    /// The tile grid, to change tiles.
    pub fn grid_mut(&mut self) -> &mut TileGrid {
        &mut self.grid
    }

    /// Replaces the tile grid.
    pub fn set_grid(&mut self, grid: TileGrid) {
        self.grid = grid;
    }

    /// Replaces every collider with `colliders`, each an id, its entity's
    /// position and the collider, and rebuilds the spatial hash. Collider
    /// `i` is the `i`th of the list. The buffers are kept, so a steady
    /// number of colliders allocates nothing.
    ///
    /// # Panics
    ///
    /// If there are more than `u32::MAX` colliders.
    pub fn rebuild(&mut self, colliders: impl IntoIterator<Item = (u64, Vec2, Collider)>) {
        self.ids.clear();
        self.positions.clear();
        self.colliders.clear();
        for (id, position, collider) in colliders {
            self.ids.push(id);
            self.positions.push(position);
            self.colliders.push(collider);
        }
        assert!(
            u32::try_from(self.ids.len()).is_ok(),
            "a collision world holds at most u32::MAX colliders"
        );
        self.built_at.clear();
        self.built_at.extend_from_slice(&self.positions);
        self.is_far.clear();
        self.is_far.resize(self.ids.len(), false);
        self.far.clear();
        self.drift = Vec2::ZERO;
        let bounds = self
            .colliders
            .iter()
            .zip(&self.positions)
            .map(|(collider, &position)| collider.bounds(position));
        self.hash.rebuild(bounds);
    }

    /// Removes every collider, keeping the buffers.
    pub fn clear(&mut self) {
        self.rebuild(std::iter::empty());
    }

    /// The number of colliders.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The id the caller gave collider `index`.
    ///
    /// # Panics
    ///
    /// If there is no collider `index`.
    pub fn id(&self, index: u32) -> u64 {
        self.ids[self.at(index)]
    }

    /// Where collider `index`'s entity is.
    ///
    /// # Panics
    ///
    /// If there is no collider `index`.
    pub fn position(&self, index: u32) -> Vec2 {
        self.positions[self.at(index)]
    }

    /// Collider `index`.
    ///
    /// # Panics
    ///
    /// If there is no collider `index`.
    pub fn collider(&self, index: u32) -> &Collider {
        &self.colliders[self.at(index)]
    }

    /// Moves collider `index`'s entity to `position` without a rebuild:
    /// later queries and moves see it there. This is how a mover's new
    /// position reaches the movers after it in the same tick.
    ///
    /// Each query then searches as much further as the farthest such move
    /// along each axis, up to a quarter of a hash cell: a few pixels for a
    /// tick's movement. A collider moved further than that, such as a fast
    /// projectile, is checked by every query instead, until the next
    /// rebuild.
    ///
    /// # Panics
    ///
    /// If there is no collider `index`.
    pub fn set_position(&mut self, index: u32, position: Vec2) {
        let at = self.at(index);
        self.positions[at] = position;
        if self.is_far[at] {
            return;
        }
        let moved = (position - self.built_at[at]).abs();
        let far = self.hash.cell_size() * 0.25;
        // Written so that a position that is not a number moves nothing.
        if moved.x > far || moved.y > far {
            if !self.hash.oversized().contains(&index) {
                self.is_far[at] = true;
                self.far.push(index);
            }
        } else {
            self.drift = self.drift.max(moved);
        }
    }

    /// Fills `out` with the colliders on a layer of `mask` that contain
    /// `point`. A point on an edge is outside.
    pub fn point(&self, point: Vec2, mask: LayerMask, out: &mut Vec<u32>) {
        let probe = Placed::new(point, Shape::Aabb { half: Vec2::ZERO });
        self.overlapping(probe, mask, out);
    }

    /// Fills `out` with the colliders on a layer of `mask` that overlap
    /// `rect`. An empty `rect` overlaps nothing, and touching is not
    /// overlapping.
    pub fn aabb(&self, rect: Rect, mask: LayerMask, out: &mut Vec<u32>) {
        out.clear();
        if rect.is_empty() {
            return;
        }
        // Against the rect's own edges: a rect as wide as f32 allows
        // neither overflows nor loses the colliders' precision.
        self.candidates(rect, |index| {
            if self.on(index, mask) && geometry::overlaps_rect(self.placed(index), rect) {
                out.push(index);
            }
        });
    }

    /// Fills `out` with the colliders on a layer of `mask` that overlap the
    /// circle of `radius` around `center`. Touching is not overlapping.
    pub fn circle(&self, center: Vec2, radius: f32, mask: LayerMask, out: &mut Vec<u32>) {
        if radius.is_nan() || radius < 0.0 {
            out.clear();
            return;
        }
        self.overlapping(Placed::new(center, Shape::Circle { radius }), mask, out);
    }

    /// The collider on a layer of `mask` nearest to `point`, other than
    /// `ignore`, by the distance to its shape (0 inside it), if one is
    /// within `radius`. Of two as near, the lower index. A search from an
    /// entity should ignore its own collider.
    pub fn nearest(
        &self,
        point: Vec2,
        radius: f32,
        mask: LayerMask,
        ignore: Option<u32>,
    ) -> Option<u32> {
        if radius.is_nan() || radius < 0.0 {
            return None;
        }
        let region = Rect::new(point - Vec2::splat(radius), point + Vec2::splat(radius));
        let mut best: Option<(f32, u32)> = None;
        self.candidates(region, |index| {
            if Some(index) == ignore || !self.on(index, mask) {
                return;
            }
            let distance = geometry::distance_to(self.placed(index), point);
            if distance <= radius && best.is_none_or(|best| (distance, index) < best) {
                best = Some((distance, index));
            }
        });
        best.map(|(_, index)| index)
    }

    /// Fills `out` with the colliders on a layer of `mask` that overlap
    /// `probe`.
    fn overlapping(&self, probe: Placed, mask: LayerMask, out: &mut Vec<u32>) {
        out.clear();
        let half = probe.shape.half_extents();
        let region = Rect::new(probe.center - half, probe.center + half);
        self.candidates(region, |index| {
            if self.on(index, mask) && geometry::overlaps(self.placed(index), probe) {
                out.push(index);
            }
        });
    }

    /// Calls `visit` with each collider that may touch `region` where it is
    /// now, edges included, once each, in the order the queries promise.
    pub(crate) fn candidates(&self, region: Rect, mut visit: impl FnMut(u32)) {
        for &index in self.hash.oversized() {
            visit(index);
        }
        for &index in &self.far {
            visit(index);
        }
        let region = Rect::new(region.min - self.drift, region.max + self.drift);
        self.hash.query(region, |index| {
            if !self.is_far[index as usize] {
                visit(index);
            }
        });
    }

    /// Whether collider `index` is on a layer of `mask`.
    pub(crate) fn on(&self, index: u32, mask: LayerMask) -> bool {
        self.colliders[index as usize].layer.intersects(mask)
    }

    /// Collider `index`'s shape, where it is.
    pub(crate) fn placed(&self, index: u32) -> Placed {
        let collider = &self.colliders[index as usize];
        Placed::new(
            collider.center(self.positions[index as usize]),
            collider.shape,
        )
    }

    /// The capacity of each buffer, to check that a steady tick keeps
    /// them.
    #[cfg(test)]
    pub(crate) fn capacities(&self) -> Vec<usize> {
        let mut capacities = vec![
            self.ids.capacity(),
            self.positions.capacity(),
            self.colliders.capacity(),
            self.built_at.capacity(),
            self.is_far.capacity(),
            self.far.capacity(),
        ];
        capacities.extend(self.hash.capacities());
        capacities
    }

    /// `index` as a position in the lists.
    fn at(&self, index: u32) -> usize {
        let at = index as usize;
        assert!(
            at < self.ids.len(),
            "there is no collider {index}: the world has {}",
            self.ids.len()
        );
        at
    }
}

impl Default for CollisionWorld {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(half: f32) -> Collider {
        Collider::solid(Shape::Aabb {
            half: Vec2::splat(half),
        })
    }

    #[test]
    fn a_moved_collider_is_found_where_it_is_now() {
        let mut world = CollisionWorld::with_cell_size(16.0);
        world.rebuild([
            (10, Vec2::new(8.0, 8.0), boxed(4.0)),
            (11, Vec2::new(100.0, 8.0), boxed(4.0)),
        ]);
        let mut out = Vec::new();
        world.point(Vec2::new(30.0, 8.0), LayerMask::ALL, &mut out);
        assert!(out.is_empty());
        // Into the next cell but one, past what the hash knows.
        world.set_position(0, Vec2::new(30.0, 8.0));
        world.point(Vec2::new(30.0, 8.0), LayerMask::ALL, &mut out);
        assert_eq!(out, [0]);
        world.point(Vec2::new(8.0, 8.0), LayerMask::ALL, &mut out);
        assert!(out.is_empty());
        assert_eq!(world.position(0), Vec2::new(30.0, 8.0));
        assert_eq!(world.id(1), 11);
    }

    #[test]
    fn queries_match_layers() {
        let mut world = CollisionWorld::new();
        let wall = Collider {
            layer: LayerMask::bit(1),
            ..boxed(4.0)
        };
        let barrel = Collider {
            layer: LayerMask::bit(2),
            ..boxed(2.0)
        };
        world.rebuild([(0, Vec2::ZERO, wall), (1, Vec2::ZERO, barrel)]);
        let mut out = Vec::new();
        world.circle(Vec2::ZERO, 1.0, LayerMask::bit(1), &mut out);
        assert_eq!(out, [0]);
        world.circle(Vec2::ZERO, 1.0, LayerMask::ALL, &mut out);
        assert_eq!(out, [0, 1]);
        world.circle(Vec2::ZERO, 1.0, LayerMask::NONE, &mut out);
        assert!(out.is_empty());
        assert_eq!(
            world.nearest(Vec2::new(10.0, 0.0), 8.0, LayerMask::ALL, None),
            Some(0)
        );
        assert_eq!(
            world.nearest(Vec2::new(10.0, 0.0), 5.0, LayerMask::ALL, None),
            None
        );
        assert_eq!(
            world.nearest(Vec2::ZERO, 0.0, LayerMask::ALL, None),
            Some(0)
        );
        assert_eq!(
            world.nearest(Vec2::ZERO, f32::NAN, LayerMask::ALL, None),
            None
        );
    }

    #[test]
    #[should_panic(expected = "there is no collider 2: the world has 1")]
    fn an_index_past_the_end_panics() {
        let mut world = CollisionWorld::new();
        world.rebuild([(0, Vec2::ZERO, boxed(1.0))]);
        world.set_position(2, Vec2::ZERO);
    }
}
