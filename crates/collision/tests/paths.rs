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

//! Pathfinding (roadmap M9.5): A* against an oracle written apart from the
//! crate's (a Dijkstra that scans for its nearest cell, with no heap and no
//! heuristic), on hand-made and random grids; corners; goals it cannot
//! reach; the queue's budget and order; smoothing and the line of sight;
//! and flow fields against the same oracle.

use nadir_collision::{
    DIAGONAL_COST, FlowField, PathQueue, PathRequest, PathResult, Pathfinder, STRAIGHT_COST,
    TileGrid, line_of_sight,
};
use nadir_core::{IVec2, Pcg32, Vec2};

/// The side of the tests' cells.
const CELL: f32 = 16.0;

/// A grid drawn as rows: `#` solid, anything else open; and where its `S`
/// and `G` are, if it has them.
fn drawn(rows: &[&str]) -> (TileGrid, Option<IVec2>, Option<IVec2>) {
    let width = rows[0].len() as u32;
    let mut grid = TileGrid::new(width, rows.len() as u32, CELL, Vec2::ZERO);
    let (mut start, mut goal) = (None, None);
    for (y, row) in rows.iter().enumerate() {
        assert_eq!(row.len() as u32, width, "{row:?}");
        for (x, letter) in row.chars().enumerate() {
            let cell = IVec2::new(x as i32, y as i32);
            match letter {
                '#' => grid.set_solid(cell, true),
                'S' => start = Some(cell),
                'G' => goal = Some(cell),
                _ => {}
            }
        }
    }
    (grid, start, goal)
}

/// Whether `cell` is in `grid` and not solid.
fn open(grid: &TileGrid, cell: IVec2) -> bool {
    cell.x >= 0
        && cell.y >= 0
        && (cell.x as u32) < grid.width()
        && (cell.y as u32) < grid.height()
        && !grid.is_solid(cell)
}

/// What the step from `a` to `b` costs, if it is one: to one of the eight
/// cells round `a`, open, and at a corner only past two open cells.
fn step_cost(grid: &TileGrid, a: IVec2, b: IVec2) -> Option<u32> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if dx.abs() > 1 || dy.abs() > 1 || (dx, dy) == (0, 0) || !open(grid, b) {
        return None;
    }
    if dx != 0 && dy != 0 {
        let sides = open(grid, IVec2::new(b.x, a.y)) && open(grid, IVec2::new(a.x, b.y));
        return sides.then_some(DIAGONAL_COST);
    }
    Some(STRAIGHT_COST)
}

/// The oracle: the cost of the shortest path from `from` to every cell,
/// by a Dijkstra that scans every cell for the nearest one not done.
fn oracle(grid: &TileGrid, from: IVec2) -> Vec<Option<u32>> {
    let (w, h) = (grid.width() as i32, grid.height() as i32);
    let at = |cell: IVec2| (cell.y * w + cell.x) as usize;
    let mut cost = vec![None; (w * h) as usize];
    let mut done = vec![false; (w * h) as usize];
    if !open(grid, from) {
        return cost;
    }
    cost[at(from)] = Some(0u32);
    loop {
        let mut nearest: Option<(u32, IVec2)> = None;
        for y in 0..h {
            for x in 0..w {
                let cell = IVec2::new(x, y);
                if let (false, Some(c)) = (done[at(cell)], cost[at(cell)])
                    && nearest.is_none_or(|(best, _)| c < best)
                {
                    nearest = Some((c, cell));
                }
            }
        }
        let Some((c, cell)) = nearest else {
            return cost;
        };
        done[at(cell)] = true;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let next = IVec2::new(cell.x + dx, cell.y + dy);
                if let Some(step) = step_cost(grid, cell, next) {
                    let through = c + step;
                    if cost[at(next)].is_none_or(|old| through < old) {
                        cost[at(next)] = Some(through);
                    }
                }
            }
        }
    }
}

/// The oracle's cost from `from` to `to`.
fn shortest(grid: &TileGrid, from: IVec2, to: IVec2) -> Option<u32> {
    if !open(grid, to) {
        return None;
    }
    oracle(grid, from)[(to.y * grid.width() as i32 + to.x) as usize]
}

/// Checks that `cells` is a path from `from` to `to` that a search may
/// take, and gives its cost.
fn walk(grid: &TileGrid, cells: &[IVec2], from: IVec2, to: IVec2) -> u32 {
    assert_eq!(cells.first(), Some(&from), "{cells:?}");
    assert_eq!(cells.last(), Some(&to), "{cells:?}");
    assert!(open(grid, from));
    cells
        .windows(2)
        .map(|pair| {
            step_cost(grid, pair[0], pair[1])
                .unwrap_or_else(|| panic!("{:?} to {:?} is no step: {cells:?}", pair[0], pair[1]))
        })
        .sum()
}

/// The middle of `cell`.
fn center(cell: IVec2) -> Vec2 {
    (cell.as_vec2() + Vec2::splat(0.5)) * CELL
}

