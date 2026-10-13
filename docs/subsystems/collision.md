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

- **Tiles** collide through a `TileGrid` (solid bitset per cell, cell size in
  pixels, origin). There are no per-tile colliders.
- **Optional per-cell shapes:** full, half-top, half-bottom, half-left,
  half-right, and four corner triangles for diagonal walls. Triangles are
  backlog; start with full cells.

## Broadphase: uniform spatial hash

- Cell size: default 64 px. Use at least 2× the typical collider size.
- Rebuilt every tick from the facade's collider list. `clear()` keeps
  capacity, so steady state does no allocation.
- Storage:
  - an open-addressing table (keys are `(i32, i32)` cells)
  - entries point into one flat `Vec<u32>` of collider indices, filled with
    a two-pass counting sort (count per cell, prefix sum, scatter)
- Queries return collider indices in deterministic order: by cell, then by
  insertion.

## Movement: move-and-slide

```rust
pub fn move_and_slide(
    world: &CollisionWorld, self_index: u32, start: Vec2, delta: Vec2, out_contacts: &mut Vec<Contact>,
) -> Vec2 /* resolved position */;
```

1. Move along X, then resolve against the tile grid and solid colliders.
   Push out along X only.
2. Move along Y, then resolve along Y only.
3. Separating axes prevents snagging on tile seams and corners.
4. If `|delta|` exceeds half the smallest collider dimension, substep so
   nothing tunnels through thin walls.
5. Projectiles use `sweep_aabb`/`sweep_circle` (ray against
   Minkowski-expanded shapes) instead, and report the first hit.

## Triggers

- Each tick, compute the overlapping (trigger, other) pairs.
- Sort them, then diff against the previous tick's sorted list. This yields
  `TriggerEnter` / `TriggerExit` events (sent through ECS events by the
  facade) and keeps them deterministic.

## Queries

`point`, `aabb`, `circle`, `segment` (DDA through grid cells plus shape
tests), `sweep_aabb`, `sweep_circle`, `nearest(within radius, mask)`.

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

- After `move_and_slide`, the collider overlaps no solid by more than
  `EPSILON` (0.01 px).
- No function returns NaN for finite input. Zero-length deltas are a no-op.
- Results don't depend on hash-map iteration order.

## Tests

- **Property tests:**
  - random solids plus random moves: no penetration, no NaN, position
    changes only along unobstructed components
  - trigger enter/exit pairing: every enter eventually has an exit when
    shapes separate
- **Scenarios:**
  - sliding along a wall of 1×1 tiles never stops at seams
  - a 45° approach into an inside corner settles without jitter
  - a fast projectile doesn't tunnel through a 1-px wall
- **A\*:**
  - optimal-length tests on small handcrafted grids
  - no corner cutting
  - an unreachable goal returns `None` within budget
- **Benchmarks:** rebuild plus 5k overlap queries; A* on 256×256 with 30%
  obstacles.
