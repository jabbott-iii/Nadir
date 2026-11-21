// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! [`FlowField`]: the way to one goal from every cell, for many agents.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use nadir_core::{IVec2, Vec2};

use crate::TileGrid;
use crate::path::{NEIGHBOURS, cell_count, cell_of, index_of, open_cell_near, step};

/// A cell the field does not reach.
const UNREACHED: u32 = u32::MAX;

/// No step: the goal, or a cell the field does not reach.
const NONE: u8 = u8::MAX;

/// The unit vector toward each of [`NEIGHBOURS`].
const DIRECTIONS: [Vec2; 8] = {
    const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
    [
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(-1.0, 0.0),
        Vec2::new(0.0, -1.0),
        Vec2::new(D, D),
        Vec2::new(-D, D),
        Vec2::new(-D, -D),
        Vec2::new(D, -D),
    ]
};

/// The way to one goal from every cell of a [`TileGrid`]: for many agents
/// heading to the same place, a chasing pack, say, where a path each would
/// cost too much.
///
/// - [`compute`](Self::compute) fills it: a Dijkstra integration field, the
///   cost of the shortest path from each cell to the goal's (in
///   thousandths of a cell, as [`Pathfinder`](crate::Pathfinder) counts),
///   then a direction field, from each cell toward its neighbor that
///   leads there soonest (the first in the order right, down, left, up,
///   then the corners, of two as soon).
/// - Its steps are A*'s: 8-connected, across open cells only, and to a
///   corner only with both cells beside it open.
/// - [`update`](Self::update), once a tick, computes it again when the
///   goal has moved to another cell, at most once every
///   [`EVERY`](Self::EVERY) ticks.
/// - An agent goes [`direction_at`](Self::direction_at) its position.
///
/// It keeps its memory, 5 bytes a cell and its heap: once they have grown
/// to fit the largest computation yet, computing it again allocates
/// nothing. Costs are `u32`s of thousandths: a cell more than 4,294,967
/// cells' sides from the goal is unreached.
#[derive(Clone, Debug, Default)]
pub struct FlowField {
    width: u32,
    height: u32,
    cell_size: f32,
    origin: Vec2,
    /// The grid's stamp when the field was computed.
    stamp: u64,
    /// The cost from each cell to the goal, or [`UNREACHED`].
    cost: Vec<u32>,
    /// The neighbor each cell steps to, or [`NONE`].
    next: Vec<u8>,
    open: BinaryHeap<Reverse<(u32, u32)>>,
    goal: Option<IVec2>,
    /// The tick of the last computation that [`FlowField::update`] made.
    computed: Option<u64>,
}

impl FlowField {
    /// The ticks [`update`](Self::update) waits at least between two
    /// computations for a goal that moves.
    pub const EVERY: u64 = 10;

    /// A field of no cells: every direction is zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fills the field for `grid`, toward the open cell nearest `goal`: the
    /// cell that holds it, or if that is solid, the nearest open one of the
    /// eight around it. Whether there is one; if not, every cell is
    /// unreached and every direction zero. A grid of `u32::MAX` cells or
    /// more is unreached everywhere too.
    pub fn compute(&mut self, grid: &TileGrid, goal: Vec2) -> bool {
        self.width = grid.width();
        self.height = grid.height();
        self.cell_size = grid.cell_size();
        self.origin = grid.origin();
        self.stamp = grid.stamp();
        self.open.clear();
        let cells = cell_count(grid).unwrap_or(0) as usize;
        self.cost.clear();
        self.cost.resize(cells, UNREACHED);
        self.next.clear();
        self.next.resize(cells, NONE);
        self.goal = None;
        if cells == 0 {
            return false;
        }
        let Some(goal) = open_cell_near(grid, goal) else {
            return false;
        };
        let Some(at) = index_of(grid, goal) else {
            return false;
        };
        self.goal = Some(goal);
        self.cost[at as usize] = 0;
        self.open.push(Reverse((0, at)));
        // The steps are the same both ways, so the cost from the goal is
        // the cost to it.
        while let Some(Reverse((cost, at))) = self.open.pop() {
            if cost != self.cost[at as usize] {
                continue;
            }
            let cell = cell_of(at, self.width);
            for direction in 0..NEIGHBOURS.len() {
                let Some((next, step)) = step(grid, cell, direction) else {
                    continue;
                };
                let Some(j) = index_of(grid, next) else {
                    continue;
                };
                let through = cost.saturating_add(step);
                if through < self.cost[j as usize] {
                    self.cost[j as usize] = through;
                    self.open.push(Reverse((through, j)));
                }
            }
        }
        for at in 0..self.cost.len() {
            let cost = self.cost[at];
            if cost == UNREACHED || cost == 0 {
                continue;
            }
            let cell = cell_of(at as u32, self.width);
            let mut best = (UNREACHED, NONE);
            for (direction, way) in (0u8..).zip(0..NEIGHBOURS.len()) {
                let Some((next, step)) = step(grid, cell, way) else {
                    continue;
                };
                let Some(j) = index_of(grid, next) else {
                    continue;
                };
                let through = self.cost[j as usize].saturating_add(step);
                if through < best.0 {
                    best = (through, direction);
                }
            }
            self.next[at] = best.1;
        }
        true
    }

