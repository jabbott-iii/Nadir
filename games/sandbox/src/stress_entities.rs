//! The entity stress scene, `--scene stress-entities`: 5,000 people
//! jostling in a maze of walls, each with a collider, for the budget of
//! 5,000 entities with collision at 60 ticks a second, in at most 4 ms a
//! tick on min-spec (`docs/00-overview.md`, the M4 exit). The report says
//! how long the ticks take.
//!
//! The maze is 128 × 128 tiles of 16 pixels, about one in five solid, with
//! a wall all round. Each person collides at their feet, turns now and
//! then, and walks 1 to 3 pixels a tick; a tenth of them are triggers,
//! which walk through the others and notice them. The people are drawn from
//! the atlas, in 1 batch, walking as in `stress-sprites`, and the walls are
//! a tile layer of stone, drawn a
//! chunk at a time: the 4 chunks or fewer of 32 × 32 tiles in view. The
//! camera follows the player, tinted yellow, who walks through everything.

use nadir::prelude::*;

use crate::stress::{
    ATLAS, PERSON, PERSON_SIZE, PLAYER_TINT, Pace, WALK, Walker, facing, heading, stagger,
    turn_crowd, unit,
};
use crate::{Gait, Ground, map, spawn_player};

/// How many people jostle.
pub(crate) const BODIES: usize = 5_000;

/// The maze's side, in tiles, and a tile's side, in pixels.
const TILES: i32 = 128;
const TILE: f32 = 16.0;

/// How much faster than the walkers of `stress-sprites` the people go:
/// 60 to 180 pixels a second.
const PACE: f32 = 3.0;

/// Adds the maze, the people, the player and the systems to `app`.
pub(crate) fn add(app: App, tiles: Handle<Texture>) -> App {
    let atlas = app.world().resource_mut::<AssetServer>().load(ATLAS);
    let mut app = app.register_component::<Walker>();
    let gait = Gait::new(&app, atlas, PERSON, WALK);
    let (grid, open) = maze();
    let walls = walls(&grid, map::tile_set(tiles));
    let world = app.world_mut();
    for (index, &cell) in (0u32..).zip(&open[..BODIES]) {
        let half = Vec2::new(5.0 + 2.0 * unit(index, 90), 3.0 + 2.0 * unit(index, 91));
        let shape = if index.is_multiple_of(2) {
            Shape::Circle { radius: half.y }
        } else {
            Shape::Aabb { half }
        };
        let extent = shape.half_extents().y;
        let base = if index.is_multiple_of(10) {
            nadir::collision::Collider::trigger(shape)
        } else {
            nadir::collision::Collider::solid(shape)
        };
        // The collider is at the feet, and the feet in the middle of the
        // tile, give or take.
        let collider = Collider(nadir::collision::Collider {
            offset: Vec2::new(0.0, -extent),
            ..base
        });
        let feet = grid.cell_rect(cell).center() + Vec2::new(0.0, extent);
        let velocity = heading(index, 0) * PACE;
        let mut sprite = Sprite::new(atlas, PERSON_SIZE);
        sprite.uv = PERSON;
        sprite.layer = 1;
        world
            .spawn()
            .with(Walker(index))
            .with(Transform::at(feet))
            .with(Velocity(velocity))
            .with(collider)
            .with(sprite)
            .with(Animator::starting_at(gait.walk, stagger(index)))
            .with(facing(velocity));
    }
    let mut person = Sprite::new(atlas, PERSON_SIZE);
    person.uv = PERSON;
    person.layer = 1;
    person.color = PLAYER_TINT;
    spawn_player(world, person, &gait);
    world.resource_mut::<Collisions>().set_grid(grid);
    let walls = world.resource_mut::<TileLayers>().add(walls);
    app.insert_resource(Pace(PACE))
        .insert_resource(gait)
        .insert_resource(Ground {
            tiles: walls,
            outside: Color::rgb(70, 60, 50),
        })
        .add_fixed_system(FixedStage::Gameplay, "turn_crowd", turn_crowd)
        .add_fixed_system(FixedStage::Movement, "face_crowd", face_crowd)
        .add_frame_system(FrameStage::Camera, "follow_camera", crate::follow_camera)
        .add_frame_system(FrameStage::Extract, "draw_tiles", crate::draw_tiles)
}

/// The walls of `grid`, as a tile layer of stone from `set`. The floor
/// is empty, and shows the clear color.
fn walls(grid: &TileGrid, set: TileSet) -> TileLayer {
    let size = UVec2::new(grid.width(), grid.height());
    let mut walls = TileLayer::new(set, size, grid.cell_size(), grid.origin());
    for y in 0..TILES {
        for x in 0..TILES {
            let cell = IVec2::new(x, y);
            if grid.is_solid(cell) {
                walls.set(cell, Some(Tile::new(map::STONE)));
            }
        }
    }
    walls
}