#[test]
fn paths_are_as_short_as_there_are_on_hand_made_grids() {
    // Each grid, and the length of its shortest path, worked out by hand.
    let grids: &[(&[&str], u32)] = &[
        (&["S...G"], 4_000),
        (&["S...", "....", "....", "...G"], 3 * 1_414),
        (&["S..", "..G"], 1_414 + 1_000),
        // Round the end of a wall, whose corners it may not cut: down two,
        // right two, a corner up and right, and up one.
        (
            &[
                "S#.G", //
                ".#..", //
                "....",
            ],
            5 * 1_000 + 1_414,
        ),
        // A corner it may not cut: round by the sides, four steps.
        (&["S..", ".#.", "..G"], 4_000),
        // A maze, where the way is long.
        (
            &[
                "S.#.....", //
                "#.#.###.", //
                "#.#...#.", //
                "#.###.#.", //
                "#.....#G",
            ],
            0,
        ),
    ];
    let mut finder = Pathfinder::new();
    let mut cells = Vec::new();
    for &(rows, by_hand) in grids {
        let (grid, start, goal) = drawn(rows);
        let (start, goal) = (start.unwrap(), goal.unwrap());
        let length = finder
            .find_cells(&grid, start, goal, u32::MAX, &mut cells)
            .unwrap_or_else(|| panic!("no path in {rows:?}"));
        assert_eq!(Some(length), shortest(&grid, start, goal), "{rows:?}");
        if by_hand != 0 {
            assert_eq!(length, by_hand, "{rows:?}");
        }
        assert_eq!(walk(&grid, &cells, start, goal), length, "{rows:?}");
    }
}

#[test]
fn paths_on_random_grids_are_the_oracle_s_length_and_steps_a_search_may_take() {
    let mut rng = Pcg32::new(0x7061_7468, 1);
    let mut finder = Pathfinder::new();
    let mut cells = Vec::new();
    let mut again = Vec::new();
    let (mut found, mut missing) = (0, 0);
    for case in 0..300 {
        let (w, h) = (rng.range(1..25u32), rng.range(1..25u32));
        let mut grid = TileGrid::new(w, h, CELL, Vec2::new(-40.0, 8.0));
        let density = rng.range(0.0..0.45f32);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                if rng.next_f32() < density {
                    grid.set_solid(IVec2::new(x, y), true);
                }
            }
        }
        let pick = |rng: &mut Pcg32| IVec2::new(rng.range(0..w as i32), rng.range(0..h as i32));
        let (start, goal) = (pick(&mut rng), pick(&mut rng));
        let length = finder.find_cells(&grid, start, goal, u32::MAX, &mut cells);
        assert_eq!(length, shortest(&grid, start, goal), "case {case}");
        match length {
            Some(length) => {
                found += 1;
                assert_eq!(walk(&grid, &cells, start, goal), length, "case {case}");
                // The same path every time.
                finder.find_cells(&grid, start, goal, u32::MAX, &mut again);
                assert_eq!(again, cells, "case {case}");
            }
            None => {
                missing += 1;
                assert!(cells.is_empty());
            }
        }
    }
    assert!(found > 150 && missing > 30, "{found} found, {missing} not");
}

#[test]
fn no_path_cuts_a_corner() {
    let mut finder = Pathfinder::new();
    let mut cells = Vec::new();
    // Only a corner joins them.
    let (grid, start, goal) = drawn(&["S#", "#G"]);
    assert_eq!(
        finder.find_cells(&grid, start.unwrap(), goal.unwrap(), u32::MAX, &mut cells),
        None
    );
    // One solid cell beside the corner is enough to forbid it.
    let (grid, start, goal) = drawn(&["S#", ".G"]);
    let length = finder.find_cells(&grid, start.unwrap(), goal.unwrap(), u32::MAX, &mut cells);
    assert_eq!(length, Some(2_000));
    assert_eq!(
        cells,
        [IVec2::new(0, 0), IVec2::new(0, 1), IVec2::new(1, 1)]
    );
}

#[test]
fn a_goal_it_cannot_reach_gives_none_within_the_limit() {
    // A 64 × 64 field with the goal walled in at (60, 60).
    let mut grid = TileGrid::new(64, 64, CELL, Vec2::ZERO);
    for y in 58..=62 {
        for x in 58..=62 {
            if x == 58 || x == 62 || y == 58 || y == 62 {
                grid.set_solid(IVec2::new(x, y), true);
            }
        }
    }
    let (start, goal) = (IVec2::new(2, 3), IVec2::new(60, 60));
    let mut finder = Pathfinder::new();
    let mut cells = vec![IVec2::ZERO];
    // No limit: every cell it can reach, and no more.
    assert_eq!(
        finder.find_cells(&grid, start, goal, u32::MAX, &mut cells),
        None
    );
    assert!(cells.is_empty());
    let reachable = 64 * 64 - 16 - 9;
    assert_eq!(finder.expansions(), reachable);
    // A limit: no more than it.
    assert_eq!(finder.find_cells(&grid, start, goal, 500, &mut cells), None);
    assert_eq!(finder.expansions(), 500);
    // A limit of 0 finds nothing, even where it starts.
    assert_eq!(finder.find_cells(&grid, start, start, 0, &mut cells), None);
    assert_eq!(
        finder.find_cells(&grid, start, start, 1, &mut cells),
        Some(0)
    );
    assert_eq!(cells, [start]);
    // Ends that are solid, or outside the grid, at once.
    for (from, to) in [
        (start, IVec2::new(58, 58)),
        (IVec2::new(62, 58), goal),
        (IVec2::new(-1, 0), start),
        (start, IVec2::new(64, 0)),
    ] {
        assert_eq!(
            finder.find_cells(&grid, from, to, u32::MAX, &mut cells),
            None
        );
        assert_eq!(finder.expansions(), 0);
    }
    // The queue says so too.
    let mut queue = PathQueue::new();
    queue.request(1, PathRequest::new(center(start), center(goal)));
    let mut updates = 0;
    while queue.is_pending(&1) {
        assert!(queue.update(&grid) <= queue.budget());
        updates += 1;
    }
    assert_eq!(queue.result(&1), Some(PathResult::NotFound));
    assert_eq!(updates, reachable.div_ceil(queue.budget()));
}

