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

//! [`Navigation`]: paths for entities, which the engine finds each tick.

use nadir_collision::{PathQueue, PathRequest, PathResult, TileGrid};
use nadir_core::time::Tick;
use nadir_ecs::{Entity, World};

use crate::Collisions;

/// Paths for entities across the tile grid of [`Collisions`], or a grid of
/// its own: a resource whose requests the engine's system
/// `nadir::navigation` searches each tick, in the stage `PostMovement`
/// after `nadir::triggers`, expanding at most its
/// [budget](PathQueue::budget) of cells (roadmap M9.5, ADR-0069).
///
/// - A game's AI, in `Gameplay`, asks with [`request`](Self::request): a
///   path from where the entity is (its collider's center, say) to where it
///   goes, smoothed for its size. The tick's searches find it if the
///   budget lasts, the oldest request first; one not finished goes on in
///   the next tick.
/// - From the next tick's `Gameplay` on, once found, [`path`](Self::path)
///   gives it: the points to walk through, where the entity was first. A
///   newer request leaves the last path there until it is found.
/// - The paths of entities despawned are forgotten.
///
/// Its queue holds the budget and the limit on one search
/// ([`queue_mut`](Self::queue_mut)), and F4's debug drawing draws its paths.
/// Paths see a grid's tiles only, not colliders: an entity walking one
/// slides along a collider it meets at an angle, and stops at one it meets
/// head-on, such as a tree's trunk across its path. A game whose props
/// stand in the way gives it a grid of its own with their cells solid
/// ([`set_grid`](Self::set_grid)).
pub struct Navigation {
    queue: PathQueue<Entity>,
    /// The grid paths cross, in place of the one of `Collisions`.
    grid: Option<TileGrid>,
}

impl Navigation {
    /// No requests, with the queue's [default
    /// budget](PathQueue::DEFAULT_BUDGET) and limit.
    pub fn new() -> Self {
        Self {
            queue: PathQueue::new(),
            grid: None,
        }
    }

    /// No requests, searching at most `budget` expansions a tick. Insert it
    /// as a resource to replace the engine's.
    pub fn with_budget(budget: u32) -> Self {
        Self {
            queue: PathQueue::with_budget(budget),
            grid: None,
        }
    }

    /// Searches `grid` from the next tick on, in place of the grid of
    /// [`Collisions`]: the level's tiles with the cells of its props set
    /// solid, say. `None` goes back to the grid of `Collisions`. A search
    /// under way starts again on it.
    pub fn set_grid(&mut self, grid: Option<TileGrid>) {
        self.grid = grid;
    }

    /// The grid of its own that it searches, if it has one.
    pub fn grid(&self) -> Option<&TileGrid> {
        self.grid.as_ref()
    }

    /// Asks for a path for `entity`, in place of its earlier request
    /// ([`PathQueue::request`]): ask again when the last path has come, or
    /// every so many ticks, not every tick.
    pub fn request(&mut self, entity: Entity, request: PathRequest) {
        self.queue.request(entity, request);
    }

    /// Forgets `entity`'s request and path. Whether it had either.
    pub fn cancel(&mut self, entity: Entity) -> bool {
        self.queue.cancel(&entity)
    }

    /// Whether a request for `entity` waits for its path.
    pub fn is_pending(&self, entity: Entity) -> bool {
        self.queue.is_pending(&entity)
    }

    /// `entity`'s last path, found or not; `None` before the first comes.
    pub fn path(&self, entity: Entity) -> Option<PathResult<'_>> {
        self.queue.result(&entity)
    }

    /// Each entity with a path, and its path.
    pub fn paths(&self) -> impl Iterator<Item = (Entity, PathResult<'_>)> {
        self.queue.results()
    }

    /// The queue, for its budget and limit.
    pub fn queue(&self) -> &PathQueue<Entity> {
        &self.queue
    }

    /// The queue, to set its budget and limit.
    pub fn queue_mut(&mut self) -> &mut PathQueue<Entity> {
        &mut self.queue
    }
}

impl Default for Navigation {
    fn default() -> Self {
        Self::new()
    }
}

/// `nadir::navigation`, in the tick stage `PostMovement` after
/// `nadir::triggers`: forgets the paths of entities despawned, then
/// searches the waiting requests across the navigation's own grid, or the
/// grid of [`Collisions`].
pub(crate) fn navigation(world: &World, _tick: &Tick) {
    let mut navigation = world.resource_mut::<Navigation>();
    let Navigation { queue, grid } = &mut *navigation;
    queue.retain(|&entity| world.is_alive(entity));
    let collisions = world.resource::<Collisions>();
    queue.update(grid.as_ref().unwrap_or(collisions.grid()));
}

#[cfg(test)]
mod tests {
    use nadir_collision::{PathResult, TileGrid};
    use nadir_core::{IVec2, Vec2};

