# nadir_engine

The engine crate of Nadir: the `App` and its main loop, the engine's
components and systems, and the subsystems a game uses, in one crate. Code
names it `nadir`.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. This is the crate a game depends on: it re-exports the
ECS, input, rendering, collision, assets and reflection as `nadir::ecs`,
`nadir::input`, `nadir::render`, `nadir::collision`, `nadir::assets` and
`nadir::scene`, so a game needs nothing else.

## Status

Version 0.3: the API will change before 1.0.

Built:

- a window, with keyboard and mouse input, on Linux (X11);
- a fixed 60 Hz simulation, which drawing interpolates between ticks;
- an ECS of entities, components, resources and events, with systems in
  stages;
- input actions, with bindings, latched once a tick, and replays of them;
- an OpenGL 3.3 renderer: y-sorted sprite batches, tile layers with
  animated tiles, and a pixel-perfect camera;
- kinematic collision: boxes and circles, a grid of solid tiles,
  move-and-slide, triggers, and queries;
- sprite animation, from clips made in code or from Aseprite's tags;
- prefabs, which extend one another, spawned through the
  `PrefabLibrary`, with the engine's sprites, colliders and animators
  reflected for them;
- levels made in LDtk and cooked, which `App::load_level` loads whole, in
  place of the last: their tile layers drawn, their grid collided with,
  their tall tiles and entities spawned;
- assets loaded on worker threads, and an offline cook that packs images,
  Aseprite exports and bitmap fonts into atlases, makes level files of
  LDtk projects, and puts all it cooks in one pack file, which release
  builds read;
- hot reload in development builds: textures, atlases and their clips,
  prefabs, the current level and shaders, reloaded while the game runs,
  behind the handles it holds;
- a world hash, to check that a replay gives the same simulation.

Not built yet:

- drawing on Windows and macOS: the window stays empty until the OpenGL
  contexts there land (roadmap tasks M2.6 and M2.7);
- audio (M6), text (M7.1), the debug tools (M7), gamepads (M8);
- game states (M9.1), settings (M9.3), saves (M9.4), pathfinding (M9.5);
- the Metal backend for macOS (M10).

## What you need