#[test]
fn the_queue_spends_its_budget_across_requests_oldest_first() {
    let (grid, _, _) = drawn(&[
        "................", //
        ".##############.", //
        "................", //
        "##############..", //
        "................", //
        "..##############", //
        "................",
    ]);
    let requests = [
        PathRequest::new(center(IVec2::new(0, 0)), center(IVec2::new(0, 6))),
        PathRequest::new(center(IVec2::new(3, 2)), center(IVec2::new(9, 2))),
        PathRequest::new(center(IVec2::new(15, 6)), center(IVec2::new(0, 4))),
    ];
    // Alone, each search's path and expansions.
    let mut finder = Pathfinder::new();
    let alone: Vec<(Vec<Vec2>, u32)> = requests
        .iter()
        .map(|&request| {
            let mut points = Vec::new();
            finder
                .find(&grid, request, u32::MAX, &mut points)
                .expect("a way");
            (points, finder.expansions())
        })
        .collect();
    let needed: u32 = alone.iter().map(|(_, expansions)| expansions).sum();

    let mut queue = PathQueue::with_budget(7);
    for (key, &request) in (10u64..).zip(&requests) {
        queue.request(key, request);
    }
    let mut finished = Vec::new();
    let mut spent = 0;
    for update in 0.. {
        let before: Vec<bool> = (10..13).map(|key| queue.is_pending(&key)).collect();
        let now = queue.update(&grid);
        assert!(now <= 7, "update {update} spent {now}");
        spent += now;
        for (key, was) in (10u64..).zip(before) {
            if was && !queue.is_pending(&key) {
                finished.push(key);
            }
        }
        if finished.len() == 3 {
            break;
        }
        assert_eq!(now, 7, "update {update} stopped short with work left");
    }
    assert_eq!(finished, [10, 11, 12], "the oldest first");
    assert_eq!(
        spent, needed,
        "a search spread over updates costs what it costs alone"
    );
    for (key, (points, _)) in (10u64..).zip(&alone) {
        assert_eq!(queue.result(&key), Some(PathResult::Found(points)), "{key}");
    }
    assert_eq!(queue.update(&grid), 0);
    let keys: Vec<u64> = queue.results().map(|(key, _)| key).collect();
    assert_eq!(keys, [10, 11, 12]);
}

#[test]
fn a_new_request_replaces_the_old_one_and_the_old_path_stays_till_it_comes() {
    let (grid, _, _) = drawn(&["..........", "..........", "..........", ".........."]);
    let a = PathRequest::new(center(IVec2::new(0, 0)), center(IVec2::new(9, 3)));
    let b = PathRequest::new(center(IVec2::new(0, 3)), center(IVec2::new(9, 0)));
    let c = PathRequest::new(center(IVec2::new(5, 0)), center(IVec2::new(5, 3)));
    let path = |request| {
        let mut points = Vec::new();
        Pathfinder::new().find(&grid, request, u32::MAX, &mut points);
        points
    };

    // Replaced while it waits: it keeps its place, before 2.
    let mut queue = PathQueue::with_budget(3);
    queue.request(1, a);
    queue.request(2, c);
    queue.request(1, b);
    assert_eq!(queue.len(), 2);
    queue.update(&grid);
    // Replaced while it is searched, by other ends: it goes behind 2, to
    // start again there.
    queue.request(1, a);
    let mut order = Vec::new();
    while queue.is_pending(&1) || queue.is_pending(&2) {
        let was = [queue.is_pending(&1), queue.is_pending(&2)];
        queue.update(&grid);
        for (key, was) in [1, 2].into_iter().zip(was) {
            if was && !queue.is_pending(&key) {
                order.push(key);
            }
        }
    }
    assert_eq!(order, [2, 1]);
    let found = |queue: &PathQueue, key| {
        queue
            .result(&key)
            .and_then(PathResult::points)
            .map(<[Vec2]>::to_vec)
    };
    assert_eq!(found(&queue, 1), Some(path(a)));
    assert_eq!(found(&queue, 2), Some(path(c)));

    // Asked again, the old path stays until the new one comes.
    queue.request(2, b);
    assert!(queue.is_pending(&2));
    assert_eq!(found(&queue, 2), Some(path(c)));
    while queue.is_pending(&2) {
        queue.update(&grid);
    }
    assert_eq!(found(&queue, 2), Some(path(b)));

    // Forgotten.
    assert!(queue.cancel(&1));
    assert!(!queue.cancel(&1));
    assert_eq!(queue.result(&1), None);
    queue.request(3, c);
    queue.retain(|&key| key != 3);
    assert!(!queue.is_pending(&3));
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.update(&grid), 0, "nothing waits");
    // A budget of 0 waits for ever, even for a request that needs no search
    // to fail.
    queue.set_budget(0);
    queue.request(5, PathRequest::new(Vec2::new(-100.0, 8.0), a.to));
    queue.request(4, a);
    assert_eq!(queue.update(&grid), 0);
    assert!(queue.is_pending(&5) && queue.is_pending(&4));
}

