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

//! Paths on the tile grid: A* ([`Pathfinder`]), smoothed by string-pulling
//! with a line of sight over the grid ([`line_of_sight`]), and a queue that
//! spreads the searches over ticks within a budget of expansions
//! ([`PathQueue`]).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::hash::Hash;

use nadir_core::{FastHashMap, IVec2, Vec2};

use crate::{EPSILON, TileGrid};

/// What a step to a cell beside costs: a cell's side, in thousandths.
pub const STRAIGHT_COST: u32 = 1_000;

/// What a step to a cell at a corner costs: √2 cells' sides, in
/// thousandths.
pub const DIAGONAL_COST: u32 = 1_414;

/// The eight neighbors of a cell, in the order searches try them: the
/// four sides (right, down, left, up), then the four corners.
pub(crate) const NEIGHBORS: [IVec2; 8] = [
    IVec2::new(1, 0),
    IVec2::new(0, 1),
    IVec2::new(-1, 0),
    IVec2::new(0, -1),
    IVec2::new(1, 1),
    IVec2::new(-1, 1),
    IVec2::new(-1, -1),
    IVec2::new(1, -1),
];

/// How many cells `grid` has, if each has an index that fits in a `u32`
/// other than `u32::MAX`.
pub(crate) fn cell_count(grid: &TileGrid) -> Option<u32> {
    let cells = u64::from(grid.width()) * u64::from(grid.height());
    u32::try_from(cells).ok().filter(|&cells| cells < u32::MAX)
}

/// The index of `cell`, row by row, if it is in `grid`.
pub(crate) fn index_of(grid: &TileGrid, cell: IVec2) -> Option<u32> {
    let x = u32::try_from(cell.x).ok().filter(|&x| x < grid.width())?;
    let y = u32::try_from(cell.y).ok().filter(|&y| y < grid.height())?;
    // Fits for the grids that `cell_count` counts, which the searches
    // check first.
    y.checked_mul(grid.width())?.checked_add(x)
}

/// The cell of `index`, in a grid `width` cells wide.
pub(crate) fn cell_of(index: u32, width: u32) -> IVec2 {
    // Below `i32::MAX`: a grid's width and height are each its cell count
    // at most, which a search keeps below `u32::MAX`; and a row or column
    // past `i32::MAX` has no `IVec2`, so a cell there is never searched.
    IVec2::new((index % width) as i32, (index / width) as i32)
}

/// Whether `cell` is in `grid` and not solid: a cell a path may cross.
pub(crate) fn is_open(grid: &TileGrid, cell: IVec2) -> bool {
    index_of(grid, cell).is_some() && !grid.is_solid(cell)
}

/// The cell that a step from `cell` toward neighbor `direction` reaches,
/// and what it costs, if the step is allowed: the cell is open, and a step
/// to a corner has both cells beside it open, so that no path cuts a
/// solid cell's corner.
pub(crate) fn step(grid: &TileGrid, cell: IVec2, direction: usize) -> Option<(IVec2, u32)> {
    let offset = NEIGHBORS[direction];
    let to = IVec2::new(cell.x.checked_add(offset.x)?, cell.y.checked_add(offset.y)?);
    if !is_open(grid, to) {
        return None;
    }
    if offset.x != 0 && offset.y != 0 {
        let beside = [IVec2::new(to.x, cell.y), IVec2::new(cell.x, to.y)];
        if !beside.into_iter().all(|side| is_open(grid, side)) {
            return None;
        }
        return Some((to, DIAGONAL_COST));
    }
    Some((to, STRAIGHT_COST))
}

/// The octile distance from `a` to `b`, in thousandths of a cell: the
/// cost of the path between them with no cell in the way.
pub(crate) fn octile(a: IVec2, b: IVec2) -> u32 {
    let dx = a.x.abs_diff(b.x);
    let dy = a.y.abs_diff(b.y);
    let (long, short) = (dx.max(dy), dx.min(dy));
    long.saturating_mul(STRAIGHT_COST)
        .saturating_add(short.saturating_mul(DIAGONAL_COST - STRAIGHT_COST))
}

