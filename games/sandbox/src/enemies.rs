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

//! The field's enemies (roadmap M9.5, ADR-0069), people tinted as the
//! person is not:
//!
//! - Three red guards walk their rounds. One that sees the person, within
//!   [`SIGHT`] and past no wall, tree or rock, chases them, and gives up
//!   [`FORGET_TICKS`] after it last saw them, walking back to its round.
//!   They walk the paths that the engine's [`Navigation`] finds for them.
//! - A violet pack waits in its den in the south-west. While the person
//!   is in the south, it hunts them down a [`FlowField`] toward them, which
//!   follows them a cell at a time; while they are not, it goes home down
//!   another, toward the den, each to its own spot there. On the way home
//!   its members walk through the people, each other included, so that the
//!   first home do not bar the den to the rest.
//!
//! They stop beside the person, and do them no harm: there is no health
//! yet. Their colliders are on the people's layer, and stop at the walls,
//! the trees, the rocks, each other and the person; the person's stop at
//! the world's only, so the person walks through them.
//!
//! Paths and flow fields see a grid's tiles only, and the field's trees
//! and rocks are colliders on open cells: an enemy that met one head-on
//! would stop there. So the enemies' paths and fields cross a grid of
//! their own, the level's with the cells of its trees and rocks solid
//! ([`NavGrid`]), which `Navigation` searches in place of the collision
//! grid, and which follows the level when it changes.

use nadir::collision::{ColliderKind, FlowField, line_of_sight};
use nadir::ecs::{EntityCommands, SparseSet};
use nadir::prelude::*;

use crate::{Gait, at_feet, map};

/// How far a guard sees, in pixels.
pub(crate) const SIGHT: f32 = 112.0;

/// How long a guard chases the person after it last saw them: 3 seconds.
pub(crate) const FORGET_TICKS: u64 = 180;

/// How often a chasing guard asks for a new path: twice a second.
const CHASE_PATH_TICKS: u64 = 30;

/// How long an enemy may walk without getting anywhere before it asks
/// again, in ticks.
const STUCK_TICKS: u64 = 40;

/// A guard's speed on its round, and chasing: slower than the person's
/// 120, so they get away.
const WALK: f32 = 60.0;
const RUN: f32 = 96.0;

/// The pack's speed.
const PACK_RUN: f32 = 84.0;

/// How near an enemy comes to the person, center to center.
pub(crate) const REACH: f32 = 14.0;

/// How near a point of its path counts as reached.
const ARRIVE: f32 = 2.0;

/// Half an enemy's collider, the person's size: the box its paths are
/// smoothed for.
const HALF: Vec2 = Vec2::new(5.0, 3.0);

/// The guards' tint.
pub(crate) const GUARD_TINT: Color = Color::rgb(255, 110, 100);

/// The pack's tint.
pub(crate) const PACK_TINT: Color = Color::rgb(170, 130, 255);

/// Each guard's round: the cells it walks between, in turn.
const ROUNDS: [&[IVec2]; 3] = [
    // Along the path of row 12.
    &[IVec2::new(8, 12), IVec2::new(44, 12)],
    // Round the meadow east of the path, above the flowers.
    &[
        IVec2::new(53, 16),
        IVec2::new(61, 16),
        IVec2::new(61, 22),
        IVec2::new(53, 22),
    ],
    // Round the grass west of the tall wall of column 40.
    &[
        IVec2::new(36, 23),
        IVec2::new(36, 32),
        IVec2::new(23, 32),
        IVec2::new(23, 23),
    ],
];

/// The pack's den, in the south-west, and where each of its five waits.
pub(crate) const DEN: IVec2 = IVec2::new(8, 44);
const PACK_SPOTS: [IVec2; 5] = [
    IVec2::new(6, 44),
    IVec2::new(8, 44),
    IVec2::new(10, 44),
    IVec2::new(7, 45),
    IVec2::new(9, 45),
];

/// How many enemies the field has: the guards and the pack.
#[cfg(test)]
pub(crate) const COUNT: usize = ROUNDS.len() + PACK_SPOTS.len();

/// What a guard is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Walking its round.
    Round,
    /// Chasing the person.
    Chase,
}

