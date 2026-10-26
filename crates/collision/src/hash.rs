//! [`SpatialHash`]: the broadphase, a uniform grid of cells hashed into an
//! open-addressing table.

use nadir_core::{IVec2, Rect, Vec2};

use crate::geometry;

/// The most cells one collider may cover before it is kept aside in
/// [`SpatialHash::oversized`] instead, which every query visits.
const MOST_CELLS: i64 = 1024;

/// The fewest slots the table has once it has any.
const FIRST_SLOTS: usize = 64;

/// One cell's slot in the table.
#[derive(Clone, Copy, Debug)]
struct Slot {
    /// The cell.
    cell: IVec2,
    /// The build that filled the slot: the slot is empty unless this is
    /// the current one.
    build: u32,
    /// Where the cell's colliders start in [`SpatialHash::entries`].
    start: u32,
    /// How many there are.
    len: u32,
}

/// One collider in one cell's run.
#[derive(Clone, Copy, Debug)]
struct Entry {
    /// The collider.
    index: u32,
    /// The first cell it covers, its top-left.
    first: IVec2,
}

const EMPTY: Slot = Slot {
    cell: IVec2::ZERO,
    build: 0,
    start: 0,
    len: 0,
};

/// A uniform grid of square cells over the colliders' bounds, rebuilt
/// from scratch each tick.
///
/// The cells that hold colliders sit in an open-addressing table, and each
/// points into one flat array of collider indices, filled by a counting
/// sort: count per cell, prefix sum, scatter. A cell's colliders are in
/// the order they were added. Rebuilding keeps every buffer, so a steady
/// tick allocates nothing.
pub(crate) struct SpatialHash {
    cell_size: f32,
    /// A power of two in length, or empty.
    slots: Vec<Slot>,
    /// The current build: slots of other builds are empty.
    build: u32,
    /// How many slots the current build fills.
    filled: usize,
    /// The colliders of each cell, cell after cell.
    entries: Vec<Entry>,
    /// The bounds each collider was hashed with, by index.
    bounds: Vec<Rect>,
    /// Colliders that cover more than [`MOST_CELLS`] cells, or whose bounds
    /// are not finite, in the order they were added.
    oversized: Vec<u32>,
    /// The first and last cell that hold colliders, if any do.
    occupied: Option<(IVec2, IVec2)>,
    /// The cells of this build and their slots, row by row: what a query
    /// walks when its region spans more cells than there are here.
    order: Vec<(IVec2, u32)>,
    /// How many cells and entries queries have looked at, for tests.
    #[cfg(test)]
    looked_at: std::cell::Cell<u64>,
}

impl SpatialHash {
    /// An empty hash of cells of `cell_size` pixels.
    pub(crate) fn new(cell_size: f32) -> Self {
        Self {
            cell_size,
            slots: Vec::new(),
            build: 0,
            filled: 0,
            entries: Vec::new(),
            bounds: Vec::new(),
            oversized: Vec::new(),
            occupied: None,
            order: Vec::new(),
            #[cfg(test)]
            looked_at: std::cell::Cell::new(0),
        }
    }

    /// Replaces the colliders with `bounds`: collider `i` covers
    /// `bounds[i]`.
    pub(crate) fn rebuild(&mut self, bounds: impl IntoIterator<Item = Rect>) {
        self.bounds.clear();
        self.bounds.extend(bounds);
        self.build = self.build.wrapping_add(1);
        if self.build == 0 {
            // Every slot of an earlier build would look current.
            self.slots.fill(EMPTY);
            self.build = 1;
        }
        self.filled = 0;
        self.oversized.clear();
        self.occupied = None;

        // Count each cell's colliders.
        for index in 0..self.bounds.len() {
            let Some((min, max)) = self.cells_of(self.bounds[index]) else {
                self.oversized.push(index as u32);
                continue;
            };
            self.occupied = Some(match self.occupied {
                Some((first, last)) => (first.min(min), last.max(max)),
                None => (min, max),
            });
            for y in min.y..=max.y {
                for x in min.x..=max.x {
                    let slot = self.slot_for(IVec2::new(x, y));
                    self.slots[slot].len += 1;
                }
            }
        }

        // Give each cell its run of entries, and count again as they fill.
        let mut start = 0u32;
        self.order.clear();
        for (at, slot) in self.slots.iter_mut().enumerate() {
            if slot.build == self.build {
                slot.start = start;
                start += slot.len;
                slot.len = 0;
                self.order.push((slot.cell, at as u32));
            }
        }
        self.order
            .sort_unstable_by_key(|&(cell, _)| (cell.y, cell.x));
        self.entries.clear();
        let unfilled = Entry {
            index: 0,
            first: IVec2::ZERO,
        };
        self.entries.resize(start as usize, unfilled);

        for index in 0..self.bounds.len() {
            let Some((min, max)) = self.cells_of(self.bounds[index]) else {
                continue;
            };
            for y in min.y..=max.y {
                for x in min.x..=max.x {
                    let Some(slot) = self.find(IVec2::new(x, y)) else {
                        continue;
                    };
                    let slot = &mut self.slots[slot];
                    self.entries[(slot.start + slot.len) as usize] = Entry {
                        index: index as u32,
                        first: min,
                    };
                    slot.len += 1;
                }
            }
        }
    }