/// The open cell a path from or to `point` uses: the one that holds it, or
/// if that one is solid or outside the grid, the open cell of the eight
/// around it nearest to `point` (the first in [`NEIGHBORS`]' order of two
/// as near). `None` if there is none, or `point` is not a number.
pub(crate) fn open_cell_near(grid: &TileGrid, point: Vec2) -> Option<IVec2> {
    if !(point.x.is_finite() && point.y.is_finite()) {
        return None;
    }
    let cell = grid.cell_at(point);
    if is_open(grid, cell) {
        return Some(cell);
    }
    let mut best: Option<(f32, IVec2)> = None;
    for offset in NEIGHBORS {
        let Some(near) = cell
            .x
            .checked_add(offset.x)
            .zip(cell.y.checked_add(offset.y))
            .map(|(x, y)| IVec2::new(x, y))
        else {
            continue;
        };
        if !is_open(grid, near) {
            continue;
        }
        let rect = grid.cell_rect(near);
        let distance = (point.clamp(rect.min, rect.max) - point).length_squared();
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, near));
        }
    }
    best.map(|(_, near)| near)
}

/// Whether a box of half size `clearance`, its center moving in a straight
/// line from `from` to `to`, stays clear of every solid cell of `grid`: it
/// does not come within [`EPSILON`] of one, edges and corners included.
/// With no clearance it is a line, which a seam between two solid cells
/// stops, as does the corner where two meet.
///
/// Cells outside the grid are not solid. A point that is not a number has
/// no line of sight; a clearance below 0, or not a number, counts as 0.
/// The answer is the same both ways. Its arithmetic is in `f64`, from the
/// end nearer the grid, and widened by a bound on its rounding, so that it
/// errs toward no sight: far from the grid, where `f32` rounding grows, a
/// box may be judged to touch a cell it misses, never the other way. The
/// cost grows with the cells under the box's path, and is never more than
/// the grid's.
pub fn line_of_sight(grid: &TileGrid, from: Vec2, to: Vec2, clearance: Vec2) -> bool {
    if !(from.x.is_finite() && from.y.is_finite() && to.x.is_finite() && to.y.is_finite()) {
        return false;
    }
    if grid.width() == 0 || grid.height() == 0 {
        return true;
    }
    let (from, to) = nearer_first(grid, from, to);
    let half = clearance.max(Vec2::ZERO) + Vec2::splat(EPSILON);
    let last = IVec2::new(
        i32::try_from(grid.width() - 1).unwrap_or(i32::MAX),
        i32::try_from(grid.height() - 1).unwrap_or(i32::MAX),
    );
    // Rounded outward: a column the box only reaches by a rounding step is
    // in.
    let first_column = grid
        .cell_at(Vec2::new((from.x.min(to.x) - half.x).next_down(), 0.0))
        .x
        .max(0);
    let last_column = grid
        .cell_at(Vec2::new((from.x.max(to.x) + half.x).next_up(), 0.0))
        .x
        .min(last.x);
    let (fx, fy) = (f64::from(from.x), f64::from(from.y));
    let (dx, dy) = (f64::from(to.x) - fx, f64::from(to.y) - fy);
    let (hx, hy) = (f64::from(half.x), f64::from(half.y));
    // A few steps of `f64` rounding of what each quantity is made from.
    let rounding = |size: f64| 4.0 * f64::EPSILON * size;
    // Column by column, the rows that the box covers while its center is
    // within half its width of the column: the box's path through a
    // column of cells is convex, so those rows are exactly the ones it
    // touches there, to within the rounding, by which they are widened.
    for column in first_column..=last_column {
        let span = grid.cell_rect(IVec2::new(column, 0));
        let (left, right) = (f64::from(span.min.x) - hx, f64::from(span.max.x) + hx);
        let (start, end, slack) = if dx == 0.0 {
            (0.0, 1.0, 0.0)
        } else {
            let a = (left - fx) / dx;
            let b = (right - fx) / dx;
            let t_slack = rounding(left.abs().max(right.abs()) + fx.abs()) / dx.abs();
            (
                (a.min(b) - t_slack).max(0.0),
                (a.max(b) + t_slack).min(1.0),
                t_slack * dy.abs(),
            )
        };
        // `min` and `max` give no NaN here.
        if start > end {
            continue;
        }
        let slack = slack + rounding(fy.abs() + dy.abs());
        let (top, bottom) = (fy + dy * start, fy + dy * end);
        // To `f32`, rounded outward.
        let low = ((top.min(bottom) - hy - slack) as f32).next_down();
        let high = ((top.max(bottom) + hy + slack) as f32).next_up();
        let first_row = grid.cell_at(Vec2::new(0.0, low)).y.max(0);
        let last_row = grid.cell_at(Vec2::new(0.0, high)).y.min(last.y);
        for row in first_row..=last_row {
            if grid.is_solid(IVec2::new(column, row)) {
                return false;
            }
        }
    }
    true
}