/// A guard: its round, and how it is getting on.
#[derive(Clone, Debug)]
pub(crate) struct Guard {
    round: &'static [IVec2],
    /// The point of its round it walks to next.
    next: usize,
    pub(crate) mode: Mode,
    /// The tick it last saw the person.
    seen: u64,
    /// When it asks for its next path, chasing.
    ask_at: u64,
    walker: Walker,
}

impl Component for Guard {}

/// One of the pack, and where it waits.
#[derive(Clone, Debug)]
pub(crate) struct PackMember {
    spot: Vec2,
}

impl Component for PackMember {}

/// The grid the enemies' paths and fields cross: the collision grid, with
/// solid the cells under the colliders of the level's trees and rocks,
/// solid on the world's layer and with no velocity.
pub(crate) struct NavGrid {
    /// The collision grid it was made from.
    base: TileGrid,
    /// The cells it set solid, which tell that a level's props moved.
    props: Vec<IVec2>,
    /// The props' cells of this tick, kept to compare with.
    now: Vec<IVec2>,
    pub(crate) grid: TileGrid,
}

/// The cells under the solid, unmoving colliders of `world` on the
/// world's layer, into `out`, sorted: the trees' and rocks'.
fn prop_cells(world: &World, grid: &TileGrid, out: &mut Vec<IVec2>) {
    out.clear();
    let transforms = world.read::<Transform>();
    let velocities = world.read::<Velocity>();
    for (entity, collider) in world.read::<Collider>().iter() {
        let collider = collider.0;
        if collider.kind != ColliderKind::Solid
            || !collider.layer.intersects(map::WORLD)
            || velocities.contains(entity)
        {
            continue;
        }
        let Some(transform) = transforms.get(entity) else {
            continue;
        };
        // The cells its box overlaps, not those it only touches.
        let bounds = collider.bounds(transform.position);
        let first = grid.cell_at(bounds.min);
        let last = grid.cell_at(bounds.max - Vec2::splat(1e-3));
        for y in first.y..=last.y {
            for x in first.x..=last.x {
                out.push(IVec2::new(x, y));
            }
        }
    }
    out.sort_by_key(|cell| (cell.y, cell.x));
    out.dedup();
}

/// `base` with `props`' cells, those inside it, solid.
fn with_props(base: &TileGrid, props: &[IVec2]) -> TileGrid {
    let mut grid = base.clone();
    for &cell in props {
        let inside = u32::try_from(cell.x).is_ok_and(|x| x < grid.width())
            && u32::try_from(cell.y).is_ok_and(|y| y < grid.height());
        if inside {
            grid.set_solid(cell, true);
        }
    }
    grid
}

impl NavGrid {
    /// The grid for `world`'s collision grid and props now.
    fn new(world: &World) -> Self {
        let base = world.resource::<Collisions>().grid().clone();
        let mut props = Vec::new();
        prop_cells(world, &base, &mut props);
        let grid = with_props(&base, &props);
        Self {
            base,
            props,
            now: Vec::new(),
            grid,
        }
    }
}

/// Makes the enemies' grid again when the collision grid or the props'
/// cells have changed since it was made, as when the hot reload makes the
/// level again, and gives it to `Navigation`. First of the enemies' in
/// `Gameplay`; a steady tick allocates nothing.
pub(crate) fn nav_grid(world: &World, _tick: &Tick) {
    let collisions = world.resource::<Collisions>();
    let mut nav = world.resource_mut::<NavGrid>();
    let NavGrid {
        base,
        props,
        now,
        grid,
    } = &mut *nav;
    prop_cells(world, collisions.grid(), now);
    if *collisions.grid() == *base && *now == *props {
        return;
    }
    *base = collisions.grid().clone();
    std::mem::swap(props, now);
    *grid = with_props(base, props);
    world
        .resource_mut::<Navigation>()
        .set_grid(Some(grid.clone()));
}

/// The pack's two flow fields.
pub(crate) struct Pack {
    /// Toward the person, while they are in the south.
    pub(crate) hunt: FlowField,
    /// Toward the den.
    home: FlowField,
}

/// How an enemy follows a path: the point it walks to, and whether it is
/// getting anywhere.
#[derive(Clone, Debug, Default)]
struct Walker {
    /// The first point of the path it follows, which tells it from the
    /// next path.
    path_from: Option<Vec2>,
    /// The index of the point it walks to.
    leg: usize,
    /// Where it was when it last got anywhere, and when.
    progress: (Vec2, u64),
}