#[test]
fn asking_the_same_again_goes_on_with_the_search_and_asking_anew_holds_up_no_one() {
    let corridor = TileGrid::new(40, 2, CELL, Vec2::ZERO);
    let far = PathRequest::new(Vec2::new(8.0, 8.0), Vec2::new(630.0, 8.0));
    // The same request before each update: the search goes on, 40 cells at
    // 4 an update.
    let mut queue = PathQueue::with_budget(4);
    let mut updates = 0;
    queue.request(1, far);
    while queue.is_pending(&1) {
        queue.request(1, far);
        queue.update(&corridor);
        updates += 1;
        assert!(updates <= 10, "the same request started the search again");
    }
    // An agent asking for a goal it cannot reach, from a point a pixel on
    // each time, more often than its search takes: the others still get
    // theirs.
    let mut walled = TileGrid::new(64, 64, CELL, Vec2::ZERO);
    for (x, y) in [
        (39, 39),
        (40, 39),
        (41, 39),
        (39, 40),
        (41, 40),
        (39, 41),
        (40, 41),
        (41, 41),
    ] {
        walled.set_solid(IVec2::new(x, y), true);
    }
    let mut queue = PathQueue::with_budget(256);
    let stuck =
        |step: f32| PathRequest::new(Vec2::new(8.0 + step, 8.0), center(IVec2::new(40, 40)));
    queue.request(1, stuck(0.0));
    queue.request(
        2,
        PathRequest::new(center(IVec2::new(5, 5)), center(IVec2::new(7, 5))),
    );
    for update in 0..40 {
        if update % 5 == 4 {
            queue.request(1, stuck(update as f32));
        }
        queue.update(&walled);
    }
    assert!(matches!(queue.result(&2), Some(PathResult::Found(_))));
}

#[test]
fn a_search_starts_again_when_its_grid_changes() {
    // 40 cells along a corridor a cell wide, 4 an update; after 3 updates
    // a wall goes up across a cell already expanded, behind the search's
    // front: carried on, it would find its way through the wall.
    let mut grid = TileGrid::new(40, 1, CELL, Vec2::ZERO);
    let far = PathRequest::new(Vec2::new(8.0, 8.0), Vec2::new(630.0, 8.0));
    let mut queue = PathQueue::with_budget(4);
    queue.request(1, far);
    for _ in 0..3 {
        queue.update(&grid);
    }
    grid.set_solid(IVec2::new(5, 0), true);
    while queue.is_pending(&1) {
        queue.update(&grid);
    }
    assert_eq!(
        queue.result(&1),
        Some(PathResult::NotFound),
        "no path through the new wall"
    );
    let mut grid = TileGrid::new(40, 2, CELL, Vec2::ZERO);
    grid.set_solid(IVec2::new(30, 0), true);
    grid.set_solid(IVec2::new(30, 1), true);
    // And a grid given in place of another of the same size, the same.
    let open = TileGrid::new(40, 2, CELL, Vec2::ZERO);
    let mut queue = PathQueue::with_budget(4);
    queue.request(1, far);
    queue.update(&grid);
    while queue.is_pending(&1) {
        queue.update(&open);
    }
    assert!(matches!(queue.result(&1), Some(PathResult::Found(_))));
    // A copy of a grid is the same grid.
    let copy = grid.clone();
    let mut queue = PathQueue::with_budget(4);
    let near = PathRequest::new(Vec2::new(8.0, 8.0), Vec2::new(400.0, 8.0));
    queue.request(1, near);
    queue.update(&grid);
    let mut spent = 4;
    while queue.is_pending(&1) {
        spent += queue.update(&copy);
    }
    let mut finder = Pathfinder::new();
    let mut points = Vec::new();
    finder.find(&grid, near, u32::MAX, &mut points);
    assert_eq!(spent, finder.expansions(), "the copy carried the search on");
}