/// `from` and `to` in the order [`line_of_sight`] walks them: the one
/// nearer `grid`'s cells first, or of two as near, the lesser by `x`, then
/// by `y`.
fn nearer_first(grid: &TileGrid, from: Vec2, to: Vec2) -> (Vec2, Vec2) {
    let size = f64::from(grid.cell_size());
    let min = grid.origin();
    let (min_x, min_y) = (f64::from(min.x), f64::from(min.y));
    let (max_x, max_y) = (
        min_x + f64::from(grid.width()) * size,
        min_y + f64::from(grid.height()) * size,
    );
    let away = |point: Vec2| {
        let (x, y) = (f64::from(point.x), f64::from(point.y));
        let across = (min_x - x).max(x - max_x).max(0.0);
        let down = (min_y - y).max(y - max_y).max(0.0);
        (across.max(down), point.x, point.y)
    };
    if away(to) < away(from) {
        (to, from)
    } else {
        (from, to)
    }
}

/// A path to find: from where to where, for an agent of what size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathRequest {
    /// Where the agent is: the center of its collider, say.
    pub from: Vec2,
    /// Where it is going.
    pub to: Vec2,
    /// Half the size of the agent's box, which the smoothed path keeps
    /// clear of solid cells ([`line_of_sight`]). Zero smooths the path as
    /// a line.
    pub clearance: Vec2,
}

impl PathRequest {
    /// A path from `from` to `to`, smoothed as a line.
    pub const fn new(from: Vec2, to: Vec2) -> Self {
        Self {
            from,
            to,
            clearance: Vec2::ZERO,
        }
    }

    /// The same path, smoothed for a box of half size `half`.
    #[must_use]
    pub const fn with_clearance(mut self, half: Vec2) -> Self {
        self.clearance = half;
        self
    }
}

/// How a search went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Search {
    /// It is under way: [`Pathfinder::run`] goes on with it.
    Running,
    /// It reached the goal.
    Found,
    /// The goal cannot be reached within the search's limit.
    NotFound,
}

/// The search under way.
#[derive(Clone, Copy, Debug)]
struct Active {
    goal: u32,
    goal_cell: IVec2,
    width: u32,
    height: u32,
    /// The grid's [stamp](TileGrid::stamp) when it began.
    stamp: u64,
    limit: u32,
}

/// A* on a [`TileGrid`], with the memory it reuses from search to search.
///
/// - Cells are 8-connected: a step to a cell beside costs [`STRAIGHT_COST`],
///   and one to a cell at a corner [`DIAGONAL_COST`], both in thousandths of a
///   cell. The heuristic is the octile distance, so the paths found are
///   the shortest there are, by that measure.
/// - Paths cross only open cells, those in the grid and not solid: cells
///   outside the grid are not solid, but a path stays inside it.
/// - A step to a corner needs both cells beside it open: no path cuts a
///   solid cell's corner.
/// - Of the open cells, a search expands first the one of least estimated
///   cost, then of least heuristic, then of least index: the same path on
///   every run.
/// - `limit` caps a search's expansions: a goal it cannot reach within
///   them gives `None`, as does one it cannot reach at all.
/// - Lengths are `u32`s of thousandths: a path longer than 4,294,967
///   cells' sides is not found.
///
/// Its memory, 12 bytes a cell of the largest grid searched and a heap of
/// the cells to expand, is kept: once both have grown to fit the largest
/// search yet, a search allocates nothing.
#[derive(Clone, Debug, Default)]
pub struct Pathfinder {
    /// The cost of the best way to each cell found in this search.
    cost: Vec<u32>,
    /// The cell each was reached from: itself for the start.
    parent: Vec<u32>,
    /// Twice the search's number once a cell is reached in it, plus one
    /// once it is expanded.
    state: Vec<u32>,
    /// The cells to expand: estimated cost, heuristic, index.
    open: BinaryHeap<Reverse<(u32, u32, u32)>>,
    /// The number of the current search, from 1.
    search: u32,
    /// The search under way, if any.
    active: Option<Active>,
    /// The width of the grid of the last search.
    width: u32,
    /// The cells expanded in the last search.
    expansions: u32,
    /// The goal's index, once the last search has found it.
    found: Option<u32>,
    /// The cells of the last path, which [`Pathfinder::find`] smooths.
    cells: Vec<IVec2>,
}