impl Walker {
    /// Whether, `moving` since the last tick, it has walked less than a
    /// pixel in [`STUCK_TICKS`]; and notes where it is. Once it says so, it
    /// counts again from there: at most once every [`STUCK_TICKS`].
    fn stuck(&mut self, at: Vec2, tick: u64, moving: bool) -> bool {
        if !moving || at.distance(self.progress.0) > 1.0 {
            self.progress = (at, tick);
        }
        let stuck = tick.saturating_sub(self.progress.1) >= STUCK_TICKS;
        if stuck {
            self.progress = (at, tick);
        }
        stuck
    }

    /// The point of `points`, a path from `Navigation`, that a walker at
    /// `at` heads for next; `None` once it has reached the last.
    fn heading(&mut self, points: &[Vec2], at: Vec2) -> Option<Vec2> {
        if self.path_from != points.first().copied() {
            self.path_from = points.first().copied();
            self.leg = 1;
        }
        while let Some(&point) = points.get(self.leg) {
            if point.distance(at) > ARRIVE {
                return Some(point);
            }
            self.leg += 1;
        }
        None
    }
}

/// The center of an enemy's or the person's collider: their feet, up half
/// its height.
fn center(feet: Vec2) -> Vec2 {
    feet - Vec2::new(0.0, HALF.y)
}

/// The middle of `cell` of the field's 16 px tiles.
fn middle(cell: IVec2) -> Vec2 {
    (cell.as_vec2() + Vec2::splat(0.5)) * 16.0
}

/// The velocity that takes something at `from` toward `to` at `speed`,
/// and no further than `to` in a tick of `dt`.
fn toward(from: Vec2, to: Vec2, speed: f32, dt: f32) -> Vec2 {
    let away = to - from;
    let distance = away.length();
    if distance <= speed * dt {
        away / dt
    } else {
        away / distance * speed
    }
}

/// An enemy at `cell`, drawn as `sprite` tinted `tint`, standing as `gait`
/// stands.
fn spawn_one<'w>(
    commands: &mut Commands<'w>,
    cell: IVec2,
    sprite: Sprite,
    tint: Color,
    gait: &Gait,
) -> EntityCommands<'w> {
    let mut sprite = sprite;
    sprite.color = tint;
    // Feet at the cell's middle, a little down, as the person stands.
    let feet = middle(cell) + Vec2::new(0.0, HALF.y);
    commands
        .spawn()
        .with(Transform::at(feet))
        .with(Velocity::default())
        .with(sprite)
        .with(Animator::new(gait.stand))
        .with(Facing::Right)
        .with(at_feet(HALF, map::PEOPLE, map::WORLD | map::PEOPLE))
}

/// Registers the enemies' components, and adds their grid and the pack's
/// flow fields, empty until [`spawn`] makes them for the level loaded.
pub(crate) fn add(mut app: App) -> App {
    let world = app.world_mut();
    world.register::<Guard>();
    world.register::<PackMember>();
    let nav = NavGrid::new(world);
    app.insert_resource(nav).insert_resource(Pack {
        hunt: FlowField::new(),
        home: FlowField::new(),
    })
}

/// Spawns the guards at the starts of their rounds, and the pack in its
/// den, drawn as `sprite`, through the world's commands, adding them to
/// `spawned`; empties the pack's flow fields, to compute on the first tick;
/// and makes the enemies' grid from the level and its props, which
/// `Navigation` searches. For the game's entering, once the level has
/// loaded.
pub(crate) fn spawn(world: &World, sprite: Sprite, gait: &Gait, spawned: &mut Vec<Entity>) {
    let nav = NavGrid::new(world);
    world
        .resource_mut::<Navigation>()
        .set_grid(Some(nav.grid.clone()));
    *world.resource_mut::<NavGrid>() = nav;
    *world.resource_mut::<Pack>() = Pack {
        hunt: FlowField::new(),
        home: FlowField::new(),
    };
    let mut commands = world.commands();
    for round in ROUNDS {
        let walker = Walker::default();
        let guard = spawn_one(&mut commands, round[0], sprite, GUARD_TINT, gait)
            .with(Guard {
                round,
                next: 1,
                mode: Mode::Round,
                seen: 0,
                ask_at: 0,
                walker,
            })
            .id();
        spawned.push(guard);
    }
    for spot in PACK_SPOTS {
        let member = spawn_one(&mut commands, spot, sprite, PACK_TINT, gait)
            .with(PackMember {
                spot: center(middle(spot) + Vec2::new(0.0, HALF.y)),
            })
            .id();
        spawned.push(member);
    }
}