#[test]
fn a_cancel_or_a_freed_slot_gives_no_key_another_s_path() {
    let (grid, _, _) = drawn(&["..........", "..........", "..........", ".........."]);
    let a = PathRequest::new(center(IVec2::new(0, 0)), center(IVec2::new(9, 3)));
    let c = PathRequest::new(center(IVec2::new(5, 0)), center(IVec2::new(5, 3)));
    let path = |request| {
        let mut points = Vec::new();
        Pathfinder::new().find(&grid, request, u32::MAX, &mut points);
        points
    };
    // The search under way cancelled: the next key's own search follows.
    let mut queue = PathQueue::with_budget(3);
    queue.request(1, a);
    queue.request(2, c);
    queue.update(&grid);
    assert!(queue.is_pending(&1));
    assert!(queue.cancel(&1));
    while queue.is_pending(&2) {
        queue.update(&grid);
    }
    assert_eq!(queue.result(&2), Some(PathResult::Found(&path(c))));
    // Two new keys after a cancel each get a slot, and a path, of their own.
    let mut queue = PathQueue::new();
    queue.request(1, a);
    queue.update(&grid);
    assert!(queue.cancel(&1));
    queue.request(5, a);
    queue.request(6, c);
    while queue.is_pending(&5) || queue.is_pending(&6) {
        queue.update(&grid);
    }
    assert_eq!(queue.result(&5), Some(PathResult::Found(&path(a))));
    assert_eq!(queue.result(&6), Some(PathResult::Found(&path(c))));
}

#[test]
fn a_straight_search_expands_only_its_line() {
    // The heuristic is the octile distance, ties go to the cell nearer the
    // goal: in an open grid the search walks straight there.
    let grid = TileGrid::new(20, 20, CELL, Vec2::ZERO);
    let mut finder = Pathfinder::new();
    let mut cells = Vec::new();
    let length = finder.find_cells(
        &grid,
        IVec2::new(0, 10),
        IVec2::new(19, 10),
        u32::MAX,
        &mut cells,
    );
    assert_eq!(length, Some(19_000));
    assert_eq!(finder.expansions(), 20);
    let length = finder.find_cells(
        &grid,
        IVec2::new(0, 0),
        IVec2::new(19, 19),
        u32::MAX,
        &mut cells,
    );
    assert_eq!(length, Some(19 * 1_414));
    assert_eq!(finder.expansions(), 20);
}

#[test]
fn an_end_in_a_solid_cell_takes_the_open_neighbour_nearest_it_and_the_first_of_two() {
    let mut field = FlowField::new();
    // Of the four beside, as near: right comes first.
    let (grid, _, _) = drawn(&["...", ".#.", "..."]);
    assert!(field.compute(&grid, Vec2::new(24.0, 24.0)));
    assert_eq!(field.goal(), Some(IVec2::new(2, 1)));
    // By each cell's nearest point, not its middle: (2, 0)'s, its corner
    // (32, 16), is 9.4 px from (27, 24), and (0, 1)'s 11 px; by their
    // middles, (0, 1) would be the nearer.
    let (grid, _, _) = drawn(&[".#.", ".##", ".##"]);
    assert!(field.compute(&grid, Vec2::new(27.0, 24.0)));
    assert_eq!(field.goal(), Some(IVec2::new(2, 0)));
}

#[test]
fn ends_in_solid_cells_start_from_the_nearest_open_one() {
    let (grid, _, _) = drawn(&["....", ".##.", "...."]);
    let mut finder = Pathfinder::new();
    let mut points = Vec::new();
    // From inside the wall's left cell, near its top: from the open cell
    // above it.
    let from = Vec2::new(20.0, 17.0);
    let to = Vec2::new(56.0, 40.0);
    assert!(
        finder
            .find(&grid, PathRequest::new(from, to), u32::MAX, &mut points)
            .is_some()
    );
    assert_eq!(points.first(), Some(&from));
    assert_eq!(points.last(), Some(&to));
    // To a solid cell: to the middle of the open one nearest the point.
    let into = Vec2::new(40.0, 30.0);
    finder.find(
        &grid,
        PathRequest::new(Vec2::new(8.0, 8.0), into),
        u32::MAX,
        &mut points,
    );
    assert_eq!(points.last(), Some(&center(IVec2::new(2, 2))));
    // Nowhere near the grid, or not a number: nothing.
    for (from, to) in [
        (Vec2::new(-100.0, 8.0), to),
        (from, Vec2::new(8.0, 500.0)),
        (Vec2::new(f32::NAN, 8.0), to),
        (from, Vec2::new(8.0, f32::INFINITY)),
    ] {
        assert_eq!(
            finder.find(&grid, PathRequest::new(from, to), u32::MAX, &mut points),
            None
        );
        assert!(points.is_empty());
    }
}