    /// The side of a cell, in pixels.
    pub(crate) fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// The colliders kept aside, which every query visits: those that
    /// cover more than [`MOST_CELLS`] cells, or whose bounds are not
    /// finite, in the order they were added.
    pub(crate) fn oversized(&self) -> &[u32] {
        &self.oversized
    }

    /// Calls `visit` with each collider not kept aside whose hashed bounds
    /// may touch `region`, once each: cell by cell, row by row, each
    /// cell's in the order they were added.
    ///
    /// It looks at each cell of `region` that holds colliders, or, if the
    /// region spans more cells than hold any, at each cell that does: so a
    /// query never costs more than the colliders' cells.
    pub(crate) fn query(&self, region: Rect, mut visit: impl FnMut(u32)) {
        // Written so that NaN finds nothing.
        if !(region.min.x <= region.max.x && region.min.y <= region.max.y) {
            return;
        }
        let Some((first, last)) = self.occupied else {
            return;
        };
        let region_first = self.cell_at(region.min);
        let min = region_first.max(first);
        let max = self.cell_at(region.max).min(last);
        if min.x > max.x || min.y > max.y {
            return;
        }
        let mut cell_of = |cell: IVec2, slot: usize| {
            let slot = self.slots[slot];
            let run = &self.entries[slot.start as usize..(slot.start + slot.len) as usize];
            #[cfg(test)]
            self.looked_at
                .set(self.looked_at.get() + 1 + u64::from(slot.len));
            for entry in run {
                // A collider covers a block of cells, and the region
                // another: they share the block where they overlap, so
                // take the collider only in its top-left cell.
                if entry.first.max(region_first) == cell {
                    visit(entry.index);
                }
            }
        };
        let columns = i64::from(max.x) - i64::from(min.x) + 1;
        let rows = i64::from(max.y) - i64::from(min.y) + 1;
        // Up to 2^32 by 2^32 cells: the product can overflow.
        if columns.saturating_mul(rows) > self.order.len() as i64 {
            let from = self
                .order
                .partition_point(|&(cell, _)| (cell.y, cell.x) < (min.y, min.x));
            for &(cell, slot) in &self.order[from..] {
                if cell.y > max.y {
                    break;
                }
                if cell.x >= min.x && cell.x <= max.x {
                    cell_of(cell, slot as usize);
                }
            }
            return;
        }
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let cell = IVec2::new(x, y);
                #[cfg(test)]
                self.looked_at.set(self.looked_at.get() + 1);
                if let Some(slot) = self.find(cell) {
                    cell_of(cell, slot);
                }
            }
        }
    }

    /// The capacity of each buffer, to check that rebuilding keeps them.
    #[cfg(test)]
    pub(crate) fn capacities(&self) -> [usize; 5] {
        [
            self.slots.len(),
            self.entries.capacity(),
            self.bounds.capacity(),
            self.oversized.capacity(),
            self.order.capacity(),
        ]
    }

    /// The cell that holds `point`.
    fn cell_at(&self, point: Vec2) -> IVec2 {
        geometry::floor(point / self.cell_size)
    }

    /// The first and last cell `bounds` covers, if it covers no more than
    /// [`MOST_CELLS`] and is finite.
    fn cells_of(&self, bounds: Rect) -> Option<(IVec2, IVec2)> {
        if !(bounds.min.x.is_finite()
            && bounds.min.y.is_finite()
            && bounds.max.x.is_finite()
            && bounds.max.y.is_finite())
        {
            return None;
        }
        let (min, max) = (self.cell_at(bounds.min), self.cell_at(bounds.max));
        let columns = i64::from(max.x) - i64::from(min.x) + 1;
        let rows = i64::from(max.y) - i64::from(min.y) + 1;
        // Up to 2^32 by 2^32 cells: the product can overflow.
        let fits = columns >= 1 && rows >= 1 && columns.saturating_mul(rows) <= MOST_CELLS;
        fits.then_some((min, max))
    }

    /// The slot of `cell` in this build, added if it has none.
    fn slot_for(&mut self, cell: IVec2) -> usize {
        if let Some(slot) = self.find(cell) {
            return slot;
        }
        if (self.filled + 1) * 2 > self.slots.len() {
            self.grow();
        }
        let mask = self.slots.len() - 1;
        let mut slot = hash(cell) & mask;
        while self.slots[slot].build == self.build {
            slot = (slot + 1) & mask;
        }
        self.slots[slot] = Slot {
            cell,
            build: self.build,
            start: 0,
            len: 0,
        };
        self.filled += 1;
        slot
    }

    /// The slot of `cell` in this build, if it has one.
    fn find(&self, cell: IVec2) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut slot = hash(cell) & mask;
        loop {
            let candidate = self.slots[slot];
            if candidate.build != self.build {
                return None;
            }
            if candidate.cell == cell {
                return Some(slot);
            }
            slot = (slot + 1) & mask;
        }
    }

    /// Doubles the table, keeping this build's slots.
    fn grow(&mut self) {
        let size = (self.slots.len() * 2).max(FIRST_SLOTS);
        let old = std::mem::replace(&mut self.slots, vec![EMPTY; size]);
        let mask = size - 1;
        for slot in old.into_iter().filter(|slot| slot.build == self.build) {
            let mut at = hash(slot.cell) & mask;
            while self.slots[at].build == self.build {
                at = (at + 1) & mask;
            }
            self.slots[at] = slot;
        }
    }
}