    /// Computes the field again, toward `goal`, at tick `tick`, if `grid`
    /// is not the field's (of another size or place, or with a cell set
    /// since), or it was never computed by an update; or if the goal is in
    /// another cell than the field's and the last computation was
    /// [`EVERY`](Self::EVERY) ticks ago or more. Whether it computed.
    pub fn update(&mut self, grid: &TileGrid, goal: Vec2, tick: u64) -> bool {
        let same_grid = (
            self.width,
            self.height,
            self.cell_size,
            self.origin,
            self.stamp,
        ) == (
            grid.width(),
            grid.height(),
            grid.cell_size(),
            grid.origin(),
            grid.stamp(),
        );
        if let Some(computed) = self.computed
            && same_grid
            && (open_cell_near(grid, goal) == self.goal
                || tick.saturating_sub(computed) < Self::EVERY)
        {
            return false;
        }
        self.compute(grid, goal);
        self.computed = Some(tick);
        true
    }

    /// The goal's cell, if the field has one.
    pub fn goal(&self) -> Option<IVec2> {
        self.goal
    }

    /// The cost of the shortest path from `cell` to the goal, in
    /// thousandths of a cell; `None` if it does not reach `cell`.
    pub fn cost(&self, cell: IVec2) -> Option<u32> {
        let at = self.index(cell)?;
        Some(self.cost[at]).filter(|&cost| cost != UNREACHED)
    }

    /// The neighbor of `cell` the way to the goal goes through; `None` at
    /// the goal, and where the field does not reach.
    pub fn next_cell(&self, cell: IVec2) -> Option<IVec2> {
        let direction = self.next[self.index(cell)?];
        let offset = NEIGHBOURS.get(usize::from(direction))?;
        Some(IVec2::new(cell.x + offset.x, cell.y + offset.y))
    }

    /// The unit vector from `cell` toward the next cell on the way to the
    /// goal; zero at the goal, and where the field does not reach.
    pub fn direction(&self, cell: IVec2) -> Vec2 {
        self.index(cell)
            .and_then(|at| DIRECTIONS.get(usize::from(self.next[at])))
            .copied()
            .unwrap_or(Vec2::ZERO)
    }

    /// The [`direction`](Self::direction) of the cell that holds `point`;
    /// zero for a point that is not a number.
    pub fn direction_at(&self, point: Vec2) -> Vec2 {
        // A field never computed has no cells, whatever its cell size.
        if !(point.x.is_finite() && point.y.is_finite()) {
            return Vec2::ZERO;
        }
        let cell = ((point - self.origin) / self.cell_size).floor().as_ivec2();
        self.direction(cell)
    }

    /// The index of `cell` in the field's cells.
    fn index(&self, cell: IVec2) -> Option<usize> {
        let x = u32::try_from(cell.x).ok().filter(|&x| x < self.width)?;
        let y = u32::try_from(cell.y).ok().filter(|&y| y < self.height)?;
        let at = u64::from(y) * u64::from(self.width) + u64::from(x);
        usize::try_from(at).ok().filter(|&at| at < self.cost.len())
    }
}