#[test]
fn smoothing_keeps_only_the_corners_and_each_leg_is_clear() {
    let mut finder = Pathfinder::new();
    let mut points = Vec::new();
    // An open room: straight there.
    let (room, _, _) = drawn(&["........", "........", "........"]);
    let request = PathRequest::new(Vec2::new(5.0, 6.0), Vec2::new(120.0, 41.0));
    finder.find(&room, request, u32::MAX, &mut points);
    assert_eq!(points, [request.from, request.to]);

    // Round the end of a wall, for a box of 10 × 6 px.
    let (grid, _, _) = drawn(&[
        "..........", //
        "..........", //
        "#######...", //
        "..........", //
        "..........",
    ]);
    let half = Vec2::new(5.0, 3.0);
    let request =
        PathRequest::new(Vec2::new(10.0, 10.0), Vec2::new(10.0, 70.0)).with_clearance(half);
    let length = finder
        .find(&grid, request, u32::MAX, &mut points)
        .expect("a way round");
    assert_eq!(points.first(), Some(&request.from));
    assert_eq!(points.last(), Some(&request.to));
    // Fewer points than cells, every leg in sight for the box, and every
    // corner the middle of a cell.
    // Its ends are 2 px each from their cells' middles, which the cells'
    // length is counted from.
    let walked: f32 = points.windows(2).map(|leg| leg[0].distance(leg[1])).sum();
    assert!(walked <= length as f32 / 1_000.0 * CELL + 4.0, "{walked}");
    assert!(points.len() >= 3 && points.len() <= 5, "{points:?}");
    for leg in points.windows(2) {
        assert!(line_of_sight(&grid, leg[0], leg[1], half), "{leg:?}");
    }
    for &corner in &points[1..points.len() - 1] {
        assert_eq!(corner, center(grid.cell_at(corner)), "{corner:?}");
    }

    // In a corridor a cell wide, a box as wide sees nowhere: it keeps
    // every cell; a narrower one goes straight.
    let (corridor, _, _) = drawn(&["#######", ".......", "#######"]);
    let from = Vec2::new(8.0, 24.0);
    let to = Vec2::new(104.0, 24.0);
    finder.find(
        &corridor,
        PathRequest::new(from, to).with_clearance(Vec2::splat(8.0)),
        u32::MAX,
        &mut points,
    );
    assert_eq!(points.len(), 2 + 5, "{points:?}");
    finder.find(
        &corridor,
        PathRequest::new(from, to).with_clearance(half),
        u32::MAX,
        &mut points,
    );
    assert_eq!(points, [from, to]);
}

#[test]
fn the_line_of_sight_is_stopped_by_seams_corners_and_what_the_box_touches() {
    let (grid, _, _) = drawn(&[
        "......", //
        "..#...", //
        "..#...", //
        "......", //
        "######",
    ]);
    let line = Vec2::ZERO;
    // Through column 2's solid cells, and along the seam between them:
    // stopped.
    assert!(!line_of_sight(
        &grid,
        Vec2::new(40.0, 0.0),
        Vec2::new(40.0, 60.0),
        line
    ));
    assert!(!line_of_sight(
        &grid,
        Vec2::new(0.0, 32.0),
        Vec2::new(90.0, 32.0),
        line
    ));
    // Along a face, within EPSILON of it: stopped; further off: seen.
    let face = 64.0;
    assert!(!line_of_sight(
        &grid,
        Vec2::new(0.0, face),
        Vec2::new(90.0, face),
        line
    ));
    assert!(!line_of_sight(
        &grid,
        Vec2::new(0.0, face - 0.005),
        Vec2::new(90.0, face - 0.005),
        line
    ));
    assert!(line_of_sight(
        &grid,
        Vec2::new(0.0, face - 0.1),
        Vec2::new(90.0, face - 0.1),
        line
    ));
    // Past a corner where two cells meet: stopped.
    let (corner, _, _) = drawn(&[".#", "#."]);
    assert!(!line_of_sight(
        &corner,
        Vec2::new(8.0, 8.0),
        Vec2::new(24.0, 24.0),
        line
    ));
    // A box: 5 px off the wall is clear for a half size of 4, not of 6.
    let beside = Vec2::new(0.0, face - 5.0);
    let far = Vec2::new(90.0, face - 5.0);
    assert!(line_of_sight(&grid, beside, far, Vec2::new(30.0, 4.0)));
    assert!(!line_of_sight(&grid, beside, far, Vec2::new(4.0, 6.0)));
    // A diagonal that clears the column's top-left corner, (32, 16), as a
    // line, but not as a box of half size 4, whose corner touches it.
    let (from, to) = (Vec2::new(26.0, 14.0), Vec2::new(34.0, 6.0));
    assert!(line_of_sight(&grid, from, to, line));
    assert!(!line_of_sight(&grid, from, to, Vec2::splat(4.0)));
    // Off the grid, nothing stops it; not a number sees nothing; a
    // clearance below 0 or not a number is 0.
    assert!(line_of_sight(
        &grid,
        Vec2::new(-50.0, -50.0),
        Vec2::new(500.0, -50.0),
        line
    ));
    assert!(!line_of_sight(
        &grid,
        Vec2::new(f32::NAN, 0.0),
        Vec2::ZERO,
        line
    ));
    assert!(line_of_sight(
        &grid,
        Vec2::new(0.0, 8.0),
        Vec2::new(20.0, 8.0),
        Vec2::splat(-3.0)
    ));
    assert!(line_of_sight(
        &grid,
        Vec2::new(0.0, 8.0),
        Vec2::new(20.0, 8.0),
        Vec2::splat(f32::NAN)
    ));
    // A line that ends a pixel into column 2's solid cell: as a line, it
    // is stopped, which a clearance taken off the box would miss.
    for clearance in [Vec2::splat(-3.0), Vec2::splat(f32::NAN)] {
        assert!(!line_of_sight(
            &grid,
            Vec2::new(0.0, 24.0),
            Vec2::new(33.0, 24.0),
            clearance
        ));
    }
    // Along the bottom face of column 2's cells, within EPSILON of it:
    // stopped; further off: seen.
    for (y, seen) in [(48.0, false), (48.005, false), (48.1, true)] {
        assert_eq!(
            line_of_sight(&grid, Vec2::new(0.0, y), Vec2::new(90.0, y), line),
            seen,
            "{y}"
        );
    }
    // A line stops where it ends: 2 px short of a solid cell, it is seen.
    let (short, _, _) = drawn(&["...", "...", ".#."]);
    assert!(line_of_sight(
        &short,
        Vec2::new(8.0, 8.0),
        Vec2::new(24.0, 30.0),
        line
    ));
    // A huge one meets the grid's solid cells, and stops.
    assert!(!line_of_sight(
        &grid,
        Vec2::new(0.0, -1e30),
        Vec2::new(0.0, 1e30),
        line
    ));
}

