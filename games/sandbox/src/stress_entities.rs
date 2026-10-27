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
//! the atlas and the walls as dark grass, so the frame is 2 batches. The
//! camera follows the player, tinted yellow, who walks through everything.

use nadir::prelude::*;

use crate::stress::{
    ATLAS, PERSON, PERSON_SIZE, PLAYER_TINT, Pace, Walker, heading, turn_crowd, unit,
};
use crate::{Sprites, spawn_player};

/// How many people jostle.
pub(crate) const BODIES: usize = 5_000;

/// The maze's side, in tiles, and a tile's side, in pixels.
const TILES: i32 = 128;
const TILE: f32 = 16.0;

/// How much faster than the walkers of `stress-sprites` the people go:
/// 60 to 180 pixels a second.
const PACE: f32 = 3.0;

/// Adds the maze, the people, the player and the systems to `app`.
pub(crate) fn add(app: App) -> App {
    let atlas = app.world().resource_mut::<AssetServer>().load(ATLAS);
    let mut app = app.register_component::<Walker>();
    let (grid, open) = maze();
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
        sprite.flip.x = velocity.x < 0.0;
        world
            .spawn()
            .with(Walker(index))
            .with(Transform::at(feet))
            .with(Velocity(velocity))
            .with(collider)
            .with(sprite);
    }
    let mut person = Sprite::new(atlas, PERSON_SIZE);
    person.uv = PERSON;
    person.layer = 1;
    person.color = PLAYER_TINT;
    spawn_player(world, person);
    world.resource_mut::<Collisions>().set_grid(grid);
    app.insert_resource(Pace(PACE))
        .add_fixed_system(FixedStage::Gameplay, "turn_crowd", turn_crowd)
        .add_fixed_system(FixedStage::Movement, "face_crowd", face_crowd)
        .add_frame_system(FrameStage::Camera, "follow_camera", crate::follow_camera)
        .add_frame_system(FrameStage::Extract, "draw_walls", draw_walls)
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
    let mut sprites = world.write::<Sprite>();
    for (entity, _) in walkers.iter() {
        if let (Some(velocity), Some(sprite)) = (velocities.get(entity), sprites.get_mut(entity)) {
            sprite.flip.x = velocity.0.x < 0.0;
        }
    }
}

/// The walls, as dark grass, under everything else.
fn draw_walls(world: &World, _frame: &Frame) {
    let grass = world.resource::<Sprites>().grass;
    let collisions = world.resource::<Collisions>();
    let grid = collisions.grid();
    let mut out = world.resource_mut::<RenderFrame>();
    out.clear = Color::rgb(70, 60, 50);
    for y in 0..TILES {
        for x in 0..TILES {
            let cell = IVec2::new(x, y);
            if grid.is_solid(cell) {
                let mut wall = SpriteDraw::new(grass, grid.cell_rect(cell).min, Vec2::splat(TILE));
                wall.pivot = Vec2::ZERO;
                wall.sort = SortMode::Fixed(0);
                wall.color = Color::rgb(90, 90, 110);
                out.sprites.push(wall);
            }
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
    fn a_frame_draws_the_walls_and_the_people_in_two_batches_worth_of_textures() {
        let mut app = scene(Scene::StressEntities);
        app.update(TICK, &[]);
        let world = app.world();
        let out = world.resource::<RenderFrame>();
        let atlas = world
            .read::<Sprite>()
            .get(player(&app))
            .expect("drawn")
            .texture;
        let grass = world.resource::<Sprites>().grass;
        let (walls, people): (Vec<&SpriteDraw>, Vec<&SpriteDraw>) =
            out.sprites.iter().partition(|sprite| sprite.layer == 0);
        assert!(walls.iter().all(|wall| wall.texture == grass));
        assert!(people.iter().all(|person| person.texture == atlas));
        assert_eq!(people.len(), BODIES + 1);
        // The edge, and about a fifth of the rest.
        let edge = 4 * (TILES as usize - 1);
        assert!(walls.len() > edge + 2_500 && walls.len() < edge + 3_900);
    }
}
