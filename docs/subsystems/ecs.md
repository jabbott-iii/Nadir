# Subsystem: `nadir-ecs`

A small, safe, single-threaded entity-component system. It uses sparse-set
storage and has no `unsafe`. See ADR-0006.

## Why ECS, and why this one

- **Why ECS at all.** Rust punishes object graphs: entities holding
  references to each other end up as `Rc<RefCell<…>>` webs with runtime
  borrow panics. An ECS stores components in arrays keyed by entity ID, so
  relationships are IDs rather than references.
- **Why sparse sets rather than archetypes.**
  - Sparse sets make add/remove O(1) and iteration is dense per component.
  - The implementation is far smaller and needs no `unsafe`.
  - Archetypes iterate multi-component queries faster, but at our entity
    counts (≤ 5,000 active) the difference sits well inside budget.
  - Revisit only if a benchmark says so.

## Core types

Built in M4.1 as described here; the notes under each block record where
the code settles what this design left open.

```rust
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Entity(GenIndex);   // index + NonZeroU32 generation, from nadir_core

pub struct SparseSet<T> {
    sparse: Vec<u32>,          // entity index -> dense slot, or u32::MAX
    dense: Vec<Entity>,        // dense slot -> entity
    data: Vec<T>,              // dense slot -> component (parallel to `dense`)
}

impl<T> SparseSet<T> {
    pub fn insert(&mut self, e: Entity, v: T) -> Option<T>;
    pub fn remove(&mut self, e: Entity) -> Option<T>;   // swap_remove, fixes sparse of moved entity
    pub fn get(&self, e: Entity) -> Option<&T>;          // checks generation via dense[slot] == e
    pub fn get_mut(&mut self, e: Entity) -> Option<&mut T>;
    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)>;
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)>;
    pub fn len(&self) -> usize;
}
```

- `Entity` has `index()` and `generation()`, and no public constructor:
  entities come only from the world. The allocator is `nadir_core`'s
  `GenArena<()>`: freed slots are reused, newest first, and a slot whose
  generation counter is exhausted is retired.
- A set does not know which entities are alive, but generations only grow,
  so `insert` tells an older entity of the same slot from a newer one: an
  older one's leftover is replaced, and when a newer entity has a component
  in that slot, a stale caller's insert is dropped and the newer one keeps
  its own. `get`, `get_mut`, `contains` and `remove` find nothing for a
  stale entity.
- So give `write::<T>().insert` only living entities. An insert for a dead
  entity whose slot holds nothing in that set is kept, and no despawn
  removes it. Commands check that their entity is alive when they apply.

## World

```rust
pub struct World {
    entities: RefCell<GenArena<()>>,                               // free list; commands reserve through &World
    storages: FastHashMap<TypeId, Box<dyn ErasedStorage>>,          // RefCell<SparseSet<T>> + queued inserts inside
    resources: FastHashMap<TypeId, RefCell<Box<dyn Any>>>,
    commands: RefCell<Vec<Op>>,                                    // queued structural changes
    events: Vec<EventClears>,                                      // how to clear each event type
}
// The storage of each type knows its name and how to remove an entity. The
// reflection hooks live in nadir-scene's TypeRegistry (M4.2), not here.

impl World {
    pub fn register<T: Component>(&mut self);
    pub fn read<T: Component>(&self) -> Read<'_, T>;      // Ref<SparseSet<T>>
    pub fn write<T: Component>(&self) -> Write<'_, T>;    // RefMut<SparseSet<T>>
    pub fn resource<R: 'static>(&self) -> Res<'_, R>;
    pub fn resource_mut<R: 'static>(&self) -> ResMut<'_, R>;
    pub fn insert_resource<R: 'static>(&mut self, r: R);
    pub fn commands(&self) -> Commands<'_>;
    pub fn is_alive(&self, e: Entity) -> bool;
    pub fn apply_commands(&mut self);
    // direct structural API for setup code and tests (not inside systems):
    pub fn spawn(&mut self) -> EntityMut<'_>;
    pub fn despawn(&mut self, e: Entity);
}
```

- **`Component`** is a marker trait (`'static`), implemented explicitly:
  `impl Component for Health {}`. The facade defines the components
  (ADR-0014), so it can implement it for each. Types must be registered
  before use. Reading an unregistered type panics with the type name and a
  hint.
- `Read`, `Write`, `Res` and `ResMut` are type aliases of `Ref<SparseSet<T>>`,
  `RefMut<SparseSet<T>>`, `Ref<R>` and `RefMut<R>`. The facade's interim
  `Resources` (ADR-0019) returns the same types.
- The world also has `is_registered`, `contains_resource` and
  `entity_count`; `spawn()` returns an `EntityMut` with `with(component)`
  and `id()`.
- **Runtime borrow checking:** storages and resources sit behind `RefCell`.
  A conflicting borrow in the same system panics, and the message includes
  the component type name and the current system name, which the schedule
  sets in a thread-local. Tests run every system at least once, so conflicts
  surface in CI.
  - The name is set by a guard that puts the previous name back when the
    system returns or panics. Outside systems it is `(none)`.

## Iterating several components safely

Always **drive iteration from the storage you mutate.** Look up the others by
entity inside the loop. This is safe, needs no `unsafe` iterator tricks, and
is fast when the driving set is the smaller one.

```rust
fn apply_velocity(world: &World, tick: &Tick) {
    let vel = world.read::<Velocity>();
    let mut pos = world.write::<Transform>();
    for (e, t) in pos.iter_mut() {
        if let Some(v) = vel.get(e) {
            t.position += v.0 * tick.dt;
        }
    }
}
```