#[test]
fn the_line_of_sight_is_the_same_both_ways_however_far_its_ends() {
    // (5, 5) solid, on the line y = x.
    let mut grid = TileGrid::new(16, 16, CELL, Vec2::ZERO);
    grid.set_solid(IVec2::new(5, 5), true);
    let near = Vec2::splat(8.0);
    for far in [1e9_f32, 1e10, 1e20, 1e30, 3e38] {
        let (out, back) = (Vec2::splat(far), Vec2::splat(-far));
        for (a, b) in [(near, out), (out, near), (back, out), (out, back)] {
            assert!(!line_of_sight(&grid, a, b, Vec2::ZERO), "{a:?} to {b:?}");
        }
        // Far off to one side of the grid, nothing.
        let (left, right) = (Vec2::new(-far, -far), Vec2::new(-far, far));
        assert!(line_of_sight(&grid, left, right, Vec2::ZERO), "{far}");
    }
    // Near the grid, the order of the ends changes nothing, on random
    // lines through random cells.
    let mut rng = Pcg32::new(0x6c6f_7321, 3);
    let mut walls = TileGrid::new(12, 12, CELL, Vec2::new(-30.0, 20.0));
    for _ in 0..30 {
        walls.set_solid(IVec2::new(rng.range(0..12), rng.range(0..12)), true);
    }
    for _ in 0..2_000 {
        let point =
            |rng: &mut Pcg32| Vec2::new(rng.range(-60.0..200.0f32), rng.range(0.0..240.0f32));
        let (a, b) = (point(&mut rng), point(&mut rng));
        let half = Vec2::new(rng.range(0.0..6.0f32), rng.range(0.0..6.0f32));
        assert_eq!(
            line_of_sight(&walls, a, b, half),
            line_of_sight(&walls, b, a, half)
        );
    }
}

#[test]
fn smoothing_falls_back_on_the_cells_own_steps_at_either_end_and_keeps_its_legs_short() {
    let mut finder = Pathfinder::new();
    let mut points = Vec::new();
    let half = Vec2::new(5.0, 3.0);
    // (0, 2) solid; from just above its top face, clear of it, a box that
    // cannot see the second cell's middle past its corner: through the
    // first cell's middle instead, and the same at the goal's end.
    let (grid, _, _) = drawn(&[".....", ".....", "#....", ".....", "....."]);
    let from = Vec2::new(16.5, 28.9);
    let to = Vec2::new(72.0, 72.0);
    for (from, to) in [(from, to), (to, from)] {
        finder.find(
            &grid,
            PathRequest::new(from, to).with_clearance(half),
            u32::MAX,
            &mut points,
        );
        assert_eq!((points.first(), points.last()), (Some(&from), Some(&to)));
        for leg in points.windows(2) {
            assert!(
                line_of_sight(&grid, leg[0], leg[1], half),
                "{leg:?} of {points:?}"
            );
        }
    }
    // From inside a solid cell, first to the middle of the open one nearest
    // it, then on in sight.
    let (grid, _, _) = drawn(&[".....", ".##..", ".#...", "....."]);
    let request = PathRequest::new(Vec2::new(28.0, 28.0), Vec2::new(72.0, 8.0));
    finder.find(&grid, request, u32::MAX, &mut points);
    assert_eq!(points[1], center(IVec2::new(2, 2)), "{points:?}");
    for leg in points[1..].windows(2) {
        assert!(line_of_sight(&grid, leg[0], leg[1], Vec2::ZERO), "{leg:?}");
    }
    // Along an open run of 40 cells, a point every 16 cells at most.
    let run = TileGrid::new(40, 1, CELL, Vec2::ZERO);
    finder.find(
        &run,
        PathRequest::new(Vec2::new(8.0, 8.0), Vec2::new(632.0, 8.0)),
        u32::MAX,
        &mut points,
    );
    let xs: Vec<f32> = points.iter().map(|point| point.x).collect();
    assert_eq!(xs, [8.0, 264.0, 520.0, 632.0]);
}