/// The person's feet, if there is a person.
fn person(world: &World) -> Option<Vec2> {
    let player = world.read::<Player>().entities().first().copied()?;
    Some(world.read::<Transform>().get(player)?.position)
}

/// Sets an enemy's velocity, faces it the way it goes and plays its walk,
/// or has it stand.
fn set_velocity(
    world: &World,
    velocities: &mut SparseSet<Velocity>,
    entity: Entity,
    velocity: Vec2,
) {
    if let Some(it) = velocities.get_mut(entity) {
        it.0 = velocity;
    }
    let gait = world.resource::<Gait>();
    if velocity.x != 0.0
        && let Some(facing) = world.write::<Facing>().get_mut(entity)
    {
        *facing = if velocity.x < 0.0 {
            Facing::Left
        } else {
            Facing::Right
        };
    }
    if let Some(animator) = world.write::<Animator>().get_mut(entity) {
        animator.play(if velocity == Vec2::ZERO {
            gait.stand
        } else {
            gait.walk
        });
    }
}

/// Whether `entity` moved in the last tick: its velocity, set the tick
/// before, is not zero.
fn moving(velocities: &SparseSet<Velocity>, entity: Entity) -> bool {
    velocities.get(entity).is_some_and(|it| it.0 != Vec2::ZERO)
}

/// The guards, in `Gameplay`: each looks for the person, chases them or
/// walks its round, along the paths it asks `Navigation` for. One that
/// sees the person and has room runs straight at them.
pub(crate) fn guards(world: &World, tick: &Tick) {
    let target = person(world).map(center);
    let collisions = world.resource::<Collisions>();
    // Whether it has room to run straight at them is judged on the
    // enemies' grid, where the trees and rocks are.
    let nav = world.resource::<NavGrid>();
    let grid = &nav.grid;
    let mut navigation = world.resource_mut::<Navigation>();
    let mut guards = world.write::<Guard>();
    let transforms = world.read::<Transform>();
    let mut velocities = world.write::<Velocity>();
    for (entity, guard) in guards.iter_mut() {
        let Some(at) = transforms.get(entity).map(|it| center(it.position)) else {
            continue;
        };
        let stuck = guard
            .walker
            .stuck(at, tick.index, moving(&velocities, entity));
        // In sight: near, and past no wall, tree or rock. Its own collider
        // is on the people's layer, which sight looks past.
        let sees = target.is_some_and(|target| {
            at.distance(target) <= SIGHT
                && collisions
                    .world()
                    .segment(at, target, map::WORLD, None)
                    .is_none()
        });
        if sees {
            if guard.mode == Mode::Round {
                guard.ask_at = tick.index;
            }
            guard.mode = Mode::Chase;
            guard.seen = tick.index;
        } else if guard.mode == Mode::Chase && tick.index.saturating_sub(guard.seen) > FORGET_TICKS
        {
            guard.mode = Mode::Round;
            navigation.request(entity, path_to(at, middle(guard.round[guard.next])));
        }
        let velocity = match (guard.mode, target) {
            (Mode::Chase, Some(target)) => {
                if at.distance(target) <= REACH {
                    Vec2::ZERO
                } else if sees && line_of_sight(grid, at, target, HALF) {
                    toward(at, target, RUN, tick.dt)
                } else {
                    if tick.index >= guard.ask_at || stuck {
                        navigation.request(entity, path_to(at, target));
                        guard.ask_at = tick.index + CHASE_PATH_TICKS;
                    }
                    follow(&navigation, entity, &mut guard.walker, at, RUN, tick.dt)
                }
            }
            _ => round(&mut navigation, entity, guard, at, stuck, tick),
        };
        set_velocity(world, &mut velocities, entity, velocity);
    }
}

