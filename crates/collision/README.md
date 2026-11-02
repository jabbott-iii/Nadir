# nadir-collision

Kinematic collision and spatial queries for top-down 2D games: box and
circle colliders, a grid of solid tiles, a spatial hash, move-and-slide,
casts, overlap queries and triggers.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine
for top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine), which re-exports it
as `nadir::collision`, fills its world from the entities' colliders each
tick and moves them with it; depend on it directly to use it on its own.

## Status

Version 0.1: the API will change before 1.0.

Built:

- box and circle colliders, solid or trigger, on 32 layers;
- a tile grid of solid cells;
- a uniform spatial hash, rebuilt each tick, whose buffers are kept;
- move-and-slide, which stops at solids and slides along walls without
  snagging on the seams between tiles, and does not tunnel through thin
  walls;
- casts (a segment, a moving box, a moving circle) and overlap queries (a
  point, a box, a circle, the nearest collider);
- triggers: which pairs began and stopped overlapping, tick by tick.

Not yet:

- pathfinding, A* and flow fields (roadmap M9.5);
- tiles other than full cells, such as half cells and diagonal walls.

Movement is kinematic: there is no rigid-body physics.

## Usage

```toml
[dependencies]
nadir-collision = "0.1"
nadir-core = "0.1"
```

Positions, sizes and cells are nadir-core's `Vec2`, `IVec2` and `Rect`, so
add it too. Units are pixels, with +X right and +Y down.

A box walks into a wall of tiles, stops against it, and slides on along
it:

```rust
use nadir_collision::{Collider, CollisionWorld, Obstacle, Shape, TileGrid, move_and_slide};
use nadir_core::{IVec2, Vec2};

// 8 × 8 cells of 16 px, with a wall down column 4, from x = 64 to 80.
let mut grid = TileGrid::new(8, 8, 16.0, Vec2::ZERO);
for y in 0..8 {
    grid.set_solid(IVec2::new(4, y), true);
}
let mut world = CollisionWorld::new();
world.set_grid(grid);

// One collider, with an id of yours and its entity's position. Its box is
// 8 px wide, and sits on the entity's feet.
let player = Collider {
    offset: Vec2::new(0.0, -4.0),
    ..Collider::solid(Shape::Aabb { half: Vec2::splat(4.0) })
};
let start = Vec2::new(40.0, 60.0);
world.rebuild([(7, start, player)]);

// 30 px right and 2 down: the wall stops the X, not the Y.
let mut contacts = Vec::new();
let end = move_and_slide(&world, 0, start, Vec2::new(30.0, 2.0), &mut contacts);
assert_eq!(end, Vec2::new(60.0, 62.0));
assert_eq!(contacts[0].obstacle, Obstacle::Tile(IVec2::new(4, 3)));
```

A line of sight is a cast of a segment; it gives the first solid in the
way:

```rust
use nadir_collision::{CollisionWorld, LayerMask, Obstacle, TileGrid};
use nadir_core::{IVec2, Vec2};

let mut grid = TileGrid::new(8, 8, 16.0, Vec2::ZERO);
grid.set_solid(IVec2::new(4, 3), true);
let mut world = CollisionWorld::new();
world.set_grid(grid);

let hit = world
    .segment(Vec2::new(8.0, 56.0), Vec2::new(120.0, 56.0), LayerMask::ALL, None)
    .expect("the tile is in the way");
assert_eq!(hit.obstacle, Obstacle::Tile(IVec2::new(4, 3)));
assert_eq!(hit.normal, Vec2::new(-1.0, 0.0));
```

## Documentation

- API reference: https://docs.rs/nadir-collision
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/collision.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