impl Pathfinder {
    /// No memory yet: the first search allocates it.
    pub fn new() -> Self {
        Self::default()
    }

    /// The shortest path from cell `start` to cell `goal`, expanding at most
    /// `limit` cells. `out` gets its cells, `start` first and `goal` last,
    /// and the result is its length, in thousandths of a cell's side.
    ///
    /// `None`, with `out` empty, if `start` or `goal` is solid or outside
    /// the grid, or the goal cannot be reached within the limit: a goal
    /// that cannot be reached at all ends the search once every cell it
    /// can reach is expanded, or at the limit. A limit of 0 finds nothing.
    /// A grid of `u32::MAX` cells or more has no paths.
    pub fn find_cells(
        &mut self,
        grid: &TileGrid,
        start: IVec2,
        goal: IVec2,
        limit: u32,
        out: &mut Vec<IVec2>,
    ) -> Option<u32> {
        out.clear();
        let mut search = self.begin(grid, start, goal, limit);
        while search == Search::Running {
            search = self.run(grid, u32::MAX).0;
        }
        (search == Search::Found).then(|| self.path(out))
    }

    /// The path for an agent: from the open cell nearest `request.from` to
    /// the one nearest `request.to` (the cell that holds the point, or if
    /// it is solid, the nearest open one of the eight around it), expanding
    /// at most `limit` cells, then smoothed by string-pulling: each corner
    /// of the cells' path that the agent can see past, with
    /// [`line_of_sight`] for a box of `request.clearance`, is left out.
    ///
    /// `out` gets the points to walk through: `request.from` first, then
    /// centers of cells on the way (where the path turns, every 16 cells at
    /// most along a straight run, and each cell's own where the agent sees
    /// past none), then `request.to`, or the center of the open cell
    /// nearest it if its own is solid. Smoothing costs about 16 cells of
    /// line of sight a cell of the path. The result is the length of the
    /// cells' path, before smoothing, in thousandths of a cell. `None`, with `out` empty, as for
    /// [`find_cells`](Self::find_cells), or if either end has no open cell
    /// near it or is not a number.
    pub fn find(
        &mut self,
        grid: &TileGrid,
        request: PathRequest,
        limit: u32,
        out: &mut Vec<Vec2>,
    ) -> Option<u32> {
        out.clear();
        let start = open_cell_near(grid, request.from)?;
        let goal = open_cell_near(grid, request.to)?;
        let mut cells = std::mem::take(&mut self.cells);
        let length = self.find_cells(grid, start, goal, limit, &mut cells);
        if length.is_some() {
            smooth(grid, &cells, request, out);
        }
        self.cells = cells;
        length
    }

    /// How many cells the last search expanded: the goal's own among them,
    /// if it was reached.
    pub fn expansions(&self) -> u32 {
        self.expansions
    }

    /// Starts a search from `start` to `goal`, expanding at most `limit`
    /// cells; [`run`](Self::run) carries it on. It ends at once, not found,
    /// if either cell is not open, or the grid has too many cells.
    pub(crate) fn begin(
        &mut self,
        grid: &TileGrid,
        start: IVec2,
        goal: IVec2,
        limit: u32,
    ) -> Search {
        self.active = None;
        self.found = None;
        self.expansions = 0;
        self.open.clear();
        let Some(cells) = cell_count(grid) else {
            return Search::NotFound;
        };
        let (Some(from), Some(to)) = (index_of(grid, start), index_of(grid, goal)) else {
            return Search::NotFound;
        };
        if grid.is_solid(start) || grid.is_solid(goal) {
            return Search::NotFound;
        }
        let stamp = grid.stamp();
        let cells = cells as usize;
        if self.state.len() < cells {
            self.cost.resize(cells, 0);
            self.parent.resize(cells, 0);
            self.state.resize(cells, 0);
        }
        // Each search marks the cells it reaches with its own number, so
        // nothing is cleared between them, until the numbers run out.
        if self.search >= u32::MAX / 2 - 1 {
            self.state.fill(0);
            self.search = 0;
        }
        self.search += 1;
        self.width = grid.width();
        let reached = self.search * 2;
        let at = from as usize;
        self.cost[at] = 0;
        self.parent[at] = from;
        self.state[at] = reached;
        let h = octile(start, goal);
        self.open.push(Reverse((h, h, from)));
        self.active = Some(Active {
            goal: to,
            goal_cell: goal,
            width: grid.width(),
            height: grid.height(),
            stamp,
            limit,
        });
        Search::Running
    }