Helpers cover the common shapes:

- `join2(&a, &b)`: read-only; iterates the smaller of the two.
- `for_each_mut_with(&mut a, &b, |e, a, b| ...)`: one mutable, one read-only.

If two components must both be mutable, iterate one mutably and call
`get_mut` on the other inside the body; each lives in its own `RefMut`.

## Commands (deferred structural changes)

```rust
let mut cmd = world.commands();
let e = cmd.spawn()
    .with(Transform::at(pos))
    .with(Sprite::from(handle))
    .id();                  // Entity is reserved immediately (allocator), components applied later
cmd.despawn(enemy);
cmd.insert(e, Stunned { ticks: 30 });
```

- Commands are applied by `World::apply_commands` after each fixed tick and
  after the frame stages: the schedule's `run_fixed` and `run_frame` call
  it. They are applied in the order they were queued.
- `Commands` has `spawn()`, `entity(e)`, `insert(e, c)`, `remove::<T>(e)` and
  `despawn(e)`. `EntityCommands` has `with(c)`, `without::<T>()` and `id()`.
- Nothing is boxed. The queue holds small ops (insert of type T, remove,
  despawn), and an insert's value waits in its own type's queue beside the
  storage. Both are reused, so a steady stream of commands allocates
  nothing once they have grown.
- An insert whose entity was despawned before it applies is dropped, at
  debug level. A command for an unregistered type panics when it is
  queued.
- Despawning an already-dead entity is a no-op that logs at debug level.

## Events

```rust
pub struct Events<T> { tick: Vec<T>, frame: Vec<T> }
impl<T: Clone> Events<T> {
    pub fn send(&mut self, ev: T);              // pushes to both buffers
    pub fn tick_events(&self) -> &[T];          // fixed-stage readers (same tick, writer ordered before reader)
    pub fn frame_events(&self) -> &[T];         // frame-stage readers (all ticks since last frame)
}
```

- `World::add_event::<T>()` adds `Events<T>` as a resource. Writers and
  readers borrow it like any resource:
  `world.resource_mut::<Events<Hit>>().send(hit)`.
- `tick` is cleared at the start of every fixed tick. `frame` is cleared after
  the frame stages. The schedule's `run_fixed` and `run_frame` do both,
  through clear functions that `add_event` records for each type.
- Writers must be ordered before readers, either by stage or by insertion
  order. Use the debug console's `schedule` command to check.

## Schedule

```rust
pub struct Schedule {
    fixed: [Vec<SystemEntry<Tick>>; FixedStage::COUNT],
    frame: [Vec<SystemEntry<Frame>>; FrameStage::COUNT],
}
struct SystemEntry<C> {
    name: &'static str,
    run: fn(&World, &C),
    condition: Option<fn(&World) -> bool>,   // run condition, e.g. "only in GameState::InGame"
}
```

- The schedule's API:
  - `add_fixed(stage, name, system)` and `add_frame(stage, name, system)`,
    with `add_fixed_if` and `add_frame_if` taking a run condition
    `fn(&World) -> bool`, asked right before each run.
  - `run_fixed(&mut World, &Tick)`: clear the tick events, run the fixed
    stages, apply the commands.
  - `run_frame(&mut World, &Frame)`: run the frame stages, apply the
    commands, clear the frame events.
  - `FixedStage` and `FrameStage` have `ALL`, in order.
- Systems run strictly in stage order, then insertion order. There is no
  automatic dependency inference and no parallelism. Names need not be
  unique; they are labels.
- Each system run is wrapped in `profile_scope!(name)` and sets the "current
  system" thread-local used in borrow-panic messages.
- `Schedule::describe()` returns the ordered list, for the console and tests:
  `SystemInfo { stage, name, conditional }`, which displays as
  `tick  Gameplay: player_move`.

## Supporting patterns

- **Hierarchy (minimal):** a `Parent { entity, offset }` component. A
  `PostMovement` system writes children's `Transform` from their parent's.
  There is no general transform tree.
- **Stable IDs for saves:** a `SaveId(u64)` component, assigned from a
  world-level counter. Saves never store raw `Entity` values.
- **Tags:** zero-sized components (`struct Enemy;`) use the normal
  `SparseSet<T>`. `Vec<ZST>` never allocates, so tags cost only the
  sparse/dense arrays.

## Invariants

- `dense.len() == data.len()`, and `sparse[dense[i].index] == i` for all `i`
  (checked by debug assertions and property tests).
- Iteration order of a storage is fully determined by its insert/remove
  history. This is required for deterministic replays.
- No storage or resource is borrowed across systems. All borrows end when the
  system returns.

## Tests

- Property test: random spawn/insert/remove/despawn sequences against a
  `BTreeMap` reference model.
- Generation safety: a stale `Entity` never reads the new occupant's data.
- Borrow-conflict test: two `write::<T>()` calls in one system panic with a
  message naming the system and the type.
- Event buffers: tick versus frame visibility across multiple ticks per
  frame.
- Benchmark: join over 5k entities with 2 and 3 components; spawn/despawn
  churn (`ecs_join2_5k`, `ecs_join3_5k`, `ecs_spawn_despawn` in
  `nadir-bench`).
- Planted bugs each failed a test in M4.1 (see the roadmap): the
  swap-remove fix-up, a despawn that leaves components, removes applied
  before inserts, and frame events cleared late.