- Rust, from [rustup](https://rustup.rs). The engine is built and tested
  with the stable toolchain its repository pins (`rust-toolchain.toml`);
  older versions are not tested.
- On Linux: an X11 session, or Wayland with XWayland, and OpenGL 3.3. The
  engine loads `libX11`, `libEGL` and OpenGL at run time, so it needs no
  development packages to build.
- On Windows: the MSVC build tools and the Windows SDK. On macOS: the
  Xcode command-line tools. A game builds and runs on both, but its window
  stays empty for now (see [Platforms](#platforms)).

## Usage

```toml
[dependencies]
nadir_engine = "0.3"
```

The package is `nadir_engine`, since `nadir` was taken on crates.io, and its
library is `nadir`. A game needs only this crate:

```rust,no_run
use nadir::prelude::*;

fn main() -> Result<(), nadir::Error> {
    App::new(Config::default())
        .add_fixed_system(FixedStage::Gameplay, "hello", |_, tick| {
            if tick.index == 0 {
                nadir::log::info!("the first tick");
            }
        })
        .run()
}
```

`App::run` opens the window and runs the game until the window is closed.

### A walking hero

This game has a hero who walks with WASD or the arrow keys, animated and
facing the way they go, on a field of grass walled in by stone that they
collide with, and a camera that follows them. Its art is cooked by
`nadir-cook` (see [Cooking assets](#cooking-assets)) from these files:

```text
assets/sprites/hero.png      the hero's sheet, exported from Aseprite
assets/sprites/hero.json     its frames and its tags, `stand` and `walk`
assets/sprites/tiles.png     a tile set: grass is tile 0, stone tile 1
assets/sprites/tiles.sprite.jsonc
                             { "tiles": [16, 16] }: cut it into 16-pixel tiles
```

```rust,no_run
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
    let keys = |up, down, left, right| Binding::KeyComposite2 { up, down, left, right };
    map.bind(Act::Move, 0, keys(Key::W, Key::S, Key::A, Key::D))?;
    map.bind(Act::Move, 1, keys(Key::Up, Key::Down, Key::Left, Key::Right))?;

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
    let hero = atlas.sprite("sprites/hero").ok_or("no sprite `sprites/hero`")?;
    let hero = Sprite::from_atlas(page(hero)?, hero, 0).ok_or("the hero has no frame")?;
    let tiles = atlas.sprite("sprites/tiles").ok_or("no sprite `sprites/tiles`")?;
    let set = tiles.tile_set(page(tiles)?).ok_or("`sprites/tiles` is not a tile set")?;

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
        .with(Collider::solid(Shape::Aabb { half: Vec2::new(5.0, 3.0) }));
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
            animator.play(if direction == Vec2::ZERO { gait.stand } else { gait.walk });
        }
    }
}

/// Centres the camera on the hero, where this frame draws them: part of the
/// way from where the last tick started to where it left them.
fn follow(world: &World, frame: &Frame) {
    let Some(player) = world.read::<Player>().entities().first().copied() else {
        return;
    };
    let Some(now) = world.read::<Transform>().get(player).map(|it| it.position) else {
        return;
    };
    let then = world.read::<PrevTransform>().get(player).map_or(now, |it| it.0.position);
    world.resource_mut::<RenderFrame>().camera.position = then.lerp(now, frame.alpha);
}

/// Draws the ground, behind every sprite.
fn draw_ground(world: &World, _frame: &Frame) {
    let ground = world.resource::<Ground>().0;
    world.resource_mut::<RenderFrame>().tile_layers.push(TileLayerDraw::new(ground));
}
```

### Running frames without a window

`App::update` runs one frame, with the window's events you give it, and
draws nothing. Tests use it to drive a game tick by tick:

```rust
use std::time::Duration;

use nadir::prelude::*;

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

fn walk(world: &World, _tick: &Tick) {
    let direction = world.resource::<ActionState<Act>>().axis2(Act::Move);
    let players = world.read::<Player>();
    let mut velocities = world.write::<Velocity>();
    for (player, _) in players.iter() {
        if let Some(velocity) = velocities.get_mut(player) {
            velocity.0 = direction * 120.0;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut map = ActionMap::<Act>::new()?;
    let wasd = Binding::KeyComposite2 { up: Key::W, down: Key::S, left: Key::A, right: Key::D };
    map.bind(Act::Move, 0, wasd)?;
    let mut app = App::new(Config::default())
        .add_actions(map)
        .add_fixed_system(FixedStage::Gameplay, "walk", walk);
    let player = app
        .world_mut()
        .spawn()
        .with(Player)
        .with(Transform::at(Vec2::ZERO))
        .with(Velocity::default())
        .id();

    // D goes down, and a frame of one tick runs: 120 pixels a second is 2
    // a tick.
    let tick = Duration::from_nanos(16_666_667);
    let d = nadir::Event::Key { key: Key::D, pressed: true, repeat: false };
    app.update(tick, &[d]);
    let at = app.world().read::<Transform>().get(player).map(|it| it.position);
    assert_eq!(at, Some(Vec2::new(2.0, 0.0)));
    Ok(())
}
```

## How a game fits together

### The app and the main loop

A game makes an `App` from a `Config`, adds its resources, actions and
systems with the builder methods, sets its world up through
`App::world_mut`, and calls `App::run`. That opens the log file and the
window, and then each frame:

1. hands the window's events to `Input`;
2. runs the simulation ticks that are due, 60 a second however fast the
   frames come; each tick latches the game's actions, then runs the tick
   stages;
3. runs the frame stages once, which fill the `RenderFrame`;
4. draws it, and makes the assets the workers have loaded.

A frame counts for at most 250 ms, so after a stall the game falls behind
rather than running a burst of ticks to catch up. `Config` sets the window
(title, size, resizing, fullscreen), the pacing (`frame_rate_limit`, 60 by
default, and `vsync`), the size of the image the world is drawn into
(`virtual_resolution`, 480 × 270 by default), and the folder the assets
load from (`assets`). `studio` and `game` name the folders of the log file.

### The world, systems and stages

The game's state is a `World` (`nadir::ecs`): entities, their components,
and resources, which are values of any type, one of each.

- A component is a type with `impl Component for MyType {}`, registered
  with `app.world_mut().register::<MyType>()` before use. A reflected one,
  defined with `nadir::scene::reflect_struct!`, `reflect_enum!` or
  `reflect_newtype!`, is registered with `App::register_component` instead:
  the world hash covers the reflected components.
- A system is a plain function, `fn(&World, &Tick)` for a stage of each
  tick or `fn(&World, &Frame)` for a stage of each frame. A closure that
  captures nothing will do too; systems keep their state in resources.
- Systems read and write through the world: `world.read::<T>()`,
  `world.write::<T>()`, `world.resource::<R>()` and
  `world.resource_mut::<R>()`. These are borrowed at run time: borrowing
  the same storage mutably twice at once panics, naming the system and the
  type.
- Inside a system, spawn and despawn through `world.commands()`. Commands
  apply after the tick's stages, or after the frame's. During setup,
  `world.spawn()` spawns at once.
- Events are resources too: `App::add_event::<T>()`, then
  `world.resource_mut::<Events<T>>().send(event)`. Tick systems read
  `tick_events()`, the tick's own; frame systems read `frame_events()`, all
  the ticks' since the last frame.

The stages of a tick are `Input`, `Gameplay`, `Movement`, `PostMovement`
and `Cleanup`; those of a frame `Animate`, `Camera`, `Ui` and `Extract`.
Systems run in stage order, then in the order they were added. The engine
adds its own first in their stages:

| Stage | Engine system | What it does |
| :- | :- | :- |
| tick `Input` | `nadir::snapshot` | Copies each `Transform` into its `PrevTransform` |
| tick `Movement` | `nadir::movement` | Moves each entity with a `Velocity`; one with a `Collider` slides along what it meets |
| tick `PostMovement` | `nadir::parent_offsets` | Puts each entity with a `Parent` at its parent's position, plus its offset |
| tick `PostMovement` | `nadir::triggers` | Sends `TriggerEnter` and `TriggerExit` |
| frame `Animate` | `nadir::animate` | Advances each `Animator`, writes its frame into its `Sprite`, and mirrors the sprites that face left |
| frame `Extract` | `nadir::extract_sprites` | Draws each `Sprite` into the `RenderFrame`, between the last two ticks |
| frame `Extract` | `nadir::extract_level` | Draws the `CurrentLevel`'s tile layers, with its background as the clear colour |

So a game sets velocities in `Gameplay`, and reacts to the moves after the
engine's, in `Movement` or `PostMovement`. Gameplay goes in tick systems,
which run with a fixed step of 1/60 s; the camera and other things that
only show go in frame systems.

### Input actions

Gameplay reads actions, never keys. The game names its actions with an
enum that implements `ActionSet`, binds keys and mouse buttons to them in an
`ActionMap` (up to four bindings an action), and adds it with
`App::add_actions`. Each tick, before the tick stages, the input is
latched into the resource `ActionState<A>`: `pressed`, `just_pressed`,
`just_released`, `value` and `axis2`. A key pressed and released between
two ticks is still seen. Read edges such as `just_pressed` in tick systems:
a frame system sees only the last tick's. The resource `Input` also holds
the pointer and the wheel.

### Sprites and the camera

An entity with a `Transform` and a `Sprite` is drawn. Its position is the
sprite's pivot, the middle of its bottom edge unless set otherwise: its
feet. Sprites are sorted by layer, higher in front, and within a layer by
the pivot's y, so that what is lower on screen is in front.
`Sprite::new(texture, size)` shows a whole texture, and
`Sprite::from_atlas` a frame of a cooked atlas.

Frames draw each sprite part of the way from where it was at the start of
the last tick to where the tick left it. The engine keeps the
`PrevTransform` that this needs. To move an entity at once, with nothing
drawn on the way, write both its `Transform` and its `PrevTransform`.

The world is drawn into an image of `Config::virtual_resolution` pixels,
scaled up into the window by the largest whole factor that fits, with bars
of `RenderFrame::clear`'s color around it. The camera is the
`RenderFrame`'s `camera`: a frame system in the `Camera` stage sets its
`position`, the world point at the centre of the view, and it keeps it from
one frame to the next.

### Tile layers

A `TileLayer` is a grid of cells showing tiles of a `TileSet`, a texture cut
into columns and rows; `Tile::animated` cycles through tiles with no work on
the CPU. Add the layer to the resource `TileLayers`, and push a
`TileLayerDraw` of its handle into `RenderFrame::tile_layers` each frame, in
the `Extract` stage. Tiles do not collide by themselves: the collision grid
is a `TileGrid`, set with `Collisions::set_grid`.

### Collision

Collision is kinematic: no forces, no rigid bodies. A `Collider` is a box or
a circle (`Shape`) on collision layers (`LayerMask`), offset from its
entity's `Transform`. `Collider::solid` stops movers; `Collider::trigger`
only notices them, and the engine sends `TriggerEnter` and `TriggerExit`
events when a collider starts and stops overlapping it.

The resource `Collisions` holds the colliders, rebuilt each tick. Its
`world()` answers queries (`point`, `aabb`, `circle`, `nearest`, and the
casts `segment`, `sweep_aabb` and `sweep_circle`) with collider indices,
which `Collisions::entity` turns into entities. Pathfinding comes with
roadmap task M9.5.

### Animation

A `Clip` is a list of frames, each a region of a texture shown for a time,
which loops or plays once. The resource `Clips` keeps them: `add`,
`add_named`, or `add_atlas` for every clip of a cooked atlas, named like
`sprites/hero/walk` from Aseprite's tags. An `Animator` plays a clip on its
entity's `Sprite`; `Animator::play` changes the clip, and does nothing if
it is the one playing, so a tick system can call it every tick. An entity
whose `Facing` is `Left` has its sprite mirrored. A clip that plays once
sends `AnimationFinished` when it ends.

Animators move in frames, by how long the simulation ran. Their time is
frame state: decide nothing in a tick system by it.

### Assets

The resource `AssetServer` loads files from `Config::assets`. A relative
path there is found from the folder the game runs in.

- `load::<Texture>(path)` returns a handle at once. A worker thread reads
  and decodes the file, and the main loop makes the texture, a couple of
  milliseconds' worth a frame. A sprite whose texture has not loaded yet is
  not drawn.
- `load_now` makes the asset before it returns, for what the game needs to
  set up, such as an `Atlas`. Before the window opens there is no GPU, so
  only an asset that needs none, an atlas but not a texture, can be made
  then.
- The asset types so far are `Texture`, a QOI image, `Atlas`, the cook's
  `atlas/<group>.kbj`, and `nadir::scene`'s `Level` and `PrefabAsset`,
  which `App::load_level` and the `PrefabLibrary` load. Asset paths are
  lowercase.
- Assets load in a `Scope`, `Global`, `Level` or `Transient`, and
  `unload_scope` frees those of one.
- Loads finish only in drawn frames, at no frame known in advance, so tick
  systems must not depend on whether an asset has loaded.

### Logs and errors

Log with `nadir::log::{error, warn, info, debug, trace}`. Records go to
standard error, and `App::run` also writes them to `nadir.log` in the
game's logs folder, keeping the last three runs' files beside it. On Linux
that folder is `$XDG_DATA_HOME/<game>` (`~/.local/share/<game>` by
default), on Windows `%LOCALAPPDATA%\<studio>\<game>`, and on macOS
`~/Library/Logs/<game>`. If the file cannot be opened, the game goes on
without it.

`App::run` returns a `nadir::Error` when the window cannot open, the
renderer cannot start, or a frame cannot be drawn. It logs the error and
shows it to the player first: on standard error on Linux, and in a message
box on Windows or an alert on macOS.

### Replays

`nadir::input::InputRecorder` records the game's `ActionState` tick by
tick, from a tick system, and `App::play_replay` plays a recording in place
of the live input. `App::world_hash` hashes the reflected components. After
the same ticks of the same replay, from the same start, it is the same
however the frames fell, as long as frame systems change no reflected
component and spawn or despawn nothing, and tick systems decide nothing by
what frames set, by whether an asset has loaded, or by the pointer or the
wheel.

## Cooking assets

The engine loads cooked assets. `nadir-cook` makes them from source files:

```bash
cargo install nadir-cook
nadir-cook assets cooked
```

`nadir-cook [--force] SOURCE OUT` cooks the files below `SOURCE` into
`OUT`:

- Each folder's PNG images, Aseprite exports and BMFont fonts go into an
  atlas named after the folder, its group: pages `atlas/<group>_<n>.qoi`
  and the index `atlas/<group>.kbj`, with each font's glyphs in
  `atlas/<font>.font.kbj`. `sprites/hero.png` is the sprite
  `sprites/hero` in the group `sprites`. An `atlas.jsonc` in a folder can
  name another group, and an image directly in `SOURCE` is an error.
- An Aseprite export is the JSON of
  `aseprite -b hero.aseprite --sheet hero.png --data hero.json --format json-array --list-tags`;
  its tags become the sprite's clips.
- A sidecar `<name>.sprite.jsonc` beside an image can cut it into frames,
  set its pivot, or make it a tile set: `{ "tiles": [16, 16] }`.
- Each JSONC file below `data/` becomes `data/<name>.kbj`; a prefab,
  `data/prefabs/<name>.jsonc`, is checked as one.
- Each LDtk 1.5.3 project, `.ldtk`, with its `.ldtkl` level files, becomes
  a level file of each level, `levels/<name>.klvl`, whose entities spawn
  the prefabs of their names.
- Every file above goes into one pack too, `game.kpak`, each file of the
  same bytes stored once, compressed where that makes it smaller.

A job whose files did not change since the last cook into `OUT` is skipped,
unless `--force`. Point `Config::assets` at `OUT`. A build without `dev`
reads the pack there when there is one, and the loose files otherwise, so
a release of the game needs only `game.kpak`; a pack that does not open
fails every load, with its error. A build with `dev` reads the loose
files. `App::load_level`
loads a level file into the world, its prefabs with it. The engine has no
asset type for the cooked fonts and other data files yet: text comes with
roadmap task M7.1.

## Features

- `dev`: for development builds.
  - The assets are the loose files of `Config::assets`, not its pack.
  - A texture that failed to load shows a placeholder, a magenta and black
    checker, rather than nothing.
  - The global allocator counts allocations. The resource `FrameAllocs`
    holds what the last frame allocated, and `nadir::thread_alloc_counts`
    and `nadir::process_alloc_counts` give the counts so far. With it on,
    no other crate in the program may declare a `#[global_allocator]`.
  - `App::run` reloads what changes while it runs: the loaded files, such
    as those `cargo xtask cook --watch` cooks again, and the engine's
    shaders. A texture draws its new texels behind its handle; an atlas's
    clips are replaced by name; a prefab is replaced in the
    `PrefabLibrary`; the current level is made again, behind the handles
    of its assets, tile layers and clips, its entities spawned anew; and a
    shader rebuilds its program. Whatever fails to reload is logged, and
    what it would replace stays. What the game copied out of an asset
    itself, such as an atlas's regions, does not change.
    `App::offscreen` never reloads.

Turn it on from your game's own feature, so that a release build leaves it
out:

```toml
[dependencies]
nadir_engine = "0.3"

[features]
dev = ["nadir_engine/dev"]
```

```bash
cargo run --features dev
```

## Platforms

- **Linux:** everything above works, under X11, or under Wayland through
  XWayland. Drawing needs OpenGL 3.3, through EGL.
- **Windows and macOS:** a game builds, and the engine's tests pass, but its
  window stays empty: there is no OpenGL context there yet (roadmap tasks
  M2.6 and M2.7). `App::run` logs the error and runs the game without
  drawing, and so without reloading; `App::offscreen` returns the error.
- Call `App::run` on the main thread: macOS opens windows only there.

## The sandbox

The repository's sandbox game is the full example, and the one each
feature is built for. Its
[`main.rs`](https://github.com/jabbott-iii/Nadir/blob/main/games/sandbox/src/main.rs)
has a tile map with animated water and flowers, a person cooked from an
Aseprite export who walks and collides, a camera that follows them, stress
scenes of thousands of sprites and colliders, recorded and replayed input,
and frames drawn with no window for the demo page. To run it:

```bash
git clone https://github.com/jabbott-iii/Nadir.git
cd Nadir
cargo xtask cook
cargo run -p sandbox
```

## Documentation

- API reference: https://docs.rs/nadir_engine
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/01-architecture.md
  (the main loop, stages and threads) and
  https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/ecs.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
