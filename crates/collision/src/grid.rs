//! [`TileGrid`]: which tiles are solid.

use nadir_core::{IVec2, Rect, Vec2};

use crate::{LayerMask, geometry};

/// A grid of square cells, each solid or not: how tiles collide. There
/// are no colliders per tile.
///
/// Cell `(0, 0)` has its top-left corner at the origin; `x` grows to the
/// right and `y` down. Cells outside the grid are not solid, so a level
/// needs walls round its edge.
#[derive(Clone, Debug, PartialEq)]
pub struct TileGrid {
    width: u32,
    height: u32,
    cell_size: f32,
    origin: Vec2,
    /// One bit per cell, row by row.
    solid: Vec<u64>,
    /// What the tiles are: a mover collides with them when its mask has
    /// it. Every layer at first.
    pub layer: LayerMask,
}

impl TileGrid {
    /// A grid of `width` × `height` cells of `cell_size` pixels, none
    /// solid, with its top-left corner at `origin`.
    ///
    /// # Panics
    ///
    /// If `cell_size` is not a finite number above 0, or the grid has more
    /// cells than memory can count.
    pub fn new(width: u32, height: u32, cell_size: f32, origin: Vec2) -> Self {
        assert!(
            cell_size.is_finite() && cell_size > 0.0,
            "a tile grid's cells must have a finite size above 0, not {cell_size}"
        );
        let cells = usize::try_from(u64::from(width) * u64::from(height))
            .expect("a tile grid has no more cells than memory can count");
        Self {
            width,
            height,
            cell_size,
            origin,
            solid: vec![0; cells.div_ceil(64)],
            layer: LayerMask::ALL,
        }
    }

    /// A grid of no cells: nothing is solid.
    pub fn empty() -> Self {
        Self::new(0, 0, 1.0, Vec2::ZERO)
    }

    /// The number of columns.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The number of rows.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The side of a cell, in pixels.
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// The top-left corner of cell `(0, 0)`.
    pub fn origin(&self) -> Vec2 {
        self.origin
    }

    /// Whether `cell` is solid. Cells outside the grid are not.
    pub fn is_solid(&self, cell: IVec2) -> bool {
        self.bit(cell)
            .is_some_and(|(word, bit)| self.solid[word] & bit != 0)
    }

    /// Makes `cell` solid or not.
    ///
    /// # Panics
    ///
    /// If `cell` is outside the grid.
    pub fn set_solid(&mut self, cell: IVec2, solid: bool) {
        let Some((word, bit)) = self.bit(cell) else {
            panic!(
                "cell ({}, {}) is outside the {} × {} tile grid",
                cell.x, cell.y, self.width, self.height
            );
        };
        if solid {
            self.solid[word] |= bit;
        } else {
            self.solid[word] &= !bit;
        }
    }

    /// Makes every cell not solid.
    pub fn clear(&mut self) {
        self.solid.fill(0);
    }

    /// The cell that holds `point`, inside the grid or not.
    pub fn cell_at(&self, point: Vec2) -> IVec2 {
        geometry::floor((point - self.origin) / self.cell_size)
    }

    /// The square that `cell` covers, inside the grid or not. Neighbouring
    /// cells share their edges exactly.
    pub fn cell_rect(&self, cell: IVec2) -> Rect {
        // The far corner's cell is added in f32, so that the last cell
        // there is does not overflow; below 2^24 it is exact, as the near
        // corner of the next cell.
        let corner = |cell: Vec2| self.origin + cell * self.cell_size;
        Rect::new(corner(cell.as_vec2()), corner(cell.as_vec2() + Vec2::ONE))
    }

    /// The cells of the grid that `rect` touches, edges included, as an
    /// inclusive range; `None` if it touches none or is not a number.
    pub(crate) fn cells_touching(&self, rect: Rect) -> Option<(IVec2, IVec2)> {
        // Written so that NaN touches nothing.
        if !(rect.min.x <= rect.max.x && rect.min.y <= rect.max.y) {
            return None;
        }
        let last = IVec2::new(
            i32::try_from(self.width).unwrap_or(i32::MAX) - 1,
            i32::try_from(self.height).unwrap_or(i32::MAX) - 1,
        );
        let min = self.cell_at(rect.min).max(IVec2::ZERO);
        let max = self.cell_at(rect.max).min(last);
        (min.x <= max.x && min.y <= max.y).then_some((min, max))
    }