/// A guard on its round: on to the next point of it once it reaches one,
/// asking for the path there, and again if it is `stuck`.
fn round(
    navigation: &mut Navigation,
    entity: Entity,
    guard: &mut Guard,
    at: Vec2,
    stuck: bool,
    tick: &Tick,
) -> Vec2 {
    let mut goal = middle(guard.round[guard.next]);
    if at.distance(goal) <= ARRIVE {
        guard.next = (guard.next + 1) % guard.round.len();
        goal = middle(guard.round[guard.next]);
        navigation.request(entity, path_to(at, goal));
    } else if stuck || navigation.path(entity).is_none() && !navigation.is_pending(entity) {
        navigation.request(entity, path_to(at, goal));
    }
    // A path to another goal, a chase's or the last point's, is not this
    // leg's.
    match navigation.path(entity).and_then(PathResult::points) {
        Some(points)
            if points
                .last()
                .is_some_and(|&last| last.distance(goal) <= ARRIVE) =>
        {
            match guard.walker.heading(points, at) {
                Some(point) => toward(at, point, WALK, tick.dt),
                None => toward(at, goal, WALK, tick.dt),
            }
        }
        _ => Vec2::ZERO,
    }
}

/// The velocity of a walker at `at` along `entity`'s path, at `speed`;
/// standing while it has none.
fn follow(
    navigation: &Navigation,
    entity: Entity,
    walker: &mut Walker,
    at: Vec2,
    speed: f32,
    dt: f32,
) -> Vec2 {
    let Some(points) = navigation.path(entity).and_then(PathResult::points) else {
        return Vec2::ZERO;
    };
    match walker.heading(points, at) {
        Some(point) => toward(at, point, speed, dt),
        None => Vec2::ZERO,
    }
}

/// A path request from `from` to `to` for an enemy's box.
fn path_to(from: Vec2, to: Vec2) -> PathRequest {
    PathRequest::new(from, to).with_clearance(HALF)
}

/// The pack, in `Gameplay`: down the hunt's flow field toward the person
/// while they are in the south, and down the home one to the den while
/// they are not, both across the enemies' grid.
pub(crate) fn pack(world: &World, tick: &Tick) {
    let feet = person(world);
    let target = feet.map(center);
    let hunting = feet.is_some_and(|feet| feet.y >= crate::SOUTH_TOP);
    let nav = world.resource::<NavGrid>();
    let grid = &nav.grid;
    let mut pack = world.resource_mut::<Pack>();
    let Pack { hunt, home } = &mut *pack;
    home.update(grid, middle(DEN), tick.index);
    if let (true, Some(target)) = (hunting, target) {
        hunt.update(grid, target, tick.index);
    }
    let members = world.read::<PackMember>();
    let transforms = world.read::<Transform>();
    let mut velocities = world.write::<Velocity>();
    let mut colliders = world.write::<Collider>();
    let mask = if hunting {
        map::WORLD | map::PEOPLE
    } else {
        map::WORLD
    };
    for (entity, member) in members.iter() {
        if let Some(collider) = colliders.get_mut(entity) {
            collider.0.mask = mask;
        }
        let Some(at) = transforms.get(entity).map(|it| center(it.position)) else {
            continue;
        };
        let velocity = match (hunting, target) {
            (true, Some(target)) if at.distance(target) <= REACH => Vec2::ZERO,
            (true, Some(target)) => down(hunt, grid, at, target, PACK_RUN, tick.dt),
            _ if at.distance(member.spot) <= ARRIVE => Vec2::ZERO,
            _ => down(home, grid, at, member.spot, PACK_RUN * 0.5, tick.dt),
        };
        set_velocity(world, &mut velocities, entity, velocity);
    }
}

/// The velocity at `speed` down `field` from `at`, toward `goal` at its
/// end: toward the middle of the next cell the field leads to, so that a
/// box keeps to the middle of its row or column and clears the corners
/// beside it; straight at the goal once it is in sight within three cells,
/// or in the goal's own cell. In the open part of a prop's cell, which the
/// field does not reach, it heads for the middle of the open cell round it
/// nearest the goal by the field.
fn down(field: &FlowField, grid: &TileGrid, at: Vec2, goal: Vec2, speed: f32, dt: f32) -> Vec2 {
    if at.distance(goal) <= 48.0 && line_of_sight(grid, at, goal, HALF) {
        return toward(at, goal, speed, dt);
    }
    let cell = grid.cell_at(at);
    if let Some(next) = field.next_cell(cell) {
        return toward(at, middle(next), speed, dt);
    }
    let out = AROUND
        .iter()
        .map(|&offset| cell + offset)
        .filter_map(|near| Some((field.cost(near)?, near)))
        .min_by_key(|&(cost, _)| cost);
    match out {
        Some((_, near)) if grid.is_solid(cell) => toward(at, middle(near), speed, dt),
        _ => toward(at, goal, speed, dt),
    }
}