    /// Whether the search under way was begun on `grid` as it is: its
    /// size, and no cell set since.
    pub(crate) fn searches(&self, grid: &TileGrid) -> bool {
        self.active.is_some_and(|active| {
            (active.width, active.height, active.stamp)
                == (grid.width(), grid.height(), grid.stamp())
        })
    }

    /// Carries the search under way on, expanding at most `budget` cells,
    /// and says how it went and how many it expanded. A grid of another
    /// size than the search began on, or whose cells were set since, ends
    /// it, not found.
    pub(crate) fn run(&mut self, grid: &TileGrid, budget: u32) -> (Search, u32) {
        let Some(active) = self.active else {
            return (Search::NotFound, 0);
        };
        if !self.searches(grid) {
            self.active = None;
            return (Search::NotFound, 0);
        }
        let reached = self.search * 2;
        let expanded = reached + 1;
        let mut spent = 0;
        while spent < budget {
            let Some(Reverse((estimate, h, at))) = self.open.pop() else {
                self.active = None;
                return (Search::NotFound, spent);
            };
            let i = at as usize;
            // A cell is in the heap once for each better way found to it:
            // only the best counts, and only once.
            if self.state[i] == expanded || estimate.saturating_sub(h) != self.cost[i] {
                continue;
            }
            if self.expansions >= active.limit {
                self.active = None;
                return (Search::NotFound, spent);
            }
            self.expansions += 1;
            spent += 1;
            self.state[i] = expanded;
            if at == active.goal {
                self.active = None;
                self.found = Some(at);
                return (Search::Found, spent);
            }
            let cell = cell_of(at, active.width);
            let cost = self.cost[i];
            for direction in 0..NEIGHBORS.len() {
                let Some((next, step)) = step(grid, cell, direction) else {
                    continue;
                };
                let Some(j) = index_of(grid, next) else {
                    continue;
                };
                let j_at = j as usize;
                let through = cost.saturating_add(step);
                let state = self.state[j_at];
                if state == expanded || (state == reached && through >= self.cost[j_at]) {
                    continue;
                }
                self.state[j_at] = reached;
                self.cost[j_at] = through;
                self.parent[j_at] = at;
                let h = octile(next, active.goal_cell);
                self.open.push(Reverse((through.saturating_add(h), h, j)));
            }
        }
        (Search::Running, spent)
    }

    /// The cells of the path the last search found, from its start to its
    /// goal, into `out`; and its length.
    pub(crate) fn path(&self, out: &mut Vec<IVec2>) -> u32 {
        out.clear();
        // Back from the goal to the start, which is its own parent. Every
        // cell on the way was reached in this search, so each parent is
        // one step nearer the start.
        let Some(mut at) = self.found else {
            return 0;
        };
        let length = self.cost[at as usize];
        loop {
            out.push(cell_of(at, self.width));
            let parent = self.parent[at as usize];
            if parent == at {
                break;
            }
            at = parent;
        }
        out.reverse();
        length
    }
}

/// The longest leg smoothing makes, in cells each way: it bounds what a
/// path's smoothing costs, about this many cells of line of sight for each
/// cell of the path.
const LONGEST_LEG: f32 = 16.0;

