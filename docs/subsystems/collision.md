# Subsystem: `nadir-collision`

Kinematic collision and spatial queries for top-down games, plus
pathfinding. It is a pure library: it depends only on `nadir-core`. The
facade wires it to the ECS. There is no rigid-body physics (ADR-0011).

## Shapes and colliders

```rust
pub enum Shape { Aabb { half: Vec2 }, Circle { radius: f32 } }

pub struct Collider {
    pub shape: Shape,
    pub offset: Vec2,          // from entity position (feet pivot)
    pub layer: LayerMask,      // what I am (bitflags, u32)
    pub mask: LayerMask,       // what I collide with / detect
    pub kind: ColliderKind,    // Solid | Trigger
}
```

- `Collider::solid(shape)` and `Collider::trigger(shape)` put the shape on
  every layer, colliding with every layer. Sizes are finite and not
  negative. Other sizes give meaningless results, but nothing panics or
  returns NaN.
- Shapes that only touch do not overlap. A point on an edge is outside.
- Who meets whom:
  - A solid collider stops a mover whose mask has the collider's layer.
  - Tiles stop a mover whose mask has the grid's layer.
  - Overlap queries match every collider, solid or trigger, whose layer
    is in the query's mask.
  - Casts stop at solids only.
  - A trigger detects any other collider whose layer is in its mask.
- **Tiles** collide through a `TileGrid`: a solid bit per cell, a cell
  size in pixels, and an origin. There are no per-tile colliders.
  - Cells outside the grid are not solid, so a level needs walls round its
    edge.
  - Neighbouring cells share their edges exactly (`cell_rect`), so a seam
    between two solid tiles is one line, whatever the cell size and
    origin.
  - The grid has a `layer`, every layer by default (ADR-0021). A level
    loader sets it to the layer its masks call "World".
  - **Per-cell shapes**, M4.3 has full cells only. Half cells (top,
    bottom, left, right) and four corner triangles for diagonal walls are
    in the backlog.

## Broadphase: uniform spatial hash

- `CollisionWorld::rebuild` replaces the colliders from the facade's list
  each tick, and rebuilds the hash.
  - Each collider has an index, its place in the list, and an id from the
    caller that lasts across rebuilds.
  - The buffers are kept, so a steady tick allocates nothing.
- Cell size: 64 px by default (`CollisionWorld::with_cell_size`). Use at
  least twice the typical collider size.
- Storage:
  - an open-addressing table, with keys the `(i32, i32)` cells. A slot is
    live when it belongs to the current build, so clearing is O(1).
  - entries pointing into one flat array, filled with a two-pass counting
    sort (count per cell, prefix sum, scatter). Each entry holds the
    collider's index and its first cell.
- A query reports a collider only from the top-left cell where its cells
  and the query's overlap, so it reports each collider once, with no
  visited set.
- A collider over 1,024 cells, or one whose bounds are not finite, is kept
  aside, and every query visits it.
- A query whose region spans more cells than hold colliders walks the
  cells that do instead, row by row: a query never costs more than the
  colliders' cells, however large or sparse the world.
- `set_position(index, position)` moves a collider between rebuilds
  (ADR-0021).
  - Queries widen their region by the largest such move, up to a quarter
    of a cell.
  - A collider moved further than that is visited by every query until
    the next rebuild.
  - The facade calls it after each move, so that later movers see the new
    position.
- Queries visit colliders in the same order on every run:
  1. those kept aside, by index;
  2. those moved far, in the order they were moved;
  3. the rest by cell, row by row, then by index.

## Movement: move-and-slide

```rust
pub fn move_and_slide(
    world: &CollisionWorld, self_index: u32, start: Vec2, delta: Vec2, out_contacts: &mut Vec<Contact>,
) -> Vec2 /* resolved position */;
```

1. Move along X, sweeping the collider against the tile grid and the solid
   colliders, and stop where it would start to overlap one. Then do the
   same along Y.
2. A solid stops an axis only if the motion goes toward its center
   (ADR-0021).
   - A mover that starts inside a solid can leave it toward the nearer
     side, but never goes deeper.
   - An exact touch counts as ahead, so a mover clamped against a wall
     stays put when pushed into it, and leaves freely.
3. A solid that the mover overlaps across the axis by `EPSILON` or less
   does not stop it. So separating the axes does not snag on tile seams or
   corners.
   - A mover no more than `EPSILON` either side of its center, across its
     motion, treats a tile and its solid neighbours as one wall, so it
     cannot slip along the seam between them.
4. If `|delta|` exceeds half the smallest collider dimension, split the
   move into substeps, at most 16, each moving X then Y, so a diagonal
   follows its diagonal. Each axis is swept, so nothing tunnels through a
   thin wall however long the move.
5. A trigger mover is stopped by tiles only.
6. `out_contacts` gets each solid that stopped an axis, once per solid and
   normal.
7. Projectiles use `sweep_aabb`/`sweep_circle` (ray against
   Minkowski-expanded shapes) instead, and report the first hit.

## Triggers

- `Triggers::update(&world)`, each tick, computes the overlapping
  `(trigger, other)` pairs, by id.
- It sorts them, then diffs them against the previous tick's sorted list.
  `entered()` and `exited()` give the pairs that began and ended, which
  the facade sends through ECS events as `TriggerEnter` and `TriggerExit`.
- Because pairs are by id, a pair that keeps overlapping is not reported
  again when the rebuild order changes. A collider that leaves the world
  ends its pairs.

## Queries

Methods on `CollisionWorld`:

| Query | What it gives |
| :- | :- |
| `point(p, mask, out)` | the colliders containing `p` |
| `aabb(rect, mask, out)` | the colliders overlapping `rect`; an empty rect overlaps nothing |
| `circle(center, radius, mask, out)` | the colliders overlapping the circle |
| `nearest(p, radius, mask, ignore)` | the collider nearest to `p` by distance to its shape (0 inside), within `radius`; of two as near, the lower index |
| `segment(start, end, mask, ignore)` | the first solid along the segment: a line of sight |
| `sweep_aabb(center, half, delta, mask, ignore)`, `sweep_circle(...)` | the first solid the moving shape meets, as a `SweepHit` (obstacle, `t` in 0 to 1, position, normal) |

- The overlap queries replace the contents of `out` with collider
  indices, in the order above.
- Casts:
  - Casts step through the tile grid at most one cell at a time, only
    over the part of the cast that crosses the grid, in at most twice the
    grid's width plus height steps. They stop after the first step whose
    span holds the earliest hit found so far. They test colliders under
    the whole cast.
  - Casts share `move_and_slide`'s tolerance (ADR-0021):
    - A cast running exactly along an axis grazes a solid it overlaps
      across the motion by `EPSILON` or less.
    - A cast passing within `EPSILON` of a circle's surface, or of a
      rounded corner's, grazes it.
    - So a cast along the wall a mover rests against passes, as the move
      would. One along the seam between two solid tiles, or within
      `EPSILON` of it, is stopped.
  - The circle maths works from the point of the cast nearest the center,
    so casts thousands of pixels long keep their precision, and motions
    too long to square still give unit normals.
  - A cast that starts inside a shape stops at once if it moves toward the
    shape's center, and passes through otherwise.
  - Of two hits at the same time, a tile comes before a collider, then the
    first tile row by row, or the lower index.
  - A cast from an entity should pass its own collider as `ignore`.

Used by AI vision, area-of-effect attacks, interaction prompts and mouse
picking.

## Pathfinding

- **A\*** on the tile grid:
  - 8-connected, octile heuristic
  - corners can't be cut through solid cells
  - `std::collections::BinaryHeap` with a reused node arena
  - path smoothing by string-pulling with grid line-of-sight
- **Budget:** at most N expansions per tick, spread across requests (a
  request queue). Deterministic order.
- **Flow fields**, for many agents heading to one goal:
  - a Dijkstra integration field from the goal
  - a direction field from it
  - recomputed when the goal moves to another cell, at most once every 10
    ticks

## Invariants

- A collider that starts overlapping no solid ends `move_and_slide`
  overlapping none by more than `EPSILON` (0.01 px), give or take one f32
  step at its position. That step is under 0.002 px within ±16,384 px of
  the origin, and under 0.008 px within ±65,536 px; a test checks both.
- No function returns NaN for finite input. Zero-length deltas are a no-op.
- Results don't depend on hash-map iteration order.

## Tests

- **Property tests**, in `crates/collision/tests/`:
  - random solids plus random moves (`slide.rs`): no penetration, no NaN,
    position changes only along unobstructed components. Also:
    - the move goes all the way when nothing is under its swept box;
    - an axis that falls short has a contact, flush against what stopped
      it;
    - layers, masks and trigger movers are honoured.
  - the queries and casts against brute force, with colliders moved since
    the rebuild (`queries.rs`). Casts are checked by sampling overlaps
    along them, apart from the ray maths.
  - trigger enter/exit pairing (`triggers.rs`): each tick's `entered` and
    `exited` equal the brute-force diff, as colliders move, leave and
    arrive in a shuffled order. Every enter eventually has an exit when
    the shapes separate.
- **Scenarios** (`slide.rs`, `queries.rs`):
  - sliding along a wall of tiles never stops at seams: 1 px and 16 px
    tiles, box and circle movers, sizes, offsets and origins that round;
  - a 45° approach into an inside corner settles without jitter, against
    both walls;
  - a fast projectile doesn't tunnel through a 1-px wall: collider and
    tiles, sweeps, segment and `move_and_slide`;
  - a diagonal follows its substeps, counted from the smallest
    dimension, past a corner;
  - a mover leaves what it touches or starts in, but goes no deeper, and
    two movers on the same spot can part;
  - the tolerance across the motion is `EPSILON`, for moves and casts:
    0.008 px passes, 0.015 px stops;
  - contacts are only what stopped the move;
  - thin movers on a seam, and past the ends of a wall;
  - a line along a seam inside a wall is stopped, and one along its face
    passes, both ways, at cell sizes and origins that round;
  - casts along a collider's edge or past its corner graze it, and circle
    casts along what they rest against pass;
  - long casts, and motions too long to square, meet what they pass
    through;
  - the query order: kept aside, moved far, by cell; and the tie rules;
  - extreme values: a rect as wide as f32 allows keeps the colliders'
    precision; a collider over the whole f32 range neither overflows nor
    hangs the hash; the last cell there is has a rect; a cast from inside
    with a motion too long to square still finds its way;
  - far from the origin, the overlap bound holds to within one f32 step.
- A steady tick keeps every buffer's capacity, and a lighter tick after a
  heavier one keeps the heavier one's (`src/tests.rs`). The facade's
  allocation counter checks it in M4.4.
- **A\*:**
  - optimal-length tests on small handcrafted grids
  - no corner cutting
  - an unreachable goal returns `None` within budget
- **Benchmarks** (`cargo xtask bench collision`):
  - `collision_hash_5k`: rebuild plus 5k overlap queries;
  - `collision_slide_5k`: 5,000 `move_and_slide` calls, a crowd among 16 px
    tiles, one in five solid; about one move in four is stopped (ADR-0021);
  - A* on 256×256 with 30% obstacles (M9.5).