/// The eight cells round a cell.
const AROUND: [IVec2; 8] = [
    IVec2::new(1, 0),
    IVec2::new(0, 1),
    IVec2::new(-1, 0),
    IVec2::new(0, -1),
    IVec2::new(1, 1),
    IVec2::new(-1, 1),
    IVec2::new(-1, -1),
    IVec2::new(1, -1),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TICK, player, position, sandbox};

    /// The guard that starts nearest `cell`.
    fn guard_at(app: &App, cell: IVec2) -> Entity {
        let start = middle(cell) + Vec2::new(0.0, HALF.y);
        let guards = app.world().read::<Guard>();
        guards
            .entities()
            .iter()
            .copied()
            .min_by(|&a, &b| {
                let near = |entity| position(app, entity).distance(start);
                near(a).total_cmp(&near(b))
            })
            .expect("guards")
    }

    /// Puts the person's feet at `feet`, where they were the tick before
    /// too.
    fn put_person(app: &mut App, feet: Vec2) {
        let player = player(app);
        let world = app.world_mut();
        world
            .write::<Transform>()
            .insert(player, Transform::at(feet));
        world
            .write::<PrevTransform>()
            .insert(player, PrevTransform(Transform::at(feet)));
    }

    fn mode(app: &App, guard: Entity) -> Mode {
        app.world()
            .read::<Guard>()
            .get(guard)
            .expect("a guard")
            .mode
    }

    fn ticks(app: &mut App, count: usize) {
        for _ in 0..count {
            app.update(TICK, &[]);
        }
    }

    #[test]
    fn a_guard_that_sees_the_person_chases_them_and_gives_up_out_of_sight() {
        let mut app = sandbox();
        let guard = guard_at(&app, IVec2::new(8, 12));
        // 90 px east along the path, in sight.
        let feet = position(&app, guard) + Vec2::new(90.0, 0.0);
        put_person(&mut app, feet);
        app.update(TICK, &[]);
        assert_eq!(mode(&app, guard), Mode::Chase);
        // It runs to them, and stops beside them.
        ticks(&mut app, 90);
        let apart = center(position(&app, guard)).distance(center(feet));
        assert!(apart <= REACH + 1.0, "{apart} px apart");
        assert_eq!(
            app.world().read::<Velocity>().get(guard).map(|it| it.0),
            Some(Vec2::ZERO)
        );
        // Gone south, past the tall walls, out of sight: it goes on for
        // three seconds after it last saw them, then back to its round.
        put_person(&mut app, Vec2::new(600.0, 700.0));
        ticks(&mut app, FORGET_TICKS as usize);
        assert_eq!(mode(&app, guard), Mode::Chase);
        ticks(&mut app, 2);
        assert_eq!(mode(&app, guard), Mode::Round);
        // Its paths came from the engine's queue.
        let navigation = app.world().resource::<Navigation>();
        assert!(
            matches!(navigation.path(guard), Some(PathResult::Found(_)))
                || navigation.is_pending(guard)
        );
    }

    #[test]
    fn a_guard_walks_its_round_from_point_to_point() {
        let mut app = sandbox();
        // The eastern guard, far from the person.
        let guard = guard_at(&app, IVec2::new(53, 16));
        let next = |app: &App| {
            app.world()
                .read::<Guard>()
                .get(guard)
                .expect("a guard")
                .next
        };
        assert_eq!(next(&app), 1);
        // 8 cells east, 128 px at 60 px a second: there in about 130 ticks.
        ticks(&mut app, 150);
        assert_eq!(mode(&app, guard), Mode::Round);
        assert_eq!(next(&app), 2);
        let corner = middle(IVec2::new(61, 16));
        assert!(center(position(&app, guard)).distance(corner) < 24.0);
        // And on, round the meadow.
        ticks(&mut app, 120);
        assert_eq!(next(&app), 3);
    }

    #[test]
    fn the_pack_goes_round_the_trees_and_rocks_to_the_person_and_home() {
        // Where the review found the pack pinned on a rock or a tree when
        // its field saw the tiles only: on the way to the person, or home.
        for cell in [IVec2::new(48, 38), IVec2::new(44, 42), IVec2::new(60, 44)] {
            let mut app = sandbox();
            let members: Vec<Entity> = app.world().read::<PackMember>().entities().to_vec();
            let start: Vec<Vec2> = members.iter().map(|&m| position(&app, m)).collect();
            let feet = middle(cell) + Vec2::new(0.0, HALF.y);
            put_person(&mut app, feet);
            ticks(&mut app, 720);
            let apart: Vec<f32> = members
                .iter()
                .map(|&m| center(position(&app, m)).distance(center(feet)))
                .collect();
            // Round them, the nearest beside them and the rest as near as the
            // others let them be. A sweep of 119 cells of the south had none
            // further than 54 px.
            let nearest = apart.iter().copied().fold(f32::INFINITY, f32::min);
            assert!(nearest <= REACH + 1.0, "{cell:?}: {apart:?}");
            assert!(
                apart.iter().all(|&apart| apart < 64.0),
                "{cell:?}: {apart:?}"
            );
            put_person(&mut app, crate::PLAYER_START);
            ticks(&mut app, 1_200);
            for (&member, &spot) in members.iter().zip(&start) {
                let home = position(&app, member).distance(spot);
                assert!(
                    home <= ARRIVE + 0.5,
                    "{cell:?}: {member:?} is {home} px from home"
                );
            }
        }
    }

    #[test]
    fn a_guard_goes_round_a_tree_s_trunk_to_the_person_behind_it() {
        let mut app = sandbox();
        let guard = guard_at(&app, IVec2::new(53, 16));
        // The tree of cell (36, 16), its trunk 580 to 588 across and 266 to
        // 272 down: the guard just above it, chasing, the person below it.
        let tree = IVec2::new(36, 16);
        assert!(app.world().resource::<NavGrid>().grid.is_solid(tree));
        let above = Vec2::new(584.0, 264.0);
        app.world_mut()
            .write::<Transform>()
            .insert(guard, Transform::at(above));
        let feet = Vec2::new(584.0, 300.0);
        put_person(&mut app, Vec2::new(584.0, 240.0));
        app.update(TICK, &[]);
        assert_eq!(mode(&app, guard), Mode::Chase);
        put_person(&mut app, feet);
        ticks(&mut app, 120);
        let apart = center(position(&app, guard)).distance(center(feet));
        assert!(
            apart <= REACH + 1.0,
            "{apart} px apart, at {:?}",
            position(&app, guard)
        );
    }

    #[test]
    fn the_pack_hunts_the_person_in_the_south_and_goes_home_when_they_leave() {
        let mut app = sandbox();
        let members: Vec<Entity> = app.world().read::<PackMember>().entities().to_vec();
        assert_eq!(members.len(), 5);
        let start: Vec<Vec2> = members.iter().map(|&m| position(&app, m)).collect();
        // In the south, twelve cells east of the den.
        let feet = middle(DEN + IVec2::new(12, 0)) + Vec2::new(0.0, HALF.y);
        put_person(&mut app, feet);
        ticks(&mut app, 240);
        assert!(app.world().resource::<Pack>().hunt.goal().is_some());
        let apart: Vec<f32> = members
            .iter()
            .map(|&m| center(position(&app, m)).distance(center(feet)))
            .collect();
        // Round them, each as near as the others let it be.
        let nearest = apart.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(nearest <= REACH + 1.0, "{apart:?}");
        for (&now, &then) in apart.iter().zip(&start) {
            assert!(
                now < 64.0 && now < center(then).distance(center(feet)) / 2.0,
                "{apart:?}"
            );
        }
        // North again: each back to its spot in the den.
        put_person(&mut app, crate::PLAYER_START);
        ticks(&mut app, 480);
        for (&member, &spot) in members.iter().zip(&start) {
            let home = position(&app, member).distance(spot);
            assert!(home <= ARRIVE + 0.5, "{member:?} is {home} px from home");
        }
    }
}