/// The maze, and its open tiles in an order that looks random and is the
/// same every run.
fn maze() -> (TileGrid, Vec<IVec2>) {
    let mut grid = TileGrid::new(TILES as u32, TILES as u32, TILE, Vec2::ZERO);
    let mut open = Vec::new();
    for y in 0..TILES {
        for x in 0..TILES {
            let cell = IVec2::new(x, y);
            let edge = x == 0 || y == 0 || x == TILES - 1 || y == TILES - 1;
            if edge || unit((y * TILES + x) as u32, 80) < 0.2 {
                grid.set_solid(cell, true);
            } else {
                open.push(cell);
            }
        }
    }
    open.sort_by_key(|cell| (unit((cell.y * TILES + cell.x) as u32, 81) * 1e6) as u32);
    (grid, open)
}

/// Faces each person the way they go, after the engine has moved them.
fn face_crowd(world: &World, _tick: &Tick) {
    let walkers = world.read::<Walker>();
    let velocities = world.read::<Velocity>();
    let mut facings = world.write::<Facing>();
    for (entity, _) in walkers.iter() {
        if let (Some(velocity), Some(way)) = (velocities.get(entity), facings.get_mut(entity)) {
            *way = facing(velocity.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;
    use crate::tests::{TICK, player, scene};

    #[test]
    fn the_people_jostle_inside_the_maze() {
        let mut app = scene(Scene::StressEntities);
        let start: Vec<(Entity, Vec2)> = {
            let transforms = app.world().read::<Transform>();
            let walkers = app.world().read::<Walker>();
            walkers
                .iter()
                .map(|(entity, _)| (entity, transforms.get(entity).expect("one").position))
                .collect()
        };
        assert_eq!(start.len(), BODIES);
        for _ in 0..30 {
            app.update(TICK, &[]);
        }
        let world = app.world();
        let transforms = world.read::<Transform>();
        let velocities = world.read::<Velocity>();
        let size = TILES as f32 * TILE;
        let mut stopped = 0;
        for &(entity, from) in &start {
            let feet = transforms.get(entity).expect("one").position;
            assert!(
                feet.x > TILE && feet.x < size - TILE && feet.y > TILE && feet.y < size,
                "{entity:?} got out to {feet:?}"
            );
            // 30 ticks at 1 to 3 px, with a turn for one in four.
            let speed = velocities.get(entity).expect("one").0.length() / 60.0;
            if (feet - from).length() < speed * 30.0 * 0.5 {
                stopped += 1;
            }
        }
        assert!(stopped > BODIES / 10, "only {stopped} were held up");
        let stats = *world.resource::<TickStats>();
        assert_eq!(stats.ticks, 1);
        assert_eq!(world.resource::<Collisions>().world().len(), BODIES);
    }

    #[test]
    fn the_people_walk_out_of_step_and_face_the_way_they_go() {
        let mut app = scene(Scene::StressEntities);
        for _ in 0..30 {
            app.update(TICK, &[]);
        }
        let world = app.world();
        let walkers = world.read::<Walker>();
        let velocities = world.read::<Velocity>();
        let sprites = world.read::<Sprite>();
        let mut frames = [0; 4];
        for (entity, _) in walkers.iter() {
            let sprite = sprites.get(entity).expect("drawn");
            let frame = WALK.iter().position(|&uv| uv == sprite.uv);
            frames[frame.expect("a frame of the walk")] += 1;
            let velocity = velocities.get(entity).expect("one").0;
            assert_eq!(sprite.flip.x, velocity.x < 0.0, "{entity:?}");
        }
        assert!(frames.iter().all(|&n| n > BODIES / 5), "{frames:?}");
    }

    #[test]
    fn a_frame_draws_the_people_from_the_atlas_and_the_walls_as_tiles() {
        let mut app = scene(Scene::StressEntities);
        app.update(TICK, &[]);
        let world = app.world();
        let out = world.resource::<RenderFrame>();
        let atlas = world
            .read::<Sprite>()
            .get(player(&app))
            .expect("drawn")
            .texture;
        assert!(out.sprites.iter().all(|person| person.texture == atlas));
        assert_eq!(out.sprites.len(), BODIES + 1);
        let ground = world.resource::<Ground>();
        assert_eq!(out.tile_layers, [TileLayerDraw::new(ground.tiles)]);
        // A wall tile on each solid cell, and nothing elsewhere.
        let layers = world.resource::<TileLayers>();
        let walls = layers.get(ground.tiles).expect("the maze's walls");
        let collisions = world.resource::<Collisions>();
        let grid = collisions.grid();
        let mut solid = 0;
        for y in 0..TILES {
            for x in 0..TILES {
                let cell = IVec2::new(x, y);
                let wall = walls.get(cell);
                assert_eq!(wall.is_some(), grid.is_solid(cell), "{cell:?}");
                solid += usize::from(wall.is_some());
            }
        }
        // The edge, and about a fifth of the rest.
        let edge = 4 * (TILES as usize - 1);
        assert!(solid > edge + 2_500 && solid < edge + 3_900, "{solid}");
    }
}