    use super::*;
    use crate::{App, Config, Transform};

    const TICK: std::time::Duration = std::time::Duration::from_nanos(16_666_667);

    #[test]
    fn a_path_asked_for_in_a_tick_comes_by_the_next() {
        let mut app = App::new(Config::default());
        let mut grid = TileGrid::new(12, 8, 16.0, Vec2::ZERO);
        for y in 0..6 {
            grid.set_solid(IVec2::new(6, y), true);
        }
        let world = app.world_mut();
        world.resource_mut::<Collisions>().set_grid(grid);
        let walker = world
            .spawn()
            .with(Transform::at(Vec2::new(24.0, 24.0)))
            .id();
        let gone = world.spawn().id();
        let request = PathRequest::new(Vec2::new(24.0, 24.0), Vec2::new(168.0, 24.0));
        {
            let mut navigation = world.resource_mut::<Navigation>();
            navigation.request(walker, request);
            navigation.request(gone, request);
            assert!(navigation.is_pending(walker));
        }
        world.despawn(gone);
        app.update(TICK, &[]);
        let navigation = app.world().resource::<Navigation>();
        assert!(!navigation.is_pending(walker));
        let Some(PathResult::Found(points)) = navigation.path(walker) else {
            panic!("a way round the wall's end");
        };
        assert_eq!(points.first(), Some(&request.from));
        assert_eq!(points.last(), Some(&request.to));
        assert!(points.iter().any(|point| point.y > 96.0), "{points:?}");
        // The despawned one's request was forgotten.
        assert_eq!(navigation.path(gone), None);
        assert_eq!(navigation.paths().count(), 1);
    }

    #[test]
    fn a_despawned_entity_s_request_spends_none_of_the_budget() {
        // 40 open cells to expand, and a budget of 40: one search a tick.
        let mut app = App::new(Config::default()).insert_resource(Navigation::with_budget(40));
        let world = app.world_mut();
        world
            .resource_mut::<Collisions>()
            .set_grid(TileGrid::new(40, 2, 16.0, Vec2::ZERO));
        let gone = world.spawn().id();
        let walker = world.spawn().id();
        let request = PathRequest::new(Vec2::new(8.0, 8.0), Vec2::new(630.0, 8.0));
        {
            let mut navigation = world.resource_mut::<Navigation>();
            navigation.request(gone, request);
            navigation.request(walker, request);
        }
        world.despawn(gone);
        app.update(TICK, &[]);
        let navigation = app.world().resource::<Navigation>();
        assert!(!navigation.is_pending(walker));
        assert_eq!(navigation.queue().len(), 1);
    }

    #[test]
    fn a_grid_of_its_own_is_searched_in_place_of_the_collisions_one() {
        let mut app = App::new(Config::default());
        let world = app.world_mut();
        world
            .resource_mut::<Collisions>()
            .set_grid(TileGrid::new(12, 1, 16.0, Vec2::ZERO));
        // Its own: a prop in cell 6, walled.
        let mut own = TileGrid::new(12, 1, 16.0, Vec2::ZERO);
        own.set_solid(IVec2::new(6, 0), true);
        let walker = world.spawn().id();
        let request = PathRequest::new(Vec2::new(8.0, 8.0), Vec2::new(180.0, 8.0));
        {
            let mut navigation = world.resource_mut::<Navigation>();
            navigation.set_grid(Some(own));
            navigation.request(walker, request);
        }
        app.update(TICK, &[]);
        assert_eq!(
            app.world().resource::<Navigation>().path(walker),
            Some(PathResult::NotFound)
        );
        // Back to the grid of `Collisions`: a way.
        {
            let mut navigation = app.world().resource_mut::<Navigation>();
            navigation.set_grid(None);
            assert!(navigation.grid().is_none());
            navigation.request(walker, request.with_clearance(Vec2::splat(1.0)));
        }
        app.update(TICK, &[]);
        let navigation = app.world().resource::<Navigation>();
        assert!(matches!(
            navigation.path(walker),
            Some(PathResult::Found(_))
        ));
    }

    #[test]
    fn the_budget_spreads_a_search_over_ticks() {
        let mut app = App::new(Config::default()).insert_resource(Navigation::with_budget(4));
        let world = app.world_mut();
        world
            .resource_mut::<Collisions>()
            .set_grid(TileGrid::new(40, 2, 16.0, Vec2::ZERO));
        let walker = world.spawn().id();
        world.resource_mut::<Navigation>().request(
            walker,
            PathRequest::new(Vec2::new(8.0, 8.0), Vec2::new(630.0, 8.0)),
        );
        // 40 cells to expand, 4 a tick.
        for _ in 0..9 {
            app.update(TICK, &[]);
            assert!(app.world().resource::<Navigation>().is_pending(walker));
        }
        app.update(TICK, &[]);
        assert!(!app.world().resource::<Navigation>().is_pending(walker));
    }
}