/// `cells`, a path from `request.from`'s cell to `request.to`'s, smoothed
/// into `out` by string-pulling: from each point kept, the furthest of the
/// path's next points that a box of `request.clearance` can see, one after
/// another and no more than [`LONGEST_LEG`] cells away, is the next one
/// kept. The points are the centers of the path's cells, then the goal; a
/// point it cannot see even the next of is kept, so the cells' own steps
/// are the fallback.
fn smooth(grid: &TileGrid, cells: &[IVec2], request: PathRequest, out: &mut Vec<Vec2>) {
    out.clear();
    out.push(request.from);
    let Some(&last) = cells.last() else {
        return;
    };
    let center = |cell: IVec2| grid.cell_rect(cell).center();
    // The goal is `to` itself when its own cell is the path's last.
    let end = if grid.cell_at(request.to) == last {
        request.to
    } else {
        center(last)
    };
    // Every cell's center, the start's and the goal's among them, so that
    // a point it cannot see past falls back on the cells' own steps; but
    // not one that is an end already.
    let targets = cells
        .iter()
        .map(|&cell| center(cell))
        .filter(|&point| point != request.from && point != end)
        .chain(std::iter::once(end));
    let reach = LONGEST_LEG * grid.cell_size();
    let sees = |from: Vec2, to: Vec2| {
        let apart = (to - from).abs();
        apart.x <= reach && apart.y <= reach && line_of_sight(grid, from, to, request.clearance)
    };
    let mut anchor = request.from;
    // The last point after `anchor` that it sees, all before it seen too.
    let mut seen: Option<Vec2> = None;
    for target in targets {
        if sees(anchor, target) {
            seen = Some(target);
            continue;
        }
        if let Some(corner) = seen.take() {
            out.push(corner);
            anchor = corner;
            if sees(anchor, target) {
                seen = Some(target);
                continue;
            }
        }
        out.push(target);
        anchor = target;
    }
    if let Some(corner) = seen {
        out.push(corner);
    }
}

/// How a path asked of a [`PathQueue`] turned out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathResult<'a> {
    /// The points to walk through, as [`Pathfinder::find`] gives them:
    /// where the agent was first, then its corners, then the goal.
    Found(&'a [Vec2]),
    /// There is no path within the queue's limit: an end has no open cell
    /// near it, or the goal cannot be reached.
    NotFound,
}

impl<'a> PathResult<'a> {
    /// The points, if a path was found.
    pub fn points(self) -> Option<&'a [Vec2]> {
        match self {
            Self::Found(points) => Some(points),
            Self::NotFound => None,
        }
    }
}

/// One key's request and its last path.
#[derive(Clone, Debug)]
struct Slot<K> {
    /// `None` while the slot is free.
    key: Option<K>,
    request: PathRequest,
    /// Whether the request waits in the queue, or is being searched.
    pending: bool,
    /// Whether the last search for it found a path, once one has ended.
    found: Option<bool>,
    /// The last path found.
    points: Vec<Vec2>,
}

/// Paths asked for by key (an entity, say), found a budget of expansions
/// at a time: the searches of a tick expand no more cells than its budget,
/// however many agents ask, and one too long for a tick goes on in the
/// next.
///
/// - [`request`](Self::request) asks for a path for a key, replacing the
///   key's earlier request, which keeps its place in the queue. One being
///   searched that asks for other ends goes to the back of the queue, to
///   start again there.
/// - [`update`](Self::update), once a tick, searches the requests in the
///   queue's order, the oldest first, spending at most the
///   [budget](Self::budget) of expansions across them, and each search at
///   most the [limit](Self::limit). The same requests give the same paths,
///   in the same ticks, on every run.
/// - The update that ends a search smooths its path too, which the budget
///   does not count: about 16 cells of line of sight for each cell of the
///   path ([`Pathfinder::find`]).
/// - [`result`](Self::result) gives a key's last path, found or not, which
///   stays while a newer request waits: an agent walks its old path until
///   the new one comes. [`is_pending`](Self::is_pending) says whether one
///   waits.
///
/// A search under way starts again when the grid it searches changes: a
/// grid of another size, or a cell set since it began. A path already
/// found stays as it was found.
///
/// Buffers are kept, so a steady tick, the same keys asking again for
/// paths no longer than before, allocates nothing.
#[derive(Clone, Debug)]
pub struct PathQueue<K = u64> {
    finder: Pathfinder,
    budget: u32,
    limit: u32,
    slots: Vec<Slot<K>>,
    free: Vec<usize>,
    by_key: FastHashMap<K, usize>,
    /// The slots whose requests wait, the oldest first. The first is being
    /// searched, when `searching` says so.
    waiting: VecDeque<usize>,
    searching: bool,
    cells: Vec<IVec2>,
}