/// Where in a table `cell` starts looking: a fixed mix of its
/// coordinates, the same on every run.
fn hash(cell: IVec2) -> usize {
    let x = u64::from(cell.x as u32);
    let y = u64::from(cell.y as u32);
    let mixed = (x | y << 32).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    (mixed >> 32) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, side: f32) -> Rect {
        Rect::from_min_size(Vec2::new(x, y), Vec2::splat(side))
    }

    fn found(hash: &SpatialHash, region: Rect) -> Vec<u32> {
        let mut found = hash.oversized().to_vec();
        hash.query(region, |index| found.push(index));
        found
    }

    #[test]
    fn a_query_finds_each_collider_once_by_cell_then_insertion() {
        let mut hash = SpatialHash::new(10.0);
        hash.rebuild([
            square(12.0, 2.0, 3.0),   // cell (1, 0)
            square(-5.0, -5.0, 30.0), // cells (-1, -1) to (2, 2)
            square(2.0, 2.0, 3.0),    // cell (0, 0)
            square(15.0, 15.0, 3.0),  // cell (1, 1)
            square(500.0, 0.0, 3.0),  // far away
        ]);
        assert_eq!(found(&hash, square(0.0, 0.0, 20.0)), [1, 2, 0, 3]);
        assert_eq!(found(&hash, square(16.0, 16.0, 1.0)), [1, 3]);
        assert_eq!(
            found(&hash, square(-100.0, -100.0, 1000.0)),
            [1, 2, 0, 4, 3]
        );
        assert!(found(&hash, square(200.0, 200.0, 5.0)).is_empty());
    }

    #[test]
    fn huge_and_broken_colliders_are_kept_aside_and_always_visited() {
        let mut hash = SpatialHash::new(10.0);
        hash.rebuild([
            square(0.0, 0.0, 1.0),
            square(0.0, 0.0, 1e6),
            Rect::new(Vec2::splat(f32::NAN), Vec2::ONE),
            Rect::new(Vec2::ZERO, Vec2::splat(f32::INFINITY)),
        ]);
        assert_eq!(found(&hash, square(0.0, 0.0, 1.0)), [1, 2, 3, 0]);
        assert_eq!(found(&hash, square(50.0, 50.0, 1.0)), [1, 2, 3]);
        let everywhere = Rect::new(Vec2::splat(f32::NEG_INFINITY), Vec2::splat(f32::INFINITY));
        assert_eq!(found(&hash, everywhere), [1, 2, 3, 0]);
        assert_eq!(
            found(&hash, Rect::new(Vec2::splat(f32::NAN), Vec2::ONE)),
            [1, 2, 3]
        );
    }

    #[test]
    fn rebuilding_keeps_the_buffers() {
        let mut hash = SpatialHash::new(8.0);
        let colliders: Vec<Rect> = (0..500)
            .map(|i| square((i % 25) as f32 * 9.0, (i / 25) as f32 * 9.0, 6.0))
            .collect();
        hash.rebuild(colliders.iter().copied());
        let capacity = (
            hash.slots.len(),
            hash.entries.capacity(),
            hash.bounds.capacity(),
        );
        for round in 0..300 {
            // Fewer colliders, then all of them again.
            let count = if round % 2 == 0 { 250 } else { 500 };
            hash.rebuild(colliders[..count].iter().copied());
            assert_eq!(found(&hash, square(0.0, 0.0, 1.0)), [0]);
        }
        assert_eq!(
            capacity,
            (
                hash.slots.len(),
                hash.entries.capacity(),
                hash.bounds.capacity()
            )
        );
    }

    #[test]
    fn a_wrapped_build_number_clears_the_table() {
        let mut hash = SpatialHash::new(10.0);
        hash.rebuild([square(0.0, 0.0, 1.0)]);
        hash.build = u32::MAX;
        hash.rebuild([square(30.0, 30.0, 1.0)]);
        assert_eq!(hash.build, 1);
        assert!(found(&hash, square(0.0, 0.0, 1.0)).is_empty());
        assert_eq!(found(&hash, square(30.0, 30.0, 1.0)), [0]);
    }

    #[test]
    fn a_huge_query_in_a_sparse_world_looks_only_at_cells_that_hold_colliders() {
        let mut hash = SpatialHash::new(8.0);
        hash.rebuild([
            square(-65_536.0, -65_536.0, 4.0),
            square(10.0, 0.0, 4.0),
            square(65_536.0, 65_536.0, 4.0),
        ]);
        let everywhere = Rect::new(Vec2::splat(-1e9), Vec2::splat(1e9));
        hash.looked_at.set(0);
        assert_eq!(found(&hash, everywhere), [0, 1, 2]);
        assert!(hash.looked_at.get() <= 12, "{}", hash.looked_at.get());
        // A small one still walks its own cells, in the same order.
        let near = Rect::new(Vec2::new(-20.0, -20.0), Vec2::new(30.0, 30.0));
        assert_eq!(found(&hash, near), [1]);
        // A band of rows across the world takes only the cells in it.
        let band = Rect::new(Vec2::new(-1e9, -4.0), Vec2::new(1e9, 4.0));
        assert_eq!(found(&hash, band), [1]);
    }

    #[test]
    fn colliders_across_the_whole_f32_range_neither_overflow_nor_hang() {
        let mut hash = SpatialHash::new(8.0);
        // Finite bounds over 2^32 by 2^32 cells: kept aside, not walked.
        let everything = Rect::new(Vec2::splat(-3e38), Vec2::splat(3e38));
        // And two small ones at the first and last cells there are.
        let low = square(-3e38, -3e38, 1.0);
        let high = square(3e38, 3e38, 1.0);
        hash.rebuild([everything, low, high]);
        assert_eq!(hash.oversized(), [0]);
        // A query over all of it walks only the cells that hold colliders.
        let all = Rect::new(Vec2::splat(f32::MIN), Vec2::splat(f32::MAX));
        hash.looked_at.set(0);
        assert_eq!(found(&hash, all), [0, 1, 2]);
        assert!(hash.looked_at.get() <= 4, "{}", hash.looked_at.get());
    }
}