    /// The word and bit of `cell`, if it is in the grid.
    fn bit(&self, cell: IVec2) -> Option<(usize, u64)> {
        let x = u32::try_from(cell.x).ok().filter(|&x| x < self.width)?;
        let y = u32::try_from(cell.y).ok().filter(|&y| y < self.height)?;
        let index = u64::from(y) * u64::from(self.width) + u64::from(x);
        // The grid's constructor checked that every index fits.
        let index = usize::try_from(index).ok()?;
        Some((index / 64, 1 << (index % 64)))
    }
}

impl Default for TileGrid {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_solid_only_inside_the_grid() {
        let mut grid = TileGrid::new(70, 3, 16.0, Vec2::new(-8.0, 0.0));
        grid.set_solid(IVec2::new(69, 2), true);
        grid.set_solid(IVec2::new(0, 1), true);
        assert!(grid.is_solid(IVec2::new(69, 2)));
        assert!(grid.is_solid(IVec2::new(0, 1)));
        assert!(!grid.is_solid(IVec2::new(1, 1)));
        assert!(!grid.is_solid(IVec2::new(-1, 1)));
        assert!(!grid.is_solid(IVec2::new(70, 2)));
        grid.set_solid(IVec2::new(0, 1), false);
        assert!(!grid.is_solid(IVec2::new(0, 1)));
        grid.clear();
        assert!(!grid.is_solid(IVec2::new(69, 2)));
        assert!(!TileGrid::empty().is_solid(IVec2::ZERO));
    }

    #[test]
    #[should_panic(expected = "outside the 4 × 4 tile grid")]
    fn setting_a_cell_outside_the_grid_panics() {
        TileGrid::new(4, 4, 8.0, Vec2::ZERO).set_solid(IVec2::new(4, 0), true);
    }

    #[test]
    #[should_panic(expected = "finite size above 0")]
    fn cells_have_a_size() {
        let _ = TileGrid::new(4, 4, 0.0, Vec2::ZERO);
    }

    #[test]
    fn points_map_to_cells_and_back() {
        let grid = TileGrid::new(4, 4, 16.0, Vec2::new(-8.0, 8.0));
        assert_eq!(grid.cell_at(Vec2::new(-8.0, 8.0)), IVec2::ZERO);
        assert_eq!(grid.cell_at(Vec2::new(7.99, 23.99)), IVec2::ZERO);
        assert_eq!(grid.cell_at(Vec2::new(8.0, 24.0)), IVec2::ONE);
        assert_eq!(grid.cell_at(Vec2::new(-8.01, 0.0)), IVec2::new(-1, -1));
        assert_eq!(
            grid.cell_rect(IVec2::new(1, 2)),
            Rect::new(Vec2::new(8.0, 40.0), Vec2::new(24.0, 56.0))
        );
    }

    #[test]
    fn the_last_cell_there_is_has_a_rect() {
        let grid = TileGrid::new(4, 4, 16.0, Vec2::ZERO);
        let far = grid.cell_at(Vec2::splat(1e30));
        assert_eq!(far, IVec2::splat(i32::MAX));
        // There, f32 cannot tell its edges apart: an empty rect, but not
        // a panic, nor one turned inside out.
        let rect = grid.cell_rect(far);
        assert!(
            rect.min.x <= rect.max.x && rect.min.y <= rect.max.y,
            "{rect:?}"
        );
        assert!(rect.min.x.is_finite() && rect.max.y.is_finite(), "{rect:?}");
        // Neighbours still share their edges.
        let cell = IVec2::new(-7, 1_000_000);
        assert_eq!(
            grid.cell_rect(cell).max,
            grid.cell_rect(cell + IVec2::ONE).min
        );
    }

    #[test]
    fn a_rect_touches_the_cells_under_it_clamped_to_the_grid() {
        let grid = TileGrid::new(4, 4, 16.0, Vec2::ZERO);
        let rect = Rect::new(Vec2::new(-5.0, 16.0), Vec2::new(32.0, 20.0));
        assert_eq!(
            grid.cells_touching(rect),
            Some((IVec2::new(0, 1), IVec2::new(2, 1)))
        );
        let outside = Rect::new(Vec2::new(64.0, 0.0), Vec2::new(80.0, 8.0));
        assert_eq!(grid.cells_touching(outside), None);
        let nan = Rect::new(Vec2::splat(f32::NAN), Vec2::ONE);
        assert_eq!(grid.cells_touching(nan), None);
        assert_eq!(TileGrid::empty().cells_touching(rect), None);
    }
}