impl<K: Copy + Eq + Hash> PathQueue<K> {
    /// The expansions an update spends, unless set: about 0.2 ms of a
    /// tick, at the 190 ns an expansion that `astar_256` measured on an
    /// i7-11800H. Most searches across a 64 × 48 level take fewer; one to a
    /// goal it cannot reach there takes up to 3 updates.
    pub const DEFAULT_BUDGET: u32 = 1_024;

    /// The expansions one search may take, unless set: a 256 × 256 grid's
    /// cells.
    pub const DEFAULT_LIMIT: u32 = 65_536;

    /// No requests, with the [default budget](Self::DEFAULT_BUDGET) and
    /// [limit](Self::DEFAULT_LIMIT).
    pub fn new() -> Self {
        Self::with_budget(Self::DEFAULT_BUDGET)
    }

    /// No requests, spending at most `budget` expansions an update.
    pub fn with_budget(budget: u32) -> Self {
        Self {
            finder: Pathfinder::new(),
            budget,
            limit: Self::DEFAULT_LIMIT,
            slots: Vec::new(),
            free: Vec::new(),
            by_key: FastHashMap::default(),
            waiting: VecDeque::new(),
            searching: false,
            cells: Vec::new(),
        }
    }

    /// The expansions an update spends at most. 0 stops the searches.
    pub fn budget(&self) -> u32 {
        self.budget
    }

    /// Sets the expansions an update spends at most.
    pub fn set_budget(&mut self, budget: u32) {
        self.budget = budget;
    }

    /// The expansions one search takes at most: past them, its path is not
    /// found.
    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// Sets the expansions one search takes at most, from the next search
    /// begun.
    pub fn set_limit(&mut self, limit: u32) {
        self.limit = limit;
    }

    /// Asks for a path for `key`, in place of its earlier request: one that
    /// waits keeps its place, and one being searched, if it asks for other
    /// ends, goes to the back of the queue, to start again there. Its last
    /// path stays until this one is found.
    ///
    /// A search too long for one update starts again each time its request
    /// changes: an agent chasing a moving goal should ask again when its
    /// last path has come, or every so many ticks, not every tick.
    pub fn request(&mut self, key: K, request: PathRequest) {
        if let Some(&at) = self.by_key.get(&key) {
            let slot = &mut self.slots[at];
            let changed = slot.request != request;
            slot.request = request;
            if !slot.pending {
                slot.pending = true;
                self.waiting.push_back(at);
            } else if changed && self.searching && self.waiting.front() == Some(&at) {
                // Its search is for ends it no longer asks: behind the
                // others, so that one asking again and again holds up no
                // one but itself.
                self.searching = false;
                self.waiting.pop_front();
                self.waiting.push_back(at);
            }
            return;
        }
        let slot = Slot {
            key: Some(key),
            request,
            pending: true,
            found: None,
            points: Vec::new(),
        };
        let at = match self.free.pop() {
            Some(at) => {
                let points = std::mem::take(&mut self.slots[at].points);
                self.slots[at] = Slot { points, ..slot };
                at
            }
            None => {
                self.slots.push(slot);
                self.slots.len() - 1
            }
        };
        self.by_key.insert(key, at);
        self.waiting.push_back(at);
    }

    /// Forgets `key`: its request, if it waits, and its path. Whether it
    /// had either.
    pub fn cancel(&mut self, key: &K) -> bool {
        let Some(at) = self.by_key.remove(key) else {
            return false;
        };
        self.forget(at);
        true
    }