#[test]
fn flow_fields_are_the_oracle_s_costs_and_lead_to_the_goal() {
    let mut rng = Pcg32::new(0x666c_6f77, 2);
    let mut field = FlowField::new();
    for case in 0..120 {
        let (w, h) = (rng.range(1..20u32), rng.range(1..20u32));
        let mut grid = TileGrid::new(w, h, CELL, Vec2::new(16.0, -32.0));
        let density = rng.range(0.0..0.4f32);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                if rng.next_f32() < density {
                    grid.set_solid(IVec2::new(x, y), true);
                }
            }
        }
        let goal = IVec2::new(rng.range(0..w as i32), rng.range(0..h as i32));
        let at = grid.cell_rect(goal).center();
        if !open(&grid, goal) {
            continue;
        }
        assert!(field.compute(&grid, at), "case {case}");
        assert_eq!(field.goal(), Some(goal));
        let costs = oracle(&grid, goal);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let cell = IVec2::new(x, y);
                let expected = costs[(y * w as i32 + x) as usize];
                assert_eq!(field.cost(cell), expected, "case {case}, {cell:?}");
                let direction = field.direction(cell);
                match (expected, field.next_cell(cell)) {
                    (Some(0), next) => {
                        assert_eq!(next, None);
                        assert_eq!(direction, Vec2::ZERO);
                    }
                    (Some(cost), Some(next)) => {
                        // Down the field by the step's own cost: a shortest
                        // way, one step at a time.
                        let step = step_cost(&grid, cell, next).expect("a step");
                        assert_eq!(field.cost(next), Some(cost - step), "case {case}");
                        let offset = (next - cell).as_vec2();
                        assert!((direction - offset.normalize_or_zero()).length() < 1e-6);
                        assert_eq!(field.direction_at(grid.cell_rect(cell).center()), direction);
                    }
                    (None, next) => {
                        assert_eq!(next, None);
                        assert_eq!(direction, Vec2::ZERO);
                    }
                    (Some(_), None) => panic!("case {case}: {cell:?} has a cost and no way"),
                }
            }
        }
    }
    // Off the field, and points that are not numbers: no way.
    assert_eq!(field.direction(IVec2::new(-1, 0)), Vec2::ZERO);
    assert_eq!(field.direction_at(Vec2::new(f32::NAN, 0.0)), Vec2::ZERO);
    assert_eq!(FlowField::new().direction_at(Vec2::ZERO), Vec2::ZERO);
}

#[test]
fn a_flow_field_follows_its_goal_at_most_once_every_ten_ticks() {
    let (grid, _, _) = drawn(&["......", "..##..", "......"]);
    let cell = |x: i32, y: i32| grid.cell_rect(IVec2::new(x, y)).center();
    let mut field = FlowField::new();
    assert!(
        field.update(&grid, cell(0, 0), 100),
        "the first update computes"
    );
    assert_eq!(field.goal(), Some(IVec2::new(0, 0)));
    // Within the same cell: nothing to do.
    assert!(!field.update(&grid, cell(0, 0) + Vec2::splat(5.0), 140));
    // To another cell, too soon after the last: it waits.
    assert!(field.update(&grid, cell(5, 2), 140));
    assert!(!field.update(&grid, cell(3, 0), 149));
    assert_eq!(field.goal(), Some(IVec2::new(5, 2)));
    assert!(field.update(&grid, cell(3, 0), 150));
    assert_eq!(field.goal(), Some(IVec2::new(3, 0)));
    // A grid of another size, at once.
    let (bigger, _, _) = drawn(&["......", "......", "......", "......"]);
    assert!(field.update(&bigger, cell(3, 0), 151));
    // A grid moved elsewhere, or with a cell set since, at once too.
    let mut moved = TileGrid::new(6, 4, CELL, Vec2::new(32.0, 0.0));
    assert!(field.update(&moved, Vec2::new(40.0, 8.0), 152));
    assert!(!field.update(&moved, Vec2::new(40.0, 8.0), 153));
    moved.set_solid(IVec2::new(3, 3), true);
    assert!(field.update(&moved, Vec2::new(40.0, 8.0), 153));
    assert_eq!(field.next_cell(IVec2::new(3, 3)), None);
    // Off the field's left or top edge by a pixel: no way, though a field
    // rounding toward zero would put it in cell (0, 0), which has one.
    assert!(field.compute(&moved, Vec2::new(120.0, 40.0)));
    assert_ne!(field.direction(IVec2::new(0, 0)), Vec2::ZERO);
    assert_eq!(field.direction_at(Vec2::new(31.0, 8.0)), Vec2::ZERO);
    assert_eq!(field.direction_at(Vec2::new(40.0, -1.0)), Vec2::ZERO);
    // A goal inside a wall: the open cell beside it nearest; none, and the
    // field leads nowhere.
    assert!(field.compute(&grid, Vec2::new(33.0, 20.0)));
    assert_eq!(field.goal(), Some(IVec2::new(1, 1)));
    let (walled, _, _) = drawn(&["###", "###", "###"]);
    assert!(!field.compute(&walled, Vec2::new(24.0, 24.0)));
    assert_eq!(field.goal(), None);
    assert_eq!(field.direction(IVec2::new(0, 0)), Vec2::ZERO);
}
