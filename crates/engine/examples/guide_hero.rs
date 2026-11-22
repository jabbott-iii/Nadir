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

//! A walking hero: actions, a cooked atlas, a tile layer, collision, animation, and a camera.
//!
//! An example of the instructions page, `site/instructions/index.html`,
//! which `cargo xtask site` copies into the page without these comments.

use std::error::Error;

use nadir::assets::Scope;
use nadir::prelude::*;

/// What the player does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Move,
}

impl ActionSet for Act {
    const ALL: &'static [Self] = &[Act::Move];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        "move"
    }

    fn kind(self) -> ActionKind {
        ActionKind::Axis2
    }
}

/// The hero's clips: the tags of their Aseprite export.
struct Gait {
    stand: Handle<Clip>,
    walk: Handle<Clip>,
}

/// The ground's tile layer.
struct Ground(Handle<TileLayer>);

/// How fast the hero walks, in pixels a second.
const SPEED: f32 = 120.0;
/// The field's size in tiles, and a tile's side in pixels.
const WIDTH: u32 = 30;
const HEIGHT: u32 = 17;
const TILE: f32 = 16.0;

fn main() -> Result<(), Box<dyn Error>> {
    let mut map = ActionMap::<Act>::new()?;
    let keys = |up, down, left, right| Binding::KeyComposite2 {
        up,
        down,
        left,
        right,
    };
    map.bind(Act::Move, 0, keys(Key::W, Key::S, Key::A, Key::D))?;
    map.bind(
        Act::Move,
        1,
        keys(Key::Up, Key::Down, Key::Left, Key::Right),
    )?;

    let mut app = App::new(Config {
        title: "My game".into(),
        game: "My game".into(),
        assets: "cooked".into(),
        ..Config::default()
    })
    .add_actions(map)
    .add_fixed_system(FixedStage::Gameplay, "walk", walk)
    .add_frame_system(FrameStage::Camera, "follow", follow)
    .add_frame_system(FrameStage::Extract, "draw_ground", draw_ground);

    let world = app.world_mut();
    // The cooked atlas, made now, and its pages, which load in the
    // background and are drawn once they have.
    let (atlas, pages) = {
        let mut server = world.resource_mut::<AssetServer>();
        let atlas = server.load_now::<Atlas>("atlas/sprites.kbj", &mut FinalizeCtx::new())?;
        let atlas = server.get(atlas).cloned().ok_or("the atlas did not load")?;
        let pages = atlas.load_pages(&mut server, Scope::Global);
        (atlas, pages)
    };
    let page = |sprite: &AtlasSprite| {
        let first = sprite.frames().first().ok_or("a sprite with no frame")?;
        pages.get(first.page()).copied().ok_or("a frame on no page")
    };
    let gait = {
        let mut clips = world.resource_mut::<Clips>();
        clips.add_atlas(&atlas, &pages)?;
        Gait {
            stand: clips.find("sprites/hero/stand").ok_or("no clip `stand`")?,
            walk: clips.find("sprites/hero/walk").ok_or("no clip `walk`")?,
        }
    };
    let hero = atlas
        .sprite("sprites/hero")
        .ok_or("no sprite `sprites/hero`")?;
    let hero = Sprite::from_atlas(page(hero)?, hero, 0).ok_or("the hero has no frame")?;
    let tiles = atlas
        .sprite("sprites/tiles")
        .ok_or("no sprite `sprites/tiles`")?;
    let set = tiles
        .tile_set(page(tiles)?)
        .ok_or("`sprites/tiles` is not a tile set")?;

    // The ground to draw, and the grid of solid cells to collide with.
    let mut ground = TileLayer::new(set, UVec2::new(WIDTH, HEIGHT), TILE, Vec2::ZERO);
    let mut grid = TileGrid::new(WIDTH, HEIGHT, TILE, Vec2::ZERO);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let cell = IVec2::new(x as i32, y as i32);
            let stone = x == 0 || y == 0 || x == WIDTH - 1 || y == HEIGHT - 1;
            ground.set(cell, Some(Tile::new(if stone { 1 } else { 0 })));
            grid.set_solid(cell, stone);
        }
    }
    let ground = world.resource_mut::<TileLayers>().add(ground);
    world.resource_mut::<Collisions>().set_grid(grid);

    world
        .spawn()
        .with(Player)
        .with(Transform::at(Vec2::new(240.0, 136.0)))
        .with(Velocity::default())
        .with(hero)
        .with(Animator::new(gait.stand))
        .with(Facing::Right)
        // A box at the hero's feet, which their Transform is.
        .with(Collider::solid(Shape::Aabb {
            half: Vec2::new(5.0, 3.0),
        }));
    world.insert_resource(gait);
    world.insert_resource(Ground(ground));

    app.run()?;
    Ok(())
}

/// Walks the hero by the `Move` action, faces them the way they go, and
/// plays their walk while they go. The engine's `nadir::movement`, after
/// this in the tick, moves them and slides them along the walls.
fn walk(world: &World, _tick: &Tick) {
    let direction = world.resource::<ActionState<Act>>().axis2(Act::Move);
    let gait = world.resource::<Gait>();
    let players = world.read::<Player>();
    let mut velocities = world.write::<Velocity>();
    let mut facings = world.write::<Facing>();
    let mut animators = world.write::<Animator>();
    for (player, _) in players.iter() {
        if let Some(velocity) = velocities.get_mut(player) {
            velocity.0 = direction * SPEED;
        }
        if let Some(facing) = facings.get_mut(player) {
            if direction.x < 0.0 {
                *facing = Facing::Left;
            } else if direction.x > 0.0 {
                *facing = Facing::Right;
            }
        }
        if let Some(animator) = animators.get_mut(player) {
            animator.play(if direction == Vec2::ZERO {
                gait.stand
            } else {
                gait.walk
            });
        }
    }
}

/// Centers the camera on the hero, where this frame draws them: part of the
/// way from where the last tick started to where it left them.
fn follow(world: &World, frame: &Frame) {
    let Some(player) = world.read::<Player>().entities().first().copied() else {
        return;
    };
    let Some(now) = world.read::<Transform>().get(player).map(|it| it.position) else {
        return;
    };
    let then = world
        .read::<PrevTransform>()
        .get(player)
        .map_or(now, |it| it.0.position);
    world.resource_mut::<RenderFrame>().camera.position = then.lerp(now, frame.alpha);
}

/// Draws the ground, behind every sprite.
fn draw_ground(world: &World, _frame: &Frame) {
    let ground = world.resource::<Ground>().0;
    world
        .resource_mut::<RenderFrame>()
        .tile_layers
        .push(TileLayerDraw::new(ground));
}