    /// Forgets every key for which `keep` says no: an agent despawned, say.
    pub fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        for at in 0..self.slots.len() {
            if let Some(key) = self.slots[at].key
                && !keep(&key)
            {
                self.by_key.remove(&key);
                self.forget(at);
            }
        }
    }

    /// Frees slot `at`, whose key is out of `by_key`.
    fn forget(&mut self, at: usize) {
        let slot = &mut self.slots[at];
        if slot.pending {
            if self.waiting.front() == Some(&at) {
                self.searching = false;
            }
            self.waiting.retain(|&waiting| waiting != at);
        }
        slot.key = None;
        slot.pending = false;
        slot.found = None;
        slot.points.clear();
        self.free.push(at);
    }

    /// Whether a request for `key` waits for its path.
    pub fn is_pending(&self, key: &K) -> bool {
        self.by_key
            .get(key)
            .is_some_and(|&at| self.slots[at].pending)
    }

    /// `key`'s last path, found or not; `None` before its first search
    /// ends, or for a key never asked or forgotten.
    pub fn result(&self, key: &K) -> Option<PathResult<'_>> {
        Self::outcome(&self.slots[*self.by_key.get(key)?])
    }

    fn outcome(slot: &Slot<K>) -> Option<PathResult<'_>> {
        match slot.found? {
            true => Some(PathResult::Found(&slot.points)),
            false => Some(PathResult::NotFound),
        }
    }

    /// Each key whose search has ended, and its last path, in an order
    /// that the requests' history fixes.
    pub fn results(&self) -> impl Iterator<Item = (K, PathResult<'_>)> {
        self.slots
            .iter()
            .filter_map(|slot| Some((slot.key?, Self::outcome(slot)?)))
    }

    /// How many keys it holds: those whose requests wait or whose paths it
    /// keeps.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// Whether it holds no key.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Searches the waiting requests on `grid`, the oldest first, until
    /// none waits or the budget is spent: one not finished goes on in the
    /// next update. Returns the expansions spent.
    pub fn update(&mut self, grid: &TileGrid) -> u32 {
        let mut spent = 0;
        while let Some(&at) = self.waiting.front() {
            if spent >= self.budget {
                break;
            }
            let slot = &self.slots[at];
            let mut search = Search::Running;
            if !self.searching || !self.finder.searches(grid) {
                let request = slot.request;
                search = match (
                    open_cell_near(grid, request.from),
                    open_cell_near(grid, request.to),
                ) {
                    (Some(start), Some(goal)) => self.finder.begin(grid, start, goal, self.limit),
                    _ => Search::NotFound,
                };
                self.searching = search == Search::Running;
            }
            if search == Search::Running {
                let (ran, expanded) = self.finder.run(grid, self.budget - spent);
                spent += expanded;
                search = ran;
            }
            if search == Search::Running {
                break;
            }
            self.searching = false;
            self.waiting.pop_front();
            let slot = &mut self.slots[at];
            slot.pending = false;
            if search == Search::Found {
                self.finder.path(&mut self.cells);
                smooth(grid, &self.cells, slot.request, &mut slot.points);
                slot.found = Some(true);
            } else {
                slot.points.clear();
                slot.found = Some(false);
            }
        }
        spent
    }
}

impl<K: Copy + Eq + Hash> Default for PathQueue<K> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capacities of every buffer a queue holds.
    fn capacities(queue: &PathQueue) -> Vec<usize> {
        let finder = &queue.finder;
        let mut all = vec![
            finder.cost.capacity(),
            finder.parent.capacity(),
            finder.state.capacity(),
            finder.open.capacity(),
            queue.slots.capacity(),
            queue.free.capacity(),
            queue.by_key.capacity(),
            queue.waiting.capacity(),
            queue.cells.capacity(),
        ];
        all.extend(queue.slots.iter().map(|slot| slot.points.capacity()));
        all
    }

    #[test]
    fn a_steady_tick_of_requests_keeps_every_buffer() {
        let mut grid = TileGrid::new(40, 30, 16.0, Vec2::ZERO);
        for y in 0..25 {
            grid.set_solid(IVec2::new(20, y), true);
        }
        let mut queue = PathQueue::with_budget(300);
        let tick = |queue: &mut PathQueue, shift: f32| {
            // One agent gone and come back each round: its slot is freed and
            // taken again, not a new one each time.
            queue.cancel(&5);
            for key in 0..12u64 {
                let from = Vec2::new(8.0 + key as f32 * 8.0, 40.0 + shift);
                let to = Vec2::new(600.0 - shift, 400.0 - key as f32 * 12.0);
                queue.request(
                    key,
                    PathRequest::new(from, to).with_clearance(Vec2::splat(4.0)),
                );
            }
            while queue.is_pending(&0) || queue.is_pending(&11) {
                queue.update(&grid);
            }
        };
        // Two rounds to grow the buffers, then the same work again and
        // again, the goals moving a little: nothing grows.
        tick(&mut queue, 0.0);
        tick(&mut queue, 3.0);
        let grown = capacities(&queue);
        for round in 0..10 {
            tick(&mut queue, (round % 2) as f32 * 3.0);
            assert_eq!(capacities(&queue), grown, "round {round}");
        }
    }
}
